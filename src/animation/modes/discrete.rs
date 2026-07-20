//! Fractals based on discrete-map dynamical systems.
//
// Copyright (c) 1996 by Tim Auckland <tda10.geo@yahoo.com>
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
// Rust port of xlockmore/modes/discrete.c.

// "discrete" shows fractals based on discrete-map dynamical systems:
// SQRT (hopalong variant), BIRDIE, STANDARD, TRIG, CUBIC, HENON,
// AILUJ (inverse Julia), HSHOE (horseshoe), and DELOG.

use crate::animation::primitives::{clear_buffer, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};
use rand::Rng;
use std::f64::consts::PI;

// xlockmore defaults from discrete.c DEFAULTS block:
//   *count:  4096
//   *cycles: 2500
//   *ncolors: 100
const DEFAULT_COUNT: i32 = 4096;
const DEFAULT_CYCLES: i32 = 2500;
const DEFAULT_NCOLORS: i32 = 100;

/// xlockmore MAXRAND = 2^31 (used as the float divisor for LRAND())
const MAXRAND: f64 = 2_147_483_648.0;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FType {
    Sqrt,
    Birdie,
    Standard,
    Trig,
    Cubic,
    Henon,
    Ailuj,
    Hshoe,
    Delog,
}

/// 18-entry bias table — exact match to C `bias[BIASES]`:
/// STANDARD×4, SQRT×4, BIRDIE×3, AILUJ×3, TRIG×2, CUBIC×1, HENON×1
const BIAS: [FType; 18] = [
    FType::Standard, FType::Standard, FType::Standard, FType::Standard,
    FType::Sqrt,     FType::Sqrt,     FType::Sqrt,     FType::Sqrt,
    FType::Birdie,   FType::Birdie,   FType::Birdie,
    FType::Ailuj,    FType::Ailuj,    FType::Ailuj,
    FType::Trig,     FType::Trig,
    FType::Cubic,
    FType::Henon,
];

#[derive(Clone, Copy)]
struct Point {
    x: i32,
    y: i32,
}

pub struct Discrete {
    maxx: i32,
    maxy: i32,

    // Attractor parameters
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,

    // Current position in phase space
    i: f64,
    j: f64,

    // Viewport: center and scale
    ic: f64,
    jc: f64,
    is: f64,
    js: f64,

    op: FType,

    /// Frame counter — resets after `cycles`
    count: i32,
    /// Draw-call counter — increments once per tick(); used by HSHOE and SQRT/STANDARD seeding
    inc: i32,

    /// Static alternating sign used by SQRT/STANDARD k==0 seed (mirrors C `static int s`)
    seed_sign: i32,

    // Color
    pix: i32,
    ncolors: i32,

    // Config
    cycles: i32,
    point_count: i32,
    delay_us: u64,

    // Points accumulated during tick(), flushed in render()
    pixel_buf: Vec<Point>,
    current_color: Color,

    /// When true, render() clears the buffer before drawing
    needs_clear: bool,
}

impl Discrete {
    /// xlockmore LRAND() — 31-bit non-negative random integer
    #[inline(always)]
    fn lrand(rng: &mut impl Rng) -> u32 {
        rng.random::<u32>() & 0x7FFF_FFFF
    }

    /// (LRAND() / MAXRAND) — uniform float in [0.0, 1.0)
    #[inline(always)]
    fn lrand_f(rng: &mut impl Rng) -> f64 {
        Self::lrand(rng) as f64 / MAXRAND
    }

    /// Map a palette index to a Color.
    fn pix_to_color(pix: i32, ncolors: i32) -> Color {
        if ncolors > 2 {
            Color::from_hsl(pix as f32 / ncolors as f32, 1.0, 0.5)
        } else {
            Color::new(255, 255, 255, 255)
        }
    }

    /// Phase-space → screen pixel.
    ///   xp->x = maxx/2 + (int)((i - ic) * is)
    ///   xp->y = maxy/2 - (int)((j - jc) * js)
    #[inline(always)]
    fn to_screen(&self, i: f64, j: f64) -> Point {
        Point {
            x: self.maxx / 2 + ((i - self.ic) * self.is) as i32,
            y: self.maxy / 2 - ((j - self.jc) * self.js) as i32,
        }
    }

