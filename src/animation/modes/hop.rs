//! Real plane iterated fractals (Hopalong-family attractors).
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
// Rust port of xlockmore/modes/hop.c.

// Algorithms include Barry Martin's Hopalong (sqrt/sine), Peter de Jong's attractor,
// Ed Kubaitis (EJK) variants, Renaldo Recuerdo (RR) generalised-exponent, and
// Clifford Pickover's Popcorn map.

use crate::animation::primitives::{clear_buffer, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};
use crate::rng::RngExt;
use std::f64::consts::PI;

// xlockmore attractor-type constants — values must match the C #defines exactly so
// the NRAND(OPS) selection produces the same distribution of modes.
const MARTIN: u32 = 0;
const EJK1: u32 = 1;
const EJK2: u32 = 2;
const EJK4: u32 = 3;
const EJK5: u32 = 4;
const RR: u32 = 5;
const JONG: u32 = 6;
const POPCORN: u32 = 7;
const SINE: u32 = 8;
const EJK3: u32 = 9;
const EJK6: u32 = 10;
const OPS: u32 = 11; // total number of operation types

// xlockmore random helpers
// LRAND() — 31-bit non-negative integer (same as `random()` on many systems)
#[inline(always)]
fn lrand(rng: &mut impl RngExt) -> u32 {
    rng.random::<u32>() & 0x7FFF_FFFF
}

// MAXRAND — the float denominator that makes LRAND()/MAXRAND a [0,1) value
const MAXRAND: f64 = 2_147_483_648.0; // 2^31

// NRAND(n) — uniform integer in [0, n)
#[inline(always)]
fn nrand(rng: &mut impl RngExt, n: u32) -> u32 {
    rng.random_range(0..n)
}

// Popcorn internal constants (from C #define)
const HVAL: f64 = 0.05;
const INCVAL: f64 = 50.0;

/// A single pending draw operation produced by tick() and consumed by render().
#[derive(Clone, Copy)]
struct DrawOp {
    x: i32,
    y: i32,
    color: Color,
}

pub struct Hop {
    // screen geometry
    centerx: i32,
    centery: i32,
    width: u32,
    height: u32,

    // attractor parameters
    a: f64,
    b: f64,
    c: f64,
    d: f64,

    // current attractor state (iterated point)
    i: f64,
    j: f64,

    inc: i32,

    // color cycling
    pix: i32,
    ncolors: i32,

    // which attractor is active
    op: u32,

    // frame / cycle counters
    count: i32,
    cycles: i32,

    // per-frame point batch (analogous to xlockmore pointBuffer)
    bufsize: i32,

    // pending pixel operations filled by tick(), drained by render()
    pending: Vec<DrawOp>,
    clear_pending: bool,

    delay_us: u64,
}

impl Hop {
    /// Colour for pixel index `pix` out of `ncolors`.
    fn pix_color(pix: i32, ncolors: i32) -> Color {
        if ncolors > 2 {
            Color::from_hsl(pix as f32 / ncolors as f32, 1.0, 0.5)
        } else {
            Color::new(255, 255, 255, 255)
        }
    }

