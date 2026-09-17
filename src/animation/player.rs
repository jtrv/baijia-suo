//! AnimationPlayer: the animation frame clock.
//!
//! Owns everything about driving an `Animation` forward in time — when the
//! next frame is due, how many ticks to catch up after a delayed wakeup,
//! the persistent pixel buffer, the render call, and the stride-safe copy
//! into a Wayland surface. Also owns the single per-mode `delay_us`
//! defaults table. Callers (the Wayland event loop, `App::render_to_surface`)
//! make no timing decisions of their own.

use super::{primitives, AnimConfig, AnimRegistry, Animation, RenderPolicy};
use crate::render::DamageRect;
use std::time::{Duration, Instant};

pub struct AnimationPlayer {
    animation: Box<dyn Animation>,
    /// Delay-resolved params used to (re)initialize `animation` on resize.
    base_params: AnimConfig,
    background: primitives::Color,
    surface_size: Option<(u32, u32)>,
    buffer: Option<Vec<u8>>,
    last_tick: Option<Instant>,
    next_wake: Option<Instant>,
    /// Floor on the frame delay, from config `max_fps`. 0 = uncapped, i.e.
    /// each mode's own clock (some ask for 100-1000 fps — binaryring,
    /// starfish, discrete — which on a 60 Hz panel is pure battery drain).
    min_delay_us: u64,
    /// The buffer was (re)created and doesn't hold a rendered frame yet.
    /// Forces one render even when no tick is due, so a redraw right after
    /// resize isn't blank. Cleared after that render.
    dirty: bool,
}

impl AnimationPlayer {
    /// Creates a player for `mode_name`, or `None` if the mode isn't
    /// registered. `params.delay_us == 0` is resolved to the mode's
    /// default here.
    pub fn new(
        mode_name: &str,
        mut params: AnimConfig,
        background_rgba: (f64, f64, f64, f64),
    ) -> Option<Self> {
        if params.delay_us == 0 {
            params.delay_us = Self::default_delay_us(mode_name);
        }
        let registry = AnimRegistry::new();
        let animation = registry.create(mode_name, &params)?;
        let min_delay_us = if params.max_fps > 0 {
            1_000_000 / params.max_fps as u64
        } else {
            0
        };
        Some(AnimationPlayer {
            animation,
            base_params: params,
            min_delay_us,
            background: rgba_to_color(background_rgba),
            surface_size: None,
            buffer: None,
            last_tick: None,
            next_wake: None,
            dirty: false,
        })
    }

    fn default_delay_us(mode: &str) -> u64 {
        match mode {
            "flame" => 750_000,
            "forest" => 400_000,
            "vines" => 200_000,
            "grav" | "hop" | "lissie" | "mountain" => 10_000,
            "discrete" => 1_000,
            "helix" => 25_000,
            "spiral" => 20_000,
            "qix" => 30_000,
            "worm" => 17_000,
            // blot.c ModStruct: delay 200000. (An earlier table entry had an
            // extra zero — blots lingered 10x too long.)
            "blot" => 200_000,
            // lisa.c / mandelbrot.c DEFAULTS: *delay: 25000. Without these
            // entries the generic 16_666 fallback ran both 1.5x fast (lisa
            // cycled to its degenerate dot figure noticeably more often).
            "lisa" | "mandelbrot" => 25_000,
            // petri.c: "*delay: 10000"; without this entry the generic
            // 16_666 fallback ran colony growth 1.67x slower than upstream.
            "petri" => 10_000,
            "pyro" => 15_000,
            "rain" => 35_000,
            "bubble" => 100_000,
            "lightning" => 10_000,
            _ => 16_666,
        }
    }

    /// Deadline the event loop should wake at to keep the animation
    /// running. `None` once the animation reports it isn't ticking
    /// (`frame_delay_us() == 0`).
    pub fn next_wake(&self) -> Option<Instant> {
        self.next_wake
    }

    /// The mode's current delay between ticks.
    pub fn frame_delay(&self) -> Duration {
        Duration::from_micros(self.animation.frame_delay_us())
    }

