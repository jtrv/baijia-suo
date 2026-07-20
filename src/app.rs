//! Application state machine and Wayland connection management.
//!
//! Handles the main event loop, Wayland connection, and state transitions
//! for the lockscreen application.

use crate::animation::Playlist;
use crate::auth::{create_verifier, AuthBackendKind, ForkedVerifier, VerifierError};
use crate::config::Config;
use crate::password::Password;
use crate::render::indicator::{
    IndicatorCtx, IndicatorStyle, COMPLETE_FADE_MS, KEYSTROKE_SETTLE_MS, NUM_SEGMENTS,
    VERIFY_ENVELOPE_MS,
};
use crate::render::{BaseFrame, BufferContent, DamageRect, RenderOutcome};
use crate::secure::SecureBuffer;
use log::{error, info, warn};
use std::time::{Duration, Instant};

/// Authentication state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AuthState {
    Idle,
    Typing,
    Verifying,
    Success,
    Invalid,
}

/// How long a PAM message stays on screen after the attempt that produced it.
const PAM_MESSAGE_TTL: Duration = Duration::from_secs(30);
/// How long the indicator lingers after the last interaction before the whole
/// thing fades out, leaving a clean background. Any keystroke brings it back.
const INDICATOR_IDLE_HIDE: Duration = Duration::from_secs(5);

/// Main application context.
pub(crate) struct App {
    pub auth_state: AuthState,
    pub password: Option<Password>,
    pub failed_attempts: u32,
    /// Last message PAM surfaced on a failed attempt (e.g. a faillock lockout
    /// notice) and when it arrived. Shown in the indicator for
    /// [`PAM_MESSAGE_TTL`], then it fades out on its own.
    last_pam_message: Option<String>,
    pam_message_at: Option<Instant>,
    /// Time of the last interaction (keystroke or auth result). The indicator
    /// shows for [`INDICATOR_IDLE_HIDE`] after this, then hides when idle.
    last_interaction: Option<Instant>,
    pub config: Config,
    /// Battery is at/below the configured low-battery threshold: animations
    /// are suspended and the solid background shows (power saver).
    pub low_power: bool,
    pub keyboard: crate::input::keyboard::KeyboardHandler,
    /// One playlist per physical output size, created lazily on the first
    /// frame for that size. Equal-sized outputs intentionally share it; this
    /// avoids duplicate simulation while preventing mixed-size reset thrash.
    pub playlists: std::collections::HashMap<(u32, u32), Playlist>,
    /// False once modes were configured but none were valid — stops the
    /// lazy creation from retrying (and re-warning) every frame.
    animation_enabled: bool,
    background_rgba: (f64, f64, f64, f64),
    auth_verifier: Option<ForkedVerifier>,
    pub keystroke_timestamps: Vec<Instant>,
    pub verification_start: Option<Instant>,
    pub auth_complete_time: Option<Instant>,
    pub segment_sequence: Vec<usize>,
}

impl App {
    /// Create a new application.
    pub fn new(config: Config) -> Self {
        let background_rgba = (
            config.background_color.r,
            config.background_color.g,
            config.background_color.b,
            config.background_color.a,
        );

        use rand::seq::SliceRandom;
        let mut segment_sequence: Vec<usize> = (0..NUM_SEGMENTS).collect();
        segment_sequence.shuffle(&mut rand::rng());

        App {
            auth_state: AuthState::Idle,
            password: None,
            failed_attempts: 0,
            last_pam_message: None,
            pam_message_at: None,
            last_interaction: None,
            animation_enabled: !config.animation.modes.is_empty(),
            config,
            keyboard: crate::input::keyboard::KeyboardHandler::new(),
            playlists: std::collections::HashMap::new(),
            background_rgba,
            auth_verifier: None,
            keystroke_timestamps: Vec::new(),
            verification_start: None,
            auth_complete_time: None,
            segment_sequence,
            low_power: false,
        }
    }

