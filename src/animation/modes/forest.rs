//! Binary trees of a fractal forest.
//
// Copyright (c) 1995 Pascal Pensa <pensa@aurora.unice.fr>
//
// Original idea : Guillaume Ramey <ramey@aurora.unice.fr>
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
// Rust port of xlockmore/modes/forest.c.

use std::cell::Cell;
use std::f64::consts::PI;

use rand::Rng;

use crate::animation::primitives::{clear_buffer, draw_line, Color};
use crate::animation::{AnimConfig, Animation};

// --- xlockmore constants (matching #define values in forest.c) ---------------

const MINTREES: i32 = 1;

const MINHEIGHT: i32 = 20; // Tree height range
const MAXHEIGHT: i32 = 40;

const MINANGLE: i32 = 15; // (degree) angle between branches
const MAXANGLE: i32 = 35;
const RANDANGLE: i32 = 15; // (degree) max random angle from default

const REDUCE: i32 = 90; // Height % from father

const ITERLEVEL: i32 = 10; // Tree recursion depth

const COLORSPEED: i32 = 2; // Color index increment per branch level

// xlockmore default delay for forest mode (microseconds)
const DEFAULT_DELAY_US: u64 = 400_000;
// xlockmore default count / cycles / ncolors
const DEFAULT_COUNT: i32 = 100;
const DEFAULT_CYCLES: i32 = 200;
const DEFAULT_NCOLORS: i32 = 100;

// DEGTORAD(x) from forest.c — note: C uses float (f32), we use f64 for precision
#[inline(always)]
fn deg_to_rad(x: i32) -> f64 {
    (x as f64) * PI / 180.0
}

// RANGE_RAND(min, max) = min + NRAND(max - min)
#[inline(always)]
fn range_rand(rng: &mut impl Rng, min: i32, max: i32) -> i32 {
    min + rng.random_range(0..(max - min))
}

// ---------------------------------------------------------------------------
// DrawOp — a single line segment accumulated by tick(), flushed by render()
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct DrawOp {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: Color,
}

// ---------------------------------------------------------------------------
// Recursive tree builder — produces DrawOps, no direct pixel writes
// ---------------------------------------------------------------------------

fn build_tree(
    ops: &mut Vec<DrawOp>,
    ncolors: i32,
    palette: &[Color],
    x: i16,
    y: i16,
    len: i16,
    a: f64,  // current angle (radians)
    as_: f64, // angle step (radians)
    mut c: i16, // color index
    level: i32,
) {
    let mut rng = rand::rng();

    // --- left branch ---
    let a1 = a + as_
        + deg_to_rad(rng.random_range(0..(2 * RANDANGLE)) - RANDANGLE);
    let x_1 = x + (a1.cos() * (len as f64)) as i16;
    let y_1 = y + (a1.sin() * (len as f64)) as i16;

    // --- right branch ---
    let a2 = a - as_
        + deg_to_rad(rng.random_range(0..(2 * RANDANGLE)) - RANDANGLE);
    let x_2 = x + (a2.cos() * (len as f64)) as i16;
    let y_2 = y + (a2.sin() * (len as f64)) as i16;

    // Color for this level (advance by COLORSPEED after drawing)
    let color = if ncolors > 2 {
        palette[c as usize]
    } else {
        Color::new(255, 255, 255, 255)
    };

    // Advance color index (mirrors: c = (c + COLORSPEED) % MI_NPIXELS)
    c = ((c as i32 + COLORSPEED) % ncolors) as i16;

    // Draw left and right branches
    ops.push(DrawOp {
        x0: x as i32,
        y0: y as i32,
        x1: x_1 as i32,
        y1: y_1 as i32,
        color,
    });
    ops.push(DrawOp {
        x0: x as i32,
        y0: y as i32,
        x1: x_2 as i32,
        y1: y_2 as i32,
        color,
    });

    // Near-root levels get doubled lines (x+1 offset) for trunk width
    if level < 2 {
        ops.push(DrawOp {
            x0: x as i32 + 1,
            y0: y as i32,
            x1: x_1 as i32 + 1,
            y1: y_1 as i32,
            color,
        });
        ops.push(DrawOp {
            x0: x as i32 + 1,
            y0: y as i32,
            x1: x_2 as i32 + 1,
            y1: y_2 as i32,
            color,
        });
    }

    // Reduce length: (len * REDUCE * 10) / 1000 (integer truncation, matching C)
    let new_len = ((len as i32 * REDUCE * 10) / 1000) as i16;

    if level < ITERLEVEL {
        build_tree(ops, ncolors, palette, x_1, y_1, new_len, a1, as_, c, level + 1);
        build_tree(ops, ncolors, palette, x_2, y_2, new_len, a2, as_, c, level + 1);
    }
}

// ---------------------------------------------------------------------------
// Forest animation state
// ---------------------------------------------------------------------------

pub struct Forest {
    width: u32,
    height: u32,
    time: i32,    // current frame counter
    ntrees: i32,  // trees to draw per cycle
    cycles: i32,  // frames per cycle (reset period)
    ncolors: i32, // palette size
    palette: Vec<Color>,

    /// Draw ops accumulated by tick(), consumed by render()
    ops: Vec<DrawOp>,

    /// True when a clear is needed at the start of render()
    needs_clear: Cell<bool>,

    delay_us: u64,
}