    /// Advance the clock to `now`: run the ticks for at most one rendered
    /// frame, render into the internal buffer, and recompute `next_wake`.
    ///
    /// Two rules, both aimed at matching the original xscreensaver loops:
    ///
    /// - At most one *frame* per wakeup: when a frame overruns its delay
    ///   budget the animation slows down smoothly instead of bursting
    ///   deficit-driven catch-up frames, which reads as stop-and-go motion
    ///   (seen on xrayswarm).
    /// - Modes whose clocks are faster than a wakeup can be serviced
    ///   (every wakeup is a full draw+blit+commit; sub-10ms clocks like
    ///   discrete's 1ms are unreachable) run a fixed number of ticks per
    ///   frame instead. Deterministic batching, not deficit catch-up, so
    ///   the simulation rate matches upstream without judder. The max_fps
    ///   cap raises the frame floor the same way: it caps rendering, never
    ///   the simulation speed.
    ///
    /// Safe to call before `ensure_sized` (e.g. the very first draw, before
    /// any output has reported its size): the clock still advances and
    /// `next_wake` still gets armed, but there's no buffer yet to render
    /// into, so the render step is skipped until a size is known.
    pub fn advance(&mut self, now: Instant) -> bool {
        /// A ~60 Hz refresh period. Mode clocks faster than this can't have
        /// every state presented anyway; clocks at or above it can.
        const REFRESH_US: u64 = 16_666;

        let delay_us = self.animation.frame_delay_us();
        if delay_us == 0 {
            self.next_wake = None;
            return false;
        }
        let interpolates = self.animation.interpolates();
        if interpolates && !self.dirty && self.next_wake.is_some_and(|wake| now < wake) {
            return false;
        }
        // The max_fps cap stretches the wakeup cadence, never the tick
        // arithmetic: capped fast modes catch up within each longer frame,
        // so rendering slows but simulation speed doesn't.
        let sched_us = delay_us.max(self.min_delay_us);

        // Two tick policies:
        //
        // - Sub-refresh clocks (upstream 10ms/1ms delays) run bounded
        //   elapsed-driven catch-up: the simulation holds its upstream rate
        //   and presentation samples it, with the natural phase drift a real
        //   display sampling upstream's 100fps draws would show. Capped so a
        //   long gap (display off) re-anchors instead of bursting.
        // - At-or-above-refresh clocks tick at most once per wakeup: every
        //   state is presentable, and when a frame overruns its budget the
        //   animation slows down smoothly instead of double-ticking, which
        //   reads as stop-and-go motion (seen on xrayswarm).
        let max_ticks: u64 = if interpolates || delay_us < REFRESH_US { 10 } else { 1 };

        // Catch-up ticks share a wall-clock budget: cheap ticks (discrete
        // plots 4096 points in microseconds) catch all the way up, while a
        // mode whose tick cost has grown (petri with a screen-spanning
        // colony front) stops early and drops the rest of the backlog.
        // Without the budget, catch-up compounds on heavy modes: a late
        // frame runs several heavy ticks, making the next frame later
        // still. Upstream never catches up at all — dropping backlog is
        // the faithful degradation.
        const TICK_BUDGET: Duration = Duration::from_millis(8);

        let (planned, last_anchor) = match self.last_tick {
            Some(last) => {
                let elapsed = now.duration_since(last).as_micros() as u64;
                if elapsed >= delay_us {
                    let n = (elapsed / delay_us).min(max_ticks);
                    // Anchor candidate keeps the absolute cadence when
                    // roughly on time; a backlog beyond the catch-up cap
                    // re-anchors to `now` instead.
                    let anchor = if elapsed >= (max_ticks + 1) * delay_us {
                        now
                    } else {
                        last + Duration::from_micros(n * delay_us)
                    };
                    (n, anchor)
                } else {
                    (0, last)
                }
            }
            None => (1, now),
        };

        let mut done: u64 = 0;
        let mut changed = false;
        if let (Some((w, h)), Some(buf)) = (self.surface_size, self.buffer.as_mut()) {
            let budget_start = Instant::now();
            match self.animation.render_policy() {
                RenderPolicy::Incremental => {
                    // tick+render pairs: incremental modes queue draw ops
                    // per tick and consume them in render, so the pairing
                    // must hold.
                    for _ in 0..planned {
                        self.animation.tick();
                        self.animation.render(buf, w, h);
                        changed = true;
                        done += 1;
                        if budget_start.elapsed() > TICK_BUDGET {
                            break;
                        }
                    }
                }
                policy => {
                    for _ in 0..planned {
                        self.animation.tick();
                        done += 1;
                        if budget_start.elapsed() > TICK_BUDGET {
                            break;
                        }
                    }
                    // Render once for the whole batch — CompleteFrame modes
                    // overwrite everything from their own canvas, so batching
                    // also skips their intermediate full-canvas copies. Skip
                    // entirely when nothing ticked and the buffer already
                    // holds the current frame: keystroke/indicator redraws
                    // call advance() far more often than most mode clocks
                    // fire, and re-rendering an identical frame is waste.
                    if done > 0 || self.dirty || interpolates {
                        if policy == RenderPolicy::ClearThenRender {
                            primitives::clear_buffer(buf, self.background);
                        }
                        if interpolates {
                            let fraction = if done < planned {
                                0.0
                            } else {
                                (now.duration_since(last_anchor).as_secs_f64()
                                    / (delay_us as f64 * 1e-6))
                                    .min(1.0)
                            };
                            self.animation.render_interpolated(buf, w, h, fraction);
                        } else {
                            self.animation.render(buf, w, h);
                        }
                        self.dirty = false;
                        changed = true;
                    }
                }
            }
        } else {
            // No buffer yet: clock-only advance, nothing to measure.
            done = planned;
        }

        let ticked = done > 0;
        self.last_tick = Some(if done < planned {
            // Budget hit — drop the remaining backlog so it can't pile up.
            now
        } else {
            last_anchor
        });
        self.next_wake = Some(self.last_tick.unwrap() + Duration::from_micros(sched_us));
        if interpolates {
            // The compositor paces interpolated frames; a second fixed clock
            // would beat against refresh and repeat frames even without jitter.
            self.next_wake = Some(now + Duration::from_micros(self.min_delay_us.max(1)));
        }

        // Variable-delay modes (moire's 5s finished-screen pause, coral,
        // abstractile, ...) change frame_delay_us() *inside* tick(). The
        // next wakeup must use the post-tick delay: scheduling with the
        // pre-tick value made the first chunk of a new moire pattern sit
        // for a full extra pause before the sweep continued.
        if ticked {
            let next_us = self.animation.frame_delay_us();
            if next_us != 0 && next_us != delay_us {
                let sched = next_us.max(self.min_delay_us);
                self.next_wake = Some(self.last_tick.unwrap() + Duration::from_micros(sched));
            }
        }
        changed
    }

