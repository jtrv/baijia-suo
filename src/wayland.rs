use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wayland_client::backend::ObjectId;
use wayland_client::{
    protocol::{
        wl_buffer::WlBuffer, wl_callback, wl_callback::WlCallback, wl_compositor::WlCompositor,
        wl_keyboard::WlKeyboard, wl_output::WlOutput, wl_pointer::WlPointer,
        wl_registry::WlRegistry, wl_seat::WlSeat, wl_shm::WlShm, wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1::ExtSessionLockManagerV1,
    ext_session_lock_surface_v1::ExtSessionLockSurfaceV1, ext_session_lock_v1::ExtSessionLockV1,
};

use crate::app::App;
use crate::auth::AuthState;
use crate::render::pool::DoublePool;
use crate::render::DamageRect;

wayland_client::delegate_noop!(WaylandState: ignore WlCompositor);
wayland_client::delegate_noop!(WaylandState: ignore WlShm);
wayland_client::delegate_noop!(WaylandState: ignore WlShmPool);
wayland_client::delegate_noop!(WaylandState: ignore ExtSessionLockManagerV1);
wayland_client::delegate_noop!(WaylandState: ignore WlSurface);

/// Set to true by SIGTERM/SIGINT; checked in the event loop.
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_shutdown_signal(_: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

fn pollable_animation_wake(any_surface_ready: bool, wake: Option<Instant>) -> Option<Instant> {
    any_surface_ready.then_some(wake).flatten()
}

fn callback_should_draw(queued: bool, wake: Option<Instant>, now: Instant) -> bool {
    queued || wake.is_some_and(|deadline| now >= deadline)
}

fn should_unlock(auth_state: AuthState, auth_settled: bool) -> bool {
    auth_settled && matches!(auth_state, AuthState::Success)
}

pub(crate) fn submitted_damage(
    damage: Option<DamageRect>,
    same_generation: bool,
    last_committed_indicator: Option<DamageRect>,
) -> Option<DamageRect> {
    match (damage, same_generation) {
        // Partial update: the compositor compares the attached buffer
        // against the last committed one — a different buffer under pool
        // rotation — so the previously committed indicator must be
        // unioned into the declared region.
        (Some(d), true) => Some(last_committed_indicator.map_or(d, |last| d.union(last))),
        // Base changed: the damage is already full-frame.
        (Some(d), false) => Some(d),
        // This buffer is unchanged, but the last committed frame may still
        // show an indicator this buffer never held (indicator hidden while
        // the other buffer was on screen): commit once to erase it. None
        // only when the surface, too, has nothing to erase.
        (None, _) => last_committed_indicator,
    }
}

pub struct OutputInfo {
    pub output: WlOutput,
    pub wl_surface: Option<WlSurface>,
    pub surface: Option<ExtSessionLockSurfaceV1>,
    pub pool: Option<DoublePool>,
    pub configured: bool,
    pub width: u32,
    pub height: u32,
    /// Integer output scale (wl_output `scale` event); 1 unless HiDPI.
    pub scale: i32,
    /// Indicator pixels in the surface's last committed buffer. This is
    /// per-surface because the SHM pool rotates between buffer identities.
    pub last_committed_indicator: Option<DamageRect>,
    /// A committed frame's wl_callback hasn't fired yet. While true we
    /// don't commit again to this surface: the compositor paces us, and
    /// when the output is off/occluded the callback simply never comes —
    /// rendering (and, via the queue-and-disarm path, the animation clock)
    /// stops with it.
    pub frame_pending: bool,
}

pub struct WaylandState {
    pub qh: QueueHandle<WaylandState>,
    pub app: App,
    pub compositor: Option<WlCompositor>,
    pub shm: Option<WlShm>,
    pub lock_manager: Option<ExtSessionLockManagerV1>,
    pub session_lock: Option<ExtSessionLockV1>,
    pub outputs: HashMap<u32, OutputInfo>,
    // Seats keyed by registry global name so a removed seat can be cleaned up.
    pub seats: HashMap<u32, WlSeat>,
    // Keyed by seat id so a seat that re-announces or loses the capability
    // doesn't leave a duplicate (double keystrokes) or a stale proxy.
    pub keyboards: HashMap<ObjectId, WlKeyboard>,
    pub pointers: HashMap<ObjectId, WlPointer>,
    pub running: bool,
    /// Only true when auth succeeded; controls whether we call unlock_and_destroy.
    pub unlock_on_exit: bool,
    /// Set once the `Locked` event lands. Gates lock-surface creation so an
    /// output hotplugged mid-lock gets a surface (else that monitor shows the
    /// compositor's blank fallback instead of the locker).
    locked: bool,

    // Key repeat
    repeat_key: Option<u32>,      // Wayland keycode currently being held
    repeat_next: Option<Instant>, // when to fire the next synthetic repeat
    repeat_delay_ms: u32,         // initial delay before first repeat (ms)
    repeat_interval_ms: u32,      // interval between repeats (ms); 0 = disabled

    // Timers
    auth_clear_at: Option<Instant>, // clear auth-failed indicator after 3 s
    password_clear_at: Option<Instant>, // clear idle password after 10 s
    /// Next low-battery poll; None when the feature is off.
    battery_check_at: Option<Instant>,
    /// A redraw was requested while every surface still owed a frame
    /// callback; the next callback runs it.
    present_queued: bool,
    /// Next animation frame deadline. Sourced from `AnimationPlayer::next_wake`
    /// after every `draw()` — this field is just what the event loop's
    /// generic timer poll uses to decide when to fire; the player owns the
    /// actual timing decision.
    anim_wake_at: Option<Instant>,
    /// Whether a PAM message was showing last tick, to force one redraw when it
    /// times out so it clears even with nothing else animating.
    pam_msg_was_live: bool,
    /// Whether the indicator was visible last tick, to force one redraw when it
    /// hides after the idle window so it clears even with nothing animating.
    indicator_was_visible: bool,

    // --debug-timing accumulators (untouched unless the flag is set)
    timing_draws: u32,
    timing_presentations: u32,
    timing_skipped_presentations: u32,
    timing_gated_animation_wakeups: u32,
    timing_advance_us: u64,
    timing_present_us: u64,
    timing_report_at: Option<Instant>,
}

impl WaylandState {
    pub fn new(app: App, qh: QueueHandle<WaylandState>) -> Self {
        Self {
            qh,
            app,
            compositor: None,
            shm: None,
            lock_manager: None,
            session_lock: None,
            outputs: HashMap::new(),
            seats: HashMap::new(),
            keyboards: HashMap::new(),
            pointers: HashMap::new(),
            running: true,
            unlock_on_exit: false,
            locked: false,
            repeat_key: None,
            repeat_next: None,
            repeat_delay_ms: 400,
            repeat_interval_ms: 33, // ~30 repeats/s
            auth_clear_at: None,
            password_clear_at: None,
            battery_check_at: None,
            present_queued: false,
            anim_wake_at: None,
            pam_msg_was_live: false,
            indicator_was_visible: false,
            timing_draws: 0,
            timing_presentations: 0,
            timing_skipped_presentations: 0,
            timing_gated_animation_wakeups: 0,
            timing_advance_us: 0,
            timing_present_us: 0,
            timing_report_at: None,
        }
    }

    /// Give one output a lock surface. No-op until the session is locked and
    /// the compositor/lock are bound, or if the output already has one. Called
    /// for every output when `Locked` lands, and for a single output when one
    /// is hotplugged mid-lock — without this, a monitor connected while locked
    /// shows the compositor's blank fallback rather than the locker.
    fn create_lock_surface(&mut self, id: u32, qh: &QueueHandle<Self>) {
        if !self.locked {
            return;
        }
        let (Some(compositor), Some(lock)) = (self.compositor.clone(), self.session_lock.clone())
        else {
            return;
        };
        let Some(info) = self.outputs.get_mut(&id) else {
            return;
        };
        if info.surface.is_some() {
            return;
        }
        let wl_surface = compositor.create_surface(qh, ());
        let lock_surface = lock.get_lock_surface(&wl_surface, &info.output, qh, ());
        info.wl_surface = Some(wl_surface);
        info.surface = Some(lock_surface);
        info.last_committed_indicator = None;
    }

    fn prune_animation_playlists(&mut self) {
        let active: Vec<_> = self
            .outputs
            .values()
            .filter(|info| info.configured && info.width > 0 && info.height > 0)
            .map(|info| {
                let scale = info.scale.max(1) as u32;
                (info.width * scale, info.height * scale)
            })
            .collect();
        self.app.retain_animation_sizes(&active);
    }

    fn has_ready_surface(&self) -> bool {
        self.outputs.values().any(|info| {
            info.configured
                && !info.frame_pending
                && info.wl_surface.is_some()
                && info.width > 0
                && info.height > 0
        })
    }

    /// Render every configured output and commit its lock surface.
    ///
    /// Ticks the animation clock exactly once per call, regardless of what
    /// triggered the draw (the animation timer, a keystroke, a Wayland
    /// configure event) — `AnimationPlayer` is the single owner of "how
    /// many ticks have elapsed," so nothing else needs to reason about it.
    pub fn draw(&mut self) {
        let shm = match self.shm.clone() {
            Some(s) => s,
            None => return,
        };
        let qh = self.qh.clone();

        // Frame-callback pacing: if every configured surface still owes us
        // a callback, drawing now would outpace the compositor. Preserve the
        // animation deadline for the callback handler, but do not poll it
        // while no surface can accept a frame.
        if !self.has_ready_surface() {
            if self
                .outputs
                .values()
                .any(|i| i.configured && i.wl_surface.is_some())
            {
                self.present_queued = true;
            }
            return;
        }

        let t_start = self.app.config.debug_timing.then(Instant::now);

        // In low-power mode the animation clock stops entirely — no ticks,
        // no wakeups — until the battery check re-enables it.
        if self.app.low_power {
            self.anim_wake_at = None;
        } else {
            self.app.advance_animations(Instant::now());
            self.anim_wake_at = self.app.next_anim_wake();
        }

        let t_advanced = t_start.map(|_| Instant::now());

        let ids: Vec<u32> = self.outputs.keys().copied().collect();
        let mut presentations = 0;
        let mut skipped_presentations = 0;
        let mut present_us = 0;
        for id in ids {
            let (lw, lh, scale, wl_surface) = {
                let info = match self.outputs.get(&id) {
                    Some(i) if i.configured => i,
                    _ => continue,
                };
                if info.frame_pending {
                    // This surface still owes a callback; let it pick up the
                    // freshest state when that callback triggers a redraw.
                    self.present_queued = true;
                    continue;
                }
                let wl_surface = match info.wl_surface.clone() {
                    Some(s) => s,
                    None => continue,
                };
                (
                    info.width as i32,
                    info.height as i32,
                    info.scale.max(1),
                    wl_surface,
                )
            };
            if lw <= 0 || lh <= 0 {
                continue;
            }
            // The configure size is logical; render into a buffer scaled up to
            // device pixels and tell the compositor its scale, so HiDPI output
            // is crisp rather than upscaled from a 1x buffer.
            //
            // These dimensions come from the compositor (a trusted component,
            // but a broken one can send garbage). Reject anything whose device
            // buffer would overflow rather than panicking downstream — a
            // Validate the byte size here,
            // where the untrusted values enter, proves it fits for every
            // downstream allocation (pool, player), so those need no checks.
            let Some((pw, ph)) =
                lw.checked_mul(scale)
                    .zip(lh.checked_mul(scale))
                    .filter(|&(pw, ph)| {
                        pw.checked_mul(ph)
                            .and_then(|px| px.checked_mul(4))
                            .is_some()
                    })
            else {
                log::error!("draw: output {id} has invalid buffer dimensions; skipping");
                continue;
            };

            let info = self.outputs.get_mut(&id).unwrap();
            let last_committed_indicator = info.last_committed_indicator;
            if info.pool.is_none() {
                info.pool = Some(DoublePool::new());
            }
            let pool = info.pool.as_mut().unwrap();
            let buf = match pool.get_buffer(&shm, pw, ph, &qh) {
                Ok(b) => b,
                Err(e) => {
                    log::error!("draw: failed to acquire buffer: {}", e);
                    continue;
                }
            };
            let presentation_start = self.app.config.debug_timing.then(Instant::now);

            let previous = buf.content();
            // A failed render may have touched pixels before returning. Mark
            // the cache unknown first so a later retry necessarily repaints.
            buf.set_content(Default::default());
            let outcome =
                match self
                    .app
                    .render_to_surface_cached(buf.data_mut(), pw, ph, scale, previous)
                {
                    Ok(outcome) => outcome,
                    Err(e) => {
                        log::error!("draw: render failed: {}", e);
                        continue;
                    }
                };
            buf.set_content(outcome.content);
            let same_generation = previous.base == outcome.content.base;
            let Some(damage) =
                submitted_damage(outcome.damage, same_generation, last_committed_indicator)
            else {
                skipped_presentations += 1;
                continue;
            };

            // Request the frame callback before the commit that latches it;
            // its Done event clears frame_pending for this output.
            wl_surface.frame(&qh, id);
            wl_surface.set_buffer_scale(scale);
            wl_surface.attach(Some(buf.buffer()), 0, 0);
            wl_surface.damage_buffer(damage.x, damage.y, damage.width, damage.height);
            wl_surface.commit();
            buf.set_busy(true);
            let info = self.outputs.get_mut(&id).unwrap();
            info.frame_pending = true;
            info.last_committed_indicator = outcome.content.indicator;
            presentations += 1;
            if let Some(start) = presentation_start {
                present_us += start.elapsed().as_micros() as u64;
            }
        }

        // render_to_surface may have created a playlist after the advance
        // pass, so resync the event-loop deadline after all outputs render.
        if !self.app.low_power {
            self.anim_wake_at = self.app.next_anim_wake();
        }
        let gated_animation_wakeup = self.anim_wake_at.is_some() && !self.has_ready_surface();

        // --debug-timing: aggregate advance vs present cost, report ~1/s.
        if let (Some(start), Some(advanced)) = (t_start, t_advanced) {
            let end = Instant::now();
            self.timing_draws += 1;
            self.timing_presentations += presentations;
            self.timing_skipped_presentations += skipped_presentations;
            self.timing_gated_animation_wakeups += u32::from(gated_animation_wakeup);
            self.timing_advance_us += advanced.duration_since(start).as_micros() as u64;
            self.timing_present_us += present_us;
            let due = self.timing_report_at.is_none_or(|t| end >= t);
            if due {
                if (self.timing_presentations > 0
                    || self.timing_skipped_presentations > 0
                    || self.timing_gated_animation_wakeups > 0)
                    && self.timing_report_at.is_some()
                {
                    log::info!(
                        "timing: {} presentations/s, {} skipped, {} animation wakeups gated, advance avg {}us/draw, client render+commit avg {}us/presentation",
                        self.timing_presentations,
                        self.timing_skipped_presentations,
                        self.timing_gated_animation_wakeups,
                        self.timing_advance_us / self.timing_draws as u64,
                        self.timing_present_us / self.timing_presentations.max(1) as u64,
                    );
                }
                self.timing_draws = 0;
                self.timing_presentations = 0;
                self.timing_skipped_presentations = 0;
                self.timing_gated_animation_wakeups = 0;
                self.timing_advance_us = 0;
                self.timing_present_us = 0;
                self.timing_report_at = Some(end + Duration::from_secs(1));
            }
        }
    }

    /// Milliseconds until the next timer fires, or None if no timers are active.
    fn next_timer_ms(&self) -> Option<u64> {
        let now = Instant::now();
        let mut min_ms: Option<u64> = None;

        // The indicator owns its clock (see render::indicator's consts);
        // the loop just redraws at frame cadence while it reports activity.
        let indicator_timer = self
            .app
            .indicator_active(now)
            .then(|| now + Duration::from_millis(crate::render::indicator::FRAME_MS));
        let animation_timer = pollable_animation_wake(self.has_ready_surface(), self.anim_wake_at);

        for t in [
            self.repeat_next,
            self.auth_clear_at,
            self.password_clear_at,
            animation_timer,
            self.battery_check_at,
            indicator_timer,
            self.app.pam_message_deadline(now),
            self.app.indicator_hide_deadline(now),
        ]
        .into_iter()
        .flatten()
        {
            let ms = if t <= now {
                0
            } else {
                t.duration_since(now).as_millis() as u64
            };
            min_ms = Some(min_ms.map_or(ms, |prev| prev.min(ms)));
        }

        min_ms
    }

    /// Fire any timers whose deadline has passed. Each branch only marks
    /// that a redraw is needed; a single draw() at the end coalesces
    /// coinciding deadlines (e.g. the animation clock firing during a
    /// typing burst) into one frame instead of back-to-back full redraws.
    fn tick_timers(&mut self) {
        self.app.update_auth();
        let now = Instant::now();
        let mut needs_draw = false;

        // Key repeat
        if let Some(key) = self.repeat_key {
            if let Some(next) = self.repeat_next {
                if now >= next {
                    if let Some(codepoint) = self.app.keyboard.process_key(key, true) {
                        self.app.handle_key(codepoint);
                        self.password_clear_at = Some(Instant::now() + Duration::from_secs(10));
                        needs_draw = true;
                    }
                    if self.repeat_interval_ms > 0 {
                        // Advance by interval (catches up if we're running slow)
                        self.repeat_next =
                            Some(next + Duration::from_millis(self.repeat_interval_ms as u64));
                    } else {
                        self.repeat_next = None;
                    }
                }
            }
        }

        // Auth-failed indicator clear
        if let Some(t) = self.auth_clear_at {
            if now >= t {
                self.auth_clear_at = None;
                self.app.clear_auth_failed();
                needs_draw = true;
            }
        }

        // Idle password clear (security: zero password after 10 s inactivity)
        if let Some(t) = self.password_clear_at {
            if now >= t {
                self.password_clear_at = None;
                self.app.handle_key(0x1B); // Escape clears the password buffer
                needs_draw = true;
            }
        }

        // Animation frame: draw() re-ticks the clock and re-syncs
        // anim_wake_at internally, so there's nothing to do here beyond
        // deciding whether the deadline has passed.
        if self.has_ready_surface() && self.anim_wake_at.is_some_and(|t| now >= t) {
            needs_draw = true;
        }

        let auth_settled = self.app.auth_settled(now);
        if should_unlock(self.app.auth_state, auth_settled) {
            self.running = false;
            self.unlock_on_exit = true;
            return;
        } else if auth_settled
            && matches!(self.app.auth_state, AuthState::Invalid)
            && self.auth_clear_at.is_none()
        {
            self.auth_clear_at = Some(now + Duration::from_millis(3000));
        }

        // Redraw once when a PAM message times out, and once when the whole
        // indicator hides after the idle window — so each clears even if
        // nothing else is animating.
        let msg_live = self.app.pam_message(now).is_some();
        if self.pam_msg_was_live && !msg_live {
            needs_draw = true;
        }
        self.pam_msg_was_live = msg_live;

        let visible = self.app.indicator_visible(now);
        if self.indicator_was_visible && !visible {
            needs_draw = true;
        }
        self.indicator_was_visible = visible;

        if self.app.indicator_active(now) {
            needs_draw = true;
        }

        // Low-battery power saver: poll sysfs every 30 s; on a state flip
        // redraw once (suspending or resuming the animation).
        let threshold = self.app.config.low_battery_percent;
        if threshold > 0 && self.battery_check_at.is_none_or(|t| now >= t) {
            self.battery_check_at = Some(now + Duration::from_secs(30));
            let low = crate::app::battery_low(threshold);
            if low != self.app.low_power {
                self.app.low_power = low;
                needs_draw = true;
            }
        }

        if needs_draw {
            self.draw();
        }
    }

    /// Called on every key press to update repeat and idle timers.
    fn on_key_press(&mut self, key: u32, codepoint: u32) {
        // Repeatable: everything except Enter, Escape, Ctrl+C
        let repeatable = !matches!(codepoint, 0x0D | 0x0A | 0x1B | 0x03);
        if repeatable && self.repeat_interval_ms > 0 {
            self.repeat_key = Some(key);
            self.repeat_next =
                Some(Instant::now() + Duration::from_millis(self.repeat_delay_ms as u64));
        } else {
            self.repeat_key = None;
            self.repeat_next = None;
        }

        // Reset idle clear on every keystroke
        self.password_clear_at = Some(Instant::now() + Duration::from_secs(10));

        // Arm auth-failed clear if auth just failed; cancel it if user typed again
        if !matches!(self.app.auth_state, crate::auth::AuthState::Invalid) {
            self.auth_clear_at = None;
        }
    }
}

// ── Dispatch impls ────────────────────────────────────────────────────────────

impl Dispatch<WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_registry::Event;
        match event {
            Event::Global {
                name, interface, ..
            } => match &interface[..] {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind::<WlCompositor, _, _>(name, 4, qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind::<WlShm, _, _>(name, 1, qh, ()));
                }
                "ext_session_lock_manager_v1" => {
                    state.lock_manager =
                        Some(registry.bind::<ExtSessionLockManagerV1, _, _>(name, 1, qh, ()));
                }
                "wl_output" => {
                    let output = registry.bind::<WlOutput, _, _>(name, 4, qh, ());
                    state.outputs.insert(
                        name,
                        OutputInfo {
                            output,
                            wl_surface: None,
                            surface: None,
                            pool: None,
                            configured: false,
                            width: 0,
                            height: 0,
                            scale: 1,
                            last_committed_indicator: None,
                            frame_pending: false,
                        },
                    );
                    // If we're already locked, this output arrived mid-lock —
                    // give it a surface now (no-op otherwise).
                    state.create_lock_surface(name, qh);
                }
                "wl_seat" => {
                    let seat = registry.bind::<WlSeat, _, _>(name, 7, qh, ());
                    state.seats.insert(name, seat);
                }
                _ => {}
            },
            // A global went away. Drop whatever we tracked for it so we don't
            // draw to a dead output or hold released input proxies.
            Event::GlobalRemove { name } => {
                if let Some(mut info) = state.outputs.remove(&name) {
                    // Stop drawing to a dead output; the compositor blanks it.
                    if let Some(s) = info.surface.take() {
                        s.destroy();
                    }
                    if let Some(s) = info.wl_surface.take() {
                        s.destroy();
                    }
                    state.prune_animation_playlists();
                }
                if let Some(seat) = state.seats.remove(&name) {
                    let sid = seat.id();
                    if let Some(kb) = state.keyboards.remove(&sid) {
                        kb.release();
                    }
                    if let Some(ptr) = state.pointers.remove(&sid) {
                        ptr.release();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlBuffer, Arc<Mutex<bool>>> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlBuffer,
        event: <WlBuffer as wayland_client::Proxy>::Event,
        busy: &Arc<Mutex<bool>>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_buffer::Event::Release = event {
            *busy.lock().unwrap() = false;
        }
    }
}

impl Dispatch<WlOutput, ()> for WaylandState {
    fn event(
        state: &mut Self,
        output: &WlOutput,
        event: <WlOutput as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Track integer output scale so the lock surface renders at full
        // device resolution on HiDPI displays (applied via set_buffer_scale
        // in draw()). A scale change re-sizes the buffer on the next frame.
        if let wayland_client::protocol::wl_output::Event::Scale { factor } = event {
            let oid = output.id();
            if let Some(info) = state.outputs.values_mut().find(|i| i.output.id() == oid) {
                info.scale = factor.max(1);
            }
            // A scale change moves this output to a new physical size.
            state.prune_animation_playlists();
        }
    }
}

impl Dispatch<WlSeat, ()> for WaylandState {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: <WlSeat as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_seat::Event::Capabilities {
            capabilities: wayland_client::WEnum::Value(caps),
        } = event
        {
            use wayland_client::protocol::wl_seat::Capability;
            let sid = seat.id();

            // Track exactly one keyboard/pointer per seat, following the seat's
            // advertised capabilities: create on gain, release on loss.
            match (
                caps.contains(Capability::Keyboard),
                state.keyboards.contains_key(&sid),
            ) {
                (true, false) => {
                    state
                        .keyboards
                        .insert(sid.clone(), seat.get_keyboard(qh, ()));
                }
                (false, true) => {
                    if let Some(kb) = state.keyboards.remove(&sid) {
                        kb.release();
                    }
                }
                _ => {}
            }
            match (
                caps.contains(Capability::Pointer),
                state.pointers.contains_key(&sid),
            ) {
                (true, false) => {
                    state.pointers.insert(sid.clone(), seat.get_pointer(qh, ()));
                }
                (false, true) => {
                    if let Some(ptr) = state.pointers.remove(&sid) {
                        ptr.release();
                    }
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for WaylandState {
    fn event(
        _: &mut Self,
        pointer: &WlPointer,
        event: <WlPointer as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The cursor over a surface is undefined until the focused client
        // sets one; never setting it leaves stale cursor images ("ghosting")
        // over the lock surface. Hide it explicitly, like other lockers do.
        if let wayland_client::protocol::wl_pointer::Event::Enter { serial, .. } = event {
            pointer.set_cursor(serial, None, 0, 0);
        }
    }
}

impl Dispatch<WlKeyboard, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _: &WlKeyboard,
        event: <WlKeyboard as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_keyboard::Event;
        match event {
            Event::Keymap {
                format:
                    wayland_client::WEnum::Value(
                        wayland_client::protocol::wl_keyboard::KeymapFormat::XkbV1,
                    ),
                fd,
                size,
            } => {
                let _ = state
                    .app
                    .keyboard
                    .update_keymap(fd.as_raw_fd(), size as usize);
            }
            Event::Key {
                key,
                state: key_state,
                ..
            } => {
                let pressed = key_state
                    == wayland_client::WEnum::Value(
                        wayland_client::protocol::wl_keyboard::KeyState::Pressed,
                    );
                if pressed {
                    if let Some(codepoint) = state.app.keyboard.process_key(key, true) {
                        state.app.handle_key(codepoint);
                        state.on_key_press(key, codepoint);
                        state.draw();
                    }
                } else {
                    // Key release: cancel repeat for this key
                    if state.repeat_key == Some(key) {
                        state.repeat_key = None;
                        state.repeat_next = None;
                    }
                }
            }
            Event::Leave { .. } => {
                // Protocol: all keys are released on leave, so no key-up
                // event will arrive to stop an in-flight repeat.
                state.repeat_key = None;
                state.repeat_next = None;
            }
            Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                state.app.keyboard.update_modifiers(
                    mods_depressed,
                    mods_latched,
                    mods_locked,
                    group,
                );
                // Redraw so caps-lock colour takes effect immediately
                state.draw();
            }
            Event::RepeatInfo { rate, delay } => {
                state.repeat_delay_ms = delay.max(0) as u32;
                state.repeat_interval_ms = if rate > 0 { 1000 / rate as u32 } else { 0 };
                log::debug!(
                    "key repeat: delay={}ms rate={}ms",
                    delay,
                    state.repeat_interval_ms
                );
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtSessionLockV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _lock: &ExtSessionLockV1,
        event: <ExtSessionLockV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_v1::Event;
        match event {
            Event::Locked => {
                if state.compositor.is_none() {
                    log::error!("compositor not bound before lock acquired");
                    state.running = false;
                    return;
                }
                state.locked = true;
                let ids: Vec<u32> = state.outputs.keys().copied().collect();
                for id in ids {
                    state.create_lock_surface(id, qh);
                }
                log::info!(
                    "session lock acquired; created {} lock surface(s)",
                    state.outputs.len()
                );

                // Lock is held: tell whoever is waiting, then detach.
                if let Some(fd) = state.app.config.ready_fd.take() {
                    unsafe {
                        libc::write(fd, b"\n".as_ptr() as *const libc::c_void, 1);
                        libc::close(fd);
                    }
                }
                if state.app.config.daemonize {
                    state.app.config.daemonize = false;
                    // Safe to fork here: no verifier attempt threads exist
                    // until the first Enter press. The child keeps the
                    // Wayland fd; stdio goes to /dev/null.
                    if unsafe { libc::daemon(0, 0) } != 0 {
                        log::warn!("daemonize failed: {}", std::io::Error::last_os_error());
                    }
                }
            }
            Event::Finished => {
                log::error!("session lock finished — could not acquire lock");
                state.running = false;
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtSessionLockSurfaceV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        surface: &ExtSessionLockSurfaceV1,
        event: <ExtSessionLockSurfaceV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_surface_v1::Event;
        if let Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            surface.ack_configure(serial);
            for info in state.outputs.values_mut() {
                if info.surface.as_ref() == Some(surface) {
                    info.configured = true;
                    info.width = width;
                    info.height = height;
                    info.last_committed_indicator = None;
                    // The protocol requires a commit after ack_configure; a
                    // callback owed for the pre-configure buffer must not
                    // gate it (and may never fire across a mode switch).
                    info.frame_pending = false;
                }
            }
            log::info!("lock surface configured: {}x{}", width, height);
            // Sizes only change on configure/scale/removal events, so prune
            // here rather than in the 60fps draw path.
            state.prune_animation_playlists();
            // draw() sizes and ticks the animation player itself, arming
            // anim_wake_at as a side effect — no separate first-configure
            // case needed.
            state.draw();
        }
    }
}

/// Frame callbacks pace rendering to the compositor (user data = output
/// registry id). While a display is off or the surface occluded, its Done
/// event is simply withheld and the whole render path sleeps with it.
impl Dispatch<WlCallback, u32> for WaylandState {
    fn event(
        state: &mut Self,
        _: &WlCallback,
        event: <WlCallback as wayland_client::Proxy>::Event,
        output_id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            if let Some(info) = state.outputs.get_mut(output_id) {
                info.frame_pending = false;
            }
            if callback_should_draw(state.present_queued, state.anim_wake_at, Instant::now()) {
                state.present_queued = false;
                state.draw();
            }
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run_wayland(app: App) -> Result<(), Box<dyn std::error::Error>> {
    // Install signal handlers: SIGTERM/SIGINT set the shutdown flag.
    // The session stays locked after we exit — the compositor holds the lock.
    unsafe {
        // Cast via *const () to satisfy the lint; sighandler_t is a C function pointer alias.
        let handler = handle_shutdown_signal as *const () as libc::sighandler_t;
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }

    let conn = Connection::connect_to_env()?;
    let display = conn.display();
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    let _registry = display.get_registry(&qh, ());

    let mut state = WaylandState::new(app, qh);

    event_queue.roundtrip(&mut state)?;
    event_queue.roundtrip(&mut state)?;

    let lock_manager = state
        .lock_manager
        .clone()
        .ok_or("ext-session-lock-v1 not supported by compositor")?;
    state.session_lock = Some(lock_manager.lock(&state.qh, ()));

    // Poll-based event loop so timers fire without Wayland events.
    'main: while state.running && !SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
        // Dispatch any events already in the queue.
        event_queue.dispatch_pending(&mut state)?;
        if !state.running || SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
            break;
        }

        // Flush outgoing requests.
        conn.flush()?;

        // Prepare to read the socket, or dispatch if events are already pending.
        match event_queue.prepare_read() {
            None => {
                event_queue.dispatch_pending(&mut state)?;
            }
            Some(read_guard) => {
                // Block until the next real deadline, or indefinitely when
                // none is armed (idle locker, display off). poll wakes on
                // Wayland socket activity (POLLIN) or a signal (EINTR), and
                // the idle state has nothing else to service — auth results
                // ride the indicator timer during verification. The old
                // 100 ms ceiling was pure wakeup waste (~10/s while asleep,
                // preventing deeper CPU sleep for a locker left overnight).
                let timeout_ms = state
                    .next_timer_ms()
                    .map(|ms| ms.min(i32::MAX as u64) as libc::c_int)
                    .unwrap_or(-1);

                let wayland_fd = read_guard.connection_fd().as_raw_fd();
                let mut fds = [libc::pollfd {
                    fd: wayland_fd,
                    events: libc::POLLIN,
                    revents: 0,
                }];
                unsafe { libc::poll(fds.as_mut_ptr(), 1, timeout_ms) };

                let _ = read_guard.read();
                event_queue.dispatch_pending(&mut state)?;
            }
        }

        if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
            break 'main;
        }

        state.tick_timers();
    }

    // Only explicitly unlock if authentication succeeded.
    // On signal/error the compositor keeps the session locked.
    if state.unlock_on_exit {
        if let Some(lock) = &state.session_lock {
            lock.unlock_and_destroy();
        }
        event_queue.roundtrip(&mut state)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_without_comments(source: &str) -> String {
        let mut uncommented = String::with_capacity(source.len());
        let mut chars = source.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '/' && chars.peek() == Some(&'/') {
                chars.next();
                for character in chars.by_ref() {
                    if character == '\n' {
                        uncommented.push(character);
                        break;
                    }
                }
            } else if character == '/' && chars.peek() == Some(&'*') {
                chars.next();
                let mut previous = '\0';
                for character in chars.by_ref() {
                    if previous == '*' && character == '/' {
                        break;
                    }
                    previous = character;
                }
            } else {
                uncommented.push(character);
            }
        }
        uncommented
    }

    #[test]
    fn only_settled_success_unlocks() {
        for (auth_state, auth_settled, expected) in [
            (AuthState::Idle, false, false),
            (AuthState::Idle, true, false),
            (AuthState::Typing, false, false),
            (AuthState::Typing, true, false),
            (AuthState::Verifying, false, false),
            (AuthState::Verifying, true, false),
            (AuthState::Success, false, false),
            (AuthState::Success, true, true),
            (AuthState::Invalid, false, false),
            (AuthState::Invalid, true, false),
        ] {
            assert_eq!(should_unlock(auth_state, auth_settled), expected);
        }
    }

    #[test]
    fn session_unlock_has_one_authentication_gated_path() {
        // This protects the invariant that only settled successful authentication releases the session lock.
        let source = source_without_comments(include_str!("wayland.rs"));
        for needle in [
            concat!("unlock_and_", "destroy"),
            concat!("unlock_on_exit = ", "true"),
        ] {
            assert_eq!(
                source.matches(needle).count(),
                1,
                "{needle} must have exactly one non-comment occurrence to preserve the single authentication-gated unlock path; update this test deliberately if that invariant legitimately changes"
            );
        }
    }

    #[test]
    fn animation_deadline_waits_for_a_ready_surface() {
        let wake = Instant::now();
        assert_eq!(pollable_animation_wake(false, Some(wake)), None);
        assert_eq!(pollable_animation_wake(true, Some(wake)), Some(wake));
    }

    #[test]
    fn frame_callback_resumes_queued_or_due_work() {
        let now = Instant::now();
        let future = now + Duration::from_secs(1);
        assert!(callback_should_draw(true, Some(future), now));
        assert!(callback_should_draw(false, Some(now), now));
        assert!(!callback_should_draw(false, Some(future), now));
        assert!(!callback_should_draw(false, None, now));
    }

    #[test]
    fn submitted_damage_covers_the_last_committed_rotated_buffer() {
        let current = DamageRect {
            x: 20,
            y: 20,
            width: 10,
            height: 10,
        };
        let last_committed = DamageRect {
            x: 60,
            y: 20,
            width: 30,
            height: 10,
        };

        assert_eq!(
            submitted_damage(Some(current), true, Some(last_committed)),
            Some(current.union(last_committed))
        );
        assert_eq!(
            submitted_damage(Some(current), false, Some(last_committed)),
            Some(current),
            "full-generation damage already covers the prior surface"
        );
        // The indicator hid while the OTHER buffer was on screen: this
        // buffer has nothing to redraw, but the surface still shows the
        // indicator — one commit must erase it.
        assert_eq!(
            submitted_damage(None, true, Some(last_committed)),
            Some(last_committed)
        );
        assert_eq!(submitted_damage(None, true, None), None);
    }
}