    /// Match init_discrete() — pick a random map type and set its parameters.
    fn init_params(&mut self, rng: &mut impl Rng) {
        self.op = BIAS[Self::lrand(rng) as usize % BIAS.len()];

        match self.op {
            FType::Hshoe => {
                self.ic = 0.0;
                self.jc = 0.0;
                self.is = self.maxx as f64 / 4.0;
                self.js = self.maxy as f64 / 4.0;
                self.a = 0.5;
                self.b = 0.5;
                self.c = 0.2;
                self.d = -1.25;
                self.e = 1.0;
                self.i = 0.0;
                self.j = 0.0;
            }
            FType::Delog => {
                self.ic = 0.5;
                self.jc = 0.3;
                self.is = self.maxx as f64 / 1.5;
                self.js = self.maxy as f64 / 1.5;
                self.a = 2.176399;
                self.i = 0.01;
                self.j = 0.01;
            }
            FType::Henon => {
                self.jc = (Self::lrand_f(rng) * 2.0 - 1.0) * 0.4;
                self.ic = 1.3 * (1.0 - (self.jc * self.jc) / (0.4 * 0.4));
                self.is = self.maxx as f64;
                self.js = self.maxy as f64 * 1.5;
                self.a = 1.0;
                self.b = 1.4;
                self.c = 0.3;
                self.i = 0.0;
                self.j = 0.0;
            }
            FType::Sqrt => {
                self.ic = 0.0;
                self.jc = 0.0;
                self.is = 1.0;
                self.js = 1.0;
                let range = f64::sqrt(
                    (self.maxx as f64 * 2.0) * (self.maxx as f64 * 2.0)
                        + (self.maxy as f64 * 2.0) * (self.maxy as f64 * 2.0),
                ) / (10.0 + (Self::lrand(rng) % 10) as f64);
                self.a = Self::lrand_f(rng) * range - range / 2.0;
                self.b = Self::lrand_f(rng) * range - range / 2.0;
                self.c = Self::lrand_f(rng) * range - range / 2.0;
                // C: `if (!(LRAND() % 2)) hp->c = 0.0;`  — zero c ~50% of the time
                if Self::lrand(rng) % 2 == 0 {
                    self.c = 0.0;
                }
                self.i = 0.0;
                self.j = 0.0;
            }
            FType::Standard => {
                self.ic = PI;
                self.jc = PI;
                self.is = self.maxx as f64 / (PI * 2.0);
                self.js = self.maxy as f64 / (PI * 2.0);
                self.a = 0.0; // decay
                self.b = Self::lrand_f(rng) * 2.0;
                self.c = 0.0;
                self.i = PI;
                self.j = PI;
            }
            FType::Birdie => {
                self.ic = 0.0;
                self.jc = 0.0;
                self.is = self.maxx as f64 / 2.0;
                self.js = self.maxy as f64 / 2.0;
                self.a = 1.99 + (Self::lrand_f(rng) * 2.0 - 1.0) * 0.2;
                self.b = 0.0;
                self.c = 0.8 + (Self::lrand_f(rng) * 2.0 - 1.0) * 0.1;
                self.i = 0.0;
                self.j = 0.0;
            }
            FType::Trig => {
                self.a = 5.0;
                self.b = 0.5 + (Self::lrand_f(rng) * 2.0 - 1.0) * 0.3;
                self.ic = self.a;
                self.jc = 0.0;
                self.is = self.maxx as f64 / (self.b * 20.0);
                self.js = self.maxy as f64 / (self.b * 20.0);
                self.i = 0.0;
                self.j = 0.0;
            }
            FType::Cubic => {
                self.a = 2.77;
                self.b = 0.1 + (Self::lrand_f(rng) * 2.0 - 1.0) * 0.1;
                self.ic = 0.0;
                self.jc = 0.0;
                self.is = self.maxx as f64 / 4.0;
                self.js = self.maxy as f64 / 4.0;
                self.i = 0.1;
                self.j = 0.1;
            }
            FType::Ailuj => {
                self.ic = 0.0;
                self.jc = 0.0;
                self.is = self.maxx as f64 / 4.0;
                // xlockmore quirk: js uses maxx (not maxy) — verbatim from C
                self.js = self.maxx as f64 / 4.0;
                // Rejection-sample: keep rolling (a,b) until the Mandelbrot orbit
                // stays bounded for MAXITER steps — ensures a connected Julia set.
                loop {
                    self.a = (Self::lrand_f(rng) * 2.0 - 1.0) * 1.5 - 0.5;
                    self.b = (Self::lrand_f(rng) * 2.0 - 1.0) * 1.5;
                    let (mut x, mut y) = (0.0_f64, 0.0_f64);
                    let mut iter = 0usize;
                    const MAXITER: usize = 10;
                    while iter < MAXITER && x * x + y * y < 13.0 {
                        let xtemp = x * x - y * y + self.a;
                        let ytemp = 2.0 * x * y + self.b;
                        x = xtemp;
                        y = ytemp;
                        iter += 1;
                    }
                    // C: `while (i < MAXITER)` — retry while orbit escaped.
                    // Break when orbit stayed bounded (iter == MAXITER).
                    if iter >= MAXITER {
                        break;
                    }
                }
                self.i = 0.1;
                self.j = 0.1;
            }
        }
    }
}