    /// The playlist for one physical output size, created on first use.
    /// Returns None when animation is disabled (no modes, none valid, or a
    /// creation failure — which disables further attempts).
    fn playlist_for(&mut self, width: u32, height: u32) -> Option<&mut Playlist> {
        if !self.animation_enabled || width == 0 || height == 0 {
            return None;
        }
        use std::collections::hash_map::Entry;
        match self.playlists.entry((width, height)) {
            Entry::Occupied(e) => Some(e.into_mut()),
            Entry::Vacant(e) => {
                let mut params = self.config.animation.params.clone();
                params.width = width;
                params.height = height;
                match Playlist::new(
                    &self.config.animation.modes,
                    self.config.animation.cycle,
                    params,
                    self.background_rgba,
                ) {
                    Some(mut p) => {
                        p.ensure_sized(width, height);
                        // A playlist created during render missed the draw
                        // cycle's advance pass. Produce its first frame and
                        // arm its clock before it is blitted.
                        p.advance(Instant::now());
                        Some(e.insert(p))
                    }
                    None => {
                        log::warn!("no valid animation modes; falling back to solid background");
                        self.animation_enabled = false;
                        None
                    }
                }
            }
        }
    }

    /// Advance every per-size playlist's clock. Called once per draw cycle
    /// by the event loop; playlists for sizes that haven't rendered yet
    /// simply don't exist and get created current on first render.
    pub fn advance_animations(&mut self, now: Instant) {
        for p in self.playlists.values_mut() {
            p.advance(now);
        }
    }

    /// Earliest next animation wakeup across all per-size playlists.
    pub fn next_anim_wake(&self) -> Option<Instant> {
        self.playlists.values().filter_map(|p| p.next_wake()).min()
    }

    /// Drop animation state for physical sizes no configured output uses.
    pub fn retain_animation_sizes(&mut self, active: &[(u32, u32)]) {
        self.playlists.retain(|size, _| active.contains(size));
    }

    /// Render the current state into the raw `wl_shm` BGRA buffer `buf`
    /// (stride `width * 4`, no padding).
    ///
    /// Existing players are advanced by `WaylandState::draw`; a player first
    /// discovered here advances itself once so its initial frame and wakeup
    /// are ready immediately.
    /// `width`/`height` are the physical buffer dimensions; `scale` is the
    /// output's integer buffer scale (1 on a standard display, 2+ on HiDPI).
    /// The background fills the buffer resolution-independently; only the
    /// indicator's pixel geometry needs `scale` so it keeps its logical size
    /// while rendering crisp at device resolution.
    #[cfg(test)]
    pub fn render_to_surface(
        &mut self,
        buf: &mut [u8],
        width: i32,
        height: i32,
        scale: i32,
    ) -> Result<(), String> {
        self.render_to_surface_cached(buf, width, height, scale, BufferContent::default())?;
        Ok(())
    }

    /// Render using the pixels already held by one reusable SHM buffer.
    /// Returns the new content identity and the smallest region that changed.
    pub(crate) fn render_to_surface_cached(
        &mut self,
        buf: &mut [u8],
        width: i32,
        height: i32,
        scale: i32,
        previous: BufferContent,
    ) -> Result<RenderOutcome, String> {
        if width <= 0 || height <= 0 || scale <= 0 {
            return Err("invalid render dimensions".into());
        }
        let needed = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .map(|bytes| bytes as usize)
            .ok_or("render dimensions overflow")?;
        if buf.len() < needed {
            return Err("render buffer is smaller than its surface".into());
        }

        let full = DamageRect::full(width, height);
        let mut base = BaseFrame::Solid;
        if !self.low_power {
            if let Some(player) = self.playlist_for(width as u32, height as u32) {
                let (playlist, generation) = player.frame_id();
                base = BaseFrame::Animation {
                    playlist,
                    generation,
                };
                if previous.base != Some(base) {
                    player.blit_into(buf, width, height)?;
                } else if let Some(rect) = previous.indicator {
                    player.blit_rect_into(buf, width, height, rect)?;
                }
            }
        }
        if base == BaseFrame::Solid {
            let bg = self.config.background_color;
            let color = (bg.r, bg.g, bg.b, bg.a);
            if previous.base != Some(base) {
                crate::render::background::render_solid_color(buf, color)?;
            } else if let Some(rect) = previous.indicator {
                crate::render::background::render_solid_color_rect(
                    buf, width, height, color, rect,
                )?;
            }
        }

        let now = Instant::now();
        // Draw the indicator only while it should be visible; when idle long
        // enough it hides entirely, leaving just the background.
        let indicator = if self.indicator_visible(now) {
            let ind = &self.config.indicator;
            let radius = ind.radius * scale as f64;
            let ctx = IndicatorCtx {
                x: ind.x_position * width as f64,
                y: ind.y_position * height as f64,
                radius,
                thickness: radius * 0.1,
                num_segments: NUM_SEGMENTS,
                now,
                auth_state: self.auth_state,
                keystrokes: &self.keystroke_timestamps,
                verification_start: self.verification_start,
                auth_complete_time: self.auth_complete_time,
                segment_sequence: &self.segment_sequence,
                failed_attempts: self.failed_attempts,
                message: self.pam_message(now),
            };
            let style = IndicatorStyle {
                mode: ind.mode,
                opacity: ind.opacity,
                color: ind.color.map(|c| (c.r, c.g, c.b)),
                caps_lock: self.keyboard.caps_lock,
            };
            crate::render::indicator::render_indicator(buf, width, height, &ctx, &style)?
        } else {
            None
        };

        let damage = if previous.base != Some(base) {
            Some(full)
        } else {
            match (previous.indicator, indicator) {
                (Some(old), Some(new)) => Some(old.union(new)),
                (Some(rect), None) | (None, Some(rect)) => Some(rect),
                (None, None) => None,
            }
        };
        Ok(RenderOutcome {
            content: BufferContent {
                base: Some(base),
                indicator,
            },
            damage,
        })
    }

