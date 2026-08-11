//! Square grid mountains.
//
// Copyright (c) 1995 Pascal Pensa <pensa@aurora.unice.fr>
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
// Rust port of xlockmore/modes/mountain.c.

use crate::animation::primitives::{clear_buffer, draw_line, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};
use rand::RngExt;

const WORLDWIDTH: usize = 50;

/// xlockmore mountain.c defaults
const DEFAULT_COUNT: i32 = 30;
const DEFAULT_CYCLES: i32 = 4000;
const DEFAULT_NCOLORS: i32 = 64;
const DEFAULT_DELAY_US: u64 = 1_000;

/// A single draw operation accumulated by tick() and flushed by render().
#[derive(Clone)]
enum DrawOp {
    /// A filled quadrilateral: 4 points (x, y), color index into the palette
    FilledQuad([i32; 4], [i32; 4], usize),
    /// Black outline over a filled quad (4 points)
    QuadOutline([i32; 4], [i32; 4]),
    /// Clear the buffer (used between mountain regens)
    Clear,
}

pub struct Mountain {
    width: u32,
    height: u32,
    // screen pixel dimensions
    pixelmode: bool,
    // whether to skip outlines (small screen)
    x: usize,
    // current cell x in [0, WORLDWIDTH-1)
    y: usize,
    // current cell y in [0, WORLDWIDTH-1)
    stage: u32,
    // 0=drawing, 1=waiting, 2=reinit
    h: [[i32; WORLDWIDTH]; WORLDWIDTH],
    // height map
    time: i32,
    // countdown timer in stage 1
    offset: usize,
    // colour palette offset
    ncolors: i32,
    // number of colours
    cycles: i32,
    // how long to wait between regens
    count: i32,
    // number of peaks to seed
    delay_us: u64,
    // frame delay
    palette: Vec<Color>,
    // precomputed colour palette
    draw_ops: Vec<DrawOp>,
    // draw ops queued by tick(), flushed by render()
}

// ──────────────────────────────────────────────────────────────────────────────
// Polygon fill helper
// ──────────────────────────────────────────────────────────────────────────────