    /// (Re)initializes the animation and buffer for a new surface size.
    /// A no-op if `width`x`height` matches the current size.
    pub fn ensure_sized(&mut self, width: u32, height: u32) -> bool {
        if self.surface_size == Some((width, height)) {
            return false;
        }

        let mut params = self.base_params.clone();
        params.width = width;
        params.height = height;
        self.animation.reset(&params);
        self.surface_size = Some((width, height));

        let buf_size = (width * height * 4) as usize;
        let mut buf = vec![0u8; buf_size];
        primitives::clear_buffer(&mut buf, self.background);
        self.buffer = Some(buf);
        self.dirty = true;
        true
    }

    /// Copies the internal BGRA buffer into the raw `wl_shm` slice `dst`
    /// (also BGRA, stride `width * 4` — no padding). A no-op if `ensure_sized`
    /// hasn't run yet.
    pub fn blit_into(&self, dst: &mut [u8], width: i32, height: i32) -> Result<(), String> {
        let Some(buf) = &self.buffer else {
            return Ok(());
        };
        let n = (width * height * 4) as usize;
        // Error rather than silently no-op on a size mismatch (matches
        // blit_rect_into). A silent skip here would be recorded upstream as
        // a fresh full frame, leaving stale pixels the cache then declines
        // to repaint.
        if dst.len() < n || buf.len() < n {
            return Err("animation buffer is smaller than its surface".into());
        }
        dst[..n].copy_from_slice(&buf[..n]);
        Ok(())
    }