    /// True while the indicator still has animation to draw: an in-flight or
    /// just-finished auth attempt (verify envelope + completion fade), or a
    /// keystroke pop still settling. The event loop redraws at
    /// [`crate::render::indicator::FRAME_MS`] while this holds.
    pub fn indicator_active(&self, now: Instant) -> bool {
        if let Some(v_start) = self.verification_start {
            match self.auth_complete_time {
                None => return true, // still waiting on the verifier
                Some(done) => {
                    if now.duration_since(v_start) < Duration::from_millis(VERIFY_ENVELOPE_MS)
                        || now.duration_since(done) < Duration::from_millis(COMPLETE_FADE_MS)
                    {
                        return true;
                    }
                }
            }
        }
        self.keystroke_timestamps.last().is_some_and(|last| {
            now.duration_since(*last) < Duration::from_millis(KEYSTROKE_SETTLE_MS)
        })
    }

    /// True once a finished auth attempt's animation envelope has fully
    /// played out — the moment to unlock (Success) or arm the failure clear.
    pub fn auth_settled(&self, now: Instant) -> bool {
        match (self.verification_start, self.auth_complete_time) {
            (Some(start), Some(done)) => {
                now.duration_since(start) >= Duration::from_millis(VERIFY_ENVELOPE_MS)
                    && now.duration_since(done) >= Duration::from_millis(COMPLETE_FADE_MS)
            }
            _ => false,
        }
    }

    /// Initialize the authentication verifier.
    pub fn init_auth(
        &mut self,
        username: &str,
        backend: AuthBackendKind,
    ) -> Result<(), VerifierError> {
        self.auth_verifier = Some(create_verifier(backend, username)?);
        Ok(())
    }

    /// Handle a key press.
    pub fn handle_key(&mut self, codepoint: u32) {
        if matches!(
            self.auth_state,
            AuthState::Verifying | AuthState::Success | AuthState::Invalid
        ) {
            return; // Ignore input while validating or animating the result
        }
        self.last_interaction = Some(Instant::now());

        if self.password.is_none() {
            match Password::new(256) {
                Ok(pw) => self.password = Some(pw),
                Err(e) => {
                    error!("Failed to create password buffer: {}", e);
                    return;
                }
            }
        }

        match codepoint {
            // Enter
            0x0D | 0x0A => {
                self.process_auth();
            }
            // Escape
            0x1B => {
                if let Some(ref mut p) = self.password {
                    p.clear();
                }
                self.keystroke_timestamps.clear();
                self.verification_start = None;
                self.auth_complete_time = None;
                self.auth_state = AuthState::Idle;
            }
            // Backspace
            0x08 => {
                if let Some(ref mut p) = self.password {
                    if !p.is_empty() {
                        self.keystroke_timestamps.pop();
                    }
                    p.backspace();
                }
                let now_empty = self.password.as_ref().map(|p| p.is_empty()).unwrap_or(true);
                self.auth_state = if now_empty {
                    AuthState::Idle
                } else {
                    AuthState::Typing
                };
            }
            // Ctrl+U
            0x15 => {
                if let Some(ref mut p) = self.password {
                    p.clear();
                }
                self.keystroke_timestamps.clear();
                self.verification_start = None;
                self.auth_complete_time = None;
                self.auth_state = AuthState::Idle;
            }
            // Ctrl+C
            0x03 => {}
            // Printable characters (excluding DEL)
            c if c >= 0x20 && c != 0x7F => {
                if let Some(ref mut p) = self.password {
                    if p.append_char(c).is_ok() {
                        self.keystroke_timestamps.push(std::time::Instant::now());
                        self.auth_state = AuthState::Typing;
                    }
                }
            }
            _ => {}
        }
    }

