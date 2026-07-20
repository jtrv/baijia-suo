//! Vector swirl.
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
// Rust port of xlockmore/modes/qix.c.

use rand::Rng;
use std::collections::VecDeque;
use crate::animation::primitives::{draw_line, put_pixel, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const MINPOINTS: usize = 2;

fn check_bounds(
    delta: &mut i32,
    val: i32,
    min: i32,
    max: i32,
    max_delta: i32,
    offset: i32,
    rng: &mut impl Rng,
) {
    let md = max_delta.max(1);
    if val < min {
        *delta = rng.random_range(0..md) + offset;
    } else if val >= max {
        *delta = -(rng.random_range(0..md)) - offset;
    }
}

fn fill_polygon(buffer: &mut [u8], width: u32, height: u32, verts: &[(i32, i32)], color: Color) {
    let n = verts.len();
    if n < 3 {
        return;
    }
    let (Some(min_y), Some(max_y)) = (
        verts.iter().map(|&(_, y)| y).min(),
        verts.iter().map(|&(_, y)| y).max(),
    ) else {
        return;
    };
    let min_y = min_y.max(0) as u32;
    let max_y = match max_y.min(height as i32 - 1) {
        v if v < 0 => return,
        v => v as u32,
    };
    let mut xs: Vec<i32> = Vec::with_capacity(8);
    for scan_y in min_y..=max_y {
        let sy = scan_y as i32;
        xs.clear();
        for i in 0..n {
            let (x0, y0) = verts[i];
            let (x1, y1) = verts[(i + 1) % n];
            if (y0 <= sy && sy < y1) || (y1 <= sy && sy < y0) {
                let x = x0 + (x1 - x0) * (sy - y0) / (y1 - y0);
                xs.push(x);
            }
        }
        xs.sort_unstable();
        let mut i = 0;
        while i + 1 < xs.len() {
            let x0 = xs[i].max(0);
            let x1 = xs[i + 1].min(width as i32 - 1);
            for x in x0..=x1 {
                put_pixel(buffer, width, height, x, sy, color);
            }
            i += 2;
        }
    }
}

struct QixPoint {
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
}

#[derive(Clone, Copy, PartialEq)]
enum QixMode {
    Normal,
    Solid,
    Complete,
    Kaleid,
}

pub struct Qix {
    points: Vec<QixPoint>,
    history: VecDeque<(Vec<(i32, i32)>, Color)>,
    max_snapshots: usize,
    nlines: usize,
    pix: usize,
    ncolors: usize,
    npoints: usize,
    max_delta: i32,
    offset: i32,
    width: u32,
    height: u32,
    // MAX(width,height)/2 — used for kaleid symmetry axis
    mid: i32,
    midx: i32,
    midy: i32,
    delay_us: u64,
    mode: QixMode,
}

impl Qix {
    fn palette_color(&self, pix: usize) -> Color {
        Color::from_hsl(pix as f32 / self.ncolors as f32, 1.0, 0.5)
    }

    fn draw_polygon(buffer: &mut [u8], width: u32, height: u32, pts: &[(i32, i32)], color: Color) {
        let n = pts.len();
        if n < 2 {
            return;
        }
        // xlockmore: for n==2 the loop body is skipped; just the closing segment is drawn.
        // for n>2: draw consecutive edges, then close.
        let loop_end = if n == 2 { 0 } else { n - 1 };
        for i in 0..loop_end {
            draw_line(buffer, width, height, pts[i].0, pts[i].1, pts[i + 1].0, pts[i + 1].1, color);
        }
        draw_line(buffer, width, height, pts[0].0, pts[0].1, pts[n - 1].0, pts[n - 1].1, color);
    }

    fn draw_complete(buffer: &mut [u8], width: u32, height: u32, pts: &[(i32, i32)], color: Color) {
        let n = pts.len();
        for i in 0..n - 1 {
            for j in i + 1..n {
                draw_line(buffer, width, height, pts[i].0, pts[i].1, pts[j].0, pts[j].1, color);
            }
        }
    }

    // 8-fold kaleidoscope segments — mirrors through midx/midy using absolute distance from mid.
    fn draw_kaleid(&self, buffer: &mut [u8], width: u32, height: u32, pts: &[(i32, i32)], color: Color) {
        let n = pts.len();
        let mx = self.midx;
        let my = self.midy;
        let mid = self.mid;
        for i in 0..n {
            let j = (i + 1) % n;
            let s0x = (pts[i].0 - mid).abs();
            let s0y = (pts[i].1 - mid).abs();
            let s1x = (pts[j].0 - mid).abs();
            let s1y = (pts[j].1 - mid).abs();
            draw_line(buffer, width, height, mx + s0x, my + s0y, mx + s1x, my + s1y, color);
            draw_line(buffer, width, height, mx - s0x, my + s0y, mx - s1x, my + s1y, color);
            draw_line(buffer, width, height, mx - s0x, my - s0y, mx - s1x, my - s1y, color);
            draw_line(buffer, width, height, mx + s0x, my - s0y, mx + s1x, my - s1y, color);
            draw_line(buffer, width, height, mx + s0y, my + s0x, mx + s1y, my + s1x, color);
            draw_line(buffer, width, height, mx + s0y, my - s0x, mx + s1y, my - s1x, color);
            draw_line(buffer, width, height, mx - s0y, my - s0x, mx - s1y, my - s1x, color);
            draw_line(buffer, width, height, mx - s0y, my + s0x, mx - s1y, my + s1x, color);
            if n == 2 {
                break;
            }
        }
    }
}

impl Animation for Qix {
    fn new(config: &AnimConfig) -> Self {
        let mut qix = Qix {
            points: Vec::new(),
            history: VecDeque::new(),
            max_snapshots: 1,
            nlines: 1,
            pix: 0,
            ncolors: 1,
            npoints: MINPOINTS,
            max_delta: 16,
            offset: 5,
            width: config.width,
            height: config.height,
            mid: 0,
            midx: 0,
            midy: 0,
            delay_us: config.delay_us,
            mode: QixMode::Normal,
        };
        qix.reset(config);
        qix
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        for i in 0..self.npoints {
            self.points[i].x += self.points[i].dx;
            self.points[i].y += self.points[i].dy;

            // 1/20 chance: redirect toward centre third instead of full screen
            let px = self.points[i].x;
            let py = self.points[i].y;
            if rng.random_range(0..20_u32) < 1 {
                let min = self.width as i32 / 3;
                let max = 2 * self.width as i32 / 3;
                check_bounds(&mut self.points[i].dx, px, min, max, self.max_delta, self.offset, &mut rng);
            } else {
                check_bounds(&mut self.points[i].dx, px, 0, self.width as i32, self.max_delta, self.offset, &mut rng);
            }
            if rng.random_range(0..20_u32) < 1 {
                let min = self.height as i32 / 3;
                let max = 2 * self.height as i32 / 3;
                check_bounds(&mut self.points[i].dy, py, min, max, self.max_delta, self.offset, &mut rng);
            } else {
                check_bounds(&mut self.points[i].dy, py, 0, self.height as i32, self.max_delta, self.offset, &mut rng);
            }
        }

        let color = self.palette_color(self.pix);
        self.pix = (self.pix + 1) % self.ncolors;

        // Recycle the Vec from the oldest snapshot instead of allocating a
        // new one each tick, when the (bounded, non-Kaleid) history is full.
        let mut snapshot = if self.mode != QixMode::Kaleid && self.history.len() >= self.max_snapshots {
            self.history.pop_front().map(|(v, _)| v).unwrap_or_default()
        } else {
            Vec::with_capacity(self.npoints)
        };
        snapshot.clear();
        snapshot.extend(self.points.iter().map(|p| (p.x, p.y)));
        self.history.push_back((snapshot, color));

        if self.mode == QixMode::Kaleid {
            // xlockmore clears immediately after drawing, so the overflow frame appears blank.
            if self.history.len() >= self.nlines {
                self.history.clear();
            }
        } else {
            while self.history.len() > self.max_snapshots {
                self.history.pop_front();
            }
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        match self.mode {
            QixMode::Normal => {
                for (pts, color) in &self.history {
                    Self::draw_polygon(buffer, width, height, pts, *color);
                }
            }
            QixMode::Complete => {
                for (pts, color) in &self.history {
                    if self.npoints > 3 {
                        Self::draw_complete(buffer, width, height, pts, *color);
                    } else {
                        Self::draw_polygon(buffer, width, height, pts, *color);
                    }
                }
            }
            QixMode::Solid => {
                let mut verts: Vec<(i32, i32)> = Vec::new();
                for ((old_pts, _), (new_pts, color)) in
                    self.history.iter().zip(self.history.iter().skip(1))
                {
                    let n = old_pts.len();
                    if n == new_pts.len() && n >= 1 {
                        verts.clear();
                        verts.extend_from_slice(old_pts);
                        for j in (0..n).rev() {
                            verts.push(new_pts[j]);
                        }
                        fill_polygon(buffer, width, height, &verts, *color);
                    }
                }
            }
            QixMode::Kaleid => {
                for (pts, color) in &self.history {
                    self.draw_kaleid(buffer, width, height, pts, *color);
                }
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.history.clear();
        self.width = config.width;
        self.height = config.height;
        self.ncolors = config.ncolors.max(2) as usize;
        self.delay_us = config.delay_us;

        // npoints from count — negative means random in [MINPOINTS, |count|]
        // xlockmore default count=-5 (random 2-5 points); 0 means "use that default"
        let count = if config.count == 0 { -5 } else { config.count };
        self.npoints = if count < -(MINPOINTS as i32) {
            let hi = (-count) as usize;
            rng.random_range(MINPOINTS..=hi)
        } else {
            (count.max(MINPOINTS as i32)) as usize
        };

        // Reduced max_delta for icon-sized windows (matches xlockmore)
        self.max_delta = if config.width < 100 { 4 } else { 16 };
        self.offset = self.max_delta / 3;

        // Mode selection mirrors xlockmore's fullrandom defaults
        let complete = rng.random_range(0..4_u32) == 0;
        let kaleid = !complete && rng.random_range(0..3_u32) == 0;
        let solid = !complete && !kaleid && rng.random::<bool>();
        self.mode = if complete {
            QixMode::Complete
        } else if kaleid {
            QixMode::Kaleid
        } else if solid {
            QixMode::Solid
        } else {
            QixMode::Normal
        };

        // xlockmore default cycles=32; fall back to it when none configured
        let cycles = if config.cycles <= 0 { 32 } else { config.cycles as usize };

        if self.mode == QixMode::Complete {
            // xlockmore halves max_delta but does NOT recompute offset — offset stays at
            // the pre-halving value (initial max_delta / 3), keeping points slower but not crawling.
            self.max_delta /= 4;
            self.nlines = self.npoints;
        } else if self.mode == QixMode::Kaleid {
            self.nlines = (cycles + 1) * 8;
        } else if self.npoints == 2 {
            self.nlines = (cycles + 1) * 2;
        } else {
            self.nlines = ((cycles + self.npoints) / self.npoints) * self.npoints;
        }

        self.max_snapshots = (self.nlines / self.npoints.max(1)).max(1);

        self.midx = config.width as i32 / 2;
        self.midy = config.height as i32 / 2;
        self.mid = config.width.max(config.height) as i32 / 2;
        self.pix = rng.random_range(0..self.ncolors);

        self.points.clear();
        let md = self.max_delta.max(1);
        for _ in 0..self.npoints {
            self.points.push(QixPoint {
                x: rng.random_range(0..config.width as i32),
                y: rng.random_range(0..config.height as i32),
                dx: rng.random_range(0..md) + self.offset,
                dy: rng.random_range(0..md) + self.offset,
            });
        }
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
