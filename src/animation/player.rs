//! AnimationPlayer: the animation frame clock.
//!
//! Owns everything about driving an `Animation` forward in time — when the
//! next frame is due, how many ticks to catch up after a delayed wakeup,
//! the persistent pixel buffer, the render call, and the stride-safe copy
//! into a Wayland surface. Also owns the single per-mode `delay_us`
//! defaults table. Callers (the Wayland event loop, `App::render_to_surface`)
//! make no timing decisions of their own.

use super::{primitives, AnimConfig, AnimRegistry, Animation};
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
    /// default here — the single place that mapping lives, replacing the
    /// two duplicated (and drifted) match tables that used to live in
    /// `app.rs`.
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
    pub fn advance(&mut self, now: Instant) {
        /// Fastest wakeup cadence worth scheduling: ~60 fps, the panel
        /// rate. Rendering faster is invisible; modes with faster upstream
        /// clocks (lightning/petri/binaryring 10ms, discrete 1ms) keep
        /// their exact simulation rate via tick batching below.
        const MIN_FRAME_US: u64 = 16_666;

        let delay_us = self.animation.frame_delay_us();
        if delay_us == 0 {
            self.next_wake = None;
            return;
        }

        let floor_us = self.min_delay_us.max(MIN_FRAME_US);
        let (ticks_per_frame, frame_us) = if delay_us < floor_us {
            let n = floor_us.div_ceil(delay_us);
            (n, delay_us * n)
        } else {
            (1, delay_us)
        };

        let ticked = match self.last_tick {
            Some(last) => {
                let elapsed = now.duration_since(last).as_micros() as u64;
                if elapsed >= frame_us {
                    // Keep the absolute cadence when roughly on time, but
                    // re-anchor to `now` once a full frame behind so a
                    // backlog never forces back-to-back frames.
                    self.last_tick = Some(if elapsed >= 2 * frame_us {
                        now
                    } else {
                        last + Duration::from_micros(frame_us)
                    });
                    true
                } else {
                    false
                }
            }
            None => {
                self.last_tick = Some(now);
                true
            }
        };

        self.next_wake = Some(self.last_tick.unwrap() + Duration::from_micros(frame_us));

        let (Some((w, h)), Some(buf)) = (self.surface_size, self.buffer.as_mut()) else {
            return;
        };

        if self.animation.clears_each_frame() {
            if ticked {
                for _ in 0..ticks_per_frame {
                    self.animation.tick();
                }
            }
            // Skip the clear + render when nothing ticked and the buffer
            // already holds the current frame: keystroke/indicator redraws
            // call advance() far more often than most mode clocks fire, and
            // re-rendering an identical frame is pure waste.
            if ticked || self.dirty {
                primitives::clear_buffer(buf, self.background);
                self.animation.render(buf, w, h);
                self.dirty = false;
            }
        } else if ticked {
            // tick+render pairs: incremental modes queue draw ops per tick
            // and consume them in render, so the pairing must be preserved.
            for _ in 0..ticks_per_frame {
                self.animation.tick();
                self.animation.render(buf, w, h);
            }
        }
    }

    /// (Re)initializes the animation and buffer for a new surface size.
    /// A no-op if `width`x`height` matches the current size.
    pub fn ensure_sized(&mut self, width: u32, height: u32) {
        if self.surface_size == Some((width, height)) {
            return;
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
    }

    /// Copies the internal BGRA buffer into the raw `wl_shm` slice `dst`
    /// (also BGRA, stride `width * 4` — no padding). A no-op if `ensure_sized`
    /// hasn't run yet.
    pub fn blit_into(&self, dst: &mut [u8], width: i32, height: i32) -> Result<(), String> {
        let Some(buf) = &self.buffer else {
            return Ok(());
        };
        let n = (width * height * 4) as usize;
        if dst.len() >= n && buf.len() >= n {
            dst[..n].copy_from_slice(&buf[..n]);
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
    fn max_fps_batches_fast_modes() {
        // binaryring hardcodes a 10_000us (100 fps) clock; with max_fps = 60
        // the player renders every 2 ticks (20_000us frames, 50 fps <= cap)
        // so the simulation rate is preserved while rendering is capped.
        let params = AnimConfig {
            max_fps: 60,
            ..AnimConfig::default()
        };
        let mut player = AnimationPlayer::new("binaryring", params, (0.0, 0.0, 0.0, 1.0))
            .expect("binaryring is registered");
        player.advance(Instant::now());
        assert_eq!(
            player.next_wake.unwrap() - player.last_tick.unwrap(),
            Duration::from_micros(20_000)
        );
    }

    #[test]
    fn frame_floor_batches_without_slowing_simulation() {
        // Default max_fps = 0: binaryring's 10_000us clock is faster than
        // the 16_666us frame floor, so the player renders every 2 ticks
        // (20_000us frames) — simulation rate preserved, wakeups halved.
        let mut player =
            AnimationPlayer::new("binaryring", AnimConfig::default(), (0.0, 0.0, 0.0, 1.0))
                .expect("binaryring is registered");
        player.advance(Instant::now());
        assert_eq!(
            player.next_wake.unwrap() - player.last_tick.unwrap(),
            Duration::from_micros(20_000)
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
