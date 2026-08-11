/* squiral, by "Jeff Epler" <jepler@inetnebr.com>, 18-mar-1999.
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Rust port of xscreensaver's squiral.c.
 */

use rand::RngExt;

use crate::animation::primitives::{clear_buffer, put_pixel, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const NCOLORSMAX: i32 = 255;

// Defaults from squiral.c's defaults table.
const DEF_DELAY_US: u64 = 10_000;
const DEF_FILL: f64 = 0.75;
const DEF_DISORDER: f64 = 0.005;
const DEF_HANDEDNESS: f64 = 0.5;
const DEF_CYCLE: bool = false;
const DEF_SCALE: i32 = 1;

/* Exact port of hsv_to_rgb from utils/hsv.c (output scaled to 8 bits). */
// 8-bit-direct rounding ((x*255.0) as u8) differs from primitives::hsv_to_rgb's
// 16-bit path (e.g. v=0.999 -> 254 here vs 255 there); kept for port fidelity.
fn hsv_to_rgb(h: i32, s: f64, v: f64) -> Color {
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let hh = (h % 360) as f64 / 60.0;
    let i = hh as i32;
    let f = hh - i as f64;
    let p1 = v * (1.0 - s);
    let p2 = v * (1.0 - s * f);
    let p3 = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i {
        0 => (v, p3, p1),
        1 => (p2, v, p1),
        2 => (p1, v, p3),
        3 => (p1, p2, v),
        4 => (p3, p1, v),
        _ => (v, p1, p2),
    };
    Color::new(
        255,
        (r * 255.0) as u8,
        (g * 255.0) as u8,
        (b * 255.0) as u8,
    )
}

#[derive(Clone, Copy)]
struct Worm {
    h: i32,
    v: i32,
    /* 0-3 left-winding, 4-7 right-winding: s = type*4 + dir */
    s: i32,
    c: i32,
    cc: i32,
}

pub struct Squiral {
    width: i32,  // grid width  (pixels / scale)
    height: i32, // grid height
    count: usize,
    cycle: bool,
    frac: f64,
    disorder: f64,
    handedness: f64,
    ncolors: i32,
    colors: Vec<Color>,
    delay_us: u64,
    cov: i32,
    dirh: [i32; 4],
    dirv: [i32; 4],
    fill: Vec<bool>,
    worms: Vec<Worm>,
    inclear: i32,
    scale: i32,
    canvas: Vec<u8>,
    pix_w: u32,
    pix_h: u32,
}

impl Squiral {
    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        for yy in y..y + h {
            for xx in x..x + w {
                put_pixel(&mut self.canvas, self.pix_w, self.pix_h, xx, yy, color);
            }
        }
    }

    fn clear1(&self, x: i32, y: i32) -> bool {
        !self.fill[((y % self.height) * self.width + x % self.width) as usize]
    }

    fn move1(&mut self, x: i32, y: i32, color: Color) {
        let gx = x % self.width;
        let gy = y % self.height;
        self.fill[(gy * self.width + gx) as usize] = true;
        self.fill_rect(gx * self.scale, gy * self.scale, self.scale, self.scale, color);
        self.cov += 1;
    }

    // CLEAR(d): both cells one and two steps in direction d are unfilled.
    fn clear_dir(&self, h: i32, v: i32, d: usize) -> bool {
        let dx = self.dirh[d];
        let dy = self.dirv[d];
        self.clear1(h + dx, v + dy) && self.clear1(h + dx + dx, v + dy + dy)
    }

    fn do_worm(&mut self, i: usize) {
        let mut rng = rand::rng();
        let mut w = self.worms[i];
        let mut typ = w.s / 4;
        let mut dir = (w.s % 4) as usize;

        w.c = (w.c + w.cc) % self.ncolors;

        if rng.random::<f64>() < self.disorder {
            typ = (rng.random::<f64>() < self.handedness) as i32;
        }
        // case 0: try CCW, straight, CW; case 1: the reverse.
        let tries = if typ == 0 {
            [(dir + 3) % 4, dir, (dir + 1) % 4]
        } else {
            [(dir + 1) % 4, dir, (dir + 3) % 4]
        };
        let mut moved = false;
        for d in tries {
            if self.clear_dir(w.h, w.v, d) {
                let color = self.colors[w.c as usize];
                let dx = self.dirh[d];
                let dy = self.dirv[d];
                self.move1(w.h + dx, w.v + dy, color);
                self.move1(w.h + dx + dx, w.v + dy + dy, color);
                w.h += dx * 2;
                w.v += dy * 2;
                dir = d;
                moved = true;
                break;
            }
        }
        if !moved {
            // RANDOM: restart the worm somewhere else.
            w.h = rng.random_range(0..self.width);
            w.v = rng.random_range(0..self.height);
            w.c = rng.random_range(0..self.ncolors);
            typ = rng.random_range(0..2);
            dir = rng.random_range(0..4);
            if self.cycle {
                w.cc = rng.random_range(0..3) + self.ncolors;
            }
        }
        w.s = typ * 4 + dir as i32;
        w.h %= self.width;
        w.v %= self.height;
        self.worms[i] = w;
    }

    fn erase_row(&mut self, row: i32) {
        let black = Color::new(255, 0, 0, 0);
        self.fill_rect(
            0,
            row * self.scale,
            (self.width - 1) * self.scale,
            self.scale,
            black,
        );
    }

    fn clear_fill_row(&mut self, row: i32) {
        let start = (row * self.width) as usize;
        for c in &mut self.fill[start..start + self.width as usize] {
            *c = false;
        }
    }

    // squiral_init_1
    fn init_1(&mut self) {
        let mut rng = rand::rng();
        self.fill = vec![false; (self.width * self.height) as usize];
        self.dirh = [0, 1, 0, self.width - 1];
        self.dirv = [self.height - 1, 0, 1, 0];
        self.worms = (0..self.count)
            .map(|_| Worm {
                h: rng.random_range(0..self.width),
                v: rng.random_range(0..self.height),
                s: rng.random_range(0..4)
                    + 4 * (rng.random::<f64>() < self.handedness) as i32,
                c: rng.random_range(0..self.ncolors),
                cc: if self.cycle {
                    rng.random_range(0..3) + self.ncolors
                } else {
                    0
                },
            })
            .collect();
    }
}