    pub(crate) fn blit_rect_into(
        &self,
        dst: &mut [u8],
        width: i32,
        height: i32,
        rect: DamageRect,
    ) -> Result<(), String> {
        let Some(src) = &self.buffer else {
            return Ok(());
        };
        let needed = (width * height * 4) as usize;
        if dst.len() < needed || src.len() < needed {
            return Err("animation buffer is smaller than its surface".into());
        }
        let row_bytes = rect.width as usize * 4;
        for y in rect.y..rect.y + rect.height {
            let start = ((y * width + rect.x) * 4) as usize;
            dst[start..start + row_bytes].copy_from_slice(&src[start..start + row_bytes]);
        }
        Ok(())
    }
}

fn rgba_to_color(rgba: (f64, f64, f64, f64)) -> primitives::Color {
    primitives::Color::new(
        (rgba.3 * 255.0) as u8,
        (rgba.0 * 255.0) as u8,
        (rgba.1 * 255.0) as u8,
        (rgba.2 * 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TrackedIco {
        ico: super::super::modes::ico::Ico,
        ticks: u64,
    }

    impl Animation for TrackedIco {
        fn new(config: &AnimConfig) -> Self {
            Self {
                ico: super::super::modes::ico::Ico::new(config),
                ticks: 0,
            }
        }

        fn tick(&mut self) {
            self.ico.tick();
            self.ticks += 1;
        }

        fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
            self.ico.render(buffer, width, height);
            buffer[..8].copy_from_slice(&(self.ticks as f64).to_ne_bytes());
        }

        fn interpolates(&self) -> bool {
            self.ico.interpolates()
        }

        fn render_interpolated(&self, buffer: &mut [u8], width: u32, height: u32, fraction: f64) {
            self.ico
                .render_interpolated(buffer, width, height, fraction);
            let phase = self.ticks as f64 - 1.0 + fraction;
            buffer[..8].copy_from_slice(&phase.to_ne_bytes());
        }

        fn reset(&mut self, config: &AnimConfig) {
            self.ico.reset(config);
            self.ticks = 0;
        }

        fn render_policy(&self) -> RenderPolicy {
            self.ico.render_policy()
        }

        fn frame_delay_us(&self) -> u64 {
            self.ico.frame_delay_us()
        }
    }

    fn check_ico_cadence(hz: u64, jitter: &[i64]) {
        let config = AnimConfig {
            width: 80,
            height: 60,
            count: 1,
            ..AnimConfig::default()
        };
        let mut player = AnimationPlayer::new("ico", config.clone(), (0.0, 0.0, 0.0, 1.0)).unwrap();
        player.animation = Box::new(TrackedIco::new(&config));
        player.ensure_sized(config.width, config.height);
        let start = Instant::now();
        player.advance(start);
        let phase = |p: &AnimationPlayer| {
            f64::from_ne_bytes(p.buffer.as_ref().unwrap()[..8].try_into().unwrap())
        };
        let initial = phase(&player);
        let mut previous = initial;
        let mut previous_us = 0;
        let mut worst_error = 0.0_f64;
        for frame in 1..=240_u64 {
            let micros =
                (frame * 1_000_000 / hz) as i64 + jitter[(frame as usize - 1) % jitter.len()];
            player.advance(start + Duration::from_micros(micros as u64));
            let current = phase(&player);
            let expected = (micros - previous_us) as f64 / player.frame_delay().as_micros() as f64;
            worst_error = worst_error.max((current - previous - expected).abs());
            previous = current;
            previous_us = micros;
        }
        assert!(
            worst_error < 1e-8,
            "{hz} Hz: worst presentation step error {worst_error} ticks"
        );
        let expected = previous_us as f64 / player.frame_delay().as_micros() as f64;
        assert!((previous - initial - expected).abs() < 1e-8);
    }

    #[test]
    fn ico_presentation_is_continuous_at_60_hz() {
        check_ico_cadence(60, &[0]);
    }

    #[test]
    fn ico_presentation_is_continuous_with_jitter() {
        check_ico_cadence(60, &[0, -2500, 1700, -900, 2200, 0]);
        check_ico_cadence(144, &[0]);
        check_ico_cadence(144, &[0, -1000, 700, -400, 900, 0]);
    }

    #[test]
    fn ico_interpolation_respects_frame_cap_and_resumes_after_pause() {
        for max_fps in [0, 30] {
            let config = AnimConfig {
                max_fps,
                ..AnimConfig::default()
            };
            let mut player = AnimationPlayer::new("ico", config, (0.0, 0.0, 0.0, 1.0)).unwrap();
            player.ensure_sized(80, 60);
            let start = Instant::now();
            assert!(player.advance(start));
            let interval = Duration::from_micros(if max_fps == 0 { 1 } else { 33_333 });
            assert_eq!(player.next_wake(), Some(start + interval));
            assert!(!player.advance(start + interval - Duration::from_nanos(1)));
            assert!(player.advance(start + interval));
            let resumed = start + Duration::from_secs(60);
            assert!(player.advance(resumed));
            assert_eq!(player.last_tick, Some(resumed));
            assert_eq!(player.next_wake(), Some(resumed + interval));
        }
    }

    #[test]
    fn default_delay_matches_known_modes() {
        assert_eq!(AnimationPlayer::default_delay_us("flame"), 750_000);
        assert_eq!(AnimationPlayer::default_delay_us("worm"), 17_000);
        assert_eq!(
            AnimationPlayer::default_delay_us("totally-unknown-mode"),
            16_666
        );
    }

    #[test]
    fn max_fps_stretches_scheduling_not_simulation() {
        // binaryring hardcodes a 10_000us (100 fps) clock; with max_fps = 60
        // wakeups stretch to 16_666us while catch-up inside each frame
        // keeps the simulation at upstream rate.
        let params = AnimConfig {
            max_fps: 60,
            ..AnimConfig::default()
        };
        let mut player = AnimationPlayer::new("binaryring", params, (0.0, 0.0, 0.0, 1.0))
            .expect("binaryring is registered");
        player.advance(Instant::now());
        assert_eq!(
            player.next_wake.unwrap() - player.last_tick.unwrap(),
            Duration::from_micros(1_000_000 / 60)
        );
    }

    #[test]
    fn no_max_fps_runs_upstream_clocks_natively() {
        // Default max_fps = 0: binaryring's 10_000us clock sits at the
        // frame floor and runs 1:1, like upstream.
        let mut player =
            AnimationPlayer::new("binaryring", AnimConfig::default(), (0.0, 0.0, 0.0, 1.0))
                .expect("binaryring is registered");
        player.advance(Instant::now());
        assert_eq!(
            player.next_wake.unwrap() - player.last_tick.unwrap(),
            Duration::from_micros(10_000)
        );
    }

    #[test]
    fn advance_arms_next_wake_before_first_size_is_known() {
        let mut player =
            AnimationPlayer::new("spiral", AnimConfig::default(), (0.0, 0.0, 0.0, 1.0))
                .expect("spiral is registered");
        assert!(player.next_wake().is_none());
        player.advance(Instant::now());
        assert!(
            player.next_wake().is_some(),
            "clock should arm even without a known surface size"
        );
    }

    #[test]
    fn ensure_sized_is_idempotent_for_same_dimensions() {
        let mut player =
            AnimationPlayer::new("spiral", AnimConfig::default(), (0.0, 0.0, 0.0, 1.0))
                .expect("spiral is registered");
        player.ensure_sized(100, 100);
        let first_len = player.buffer.as_ref().unwrap().len();
        player.ensure_sized(100, 100);
        assert_eq!(player.buffer.as_ref().unwrap().len(), first_len);
    }
}
