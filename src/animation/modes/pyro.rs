//! Fireworks.
//
// Copyright (c) 1991 by Patrick J. Naughton.
//
// Permission to use, copy, modify, and distribute this software and its
// documentation for any purpose and without fee is hereby granted,
// provided that the above copyright notice appear in all copies and that
// both that copyright notice and this permission notice appear in
// supporting documentation.
//
// This file is provided AS IS with no warranties of any kind.  The author
// shall have no liability with respect to the infringement of copyrights,
// trade secrets or any patents by this file or any part thereof.  In no
// event will the author be liable for any lost revenue or profits or
// other special, indirect and consequential damages.
//
// Rust port of xlockmore/modes/pyro.c; deviations noted below.

// 1-to-1 port of xlockmore's pyro.c (2D mode only — no 3D / use3d support).
//
// xlockmore defaults (from `ModStruct pyro_description`):
//     delay=15000, count=100, cycles=1, size=-3, ncolors=64
// Defaults handled at the bottom of `reset()` using `config.* == 0` as the
// "unset" sentinel — that's how the rest of this project's ports do it.

use crate::rng::RngExt;
use std::f32::consts::PI;

use crate::animation::primitives::{put_pixel, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const TWOPI: f32 = 2.0 * PI;

const MINROCKETS: i32 = 1;
const MINSIZE: i32 = 1;

// State machine for a single rocket.
const STATE_SILENT: u8 = 0;
const STATE_REDGLARE: u8 = 1;
const STATE_BURSTINGINAIR: u8 = 2;

// Shell types — bit-flags, matching the C `shelltype` field.
const TYPE_CLOUD: u8 = 0;
const TYPE_DOUBLECLOUD: u8 = 1;
const TYPE_COLORCLOUD: u8 = 2;

// Probability denominators (reciprocal of probability per cycle).
const P_IGNITE: u32 = 5000;
const P_DOUBLECLOUD: u32 = 10;
const P_COLORCLOUD: u32 = 5;
const P_MULTI: u32 = 75;
const P_FUSILLADE: u32 = 250;

const ROCKETW: i32 = 2;
const ROCKETH: i32 = 4;
const XVELFACTOR: f32 = 0.0025;
const MINYVELFACTOR: f32 = 0.016;
const MAXYVELFACTOR: f32 = 0.018;
const GRAVFACTOR: f32 = 0.0002;
const MINFUSE: i32 = 50;
const MAXFUSE: i32 = 100;

const FUSILFACTOR: u32 = 10;
const FUSILLEN: i32 = 100;

const SVELFACTOR: f32 = 0.1;
const BUOYANCY: f32 = 0.2;
const MAXSTARS: usize = 150;
const MINSTARS: usize = 50;
const MINSFUSE: i32 = 50;
const MAXSFUSE: i32 = 100;

// xlockmore: `#define ORANGE (5 * MI_NPIXELS(mi) / 64)` — palette index for the
// rocket pixel color. With the default 64-color HSV palette this is a saturated
// orange.
fn orange_color(ncolors: i32) -> Color {
    if ncolors > 3 {
        let pix = (5 * ncolors / 64).max(0) as f32;
        Color::from_hsl(pix / ncolors as f32, 1.0, 0.5)
    } else {
        Color::new(255, 255, 255, 255)
    }
}

// Index a uniformly-spaced HSV palette of size `ncolors`, matching
// xlockmore's MI_PIXEL/MI_NPIXELS scheme as closely as we can without an
// actual indexed palette.
fn palette_color(idx: i32, ncolors: i32) -> Color {
    let n = ncolors.max(1);
    let i = ((idx % n) + n) % n;
    Color::from_hsl(i as f32 / n as f32, 1.0, 0.5)
}

#[derive(Clone, Copy)]
struct Star {
    color: Color,
    sx: f32,
    sy: f32,
    sxvel: f32,
    syvel: f32,
}

impl Default for Star {
    fn default() -> Self {
        Star {
            color: Color::new(255, 0, 0, 0),
            sx: 0.0,
            sy: 0.0,
            sxvel: 0.0,
            syvel: 0.0,
        }
    }
}

#[derive(Clone)]
struct Rocket {
    state: u8,
    shelltype: u8,
    fuse: i32,
    xvel: f32,
    yvel: f32,
    x: f32,
    y: f32,
    color: [Color; 2],
    nstars: usize,
    stars: Vec<Star>,
    // xlockmore generates `NRAND(4)` flicker every shootup call. We compute it
    // in tick() so render() stays a pure function of state.
    flicker: i32,
}

impl Rocket {
    fn new() -> Self {
        Rocket {
            state: STATE_SILENT,
            shelltype: TYPE_CLOUD,
            fuse: 0,
            xvel: 0.0,
            yvel: 0.0,
            x: 0.0,
            y: 0.0,
            color: [Color::new(255, 0, 0, 0); 2],
            nstars: 0,
            stars: vec![Star::default(); MAXSTARS],
            flicker: 0,
        }
    }
}

pub struct Pyro {
    p_ignite: u32,
    orig_p_ignite: u32,
    rockpixel: Color,
    nrockets: usize,
    nflying: usize,
    fusilcount: i32,
    width: u32,
    height: u32,
    lmargin: f32,
    rmargin: f32,
    star_size: i32,
    minvelx: f32,
    maxvelx: f32,
    minvely: f32,
    maxvely: f32,
    // xlockmore keeps `maxsvel` SIGNED — it is `minvely * SVELFACTOR`, and
    // since `minvely` is negative `maxsvel` is also negative. The radius `r`
    // for star velocities is then `FLOATRAND(0, maxsvel)`, i.e. negative.
    // The visual result is unchanged because `r` is multiplied by `cos(theta)`
    // / `sin(theta)` with `theta` uniform on [0, 2π), but we preserve the sign
    // for byte-for-byte parity.
    maxsvel: f32,
    rockdecel: f32,
    stardecel: f32,
    rockq: Vec<Rocket>,
    delay_us: u64,
    ncolors: i32,
    just_started: bool,
}

impl Pyro {
    fn fill_rect(
        buffer: &mut [u8],
        width: u32,
        height: u32,
        x: i32,
        y: i32,
        rw: i32,
        rh: i32,
        color: Color,
    ) {
        for dy in 0..rh {
            for dx in 0..rw {
                put_pixel(buffer, width, height, x + dx, y + dy, color);
            }
        }
    }

    // xlockmore `FLOATRAND(min, max)` = `min + rand_in_[0,1) * (max - min)`.
    // Matches the C macro exactly (range is `[min, max)` — note that `min` may
    // be numerically larger than `max`, in which case the range is `(max, min]`).
    fn floatrand(rng: &mut impl RngExt, min: f32, max: f32) -> f32 {
        min + rng.random::<f32>() * (max - min)
    }

    // xlockmore `INTRAND(min, max)` = `NRAND(max+1-min) + min` — inclusive on
    // both ends.
    fn intrand(rng: &mut impl RngExt, min: i32, max: i32) -> i32 {
        if max < min {
            return min;
        }
        rng.random_range(min..=max)
    }

    fn ignite(&mut self, rng: &mut impl RngExt) {
        let x = rng.random_range(0..self.width) as f32;
        let mut xvel = Self::floatrand(rng, -self.maxvelx, self.maxvelx);
        // "All this to stop too many rockets going offscreen:"
        if (x < self.lmargin && xvel < 0.0) || (x > self.rmargin && xvel > 0.0) {
            xvel = -xvel;
        }
        let yvel = Self::floatrand(rng, self.minvely, self.maxvely);
        let fuse = Self::intrand(rng, MINFUSE, MAXFUSE);
        let nstars = Self::intrand(rng, MINSTARS as i32, MAXSTARS as i32) as usize;

        let (c1, c2) = if self.ncolors > 2 {
            let pix = rng.random_range(0..self.ncolors);
            let pix2 = (pix + self.ncolors / 2) % self.ncolors;
            (palette_color(pix, self.ncolors), palette_color(pix2, self.ncolors))
        } else {
            let white = Color::new(255, 255, 255, 255);
            (white, white)
        };

        // xlockmore: `if (NRAND(P_DOUBLECLOUD) == 0) shelltype = DOUBLECLOUD;
        //             else { shelltype = CLOUD; if (NRAND(P_MULTI) == 0) multi = INTRAND(5,15); }`
        // Note `multi` is only randomised in the non-DOUBLECLOUD branch.
        let mut multi: i32 = 1;
        let mut shelltype: u8 = if rng.random_range(0..P_DOUBLECLOUD) == 0 {
            TYPE_DOUBLECLOUD
        } else {
            if rng.random_range(0..P_MULTI) == 0 {
                multi = Self::intrand(rng, 5, 15);
            }
            TYPE_CLOUD
        };
        if rng.random_range(0..P_COLORCLOUD) == 0 {
            shelltype |= TYPE_COLORCLOUD;
        }

        // xlockmore walks `rp` forward through `rockq` once, claiming each
        // SILENT slot it encounters until `multi` rockets have been spawned or
        // `nflying == nrockets`. The `multi` loop in C is `while (multi--)`,
        // which loops the same number of times as our `for m in (0..multi).rev()`.
        let mut idx = 0usize;
        for m in (0..multi).rev() {
            if self.nflying >= self.nrockets {
                return;
            }
            while idx < self.rockq.len() && self.rockq[idx].state != STATE_SILENT {
                idx += 1;
            }
            if idx >= self.rockq.len() {
                return;
            }
            self.nflying += 1;
            let rp = &mut self.rockq[idx];
            rp.shelltype = shelltype;
            rp.state = STATE_REDGLARE;
            rp.color[0] = c1;
            rp.color[1] = c2;
            rp.xvel = xvel;
            rp.yvel = Self::floatrand(rng, yvel * 0.97, yvel * 1.03);
            rp.fuse = Self::intrand(rng, fuse * 90 / 100, fuse * 110 / 100);
            rp.x = x + Self::floatrand(rng, m as f32 * 7.6, m as f32 * 8.4);
            rp.y = (self.height as i32 - 1) as f32;
            rp.nstars = nstars;
            rp.flicker = 0;
        }
    }
}

impl Animation for Pyro {
    fn new(config: &AnimConfig) -> Self {
        let mut pyro = Pyro {
            p_ignite: 0,
            orig_p_ignite: 0,
            rockpixel: Color::new(255, 255, 255, 255),
            nrockets: 0,
            nflying: 0,
            fusilcount: 0,
            width: config.width,
            height: config.height,
            lmargin: 0.0,
            rmargin: 0.0,
            star_size: 1,
            minvelx: 0.0,
            maxvelx: 0.0,
            minvely: 0.0,
            maxvely: 0.0,
            maxsvel: 0.0,
            rockdecel: 0.0,
            stardecel: 0.0,
            rockq: Vec::new(),
            delay_us: 15_000,
            ncolors: config.ncolors,
            just_started: true,
        };
        pyro.reset(config);
        pyro
    }

    fn tick(&mut self) {
        let mut rng = crate::rng::rng();

        // Ignition step — mirrors the top of xlockmore's `draw_pyro`.
        if self.just_started || (self.p_ignite > 0 && rng.random_range(0..self.p_ignite) == 0) {
            self.just_started = false;
            if rng.random_range(0..P_FUSILLADE) == 0 {
                self.p_ignite = (self.orig_p_ignite / FUSILFACTOR).max(1);
                self.fusilcount = Self::intrand(&mut rng, FUSILLEN * 9 / 10, FUSILLEN * 11 / 10);
            }
            self.ignite(&mut rng);
            if self.fusilcount > 0 {
                self.fusilcount -= 1;
                if self.fusilcount == 0 {
                    self.p_ignite = self.orig_p_ignite;
                }
            }
        }

        let sd = self.stardecel;
        let rd = self.rockdecel;
        let ncolors = self.ncolors;
        let maxsvel = self.maxsvel;

        for rp in self.rockq.iter_mut() {
            // REDGLARE → BURSTINGINAIR (or advance the rocket)
            if rp.state == STATE_REDGLARE {
                // xlockmore: `if (rp->fuse-- <= 0) { state = BURSTINGINAIR; return; }`
                // (post-decrement: compare *then* decrement)
                let fuse_was = rp.fuse;
                rp.fuse -= 1;
                if fuse_was <= 0 {
                    rp.state = STATE_BURSTINGINAIR;
                    // Initialise stars at the rocket's current position. xlockmore
                    // sets `stars[*].sx = sy = 0` and assigns random velocities; the
                    // burst step that follows in the same tick advances them once
                    // before drawing.
                    for starn in 0..rp.nstars {
                        rp.stars[starn].sx = 0.0;
                        rp.stars[starn].sy = 0.0;

                        // xlockmore only assigns `stars[*].color` when this is a
                        // COLORCLOUD shell — non-COLORCLOUD shells draw every
                        // star in `rp->color[0]` directly, so the per-star
                        // color field is left alone.
                        if (rp.shelltype & TYPE_COLORCLOUD) != 0 && ncolors > 2 {
                            if rng.random_range(0..6) < 1 {
                                rp.stars[starn].color = Color::new(255, 255, 255, 255);
                            } else {
                                let idx = rng.random_range(0..ncolors);
                                rp.stars[starn].color = palette_color(idx, ncolors);
                            }
                        }

                        let r = Self::floatrand(&mut rng, 0.0, maxsvel);
                        let theta = Self::floatrand(&mut rng, 0.0, TWOPI);
                        rp.stars[starn].sxvel = r * theta.cos();
                        rp.stars[starn].syvel = r * theta.sin();
                    }
                    rp.fuse = Self::intrand(&mut rng, MINSFUSE, MAXSFUSE);
                } else {
                    rp.x += rp.xvel;
                    rp.y += rp.yvel;
                    rp.yvel += rd;
                    rp.flicker = rng.random_range(0..4);
                }
            }

            // BURSTINGINAIR — note this can be entered in the same tick as the
            // REDGLARE→BURSTINGINAIR transition above, matching xlockmore's
            // `animate()` control flow.
            if rp.state == STATE_BURSTINGINAIR {
                let fuse_was = rp.fuse;
                rp.fuse -= 1;
                if fuse_was <= 0 {
                    rp.state = STATE_SILENT;
                    if self.nflying > 0 {
                        self.nflying -= 1;
                    }
                    continue;
                }
                // "Stagger the stars' decay" — once we're near the end of the
                // shell's fuse, shed 10% of the stars per tick.
                if rp.fuse <= 7 {
                    rp.nstars = rp.nstars * 90 / 100;
                    if rp.nstars == 0 {
                        continue;
                    }
                }

                let nstars = rp.nstars;
                for starn in 0..nstars {
                    rp.stars[starn].sx += rp.stars[starn].sxvel;
                    rp.stars[starn].sy += rp.stars[starn].syvel;
                    rp.stars[starn].syvel += sd;
                }
                // Rocket center drifts with its own velocity; the burst phase
                // uses `stardecel` (buoyancy) rather than `rockdecel`.
                rp.x += rp.xvel;
                rp.y += rp.yvel;
                rp.yvel += sd;
            }
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for rp in &self.rockq {
            if rp.state == STATE_REDGLARE {
                let h = ROCKETH + rp.flicker;
                Self::fill_rect(
                    buffer,
                    width,
                    height,
                    rp.x as i32,
                    rp.y as i32,
                    ROCKETW,
                    h,
                    self.rockpixel,
                );
            } else if rp.state == STATE_BURSTINGINAIR {
                let is_double = (rp.shelltype & TYPE_DOUBLECLOUD) != 0;
                let is_color = (rp.shelltype & TYPE_COLORCLOUD) != 0 && self.ncolors > 2;

                // xlockmore captures `rx, ry = rp->x, rp->y` *before* advancing
                // the rocket center. We did the advance in tick(), so back it
                // out here.
                let rx = rp.x - rp.xvel;
                let ry = rp.y - rp.yvel;

                for starn in 0..rp.nstars {
                    let star = &rp.stars[starn];
                    let color0 = if is_color { star.color } else { rp.color[0] };

                    let x0 = (rx + star.sx) as i32;
                    let y0 = (ry + star.sy) as i32;
                    Self::fill_rect(
                        buffer,
                        width,
                        height,
                        x0,
                        y0,
                        self.star_size,
                        self.star_size,
                        color0,
                    );

                    if is_double {
                        // xlockmore: `(int)(rx + 1.7 * sx)` — 1.7× radial offset
                        // for the second (outer) cloud layer.
                        let x1 = (rx + 1.7 * star.sx) as i32;
                        let y1 = (ry + 1.7 * star.sy) as i32;
                        Self::fill_rect(
                            buffer,
                            width,
                            height,
                            x1,
                            y1,
                            self.star_size,
                            self.star_size,
                            rp.color[1],
                        );
                    }
                }
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = crate::rng::rng();

        self.width = config.width;
        self.height = config.height;
        self.lmargin = (self.width / 16) as f32;
        self.rmargin = (self.width - (self.width / 16)) as f32;

        // xlockmore defaults: count=100, size=-3, ncolors=64, delay=15000.
        // We treat `0` as the "unspecified" sentinel for count/size/delay.
        let count_in = if config.count == 0 { 100 } else { config.count };
        let size_in = if config.size == 0 { -3 } else { config.size };
        self.ncolors = if config.ncolors <= 0 { 64 } else { config.ncolors };
        self.delay_us = if config.delay_us == 0 { 15_000 } else { config.delay_us };

        // Same nrockets logic as `init_pyro`:
        //   count < -MIN: random in [MIN, -count]
        //   else if count < MIN: clamp up to MIN
        //   else: count as-is
        let nrockets = if count_in < -MINROCKETS {
            rng.random_range(0..(-count_in - MINROCKETS + 1)) + MINROCKETS
        } else if count_in < MINROCKETS {
            MINROCKETS
        } else {
            count_in
        };
        self.nrockets = nrockets as usize;

        // star_size — direct port of xlockmore's branched formula.
        let min_dim = (self.width.min(self.height)) as i32;
        self.star_size = if size_in < -MINSIZE {
            let cap = (-size_in).min(MINSIZE.max(min_dim / 64));
            rng.random_range(0..(cap - MINSIZE + 1).max(1)) + MINSIZE
        } else if size_in < MINSIZE {
            // xlockmore: `if (!size) MAX(MINSIZE, MIN(w,h)/64); else MINSIZE;`
            // The `size_in == 0` arm is unreachable in practice because we
            // substituted `-3` for `0` above, but it is kept for parity in
            // case a caller pushes `size == 0` past the sentinel.
            if size_in == 0 {
                MINSIZE.max(min_dim / 64)
            } else {
                MINSIZE
            }
        } else {
            size_in.min(MINSIZE.max(min_dim / 64))
        };

        self.orig_p_ignite = (P_IGNITE / self.nrockets as u32).max(1);
        self.p_ignite = self.orig_p_ignite;

        self.rockq.clear();
        self.rockq.resize_with(self.nrockets, Rocket::new);
        self.nflying = 0;
        self.fusilcount = 0;

        self.rockpixel = orange_color(self.ncolors);

        // Geometry-dependent physical data — directly mirrors `init_pyro`.
        self.maxvelx = self.width as f32 * XVELFACTOR;
        self.minvelx = -self.maxvelx;
        self.minvely = -(self.height as f32) * MINYVELFACTOR;
        self.maxvely = -(self.height as f32) * MAXYVELFACTOR;
        // Intentionally *signed* — see comment on the `maxsvel` field.
        self.maxsvel = self.minvely * SVELFACTOR;
        self.rockdecel = self.height as f32 * GRAVFACTOR;
        self.stardecel = self.rockdecel * BUOYANCY;
        self.just_started = true;
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