impl Animation for Discrete {
    fn new(config: &AnimConfig) -> Self {
        let mut d = Discrete {
            maxx: config.width as i32,
            maxy: config.height as i32,
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: 0.0,
            i: 0.0,
            j: 0.0,
            ic: 0.0,
            jc: 0.0,
            is: 1.0,
            js: 1.0,
            op: FType::Standard,
            count: 0,
            inc: 0,
            seed_sign: 1,
            pix: 0,
            ncolors: if config.ncolors == 0 { DEFAULT_NCOLORS } else { config.ncolors },
            cycles: if config.cycles == 0 { DEFAULT_CYCLES } else { config.cycles },
            point_count: if config.count == 0 { DEFAULT_COUNT } else { config.count },
            delay_us: config.delay_us,
            pixel_buf: Vec::new(),
            current_color: Color::new(255, 255, 255, 255),
            needs_clear: true,
        };
        d.reset(config);
        d
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        self.pixel_buf.clear();

        // Color update: advance pix once per draw call (matches draw_discrete color logic)
        if self.ncolors > 2 {
            self.current_color = Self::pix_to_color(self.pix, self.ncolors);
            self.pix += 1;
            if self.pix >= self.ncolors {
                self.pix = 0;
            }
        } else {
            self.current_color = Color::new(255, 255, 255, 255);
        }

        // hp->inc++ at start of each draw call
        self.inc += 1;

        // C: `k = count; xp = hp->pointBuffer; while (k--) { ... }`
        // Post-decrement: k goes count, count-1, ..., 1, 0 inside the loop body.
        let count = self.point_count;
        let mut k = count; // k value seen inside the loop body (before decrement)

        while k > 0 {
            k -= 1; // mirror: k inside body = k after post-decrement

            let oldj = self.j;
            let oldi = self.i;

            match self.op {
                FType::Hshoe => {
                    // The C code (with #define HD active) sets i,j from k, then
                    // applies the map inside a for loop:
                    //
                    //   if   (k < count/4)     { i = k/count*8-1;         j = 1  }
                    //   elif (k < count/2)     { i = 1;                   j = 3-k/count*8 }
                    //   elif (k < 3*count/4)   { i = 5-k/count*8;         j = -1 }
                    //   else                   { i = -1;                  j = k/count*8-7 }
                    //   for (i_inner = 1; i_inner < (inc%15); i_inner++) {
                    //       oldi = i; oldj = j;
                    //       i = (a*oldi + b)*oldj;
                    //       j = (e - d + c*oldi)*oldj*oldj - c*oldi + d;
                    //   }
                    //
                    // When inc%15 <= 1 the loop never runs; the raw (i,j) is plotted.

                    let kf = k as f64;
                    let cf = count as f64;
                    if k < count / 4 {
                        self.i = (kf / cf) * 8.0 - 1.0;
                        self.j = 1.0;
                    } else if k < count / 2 {
                        self.i = 1.0;
                        self.j = 3.0 - (kf / cf) * 8.0;
                    } else if k < 3 * count / 4 {
                        self.i = 5.0 - (kf / cf) * 8.0;
                        self.j = -1.0;
                    } else {
                        self.i = -1.0;
                        self.j = (kf / cf) * 8.0 - 7.0;
                    }

                    let inner_limit = self.inc % 15;
                    for _inner in 1..inner_limit {
                        let oi = self.i;
                        let oj = self.j;
                        self.i = (self.a * oi + self.b) * oj;
                        self.j = (self.e - self.d + self.c * oi) * oj * oj
                            - self.c * oi + self.d;
                    }
                }

                FType::Delog => {
                    self.j = oldi;
                    self.i = self.a * oldi * (1.0 - oldj);
                }

                FType::Henon => {
                    self.i = oldj + self.a - self.b * oldi * oldi;
                    self.j = self.c * oldi;
                }

                FType::Sqrt => {
                    if k != 0 {
                        // Normal iteration
                        self.j = self.a + self.i;
                        self.i = -oldj
                            + if self.i < 0.0 {
                                f64::sqrt(f64::abs(self.b * (self.i - self.c)))
                            } else {
                                -f64::sqrt(f64::abs(self.b * (self.i - self.c)))
                            };
                    } else {
                        // k==0: seed point. C uses `static int s = 1` that flips
                        // each time the k==0 branch executes (once per draw call).
                        let s = self.seed_sign as f64;
                        self.i = s * (self.inc as f64) * (self.maxx as f64)
                            / (self.cycles as f64)
                            / 2.0;
                        self.j = self.a + self.i;
                        self.seed_sign = -self.seed_sign;
                    }
                }

                FType::Standard => {
                    if k != 0 {
                        self.j = (1.0 - self.a) * oldj
                            + self.b * f64::sin(oldi)
                            + self.a * self.c;
                        self.j = f64::rem_euclid(self.j + 2.0 * PI, 2.0 * PI);
                        self.i = oldi + self.j;
                        self.i = f64::rem_euclid(self.i + 2.0 * PI, 2.0 * PI);
                    } else {
                        // k==0: seed point, same static-s pattern.
                        let s = self.seed_sign as f64;
                        self.j = PI
                            + f64::rem_euclid(
                                s * (self.inc as f64) * 2.0 * PI
                                    / (self.cycles as f64 - 0.5),
                                PI,
                            );
                        self.i = PI;
                        self.seed_sign = -self.seed_sign;
                    }
                }

                FType::Birdie => {
                    self.j = oldi;
                    self.i = (1.0 - self.c)
                        * f64::cos(PI * self.a * oldj)
                        + self.c * self.b;
                    self.b = oldj;
                }

                FType::Trig => {
                    let r2 = oldi * oldi + oldj * oldj;
                    self.i =
                        self.a + self.b * (oldi * f64::cos(r2) - oldj * f64::sin(r2));
                    self.j = self.b * (oldj * f64::cos(r2) + oldi * f64::sin(r2));
                }

                FType::Cubic => {
                    self.i = oldj;
                    self.j =
                        self.a * oldj - oldj * oldj * oldj - self.b * oldi;
                }

                FType::Ailuj => {
                    // Inverse Julia: random square-root sign choice.
                    // C: `LRAND() < MAXRAND / 2` — tests the 31-bit value against 2^30.
                    let sign: f64 =
                        if Self::lrand(&mut rng) < (MAXRAND / 2.0) as u32 {
                            -1.0
                        } else {
                            1.0
                        };
                    let inner = ((oldi - self.a)
                        + f64::sqrt(
                            (oldi - self.a) * (oldi - self.a)
                                + (oldj - self.b) * (oldj - self.b),
                        ))
                        / 2.0;
                    self.i = sign * f64::sqrt(inner.abs()); // abs guard for numerical safety
                    // C epsilon guard: `if (hp->i < 0.00000001 && hp->i > -0.00000001)`
                    if self.i < 1e-8 && self.i > -1e-8 {
                        self.i = if self.i >= 0.0 { 1e-8 } else { -1e-8 };
                    }
                    self.j = (oldj - self.b) / (2.0 * self.i);
                }
            }

            let pt = self.to_screen(self.i, self.j);
            self.pixel_buf.push(pt);
        }

        // Advance frame counter; reinitialize when cycles exhausted.
        self.count += 1;
        if self.count > self.cycles {
            self.count = 0;
            self.inc = 0;
            self.pix = 0;
            self.seed_sign = 1;
            self.needs_clear = true;
            self.init_params(&mut rng);
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if self.needs_clear {
            clear_buffer(buffer, Color::new(255, 0, 0, 0));
        }
        let color = self.current_color;
        for pt in &self.pixel_buf {
            put_pixel(buffer, width, height, pt.x, pt.y, color);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.maxx = config.width as i32;
        self.maxy = config.height as i32;
        self.ncolors = if config.ncolors == 0 { DEFAULT_NCOLORS } else { config.ncolors };
        self.cycles = if config.cycles == 0 { DEFAULT_CYCLES } else { config.cycles };
        self.point_count = if config.count == 0 { DEFAULT_COUNT } else { config.count };
        self.delay_us = config.delay_us;

        self.pix = 0;
        self.inc = 0;
        self.count = 0;
        self.seed_sign = 1;
        self.needs_clear = true;
        self.pixel_buf.clear();

        self.current_color = if self.ncolors > 2 {
            Self::pix_to_color(0, self.ncolors)
        } else {
            Color::new(255, 255, 255, 255)
        };

        self.init_params(&mut rng);
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
