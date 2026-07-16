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
    pub keyboard: crate::input::keyboard::KeyboardHandler,
    pub playlist: Option<Playlist>,
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

        let playlist = Playlist::new(
            &config.animation.modes,
            config.animation.cycle,
            config.animation.params.clone(),
            background_rgba,
        );
        if playlist.is_none() && !config.animation.modes.is_empty() {
            log::warn!("no valid animation modes; falling back to solid background");
        }

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
            config,
            keyboard: crate::input::keyboard::KeyboardHandler::new(),
            playlist,
            auth_verifier: None,
            keystroke_timestamps: Vec::new(),
            verification_start: None,
            auth_complete_time: None,
            segment_sequence,
        }
    }

    /// Render the current state into the raw `wl_shm` BGRA buffer `buf`
    /// (stride `width * 4`, no padding).
    ///
    /// Assumes `AnimationPlayer::advance` has already been called for this
    /// frame (see `WaylandState::draw`, which ticks the clock once per
    /// draw cycle, then calls this once per output).
    /// `width`/`height` are the physical buffer dimensions; `scale` is the
    /// output's integer buffer scale (1 on a standard display, 2+ on HiDPI).
    /// The background fills the buffer resolution-independently; only the
    /// indicator's pixel geometry needs `scale` so it keeps its logical size
    /// while rendering crisp at device resolution.
    pub fn render_to_surface(
        &mut self,
        buf: &mut [u8],
        width: i32,
        height: i32,
        scale: i32,
    ) -> Result<(), String> {
        // Only fill the background when there's no animation: blit_into
        // overwrites the full buffer anyway (ensure_sized just below
        // guarantees matching dimensions), so a fill before it is pure waste.
        if self.playlist.is_none() {
            let bg = self.config.background_color;
            crate::render::background::render_solid_color(buf, (bg.r, bg.g, bg.b, bg.a))?;
        }

        if let Some(ref mut player) = self.playlist {
            player.ensure_sized(width as u32, height as u32);
            player.blit_into(buf, width, height)?;
        }

        let now = Instant::now();
        // Draw the indicator only while it should be visible; when idle long
        // enough it hides entirely, leaving just the background.
        if self.indicator_visible(now) {
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
            crate::render::indicator::render_indicator(buf, width, height, &ctx, &style)?;
        }

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