    /// Process the actual authentication synchronously.
    pub fn process_auth(&mut self) {
        if self.password.is_none() || self.password.as_ref().map(|p| p.is_empty()).unwrap_or(true) {
            return;
        }

        let password = match self.password.take() {
            Some(p) => p,
            None => return,
        };

        // Copy the typed password into a fresh secure buffer for transfer.
        let mut secure_pw = match SecureBuffer::new(256) {
            Ok(buf) => buf,
            Err(e) => {
                error!("failed to create secure buffer: {}", e);
                self.fail_attempt();
                return;
            }
        };
        for &byte in password.as_bytes() {
            if secure_pw.try_push(&[byte]).is_err() {
                error!("password exceeds buffer capacity");
                self.fail_attempt();
                return;
            }
        }

        self.auth_state = AuthState::Verifying;
        self.verification_start = Some(std::time::Instant::now());
        self.auth_complete_time = None;

        match self.auth_verifier.as_mut() {
            Some(verifier) => verifier.start(secure_pw),
            None => {
                // Unreachable in practice: `main` refuses to lock without a
                // verifier. Fail closed regardless of how we got here.
                error!("no authentication verifier; refusing to unlock");
                self.fail_attempt();
            }
        }
    }

    /// Check if the in-flight authentication attempt has finished.
    pub fn update_auth(&mut self) {
        let result = match self.auth_verifier.as_mut() {
            Some(verifier) => verifier.poll(),
            None => None,
        };

        if let Some(result) = result {
            let now = std::time::Instant::now();
            self.auth_complete_time = Some(now);
            self.last_interaction = Some(now); // keep the result on screen a beat

            match result {
                Ok((true, _)) => {
                    info!("authentication successful");
                    self.set_pam_message(None, now);
                    self.auth_state = AuthState::Success;
                }
                Ok((false, message)) => {
                    warn!(
                        "authentication failed (attempt {})",
                        self.failed_attempts + 1
                    );
                    // PAM (e.g. faillock) may explain why — surface it verbatim.
                    self.set_pam_message(message, now);
                    self.fail_attempt();
                }
                Err(e) => {
                    error!("authentication error: {}", e);
                    self.set_pam_message(None, now);
                    self.fail_attempt();
                }
            }
        }
    }

    fn set_pam_message(&mut self, message: Option<String>, now: Instant) {
        self.pam_message_at = message.as_ref().map(|_| now);
        self.last_pam_message = message;
    }

    /// The PAM message to display, or `None` once it has been up for
    /// [`PAM_MESSAGE_TTL`].
    pub fn pam_message(&self, now: Instant) -> Option<&str> {
        match (&self.last_pam_message, self.pam_message_at) {
            (Some(m), Some(at)) if now.duration_since(at) < PAM_MESSAGE_TTL => Some(m),
            _ => None,
        }
    }

    /// When the current message stops showing, or `None` if none is live — the
    /// event loop wakes here to redraw the message away.
    pub fn pam_message_deadline(&self, now: Instant) -> Option<Instant> {
        self.pam_message_at
            .filter(|_| self.last_pam_message.is_some())
            .map(|at| at + PAM_MESSAGE_TTL)
            .filter(|deadline| now < *deadline)
    }

    /// Whether the indicator should be drawn at all. It shows during an active
    /// attempt/result, while a PAM message is still live, and for a short while
    /// after the last interaction — then hides to leave a clean background.
    pub fn indicator_visible(&self, now: Instant) -> bool {
        !matches!(self.auth_state, AuthState::Idle)
            || self.pam_message(now).is_some()
            || self
                .last_interaction
                .is_some_and(|t| now.duration_since(t) < INDICATOR_IDLE_HIDE)
    }