impl Forest {
    /// Resolve defaults the same way xlockmore does:
    ///   delay=400000, count=100, cycles=200, ncolors=100
    fn resolve_ntrees(count: i32, rng: &mut impl Rng) -> i32 {
        let c = if count == 0 { DEFAULT_COUNT } else { count };
        if c < -MINTREES {
            // NRAND(-fp->ntrees - MINTREES + 1) + MINTREES
            rng.random_range(0..(-c - MINTREES + 1)) + MINTREES
        } else if c < MINTREES {
            MINTREES
        } else {
            c
        }
    }

    fn build_palette(ncolors: i32) -> Vec<Color> {
        (0..ncolors.max(0))
            .map(|i| Color::from_hsl(i as f32 / ncolors as f32, 1.0, 0.5))
            .collect()
    }
}

impl Animation for Forest {
    fn new(config: &AnimConfig) -> Self {
        let mut rng = rand::rng();
        let ncolors = if config.ncolors <= 0 {
            DEFAULT_NCOLORS
        } else {
            config.ncolors
        };
        let ntrees = Self::resolve_ntrees(config.count, &mut rng);
        let cycles = if config.cycles <= 0 {
            DEFAULT_CYCLES
        } else {
            config.cycles
        };
        let delay_us = if config.delay_us == 0 {
            DEFAULT_DELAY_US
        } else {
            config.delay_us
        };

        Forest {
            width: config.width,
            height: config.height,
            time: 0,
            ntrees,
            cycles,
            ncolors,
            palette: Self::build_palette(ncolors),
            ops: Vec::new(),
            needs_clear: Cell::new(true),
            delay_us,
        }
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        self.ops.clear();

        // Only draw a tree if we still have trees to plant this cycle
        if self.time < self.ntrees {
            // Random root position: x in [0, width), y in [0, height + MAXHEIGHT)
            let x = range_rand(&mut rng, 0, self.width as i32) as i16;
            let y = range_rand(&mut rng, 0, self.height as i32 + MAXHEIGHT) as i16;

            // Trunk angle: nearly straight up (-π/2) ± RANDANGLE degrees
            let a = -PI / 2.0
                + deg_to_rad(rng.random_range(0..(2 * RANDANGLE)) - RANDANGLE);

            // Branch angle step: random in [MINANGLE, MAXANGLE) degrees
            let as_ = deg_to_rad(range_rand(&mut rng, MINANGLE, MAXANGLE));

            // Trunk length: scaled by screen width (matching C formula exactly)
            let len = (range_rand(&mut rng, MINHEIGHT, MAXHEIGHT) * (self.width as i32 / 20)
                / 50)
                + 2;
            let len = len as i16;

            // Starting color index for this tree
            let mut c = if self.ncolors > 2 {
                rng.random_range(0..self.ncolors) as i16
            } else {
                0i16
            };

            let trunk_color = if self.ncolors > 2 {
                self.palette[c as usize]
            } else {
                Color::new(255, 255, 255, 255)
            };

            // Advance color (mirrors: c = (c + COLORSPEED) % MI_NPIXELS after XSetForeground)
            c = ((c as i32 + COLORSPEED) % self.ncolors) as i16;

            // Trunk tip coords
            let x_2 = x + (a.cos() * (len as f64)) as i16;
            let y_2 = y + (a.sin() * (len as f64)) as i16;

            // Draw trunk (doubled like the branches near root)
            self.ops.push(DrawOp {
                x0: x as i32,
                y0: y as i32,
                x1: x_2 as i32,
                y1: y_2 as i32,
                color: trunk_color,
            });
            self.ops.push(DrawOp {
                x0: x as i32 + 1,
                y0: y as i32,
                x1: x_2 as i32 + 1,
                y1: y_2 as i32,
                color: trunk_color,
            });

            // Recursively build branch ops starting from trunk tip, level=1
            // Initial len for branches: (len * REDUCE) / 100
            let branch_len = ((len as i32 * REDUCE) / 100) as i16;
            build_tree(
                &mut self.ops,
                self.ncolors,
                &self.palette,
                x_2,
                y_2,
                branch_len,
                a,
                as_,
                c,
                1,
            );
        }

        // Advance time; if past cycles, schedule reset
        self.time += 1;
        if self.time > self.cycles {
            self.time = 0;
            self.needs_clear.set(true);
            // Re-roll ntrees for the new cycle
            let mut rng2 = rand::rng();
            self.ntrees = Self::resolve_ntrees(
                // preserve the original count semantics — use ntrees as-is
                // (we store ntrees already resolved, so pass it back positively)
                self.ntrees,
                &mut rng2,
            );
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if self.needs_clear.get() {
            clear_buffer(buffer, Color::new(255, 0, 0, 0));
            self.needs_clear.set(false);
        }

        for op in &self.ops {
            draw_line(buffer, width, height, op.x0, op.y0, op.x1, op.y1, op.color);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;
        self.ncolors = if config.ncolors <= 0 {
            DEFAULT_NCOLORS
        } else {
            config.ncolors
        };
        self.palette = Self::build_palette(self.ncolors);
        self.cycles = if config.cycles <= 0 {
            DEFAULT_CYCLES
        } else {
            config.cycles
        };
        self.delay_us = if config.delay_us == 0 {
            DEFAULT_DELAY_US
        } else {
            config.delay_us
        };
        self.ntrees = Self::resolve_ntrees(config.count, &mut rng);
        self.time = 0;
        self.ops.clear();
        self.needs_clear.set(true);
    }

    fn clears_each_frame(&self) -> bool {
        // Draws incrementally onto a black background; only clears on reset.
        false
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