impl Animation for Squiral {
    fn new(config: &AnimConfig) -> Self {
        let mut s = Squiral {
            width: 1,
            height: 1,
            count: 1,
            cycle: DEF_CYCLE,
            frac: DEF_FILL,
            disorder: DEF_DISORDER,
            handedness: DEF_HANDEDNESS,
            ncolors: 1,
            colors: Vec::new(),
            delay_us: DEF_DELAY_US,
            cov: 0,
            dirh: [0; 4],
            dirv: [0; 4],
            fill: Vec::new(),
            worms: Vec::new(),
            inclear: 0,
            scale: DEF_SCALE,
            canvas: Vec::new(),
            pix_w: config.width,
            pix_h: config.height,
        };
        s.reset(config);
        s
    }

    fn tick(&mut self) {
        // The double-row wipe from squiral_draw.
        if self.inclear < self.height {
            self.erase_row(self.inclear);
            self.clear_fill_row(self.inclear);
            self.erase_row(self.height - self.inclear - 1);
            self.clear_fill_row(self.height - self.inclear - 1);
            self.inclear += 1;
            self.erase_row(self.inclear);
            if self.inclear < self.height {
                self.clear_fill_row(self.inclear);
            }
            self.erase_row(self.height - self.inclear - 1);
            if self.height - self.inclear >= 1 {
                self.clear_fill_row(self.height - self.inclear - 1);
            }
            self.inclear += 1;
            if self.inclear > self.height / 2 {
                self.inclear = self.height;
            }
        } else if self.cov as f64 > self.frac * self.width as f64 * self.height as f64 {
            self.inclear = 0;
            self.cov = 0;
        }
        for i in 0..self.worms.len() {
            self.do_worm(i);
        }
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        let len = self.canvas.len().min(buffer.len());
        buffer[..len].copy_from_slice(&self.canvas[..len]);
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.delay_us = DEF_DELAY_US;
        self.pix_w = config.width;
        self.pix_h = config.height;

        self.scale = DEF_SCALE;
        if config.width > 2560 || config.height > 2560 {
            self.scale *= 3; /* Retina displays */
        }
        self.width = (config.width as i32 / self.scale).max(1);
        self.height = (config.height as i32 / self.scale).max(1);

        self.ncolors = config.ncolors;
        if self.ncolors < 0 || self.ncolors > NCOLORSMAX {
            self.ncolors = NCOLORSMAX;
        }
        if self.ncolors > 0 {
            // make_uniform_colormap: hue ramp 0..359 with random S,V in 66%-99%.
            let s = (rng.random_range(0..34) + 66) as f64 / 100.0;
            let v = (rng.random_range(0..34) + 66) as f64 / 100.0;
            self.colors = (0..self.ncolors)
                .map(|i| hsv_to_rgb((i as f64 * 359.0 / self.ncolors as f64) as i32, s, v))
                .collect();
        }
        if self.ncolors <= 0 {
            self.ncolors = 1;
            self.colors = vec![Color::new(255, 255, 255, 255)];
        }

        self.frac = DEF_FILL.clamp(0.01, 0.99);
        self.cycle = DEF_CYCLE;
        self.disorder = DEF_DISORDER;
        self.handedness = DEF_HANDEDNESS;

        let mut count = config.count;
        if count == 0 {
            count = self.width / 32;
        }
        self.count = count.clamp(1, 1000) as usize;

        self.cov = 0;
        self.inclear = 0;

        let canvas_len = (config.width * config.height * 4) as usize;
        self.canvas = vec![0u8; canvas_len];
        // Opaque black background so undrawn pixels are not transparent.
        clear_buffer(&mut self.canvas, Color::new(255, 0, 0, 0));

        self.init_1();
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::CompleteFrame
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