    /// When the post-interaction idle window ends (while otherwise idle), so
    /// the event loop wakes to hide the indicator. `None` if not applicable.
    pub fn indicator_hide_deadline(&self, now: Instant) -> Option<Instant> {
        if !matches!(self.auth_state, AuthState::Idle) {
            return None;
        }
        self.last_interaction
            .map(|t| t + INDICATOR_IDLE_HIDE)
            .filter(|deadline| now < *deadline)
    }

    /// Record a failed authentication attempt; the screen stays locked.
    /// Lockout is enforced by PAM (pam_faillock in the account stack), not
    /// here — we only surface whatever message it returned.
    fn fail_attempt(&mut self) {
        self.failed_attempts += 1;
        self.auth_state = AuthState::Invalid;
    }

    /// Clear failed attempt state.
    pub fn clear_auth_failed(&mut self) {
        self.keystroke_timestamps.clear();
        self.verification_start = None;
        self.auth_complete_time = None;
        self.auth_state = AuthState::Idle;
    }
}

/// True when the system runs on battery at or below `threshold` percent.
///
/// Sums energy (falling back to charge, then capacity) across all batteries
/// so dual-battery ThinkPads read as their combined level, and requires at
/// least one battery to be discharging. Any read error reads as "not low" —
/// a lock screen must never blank the animation because sysfs moved.
pub fn battery_low(threshold: u32) -> bool {
    let entries = match std::fs::read_dir("/sys/class/power_supply") {
        Ok(d) => d,
        Err(_) => return false,
    };
    let read = |p: &std::path::Path, f: &str| -> Option<u64> {
        std::fs::read_to_string(p.join(f)).ok()?.trim().parse().ok()
    };

    let mut discharging = false;
    let mut now_sum: u64 = 0;
    let mut full_sum: u64 = 0;
    let mut capacity_fallback: Option<u64> = None;

    for e in entries.flatten() {
        let p = e.path();
        let is_battery = std::fs::read_to_string(p.join("type"))
            .map(|t| t.trim() == "Battery")
            .unwrap_or(false);
        if !is_battery {
            continue;
        }
        if std::fs::read_to_string(p.join("status"))
            .map(|s| s.trim() == "Discharging")
            .unwrap_or(false)
        {
            discharging = true;
        }
        if let (Some(now), Some(full)) = (
            read(&p, "energy_now").or_else(|| read(&p, "charge_now")),
            read(&p, "energy_full").or_else(|| read(&p, "charge_full")),
        ) {
            now_sum += now;
            full_sum += full;
        } else if let Some(cap) = read(&p, "capacity") {
            capacity_fallback = Some(capacity_fallback.map_or(cap, |c| c.min(cap)));
        }
    }

    if !discharging {
        return false;
    }
    let percent = if let Some(p) = (100 * now_sum).checked_div(full_sum) {
        p as u32
    } else if let Some(cap) = capacity_fallback {
        cap as u32
    } else {
        return false;
    };
    percent <= threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTINEL: u8 = 0xa5;

    fn assert_untouched_outside(buf: &[u8], width: i32, rect: Option<DamageRect>) {
        for (pixel, bytes) in buf.chunks_exact(4).enumerate() {
            let x = pixel as i32 % width;
            let y = pixel as i32 / width;
            let touched = rect
                .is_some_and(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height);
            if !touched {
                assert_eq!(bytes, [SENTINEL; 4], "pixel ({x}, {y}) changed");
            }
        }
    }

    #[test]
    fn app_initial_state() {
        let app = App::new(Config::default());
        assert!(matches!(app.auth_state, AuthState::Idle));
    }

    #[test]
    fn app_key_handling() {
        let mut app = App::new(Config::default());

        // Type some characters
        app.handle_key(b'a' as u32);
        assert!(matches!(app.auth_state, AuthState::Typing));
        assert!(app.password.is_some());

        // Clear with Escape
        app.handle_key(0x1B);
        assert!(matches!(app.auth_state, AuthState::Idle));
    }

    #[test]
    fn fail_attempt_counts_and_sets_invalid() {
        let mut app = App::new(Config::default());
        app.fail_attempt();
        assert_eq!(app.failed_attempts, 1);
        assert!(matches!(app.auth_state, AuthState::Invalid));
        app.fail_attempt();
        assert_eq!(app.failed_attempts, 2);
    }

    #[test]
    fn pam_message_persists_through_typing() {
        let mut app = App::new(Config::default());
        let now = Instant::now();
        app.set_pam_message(Some("Account locked".to_string()), now);
        app.fail_attempt();
        app.clear_auth_failed(); // Invalid envelope done -> Idle
        app.handle_key(b'a' as u32); // typing the next attempt
        assert_eq!(
            app.pam_message(now),
            Some("Account locked"),
            "the message stays while typing the next attempt"
        );
    }

    #[test]
    fn indicator_hides_when_idle_but_message_keeps_it() {
        let mut app = App::new(Config::default());
        let t0 = Instant::now();
        // Fresh lock, no interaction yet → nothing shown.
        assert!(!app.indicator_visible(t0));

        // An interaction shows it, and it lingers, then hides once idle.
        app.last_interaction = Some(t0);
        assert!(app.indicator_visible(t0 + Duration::from_secs(1)));
        assert!(!app.indicator_visible(t0 + INDICATOR_IDLE_HIDE + Duration::from_secs(1)));

        // A live PAM message keeps it visible past the idle window.
        app.set_pam_message(Some("Account locked".to_string()), t0);
        assert!(app.indicator_visible(t0 + INDICATOR_IDLE_HIDE + Duration::from_secs(1)));
    }

    #[test]
    fn pam_message_expires_after_ttl() {
        let mut app = App::new(Config::default());
        let now = Instant::now();
        app.set_pam_message(Some("Account locked".to_string()), now);
        assert!(app.pam_message(now).is_some());
        let later = now + PAM_MESSAGE_TTL + Duration::from_secs(1);
        assert!(
            app.pam_message(later).is_none(),
            "message drops after its TTL"
        );
        assert!(app.pam_message_deadline(later).is_none());
    }

    #[test]
    fn mixed_output_sizes_get_independent_players() {
        let mut config = Config::default();
        config.animation.modes = vec!["spiral".into()];
        let mut app = App::new(config);
        let (w1, h1) = (100i32, 80i32);
        let (w2, h2) = (160i32, 120i32);
        let mut buf1 = vec![0u8; (w1 * h1 * 4) as usize];
        let mut buf2 = vec![0u8; (w2 * h2 * 4) as usize];
        // Interleave two sizes: with the old single shared player this
        // reset the animation on every call; per-size players must end up
        // as exactly two stable entries.
        for _ in 0..3 {
            app.advance_animations(Instant::now());
            app.render_to_surface(&mut buf1, w1, h1, 1).unwrap();
            app.render_to_surface(&mut buf2, w2, h2, 1).unwrap();
        }
        assert_eq!(app.playlists.len(), 2, "one player per physical size");

        app.retain_animation_sizes(&[(w1 as u32, h1 as u32)]);
        assert_eq!(app.playlists.len(), 1, "unused sizes are pruned");
        assert!(app.playlists.contains_key(&(w1 as u32, h1 as u32)));
    }

    #[test]
    fn first_render_arms_a_lazily_created_playlist() {
        let mut config = Config::default();
        config.animation.modes = vec!["spiral".into()];
        let mut app = App::new(config);
        let (width, height) = (100, 80);
        let mut buffer = vec![0; (width * height * 4) as usize];

        app.render_to_surface(&mut buffer, width, height, 1)
            .unwrap();

        assert!(app.next_anim_wake().is_some());
    }

    #[test]
    fn cached_render_skips_an_unchanged_frame() {
        let mut config = Config::default();
        config.animation.modes = vec!["spiral".into()];
        let mut app = App::new(config);
        let (width, height) = (100, 80);
        let mut buffer = vec![0; (width * height * 4) as usize];

        let first = app
            .render_to_surface_cached(&mut buffer, width, height, 1, BufferContent::default())
            .unwrap();
        assert_eq!(first.damage, Some(DamageRect::full(width, height)));

        buffer.fill(SENTINEL);
        let second = app
            .render_to_surface_cached(&mut buffer, width, height, 1, first.content)
            .unwrap();
        assert_eq!(second.damage, None);
        assert_untouched_outside(&buffer, width, None);
    }

    #[test]
    fn cached_render_full_damage_on_changed_generation() {
        let mut config = Config::default();
        config.animation.modes = vec!["spiral".into()];
        let mut app = App::new(config);
        let (width, height) = (100, 80);
        let mut buffer = vec![0; (width * height * 4) as usize];
        let current = app
            .render_to_surface_cached(&mut buffer, width, height, 1, BufferContent::default())
            .unwrap();
        let Some(BaseFrame::Animation {
            playlist,
            generation,
        }) = current.content.base
        else {
            panic!("animation frame expected");
        };
        let stale = BufferContent {
            base: Some(BaseFrame::Animation {
                playlist,
                generation: generation.wrapping_sub(1),
            }),
            indicator: None,
        };

        buffer.fill(SENTINEL);
        let changed = app
            .render_to_surface_cached(&mut buffer, width, height, 1, stale)
            .unwrap();
        assert_eq!(changed.damage, Some(DamageRect::full(width, height)));
    }

    #[test]
    fn cached_render_indicator_transitions_are_table_driven_and_partial() {
        let mut config = Config::default();
        config.animation.modes.clear();
        let mut app = App::new(config);
        let (width, height) = (800, 600);
        let mut buffer = vec![0; (width * height * 4) as usize];

        let mut content = app
            .render_to_surface_cached(&mut buffer, width, height, 1, BufferContent::default())
            .unwrap()
            .content;
        let cases = [
            ("appear", Some(0.25)),
            ("move", Some(0.75)),
            ("disappear", None),
        ];

        for (name, x) in cases {
            if let Some(x) = x {
                app.config.indicator.x_position = x;
                app.auth_state = AuthState::Typing;
                app.last_interaction = Some(Instant::now());
            } else {
                app.auth_state = AuthState::Idle;
                app.last_interaction =
                    Some(Instant::now() - INDICATOR_IDLE_HIDE - Duration::from_millis(1));
            }
            buffer.fill(SENTINEL);
            let outcome = app
                .render_to_surface_cached(&mut buffer, width, height, 1, content)
                .unwrap();
            let expected = match (content.indicator, outcome.content.indicator) {
                (Some(old), Some(new)) => Some(old.union(new)),
                (Some(rect), None) | (None, Some(rect)) => Some(rect),
                (None, None) => None,
            };
            assert_eq!(outcome.damage, expected, "{name}");
            assert_untouched_outside(&buffer, width, expected);
            content = outcome.content;
        }
    }

    #[test]
    fn cached_render_rotating_buffers_submit_last_committed_indicator_damage() {
        let mut config = Config::default();
        config.animation.modes.clear();
        let mut app = App::new(config);
        let (width, height) = (800, 600);
        let mut buffers = [
            vec![0; (width * height * 4) as usize],
            vec![0; (width * height * 4) as usize],
        ];
        let mut contents = [BufferContent::default(); 2];
        for i in 0..2 {
            contents[i] = app
                .render_to_surface_cached(
                    &mut buffers[i],
                    width,
                    height,
                    1,
                    BufferContent::default(),
                )
                .unwrap()
                .content;
        }

        app.auth_state = AuthState::Typing;
        app.last_interaction = Some(Instant::now());
        app.config.indicator.x_position = 0.25;
        buffers[0].fill(SENTINEL);
        let first = app
            .render_to_surface_cached(&mut buffers[0], width, height, 1, contents[0])
            .unwrap();
        assert_untouched_outside(&buffers[0], width, first.damage);
        contents[0] = first.content;

        app.config.indicator.x_position = 0.75;
        buffers[1].fill(SENTINEL);
        let second = app
            .render_to_surface_cached(&mut buffers[1], width, height, 1, contents[1])
            .unwrap();
        assert_untouched_outside(&buffers[1], width, second.damage);

        app.auth_state = AuthState::Idle;
        app.last_interaction =
            Some(Instant::now() - INDICATOR_IDLE_HIDE - Duration::from_millis(1));
        buffers[0].fill(SENTINEL);
        let third = app
            .render_to_surface_cached(&mut buffers[0], width, height, 1, contents[0])
            .unwrap();
        assert_untouched_outside(&buffers[0], width, third.damage);

        let last_committed = second.content.indicator.expect("second indicator");
        let expected = third.damage.unwrap().union(last_committed);
        assert_eq!(
            crate::wayland::submitted_damage(third.damage, true, Some(last_committed)),
            Some(expected),
            "rotating back to buffer A must clear buffer B's on-screen indicator"
        );
    }
}
