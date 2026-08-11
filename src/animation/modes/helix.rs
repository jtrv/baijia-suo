//! String art.
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
// Rust port of xlockmore/modes/helix.c.

use rand::RngExt;
use std::f64::consts::PI;
use crate::animation::primitives::{draw_line, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const ANGLES: usize = 360;

fn build_trig_tables() -> ([f64; ANGLES], [f64; ANGLES]) {
    let mut cos_array = [0.0f64; ANGLES];
    let mut sin_array = [0.0f64; ANGLES];
    for i in 0..ANGLES {
        let theta = (i as f64 / (ANGLES / 2) as f64) * PI;
        cos_array[i] = theta.cos();
        sin_array[i] = theta.sin();
    }
    (cos_array, sin_array)
}

// True modulo — always returns a value in [0, y).
#[inline]
fn pmod(x: i32, y: i32) -> usize {
    let r = x % y;
    (if r >= 0 { r } else { r + y }) as usize
}

fn gcd(a: i32, b: i32) -> i32 {
    let mut a = a;
    let mut b = b;
    while b > 0 {
        let tmp = a % b;
        a = b;
        b = tmp;
    }
    if a < 0 { -a } else { a }
}

struct HelixParams {
    radius1: i32,
    radius2: i32,
    d_angle: i32,
    factor1: i32,
    factor2: i32,
    factor3: i32,
    factor4: i32,
}

struct TrigParams {
    d_angle: i32,
    d_angle_offset: i32,
    factor1: i32,
    factor2: i32,
    offset: i32,
    density: i32,
    dir: i32,
}

enum Mode {
    Helix(HelixParams),
    Trig(TrigParams),
}

pub struct Helix {
    width: u32,
    height: u32,
    xmid: i32,
    ymid: i32,
    color: usize,
    ncolors: usize,
    time: i32,
    cycles: i32,
    mode: Mode,
    cos_array: [f64; ANGLES],
    sin_array: [f64; ANGLES],
    // Pixel buffer owned by this animation; flushed into the external buffer on render.
    canvas: Vec<u8>,
    delay_us: u64,
}

impl Helix {
    fn color_advance(&mut self) -> Color {
        let c = self.make_color(self.color);
        self.color += 1;
        if self.color >= self.ncolors {
            self.color = 0;
        }
        c
    }

    fn make_color(&self, idx: usize) -> Color {
        Color::from_hsl(idx as f32 / self.ncolors as f32, 1.0, 0.5)
    }

    fn draw_helix_pattern(&mut self) {
        let (radius1, radius2, d_angle, factor1, factor2, factor3, factor4) = match &self.mode {
            Mode::Helix(p) => (p.radius1, p.radius2, p.d_angle, p.factor1, p.factor2, p.factor3, p.factor4),
            Mode::Trig(_) => return,
        };

        let limit = 1 + ANGLES as i32 / gcd(ANGLES as i32, d_angle);

        // xlockmore calls XSetForeground (advance) once before the loop, then once more
        // before each XDrawLine inside the loop. Match that initial advance here.
        self.color_advance();

        // Starting point mirrors xlockmore: x_2 = xmid, y_2 = ymid + radius1.
        let mut x_2 = self.xmid;
        let mut y_2 = self.ymid + radius1;

        let w = self.width;
        let h = self.height;

        for i in 0..limit {
            let angle = i * d_angle;
            let x_1 = self.xmid + (radius1 as f64 * self.sin_array[pmod(angle * factor1, ANGLES as i32)]) as i32;
            let y_1 = self.ymid + (radius2 as f64 * self.cos_array[pmod(angle * factor2, ANGLES as i32)]) as i32;

            let color = self.color_advance();
            draw_line(&mut self.canvas, w, h, x_1, y_1, x_2, y_2, color);

            x_2 = self.xmid + (radius2 as f64 * self.sin_array[pmod(angle * factor3, ANGLES as i32)]) as i32;
            y_2 = self.ymid + (radius1 as f64 * self.cos_array[pmod(angle * factor4, ANGLES as i32)]) as i32;

            let color = self.color_advance();
            draw_line(&mut self.canvas, w, h, x_1, y_1, x_2, y_2, color);
        }
    }

    fn draw_trig_pattern(&mut self) {
        let (factor1, factor2, d_angle_offset, offset, density, dir) = match &self.mode {
            Mode::Trig(p) => (p.factor1, p.factor2, p.d_angle_offset, p.offset, p.density, p.dir),
            Mode::Helix(_) => return,
        };

        let w = self.width;
        let h = self.height;

        let mut d_angle = match &self.mode {
            Mode::Trig(p) => p.d_angle,
            Mode::Helix(_) => return,
        };

        // random_trig sets the GC to color[C] then advances to C+1 before calling trig().
        // trig() draws with the GC color already set (color[C]), then advances after each draw.
        // We capture the pre-advance color for the first draw, then advance at the end of each step.
        let mut draw_color = self.make_color(self.color);
        self.color += 1;
        if self.color >= self.ncolors {
            self.color = 0;
        }

        while d_angle.abs() <= ANGLES as i32 {
            let angle = d_angle + d_angle_offset;
            let x_1 = (self.sin_array[pmod(angle * factor1, ANGLES as i32)] * self.xmid as f64) as i32 + self.xmid;
            let y_1 = (self.cos_array[pmod(angle * factor1, ANGLES as i32)] * self.ymid as f64) as i32 + self.ymid;
            let x_2 = (self.sin_array[pmod(angle * factor2 + offset, ANGLES as i32)] * self.xmid as f64) as i32 + self.xmid;
            let y_2 = (self.cos_array[pmod(angle * factor2 + offset, ANGLES as i32)] * self.ymid as f64) as i32 + self.ymid;

            draw_line(&mut self.canvas, w, h, x_1, y_1, x_2, y_2, draw_color);

            // Advance color after the draw, set next draw_color.
            draw_color = self.make_color(self.color);
            self.color += 1;
            if self.color >= self.ncolors {
                self.color = 0;
            }

            let mut step = ANGLES as i32 / (2 * density * factor1 * factor2);
            if step == 0 {
                // Avoid infinite loop — xlockmore comment: "Would not need if floating point"
                step = 1;
            }
            d_angle += dir * step;
        }

        if let Mode::Trig(p) = &mut self.mode {
            p.d_angle = d_angle;
        }
    }

    fn randomize(&mut self) {
        let mut rng = rand::rng();

        self.color = rng.random_range(0..self.ncolors);

        // 1:5 chance of trig/ellipse mode (matches fullrandom behavior in init_helix).
        let use_trig = rng.random_range(0..5u32) == 0;

        if use_trig {
            let factor1 = rng.random_range(1..=8i32);
            let factor2 = loop {
                let f = rng.random_range(1..=8i32);
                if f != factor1 { break f; }
            };
            let dir = if rng.random_range(0..2u32) == 0 { 1i32 } else { -1 };
            let d_angle_offset = rng.random_range(0..ANGLES as i32);
            let offset = (rng.random_range(0..(ANGLES as i32 / 4 - 1)) + 1) / 4;
            let density = 1 << (rng.random_range(0..4u32) + 4);

            self.mode = Mode::Trig(TrigParams {
                d_angle: 0,
                d_angle_offset,
                factor1,
                factor2,
                offset,
                density,
                dir,
            });
            self.draw_trig_pattern();
        } else {
            let radius = self.xmid.min(self.ymid);
            // divisor in range [1,4) * random sign, matching:
            //   LRAND()/MAXRAND * 3.0 + 1   ->  [1, 4)
            //   (LRAND() & 1) * 2 - 1       ->  +1 or -1
            let divisor_mag: f64 = rng.random::<f64>() * 3.0 + 1.0;
            let divisor_sign: f64 = if rng.random_range(0..2u32) == 0 { 1.0 } else { -1.0 };
            let divisor = divisor_mag * divisor_sign;

            let (radius1, radius2) = if rng.random_range(0..2u32) == 0 {
                (radius, (radius as f64 / divisor) as i32)
            } else {
                ((radius as f64 / divisor) as i32, radius)
            };

            let mut d_angle = 0i32;
            while gcd(ANGLES as i32, d_angle) >= 2 {
                d_angle = rng.random_range(0..ANGLES as i32);
            }

            let random_factor = |rng: &mut rand::rngs::ThreadRng| -> i32 {
                let mag = if rng.random_range(0..7u32) != 0 {
                    (rng.random_range(0..2u32) + 1) as i32
                } else {
                    3
                };
                let sign = (rng.random_range(0..2u32) as i32 * 2) - 1;
                mag * sign
            };

            let (factor1, factor2, factor3, factor4) = loop {
                let f1 = random_factor(&mut rng);
                let f2 = random_factor(&mut rng);
                let f3 = random_factor(&mut rng);
                let f4 = random_factor(&mut rng);
                if gcd(gcd(gcd(f1, f2), f3), f4) == 1 {
                    break (f1, f2, f3, f4);
                }
            };

            self.mode = Mode::Helix(HelixParams {
                radius1,
                radius2,
                d_angle,
                factor1,
                factor2,
                factor3,
                factor4,
            });
            self.draw_helix_pattern();
        }
    }
}

impl Animation for Helix {
    fn new(config: &AnimConfig) -> Self {
        let (cos_array, sin_array) = build_trig_tables();
        let canvas_len = (config.width * config.height * 4) as usize;
        let ncolors = config.ncolors.max(2) as usize;
        let cycles = if config.cycles <= 0 { 100 } else { config.cycles };

        let mut h = Helix {
            width: config.width,
            height: config.height,
            xmid: config.width as i32 / 2,
            ymid: config.height as i32 / 2,
            color: 0,
            ncolors,
            time: 0,
            cycles,
            mode: Mode::Helix(HelixParams {
                radius1: 0,
                radius2: 0,
                d_angle: 1,
                factor1: 1,
                factor2: 1,
                factor3: 1,
                factor4: 1,
            }),
            cos_array,
            sin_array,
            canvas: {
                // Initialize to opaque black so undrawn pixels are not transparent.
                let mut v = vec![0u8; canvas_len];
                let pixels = unsafe { std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut u32, canvas_len / 4) };
                pixels.fill(0xFF00_0000u32);
                v
            },
            delay_us: config.delay_us,
        };
        h.randomize();
        h
    }

    fn tick(&mut self) {
        self.time += 1;
        if self.time > self.cycles {
            // Clear canvas to opaque black and redraw a new pattern.
            let pixels = unsafe { std::slice::from_raw_parts_mut(self.canvas.as_mut_ptr() as *mut u32, self.canvas.len() / 4) };
            pixels.fill(0xFF00_0000u32);
            self.time = 0;
            self.randomize();
        }
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        let len = self.canvas.len().min(buffer.len());
        buffer[..len].copy_from_slice(&self.canvas[..len]);
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.xmid = config.width as i32 / 2;
        self.ymid = config.height as i32 / 2;
        self.ncolors = config.ncolors.max(2) as usize;
        self.cycles = if config.cycles <= 0 { 100 } else { config.cycles };
        self.delay_us = config.delay_us;
        self.time = 0;
        let canvas_len = (config.width * config.height * 4) as usize;
        self.canvas = {
            let mut v = vec![0u8; canvas_len];
            let pixels = unsafe { std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut u32, canvas_len / 4) };
            pixels.fill(0xFF00_0000u32);
            v
        };
        self.randomize();
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::CompleteFrame
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