/// Scanline fill of an arbitrary polygon given as (xs[i], ys[i]).
/// Matches XFillPolygon semantics well enough for the convex/near-convex quads
/// produced by the mountain projection.
fn fill_polygon(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    xs: &[i32],
    ys: &[i32],
    color: Color,
) {
    let n = xs.len();
    if n < 3 {
        return;
    }

    // n < 3 guard above checks xs; ys is a separate slice, so don't trust it.
    let (Some(&y_min), Some(&y_max)) = (ys.iter().min(), ys.iter().max()) else {
        return;
    };
    let h = height as i32;
    let w = width as i32;

    for y in y_min..=y_max {
        if y < 0 || y >= h {
            continue;
        }
        // Find x intersections for this scanline using the polygon edges
        let mut intersections: Vec<i32> = Vec::new();
        for i in 0..n {
            let j = (i + 1) % n;
            let (y0, y1) = (ys[i], ys[j]);
            let (x0, x1) = (xs[i], xs[j]);
            if y0 == y1 {
                continue;
            }
            if y < y0.min(y1) || y >= y0.max(y1) {
                continue;
            }
            // x intercept (integer)
            let x = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
            intersections.push(x);
        }
        intersections.sort_unstable();
        let mut i = 0;
        while i + 1 < intersections.len() {
            let xa = intersections[i].max(0).min(w - 1);
            let xb = intersections[i + 1].max(0).min(w - 1);
            for x in xa..=xb {
                put_pixel(buffer, width, height, x, y, color);
            }
            i += 2;
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Terrain helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Spread: average each neighbour with the centre cell.
/// Directly mirrors the C `spread()` function.
fn spread(m: &mut [[i32; WORLDWIDTH]; WORLDWIDTH], x: usize, y: usize) {
    let h = m[x][y];
    let x = x as i32;
    let y = y as i32;
    for y2 in (y - 1)..=(y + 1) {
        for x2 in (x - 1)..=(x + 1) {
            if x2 >= 0
                && y2 >= 0
                && x2 < WORLDWIDTH as i32
                && y2 < WORLDWIDTH as i32
            {
                let (ux, uy) = (x2 as usize, y2 as usize);
                m[ux][uy] = (m[ux][uy] + h) / 2;
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Mountain impl
// ──────────────────────────────────────────────────────────────────────────────

impl Mountain {
    fn build_palette(ncolors: i32) -> Vec<Color> {
        let n = ncolors.max(2) as usize;
        (0..n)
            .map(|i| Color::from_hsl(i as f32 / n as f32, 0.8, 0.45))
            .collect()
    }

    fn init_terrain(&mut self) {
        let mut rng = rand::rng();

        let max_height = 3 * (self.width as i32 + self.height as i32);

        // Zero the height map
        for row in self.h.iter_mut() {
            for cell in row.iter_mut() {
                *cell = 0;
            }
        }

        // Seed j random peaks — mirrors C: RANGE_RAND(1, WORLDWIDTH-1) = 1..49
        let j = if self.count < 0 {
            rng.random_range(0..(-self.count)) + 1
        } else {
            self.count
        };
        for _ in 0..j {
            let px: usize = rng.random_range(1..(WORLDWIDTH - 1));
            let py: usize = rng.random_range(1..(WORLDWIDTH - 1));
            self.h[px][py] = rng.random_range(0..max_height);
        }

        // Spread all cells
        for y in 0..WORLDWIDTH {
            for x in 0..WORLDWIDTH {
                spread(&mut self.h, x, y);
            }
        }

        // Add noise and clamp low values to 0
        for y in 0..WORLDWIDTH {
            for x in 0..WORLDWIDTH {
                self.h[x][y] += rng.random_range(0..10) - 5;
                if self.h[x][y] < 10 {
                    self.h[x][y] = 0;
                }
            }
        }

        // Random colour offset
        if self.ncolors > 2 {
            self.offset = rng.random_range(0..self.ncolors as usize);
        } else {
            self.offset = 0;
        }

        // Reset scan position and stage
        self.x = 0;
        self.y = 0;
        self.stage = 0;
        self.time = 0;

        // Queue a clear so render() wipes the screen before new drawing starts
        self.draw_ops.push(DrawOp::Clear);
    }

    /// Compute the 4 projected vertices for cell (cx, cy) and the colour index.
    ///
    /// Mirrors `drawamountain()` vertex calculation verbatim:
    ///
    /// x2 = cx * (2*W) / (3*WORLDWIDTH)        (note: W = screen width)
    /// y2 = cy * (2*H) / (3*WORLDWIDTH)
    /// p[0] = (x2 - y2/2 + W/4,  y2 - h[cx][cy]   + H/4)
    /// p[1] = (x3 - y3/2 + W/4,  y3 - h[cx+1][cy] + H/4)
    /// p[2] = (x3 - y4/2 + W/4,  y4 - h[cx+1][cy+1] + H/4)
    /// p[3] = (x2 - y5/2 + W/4,  y5 - h[cx][cy+1] + H/4)
    ///
    /// (y4 == y5 in the C code — the two y-coordinate calculations use the same
    /// formula, so they are identical; the C keeps them as separate variables,
    /// we preserve that faithfully.)
    fn make_quad(&self, cx: usize, cy: usize) -> ([i32; 4], [i32; 4], usize) {
        let w = self.width as i32;
        let h = self.height as i32;
        let ww = WORLDWIDTH as i32;

        let x2 = cx as i32 * (2 * w) / (3 * ww);
        let y2 = cy as i32 * (2 * h) / (3 * ww);
        let x3 = (cx as i32 + 1) * (2 * w) / (3 * ww);
        let y3 = cy as i32 * (2 * h) / (3 * ww);
        let y4 = (cy as i32 + 1) * (2 * h) / (3 * ww);
        let y5 = (cy as i32 + 1) * (2 * h) / (3 * ww); // identical to y4 — C quirk preserved

        let px = [
            (x2 - y2 / 2) + (w / 4),
            (x3 - y3 / 2) + (w / 4),
            (x3 - y4 / 2) + (w / 4),
            (x2 - y5 / 2) + (w / 4),
        ];
        let py = [
            (y2 - self.h[cx][cy]) + h / 4,
            (y3 - self.h[cx + 1][cy]) + h / 4,
            (y4 - self.h[cx + 1][cy + 1]) + h / 4,
            (y5 - self.h[cx][cy + 1]) + h / 4,
        ];

        // Colour: average of 4 corner heights / 10 + offset, mod ncolors
        let c = if self.ncolors > 2 {
            let avg = (self.h[cx][cy]
                + self.h[cx + 1][cy]
                + self.h[cx][cy + 1]
                + self.h[cx + 1][cy + 1])
                / 4;
            ((avg / 10) as usize + self.offset) % (self.ncolors as usize)
        } else {
            0
        };

        (px, py, c)
    }
}

impl Animation for Mountain {
    fn new(config: &AnimConfig) -> Self {
        let ncolors = if config.ncolors <= 0 {
            DEFAULT_NCOLORS
        } else {
            config.ncolors
        };
        let count = if config.count == 0 {
            DEFAULT_COUNT
        } else {
            config.count
        };
        let cycles = if config.cycles == 0 {
            DEFAULT_CYCLES
        } else {
            config.cycles
        };
        let delay_us = if config.delay_us == 0 {
            DEFAULT_DELAY_US
        } else {
            config.delay_us
        };

        let pixelmode = config.width + config.height < 200;

        let mut m = Mountain {
            width: config.width,
            height: config.height,
            pixelmode,
            x: 0,
            y: 0,
            stage: 0,
            h: [[0i32; WORLDWIDTH]; WORLDWIDTH],
            time: 0,
            offset: 0,
            ncolors,
            cycles,
            count,
            delay_us,
            palette: Self::build_palette(ncolors),
            draw_ops: Vec::new(),
        };
        m.init_terrain();
        m
    }

    /// Each call to `tick()` processes exactly one cell draw step, matching the
    /// C `draw_mountain()` → `drawamountain()` → advance x/y pattern.
    fn tick(&mut self) {
        // The player's buffer persists between frames (clears_each_frame is
        // false), so render() only needs this tick's ops. Without this the
        // queue replays every op since startup and grows without bound.
        self.draw_ops.clear();
        match self.stage {
            0 => {
                // Draw one quad cell
                if self.x < WORLDWIDTH - 1 && self.y < WORLDWIDTH - 1 {
                    let (px, py, c) = self.make_quad(self.x, self.y);
                    self.draw_ops.push(DrawOp::FilledQuad(px, py, c));
                    if !self.pixelmode {
                        self.draw_ops.push(DrawOp::QuadOutline(px, py));
                    }
                }

                // Advance position — mirrors C: mp->x++ then wrap
                self.x += 1;
                if self.x == WORLDWIDTH - 1 {
                    self.y += 1;
                    self.x = 0;
                }
                if self.y == WORLDWIDTH - 1 {
                    self.stage += 1;
                }
            }
            1 => {
                // Wait for `cycles` ticks before reinitialising
                self.time += 1;
                if self.time > self.cycles {
                    self.stage += 1;
                }
            }
            2 => {
                // Reinitialise — mirrors C calling init_mountain() from draw_mountain()
                self.init_terrain();
            }
            _ => {}
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for op in &self.draw_ops {
            match op {
                DrawOp::Clear => {
                    clear_buffer(buffer, Color::new(255, 0, 0, 0));
                }
                DrawOp::FilledQuad(px, py, c) => {
                    let color = if self.ncolors > 2 {
                        self.palette[*c]
                    } else {
                        Color::new(255, 255, 255, 255)
                    };
                    fill_polygon(buffer, width, height, px, py, color);
                }
                DrawOp::QuadOutline(px, py) => {
                    let black = Color::new(255, 0, 0, 0);
                    // Draw the 4 edges + close (p[4] = p[0] in C)
                    let n = px.len();
                    for i in 0..n {
                        let j = (i + 1) % n;
                        draw_line(
                            buffer, width, height, px[i], py[i], px[j], py[j], black,
                        );
                    }
                    // Close back to first point (C draws p[0]..p[4] = 5-point closed polyline)
                    draw_line(
                        buffer, width, height, px[n - 1], py[n - 1], px[0], py[0], black,
                    );
                }
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.pixelmode = config.width + config.height < 200;
        self.ncolors = if config.ncolors <= 0 {
            DEFAULT_NCOLORS
        } else {
            config.ncolors
        };
        self.count = if config.count == 0 {
            DEFAULT_COUNT
        } else {
            config.count
        };
        self.cycles = if config.cycles == 0 {
            DEFAULT_CYCLES
        } else {
            config.cycles
        };
        self.delay_us = if config.delay_us == 0 {
            DEFAULT_DELAY_US
        } else {
            config.delay_us
        };
        self.palette = Self::build_palette(self.ncolors);
        self.draw_ops.clear();
        self.init_terrain();
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
