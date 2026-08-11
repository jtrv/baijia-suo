//! Vine fractals.
//
// Copyright (c) 1997 by Tracy Camp campt@hurrah.com
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
// Rust port of xlockmore/modes/vines.c.

use rand::RngExt;
use std::cell::Cell;
use std::f64::consts::PI;
use crate::animation::primitives::{draw_line, clear_buffer, Color};
use crate::animation::{AnimConfig, Animation};

// xlockmore defaults from ModStruct / DEFAULTS:
//   delay:  200000
//   count:  0  (0 => draw whole vine each frame)
//   ncolors: 64
//   iterations randomly chosen: 30 + NRAND(100)
//   ang:    60 + NRAND(720)
//   length: 100 + NRAND(3000)
//   constant: length * (10 + NRAND(10))

/// A pending draw operation: one line segment with a colour.
#[derive(Clone)]
struct LineOp {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: Color,
}

pub struct Vines {
    width: u32,
    height: u32,
    ncolors: usize,
    // count controls steps-per-tick (0 => entire vine at once)
    count: i32,
    delay_us: u64,

    // per-vine state (mirrors vinestruct in vines.c)
    a: i64,        // accumulated angle (integer radians, grows large)
    x1: i64,
    y1: i64,
    x2: i64,
    y2: i64,
    i: i64,        // current step within vine
    length: i64,   // total steps for this vine
    iterations: i32,
    constant: i64,
    ang: i64,
    centerx: i32,
    centery: i32,
    color: Color,

    // Whether the canvas needs clearing on the next render (set after init).
    // Uses Cell so render(&self) can consume the flag without &mut self.
    needs_clear: Cell<bool>,

    // Draw ops accumulated in tick(), flushed in render().
    pending: Vec<LineOp>,
}

impl Vines {
    fn init_vine(&mut self) {
        let mut rng = rand::rng();

        self.i = 0;
        self.a = 0;
        self.x1 = 0;
        self.y1 = 0;
        self.x2 = 1;
        self.y2 = 0;

        self.centerx = rng.random_range(0..self.width as i32);
        self.centery = rng.random_range(0..self.height as i32);
        self.ang = 60 + rng.random_range(0..720i64);
        self.length = 100 + rng.random_range(0..3000i64);
        self.constant = self.length * (10 + rng.random_range(0..10i64));

        // Pick a random colour from the palette.
        let ci = rng.random_range(0..self.ncolors);
        self.color = Color::from_hsl(ci as f32 / self.ncolors as f32, 1.0, 0.5);
    }

    fn full_reset(&mut self) {
        let mut rng = rand::rng();
        self.iterations = 30 + rng.random_range(0..100i32);
        self.length = 0; // force init_vine on next tick
        self.i = 0;
        self.needs_clear.set(true);
    }
}

impl Animation for Vines {
    fn new(config: &AnimConfig) -> Self {
        let mut rng = rand::rng();
        let ncolors = config.ncolors.max(2) as usize;
        let count = config.count; // 0 means "whole vine per frame"

        let v = Vines {
            width: config.width,
            height: config.height,
            ncolors,
            count,
            delay_us: config.delay_us,

            a: 0,
            x1: 0,
            y1: 0,
            x2: 1,
            y2: 0,
            i: 0,
            length: 0,
            iterations: 30 + rng.random_range(0..100i32),
            constant: 1,
            ang: 0,
            centerx: 0,
            centery: 0,
            color: Color::new(255, 255, 255, 255),

            needs_clear: Cell::new(true),
            pending: Vec::new(),
        };
        // length=0 will trigger init_vine on the first tick.
        v
    }

    fn tick(&mut self) {
        self.pending.clear();

        // If we have finished the current vine (i >= length), start a new one.
        if self.i >= self.length {
            self.iterations -= 1;
            if self.iterations == 0 {
                // C: init_vines — clears window and resets iterations.
                self.full_reset();
                // Nothing more to draw this tick (just queued a clear).
                return;
            }
            self.init_vine();
        }

        // Determine how many steps to draw this tick.
        // C: count = fp->i + MI_COUNT(mi);
        //    if ((count <= fp->i) || (count > fp->length)) count = fp->length;
        let count_target: i64 = {
            let raw = self.i + self.count as i64;
            if raw <= self.i || raw > self.length {
                self.length
            } else {
                raw
            }
        };

        while self.i < count_target {
            // Draw line from (x1/constant, -y1/constant) to (x2/constant, -y2/constant)
            // relative to centre. C uses integer division (truncation toward zero).
            let px0 = self.centerx + (self.x1 / self.constant) as i32;
            let py0 = self.centery - (self.y1 / self.constant) as i32;
            let px1 = self.centerx + (self.x2 / self.constant) as i32;
            let py1 = self.centery - (self.y2 / self.constant) as i32;

            self.pending.push(LineOp {
                x0: px0,
                y0: py0,
                x1: px1,
                y1: py1,
                color: self.color,
            });

            // Update angle: a += ang * i  (before incrementing i, matching C).
            self.a += self.ang * self.i;

            // Shift (x1,y1) <- (x2,y2).
            self.x1 = self.x2;
            self.y1 = self.y2;

            // x2 += (int)(i * cos(a) * 360.0 / (2.0 * M_PI))
            // y2 += (int)(i * sin(a) * 360.0 / (2.0 * M_PI))
            // C casts to int, which truncates toward zero.
            let a_f = self.a as f64;
            let i_f = self.i as f64;
            let scale = 360.0 / (2.0 * PI);
            self.x2 += (i_f * a_f.cos() * scale) as i64;
            self.y2 += (i_f * a_f.sin() * scale) as i64;

            self.i += 1;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        // Consume the clear flag atomically: fire once, then reset.
        if self.needs_clear.get() {
            self.needs_clear.set(false);
            clear_buffer(buffer, Color::new(255, 0, 0, 0));
        }
        for op in &self.pending {
            draw_line(buffer, width, height, op.x0, op.y0, op.x1, op.y1, op.color);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.ncolors = config.ncolors.max(2) as usize;
        self.count = config.count;
        self.delay_us = config.delay_us;
        self.full_reset();
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