    /// Re-randomise all attractor parameters (mirrors init_hop).
    fn randomize(&mut self) {
        let mut rng = crate::rng::rng();

        self.op = nrand(&mut rng, OPS);

        // range mirrors: sqrt(centerx^2 + centery^2) / (1 + LRAND/MAXRAND)
        let range = ((self.centerx as f64 * self.centerx as f64)
            + (self.centery as f64 * self.centery as f64))
            .sqrt()
            / (1.0 + lrand(&mut rng) as f64 / MAXRAND);

        self.i = 0.0;
        self.j = 0.0;

        // inc: (LRAND/MAXRAND)*200 - 100  → [-100, 100)
        self.inc = ((lrand(&mut rng) as f64 / MAXRAND) * 200.0) as i32 - 100;

        match self.op {
            MARTIN => {
                self.a = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 20.0;
                self.b = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 20.0;
                self.c = if lrand(&mut rng) & 1 != 0 {
                    ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 20.0
                } else {
                    0.0
                };
            }
            EJK1 => {
                self.a = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 30.0;
                self.c = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 40.0;
                self.b = lrand(&mut rng) as f64 / MAXRAND * 0.4;
            }
            EJK2 => {
                self.a = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 30.0;
                self.b = 10_f64.powf(6.0 + (lrand(&mut rng) as f64 / MAXRAND) * 24.0);
                if lrand(&mut rng) & 1 != 0 {
                    self.b = -self.b;
                }
                self.c = 10_f64.powf(lrand(&mut rng) as f64 / MAXRAND * 9.0);
                if lrand(&mut rng) & 1 != 0 {
                    self.c = -self.c;
                }
            }
            EJK3 => {
                self.a = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 30.0;
                self.c = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 70.0;
                self.b = lrand(&mut rng) as f64 / MAXRAND * 0.35 + 0.5;
            }
            EJK4 => {
                self.a = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 2.0;
                self.c = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 200.0;
                self.b = lrand(&mut rng) as f64 / MAXRAND * 9.0 + 1.0;
            }
            EJK5 => {
                self.a = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 2.0;
                self.c = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 200.0;
                self.b = lrand(&mut rng) as f64 / MAXRAND * 0.3 + 0.1;
            }
            EJK6 => {
                self.a = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 30.0;
                self.b = lrand(&mut rng) as f64 / MAXRAND + 0.5;
            }
            RR => {
                self.a = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 40.0;
                self.b = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 200.0;
                self.c = ((lrand(&mut rng) as f64 / MAXRAND) * 2.0 - 1.0) * range / 20.0;
                self.d = lrand(&mut rng) as f64 / MAXRAND * 0.9;
            }
            POPCORN => {
                self.a = 0.0;
                self.b = 0.0;
                self.c = (lrand(&mut rng) as f64 / MAXRAND * 2.0 - 1.0) * 0.24 + 0.25;
                // xlockmore forces inc=100 for POPCORN so the first iteration resets i/j
                self.inc = 100;
            }
            JONG => {
                self.a = (lrand(&mut rng) as f64 / MAXRAND * 2.0 - 1.0) * PI;
                self.b = (lrand(&mut rng) as f64 / MAXRAND * 2.0 - 1.0) * PI;
                self.c = (lrand(&mut rng) as f64 / MAXRAND * 2.0 - 1.0) * PI;
                self.d = (lrand(&mut rng) as f64 / MAXRAND * 2.0 - 1.0) * PI;
            }
            SINE => {
                // MARTIN2: a near π  (±0.7 wide)
                self.a = PI + (lrand(&mut rng) as f64 / MAXRAND * 2.0 - 1.0) * 0.7;
            }
            _ => {}
        }

        // Colour selection mirrors: if (MI_NPIXELS(mi) > 2) hp->pix = NRAND(MI_NPIXELS(mi))
        if self.ncolors > 2 {
            self.pix = nrand(&mut rng, self.ncolors as u32) as i32;
        }

        self.count = 0;
    }

