//! Rorschach's ink blot test.
//
// Copyright (c) 1992 by Jamie Zawinski
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
// Rust port of xlockmore/modes/blot.c.

use crate::rng::RngExt;
use std::cell::Cell;

use crate::animation::primitives::{clear_buffer, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

// Defaults matching xlockmore
const DEFAULT_DELAY_US: u64 = 200_000;
const DEFAULT_COUNT: i32 = 6;
const DEFAULT_CYCLES: i32 = 30;
const DEFAULT_NCOLORS: i32 = 200;

pub struct Blot {
    width: u32,
    height: u32,
    xmid: i32,
    ymid: i32,
    offset: i32,
    xsym: bool,
    ysym: bool,
    size: i32,
    pix: i32,
    count: i32,
    orig_count: i32,

    // config limits
    cycles: i32,
    ncolors: i32,
    delay_us: u64,

    // state
    points: Vec<(i32, i32)>,
    clear_frames: Cell<u8>,
    current_color: Color,
}

impl Blot {
    fn init(&mut self) {
        let mut rng = crate::rng::rng();

        self.xmid = (self.width / 2) as i32;
        self.ymid = (self.height / 2) as i32;

        self.offset = 4;
        self.ysym = rng.random::<bool>();
        self.xsym = if self.ysym { rng.random::<bool>() } else { true };

        if self.ncolors > 2 {
            self.pix = rng.random_range(0..self.ncolors);
        }

        if self.offset <= 0 {
            self.offset = 3;
        }

        let c = if self.orig_count == 0 { DEFAULT_COUNT } else { self.orig_count };
        if c < 0 {
            self.size = rng.random_range(0..(-c + 1));
        } else {
            self.size = c;
        }

        // Fudge the size so it takes up the whole screen
        self.size *= ((self.width / 32) + 1) as i32 * ((self.height / 32) + 1) as i32;
        // Deviation from xlockmore: with the edge reflection below keeping
        // the whole walk visible (upstream lost off-screen excursions), the
        // full fudged length reads as a longer, denser wander. Trim to
        // compensate; tune the factor here if blots feel too thin/dense.
        self.size = self.size * 3 / 4;

        self.count = 0;
        self.clear_frames.set(2);
    }
}

impl Animation for Blot {
    fn new(config: &AnimConfig) -> Self {
        let mut blot = Blot {
            width: config.width,
            height: config.height,
            xmid: 0,
            ymid: 0,
            offset: 0,
            xsym: false,
            ysym: false,
            size: 0,
            pix: 0,
            count: 0,
            orig_count: config.count,
            cycles: if config.cycles <= 0 { DEFAULT_CYCLES } else { config.cycles },
            ncolors: if config.ncolors <= 0 { DEFAULT_NCOLORS } else { config.ncolors },
            delay_us: if config.delay_us == 0 { DEFAULT_DELAY_US } else { config.delay_us },
            points: Vec::new(),
            clear_frames: Cell::new(2),
            current_color: Color::new(255, 255, 255, 255),
        };
        blot.init();
        blot
    }

    fn tick(&mut self) {
        let mut rng = crate::rng::rng();

        if self.count > self.cycles {
            self.init();
            self.points.clear();
            return;
        }

        if self.ncolors > 2 {
            self.current_color = Color::from_hsl(self.pix as f32 / self.ncolors as f32, 1.0, 0.5);
            self.pix += 1;
            if self.pix >= self.ncolors {
                self.pix = 0;
            }
        } else {
            self.current_color = Color::new(255, 255, 255, 255);
        }

        self.points.clear();
        let mut x = self.xmid;
        let mut y = self.ymid;
        let mut k = self.size;

        while k >= 4 {
            let dx = rng.random_range(0..(1 + (self.offset << 1))) - self.offset;
            let dy = rng.random_range(0..(1 + (self.offset << 1))) - self.offset;
            x += dx;
            y += dy;

            // Deviation from xlockmore: the C lets the random walk wander
            // off-screen (X11 just clips), which often leaves much of the
            // blot invisible. Reflect the walk at the edges instead — same
            // step statistics, but the blot stays composed on screen.
            if x < 0 {
                x = -x;
            } else if x >= self.width as i32 {
                x = 2 * (self.width as i32 - 1) - x;
            }
            if y < 0 {
                y = -y;
            } else if y >= self.height as i32 {
                y = 2 * (self.height as i32 - 1) - y;
            }

            k -= 1;
            self.points.push((x, y));

            if self.xsym {
                k -= 1;
                self.points.push((self.width as i32 - x, y));
            }
            if self.ysym {
                k -= 1;
                self.points.push((x, self.height as i32 - y));
            }
            if self.xsym && self.ysym {
                k -= 1;
                self.points.push((self.width as i32 - x, self.height as i32 - y));
            }
        }

        self.xmid = x;
        self.ymid = y;
        self.count += 1;
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let cf = self.clear_frames.get();
        if cf > 0 {
            clear_buffer(buffer, Color::new(255, 0, 0, 0));
            self.clear_frames.set(cf - 1);
        }

        for &(px, py) in &self.points {
            put_pixel(buffer, width, height, px, py, self.current_color);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.orig_count = config.count;
        self.cycles = if config.cycles <= 0 { DEFAULT_CYCLES } else { config.cycles };
        self.ncolors = if config.ncolors <= 0 { DEFAULT_NCOLORS } else { config.ncolors };
        self.delay_us = if config.delay_us == 0 { DEFAULT_DELAY_US } else { config.delay_us };
        self.init();
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
