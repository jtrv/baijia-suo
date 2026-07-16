/* xscreensaver, Copyright (c) 1997-2013 Jamie Zawinski <jwz@jwz.org>
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Concept snarfed from Michael D. Bayne in
 * http://samskivert.com/internet/deep/1997/04/16/body.html
 *
 * Rust port of xscreensaver's moire.c for baijia-suo.
 */

use crate::animation::primitives::{clear_buffer, hsv_to_rgb, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};
use rand::Rng;

// defaults from moire.c: *delay: 5 (seconds), *ncolors: 64, *offset: 50,
// *random: true, .background: blue, .foreground: red
const DELAY_SEC: u64 = 5;
const OFFSET: i32 = 50;
// Deviation from xscreensaver: moire.c draws 20-row chunks every 50ms
// (400 rows/s), a visibly steppy sweep. Draw 7-row chunks every 17.5ms —
// the same 400 rows/s, just finer-grained, so the scan glides instead of
// stepping. The finished-screen pause is unchanged.
const CHUNK_SIZE: i32 = 7;

fn rgb_to_hsv(r: u16, g: u16, b: u16) -> (i32, f64, f64) {
    let rr = r as f64 / 65535.0;
    let gg = g as f64 / 65535.0;
    let bb = b as f64 / 65535.0;
    let (mut cmax, mut cmin, mut imax) = (rr, gg, 1);
    if cmax < gg {
        cmax = gg;
        cmin = rr;
        imax = 2;
    }
    if cmax < bb {
        cmax = bb;
        imax = 3;
    }
    if cmin > bb {
        cmin = bb;
    }
    let cmm = cmax - cmin;
    let v = cmax;
    let (h, s) = if cmm == 0.0 {
        (0.0, 0.0)
    } else {
        let s = cmm / cmax;
        let mut h = match imax {
            1 => (gg - bb) / cmm,
            2 => 2.0 + (bb - rr) / cmm,
            _ => 4.0 + (rr - gg) / cmm,
        };
        if h < 0.0 {
            h += 6.0;
        }
        (h, s)
    };
    ((h * 60.0) as i32, s, v)
}

/* port of utils/colors.c make_color_ramp (color computation only) */
fn make_color_ramp(
    h1: i32,
    s1: f64,
    v1: f64,
    h2: i32,
    s2: f64,
    v2: f64,
    total: usize,
    closed: bool,
) -> Vec<Color> {
    let n = if closed { total / 2 + 1 } else { total };
    let dh = (h2 - h1) as f64 / n as f64;
    let ds = (s2 - s1) / n as f64;
    let dv = (v2 - v1) / n as f64;
    let mut out = vec![Color::new(255, 0, 0, 0); total];
    for i in 0..n.min(total) {
        let (r, g, b) = hsv_to_rgb(
            (h1 as f64 + i as f64 * dh) as i32,
            s1 + i as f64 * ds,
            v1 + i as f64 * dv,
        );
        out[i] = Color::new(255, (r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8);
    }
    if closed {
        for i in n..total {
            out[i] = out[total - i];
        }
    }
    out
}

pub struct Moire {
    width: u32,
    height: u32,
    ncolors: usize,
    colors: Vec<Color>,
    draw_y: i32,
    draw_xo: i32,
    draw_yo: i32,
    draw_factor: i32,
    pixels: Vec<u8>,
    next_delay_us: u64,
}

impl Moire {
    /* port of moire_init_1: *random is true by default, so both endpoint
    colors are re-picked at random for each screen */
    fn init_1(&mut self, rng: &mut impl Rng) {
        let (fr, fg, fb) = (
            rng.random_range(0..=0xFFFF_u32) as u16,
            rng.random_range(0..=0xFFFF_u32) as u16,
            rng.random_range(0..=0xFFFF_u32) as u16,
        );
        let (br, bg, bb) = (
            rng.random_range(0..=0xFFFF_u32) as u16,
            rng.random_range(0..=0xFFFF_u32) as u16,
            rng.random_range(0..=0xFFFF_u32) as u16,
        );
        let (fgh, fgs, fgv) = rgb_to_hsv(fr, fg, fb);
        let (bgh, bgs, bgv) = rgb_to_hsv(br, bg, bb);
        self.colors = make_color_ramp(fgh, fgs, fgv, bgh, bgs, bgv, self.ncolors, true);
    }
}

impl Animation for Moire {
    fn new(config: &AnimConfig) -> Self {
        let ncolors = config.ncolors.max(2) as usize;
        let mut pixels = vec![0u8; (config.width * config.height * 4) as usize];
        // .background: blue -- visible until the first pass completes
        clear_buffer(&mut pixels, Color::new(255, 0x00, 0x00, 0xFF));
        Moire {
            width: config.width,
            height: config.height,
            ncolors,
            colors: Vec::new(),
            draw_y: 0,
            draw_xo: 0,
            draw_yo: 0,
            draw_factor: 1,
            pixels,
            next_delay_us: DELAY_SEC * 3_500, // 7 rows per chunk (see CHUNK_SIZE), same rows/s as upstream
        }
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();
        let w = self.width as i32;
        let h = self.height as i32;

        if self.draw_y == 0 {
            self.init_1(&mut rng);
            self.draw_xo = rng.random_range(0..w.max(1)) - w / 2;
            self.draw_yo = rng.random_range(0..h.max(1)) - h / 2;
            self.draw_factor = rng.random_range(0..OFFSET) + 1;
        }

        for ii in 0..CHUNK_SIZE {
            let y = self.draw_y + ii;
            if y >= h {
                break;
            }
            for x in 0..w {
                let xx = (x + self.draw_xo) as f64;
                let yy = (y + self.draw_yo) as f64;
                let i = (xx * xx + yy * yy) / self.draw_factor as f64;
                let color = self.colors[(i as i64 % self.ncolors as i64) as usize];
                put_pixel(&mut self.pixels, self.width, self.height, x, y, color);
            }
        }
        self.draw_y += CHUNK_SIZE;

        if self.draw_y >= h {
            self.draw_y = 0;
            self.next_delay_us = DELAY_SEC * 1_000_000; // pause on the finished screen
        } else {
            self.next_delay_us = DELAY_SEC * 3_500;
        }
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        if buffer.len() == self.pixels.len() {
            buffer.copy_from_slice(&self.pixels);
        } else {
            clear_buffer(buffer, Color::new(255, 0, 0, 0));
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        *self = Self::new(config);
    }

    fn clears_each_frame(&self) -> bool {
        false
    }

    fn frame_delay_us(&self) -> u64 {
        self.next_delay_us
    }
}