    /// Iterate the attractor once and push the resulting pixel to `pending`.
    /// Mirrors the inner `while (k--)` body of draw_hop.
    fn iterate_one(&mut self, color: Color) {
        let oldj = self.j;
        let x: i32;
        let y: i32;

        match self.op {
            MARTIN => {
                // hp->j = hp->a - hp->i;
                // oldi = hp->i + hp->inc;
                // hp->i = oldj + (hp->i < 0 ? sqrt(...) : -sqrt(...))
                // xlockmore computes oldi BEFORE updating j, but j is only used after;
                // the C statement order is:
                //   oldi = hp->i + hp->inc;
                //   hp->j = hp->a - hp->i;   (uses original i)
                //   hp->i = oldj + ...
                let oldi = self.i + self.inc as f64;
                self.j = self.a - self.i;
                let sq = (self.b * oldi - self.c).abs().sqrt();
                self.i = oldj + if self.i < 0.0 { sq } else { -sq };
                x = self.centerx + (self.i + self.j) as i32;
                y = self.centery - (self.i - self.j) as i32;
            }
            EJK1 => {
                let oldi = self.i + self.inc as f64;
                self.j = self.a - self.i;
                let term = self.b * oldi - self.c;
                self.i = oldj - if self.i > 0.0 { term } else { -term };
                x = self.centerx + (self.i + self.j) as i32;
                y = self.centery - (self.i - self.j) as i32;
            }
            EJK2 => {
                let oldi = self.i + self.inc as f64;
                self.j = self.a - self.i;
                let lg = (self.b * oldi - self.c).abs().ln();
                self.i = oldj - if self.i < 0.0 { lg } else { -lg };
                x = self.centerx + (self.i + self.j) as i32;
                y = self.centery - (self.i - self.j) as i32;
            }
            EJK3 => {
                let oldi = self.i + self.inc as f64;
                self.j = self.a - self.i;
                let term = if self.i > 0.0 {
                    (self.b * oldi).sin() - self.c
                } else {
                    -(self.b * oldi).sin() - self.c
                };
                self.i = oldj - term;
                x = self.centerx + (self.i + self.j) as i32;
                y = self.centery - (self.i - self.j) as i32;
            }
            EJK4 => {
                let oldi = self.i + self.inc as f64;
                self.j = self.a - self.i;
                let term = if self.i > 0.0 {
                    (self.b * oldi).sin() - self.c
                } else {
                    -((self.b * oldi - self.c).abs().sqrt())
                };
                self.i = oldj - term;
                x = self.centerx + (self.i + self.j) as i32;
                y = self.centery - (self.i - self.j) as i32;
            }
            EJK5 => {
                let oldi = self.i + self.inc as f64;
                self.j = self.a - self.i;
                let term = if self.i > 0.0 {
                    (self.b * oldi).sin() - self.c
                } else {
                    -(self.b * oldi - self.c)
                };
                self.i = oldj - term;
                x = self.centerx + (self.i + self.j) as i32;
                y = self.centery - (self.i - self.j) as i32;
            }
            EJK6 => {
                // hp->i = oldj - asin((b*oldi) - (long)(b*oldi))
                // The C cast `(long)` truncates toward zero; Rust f64 as i64 does the same.
                let oldi = self.i + self.inc as f64;
                self.j = self.a - self.i;
                let frac = self.b * oldi - (self.b * oldi) as i64 as f64;
                self.i = oldj - frac.clamp(-1.0, 1.0).asin();
                x = self.centerx + (self.i + self.j) as i32;
                y = self.centery - (self.i - self.j) as i32;
            }
            RR => {
                let oldi = self.i + self.inc as f64;
                self.j = self.a - self.i;
                let base = (self.b * oldi - self.c).abs().powf(self.d);
                self.i = oldj - if self.i < 0.0 { -base } else { base };
                x = self.centerx + (self.i + self.j) as i32;
                y = self.centery - (self.i - self.j) as i32;
            }
            POPCORN => {
                // xlockmore POPCORN logic:
                //   inc++ happens in draw_hop outer code; here inc is already incremented.
                //   if (hp->inc >= 100) hp->inc = 0;
                //   if (hp->inc == 0) { reset i/j from a/b counters }
                //   iterate i, j with Pickover formula
                //   pixel uses MI_WIDTH/40 and MI_HEIGHT/40 scaling

                // Note: `inc` was already incremented before iterate_one is called,
                // mirroring the `hp->inc++` at the top of draw_hop.
                if self.inc >= 100 {
                    self.inc = 0;
                }
                if self.inc == 0 {
                    // C: if (hp->a++ >= INCVAL) — post-increment: compare THEN advance.
                    // When the condition fires (a == INCVAL), a is immediately reset to 0.
                    let old_a = self.a;
                    self.a += 1.0;
                    if old_a >= INCVAL {
                        self.a = 0.0;
                        let old_b = self.b;
                        self.b += 1.0;
                        if old_b >= INCVAL {
                            self.b = 0.0;
                        }
                    }
                    self.i = (-self.c * INCVAL / 2.0 + self.c * self.a) * PI / 180.0;
                    self.j = (-self.c * INCVAL / 2.0 + self.c * self.b) * PI / 180.0;
                }
                let tempi = self.i - HVAL * (self.j + (3.0 * self.j).tan()).sin();
                let tempj = self.j - HVAL * (self.i + (3.0 * self.i).tan()).sin();
                x = self.centerx + (self.width as f64 / 40.0 * tempi) as i32;
                y = self.centery + (self.height as f64 / 40.0 * tempj) as i32;
                self.i = tempi;
                self.j = tempj;
            }
            JONG => {
                // oldi = hp->i + 4*hp->inc/hp->centerx (or just hp->i when centerx==0)
                let oldi = if self.centerx > 0 {
                    self.i + 4.0 * self.inc as f64 / self.centerx as f64
                } else {
                    self.i
                };
                // C order: j is updated first (using original i), then i (using oldj/oldi)
                self.j = (self.c * self.i).sin() - (self.d * oldj).cos();
                self.i = (self.a * oldj).sin() - (self.b * oldi).cos();
                x = self.centerx + (self.centerx as f64 * (self.i + self.j) / 4.0) as i32;
                y = self.centery - (self.centery as f64 * (self.i - self.j) / 4.0) as i32;
            }
            SINE => {
                // MARTIN2
                let oldi = self.i + self.inc as f64;
                self.j = self.a - self.i;
                self.i = oldj - oldi.sin();
                x = self.centerx + (self.i + self.j) as i32;
                y = self.centery - (self.i - self.j) as i32;
            }
            _ => return,
        }

        self.pending.push(DrawOp { x, y, color });
    }
}

impl Animation for Hop {
    fn new(config: &AnimConfig) -> Self {
        let width = config.width;
        let height = config.height;

        // xlockmore defaults: count=1000, cycles=2500, ncolors=200
        let bufsize = if config.count == 0 { 1000 } else { config.count }.max(1);
        let cycles = if config.cycles == 0 { 2500 } else { config.cycles };
        let ncolors = if config.ncolors == 0 { 200 } else { config.ncolors };

        let mut hop = Hop {
            centerx: (width / 2) as i32,
            centery: (height / 2) as i32,
            width,
            height,
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            i: 0.0,
            j: 0.0,
            inc: 0,
            pix: 0,
            ncolors,
            op: MARTIN,
            count: 0,
            cycles,
            bufsize,
            pending: Vec::with_capacity(bufsize as usize),
            clear_pending: false,
            delay_us: config.delay_us,
        };
        hop.randomize();
        hop
    }

    fn tick(&mut self) {
        self.pending.clear();
        self.clear_pending = false;

        // draw_hop increments inc once per call, before the point loop
        self.inc += 1;

        // Colour step: advance pix each frame, mirroring draw_hop's color cycling
        let color = if self.ncolors > 2 {
            let c = Self::pix_color(self.pix, self.ncolors);
            self.pix += 1;
            if self.pix >= self.ncolors {
                self.pix = 0;
            }
            c
        } else {
            Color::new(255, 255, 255, 255)
        };

        // Inner point-generation loop: k = bufsize, k-- until 0
        for _ in 0..self.bufsize {
            self.iterate_one(color);
        }

        // xlockmore checks `++count > cycles` after drawing, then init_hop
        // clears the window and re-randomises the attractor.
        self.count += 1;
        if self.count > self.cycles {
            self.randomize();
            self.clear_pending = true;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for op in &self.pending {
            put_pixel(buffer, width, height, op.x, op.y, op.color);
        }
        if self.clear_pending {
            clear_buffer(buffer, Color::new(255, 0, 0, 0));
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.centerx = (config.width / 2) as i32;
        self.centery = (config.height / 2) as i32;
        self.ncolors = if config.ncolors == 0 { 200 } else { config.ncolors };
        self.bufsize = if config.count == 0 { 1000 } else { config.count }.max(1);
        self.cycles = if config.cycles == 0 { 2500 } else { config.cycles };
        self.delay_us = config.delay_us;
        self.pending.clear();
        self.clear_pending = false;
        self.randomize();
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clears_after_upstream_cycle_boundary() {
        let config = AnimConfig {
            width: 8,
            height: 8,
            count: 1,
            cycles: 1,
            ..AnimConfig::default()
        };
        let mut hop = Hop::new(&config);
        let mut buffer = vec![255; 8 * 8 * 4];

        for _ in 0..2 {
            hop.tick();
            hop.render(&mut buffer, 8, 8);
        }

        assert!(buffer.as_chunks::<4>().0.iter().all(|pixel| pixel == &[0, 0, 0, 255]));
        assert_eq!(hop.count, 0);
    }
}
