/* flame --- recursive fractal cosmic flames
 *
 * Copyright (c) 1991 by Patrick J. Naughton.
 *
 * Permission to use, copy, modify, and distribute this software and its
 * documentation for any purpose and without fee is hereby granted,
 * provided that the above copyright notice appear in all copies and that
 * both that copyright notice and this permission notice appear in
 * supporting documentation.
 *
 * Rust port of xlockmore/modes/flame.c (5.00 2000/11/01).
 */

use std::cell::Cell;

use crate::animation::primitives::{clear_buffer, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};
use rand::RngExt;

const MAXLEV: usize = 4;
const MAXKINDS: usize = 9;
const MAXBATCH: usize = 12;
// xlockmore flame defaults: delay 750000, count 20, cycles 10000.
const DEF_DELAY_US: u64 = 750_000;
const DEF_COUNT: i32 = 20;
const DEF_CYCLES: u32 = 10_000;

pub struct Flame {
    f: [[[f64; MAXLEV]; 3]; 2],
    variation: usize,
    max_levels: usize,
    cur_level: usize,
    snum: usize,
    anum: usize,
    width: u32,
    height: u32,
    num_points: usize,
    total_points: u32,
    pixcol: i32,
    cycles: u32,
    alt: bool,
    pts: [(i32, i32); MAXBATCH],
    lasthalf: u16,
    ncolors: i32,
    pixel_buf: Vec<(i32, i32, Color)>,
    color: Color,
    // Cleared inside render() and set in tick(); Cell allows mutation through &self.
    needs_clear: Cell<bool>,
}

impl Flame {
    fn halfrandom(&mut self, mv: u32) -> u32 {
        let r: u32;
        if self.lasthalf != 0 {
            r = self.lasthalf as u32;
            self.lasthalf = 0;
        } else {
            // LRAND() is a 31-bit nonnegative value; the saved half is bits 16..31 (15 bits).
            let full: u32 = rand::rng().random::<u32>() >> 1;
            self.lasthalf = (full >> 16) as u16;
            r = full;
        }
        r % mv
    }

    fn lrand_coef() -> f64 {
        let bits: u32 = rand::rng().random::<u32>() & 1023;
        bits as f64 / 512.0 - 1.0
    }

    fn pixcol_to_color(pixcol: i32, ncolors: i32) -> Color {
        if ncolors > 2 {
            Color::from_hsl(pixcol as f32 / ncolors as f32, 1.0, 0.5)
        } else {
            Color::new(255, 255, 255, 255)
        }
    }

    fn recurse(
        pts: &mut [(i32, i32); MAXBATCH],
        num_points: &mut usize,
        total_points: &mut u32,
        pixel_buf: &mut Vec<(i32, i32, Color)>,
        f: &[[[f64; MAXLEV]; 3]; 2],
        variation: usize,
        max_levels: usize,
        snum: usize,
        anum: usize,
        width: u32,
        height: u32,
        cycles: u32,
        color: Color,
        x: f64,
        y: f64,
        l: usize,
    ) -> bool {
        if l == max_levels {
            *total_points += 1;
            if *total_points > cycles {
                return false;
            }

            if x > -1.0 && x < 1.0 && y > -1.0 && y < 1.0 {
                let px = ((width as f64 / 2.0) * (x + 1.0)) as i32;
                let py = ((height as f64 / 2.0) * (y + 1.0)) as i32;
                pts[*num_points] = (px, py);
                *num_points += 1;
                if *num_points >= MAXBATCH {
                    for k in 0..*num_points {
                        pixel_buf.push((pts[k].0, pts[k].1, color));
                    }
                    *num_points = 0;
                }
            }
        } else {
            for i in 0..snum {
                let mut nx = f[0][0][i] * x + f[0][1][i] * y + f[0][2][i];
                let mut ny = f[1][0][i] * x + f[1][1][i] * y + f[1][2][i];
                if i < anum {
                    match variation {
                        0 => {
                            nx = nx.sin();
                            ny = ny.sin();
                        }
                        1 => {
                            let r2 = nx * nx + ny * ny + 1e-6;
                            nx /= r2;
                            ny /= r2;
                        }
                        2 => {
                            if nx < 0.0 {
                                nx *= 2.0;
                            }
                            if ny < 0.0 {
                                ny /= 2.0;
                            }
                        }
                        3 => {
                            let r = nx * nx + ny * ny;
                            let c1 = r.sin();
                            let c2 = r.cos();
                            let t = nx;
                            if !(-1e4..=1e4).contains(&nx) || !(-1e4..=1e4).contains(&ny) {
                                ny = 1e4;
                            } else {
                                ny = c2 * t + c1 * ny;
                            }
                            // xlockmore quirk: nx is computed after ny, so it uses the
                            // already-updated ny value instead of the original.
                            nx = c1 * nx - c2 * ny;
                        }
                        4 => {
                            let r = if nx == 0.0 && ny == 0.0 {
                                0.0
                            } else {
                                nx.atan2(ny)
                            };
                            let c1 = r.sin();
                            let c2 = r.cos();
                            let t = nx;
                            nx = c1 * nx - c2 * ny;
                            ny = c2 * t + c1 * ny;
                        }
                        5 => {
                            let t = if nx == 0.0 && ny == 0.0 {
                                0.0
                            } else {
                                nx.atan2(ny) / std::f64::consts::PI
                            };
                            if !(-1e4..=1e4).contains(&nx) || !(-1e4..=1e4).contains(&ny) {
                                ny = 1e4;
                            } else {
                                ny = (nx * nx + ny * ny).sqrt() - 1.0;
                            }
                            nx = t;
                        }
                        6 => {
                            if nx > 1.0 {
                                nx -= 1.0;
                            }
                            if nx < -1.0 {
                                nx += 1.0;
                            }
                            if ny > 1.0 {
                                ny -= 1.0;
                            }
                            if ny < -1.0 {
                                ny += 1.0;
                            }
                        }
                        7 => {
                            let r = 0.5 + (nx * nx + ny * ny + 1e-6).sqrt();
                            nx /= r;
                            ny /= r;
                        }
                        8 => {
                            nx = nx.atan() / std::f64::consts::FRAC_PI_2;
                            ny = ny.atan() / std::f64::consts::FRAC_PI_2;
                        }
                        _ => {
                            nx = nx.sin();
                            ny = ny.sin();
                        }
                    }
                }
                if !Self::recurse(
                    pts,
                    num_points,
                    total_points,
                    pixel_buf,
                    f,
                    variation,
                    max_levels,
                    snum,
                    anum,
                    width,
                    height,
                    cycles,
                    color,
                    nx,
                    ny,
                    l + 1,
                ) {
                    return false;
                }
            }
        }
        true
    }
}

impl Animation for Flame {
    fn new(config: &AnimConfig) -> Self {
        let mut flame = Flame {
            f: [[[0.0; MAXLEV]; 3]; 2],
            variation: 0,
            max_levels: 1,
            cur_level: 0,
            snum: 0,
            anum: 0,
            width: config.width,
            height: config.height,
            num_points: 0,
            total_points: 0,
            pixcol: 0,
            cycles: 0,
            alt: false,
            pts: [(0, 0); MAXBATCH],
            lasthalf: 0,
            ncolors: config.ncolors,
            pixel_buf: Vec::new(),
            color: Color::new(255, 255, 255, 255),
            needs_clear: Cell::new(true),
        };
        flame.reset(config);
        flame
    }

    fn tick(&mut self) {
        // cur_level starts at 0; the check mirrors `!(fp->cur_level++ % fp->max_levels)`.
        if self.cur_level.is_multiple_of(self.max_levels) {
            self.needs_clear.set(true);
            if self.ncolors <= 2 {
                self.color = Color::new(255, 255, 255, 255);
            }
            self.alt = !self.alt;
        } else if self.ncolors > 2 {
            // C sets the foreground from the current pixcol, THEN decrements it.
            self.color = Self::pixcol_to_color(self.pixcol, self.ncolors);
            self.pixcol -= 1;
            if self.pixcol < 0 {
                self.pixcol = self.ncolors - 1;
            }
        }

        self.cur_level += 1;

        // C code: fp->snum = 2 + (fp->cur_level % (MAXLEV - 1)) after the post-increment,
        // so snum uses the already-incremented cur_level.
        self.snum = 2 + (self.cur_level % (MAXLEV - 1));

        if self.alt {
            self.anum = 0;
        } else {
            let sn = self.snum as u32;
            self.anum = self.halfrandom(sn) as usize + 2;
        }

        for k in 0..self.snum {
            for i in 0..2 {
                for j in 0..3 {
                    self.f[i][j][k] = Self::lrand_coef();
                }
            }
        }

        self.num_points = 0;
        self.total_points = 0;

        let color = self.color;
        let mut pts = self.pts;
        let mut num_points = 0usize;
        let mut total_points = 0u32;
        let mut pixel_buf = std::mem::take(&mut self.pixel_buf);
        pixel_buf.clear();

        Self::recurse(
            &mut pts,
            &mut num_points,
            &mut total_points,
            &mut pixel_buf,
            &self.f,
            self.variation,
            self.max_levels,
            self.snum,
            self.anum,
            self.width,
            self.height,
            self.cycles,
            color,
            0.0,
            0.0,
            0,
        );

        // Flush remaining buffered points, mirroring the final XDrawPoints call in draw_flame.
        for k in 0..num_points {
            pixel_buf.push((pts[k].0, pts[k].1, color));
        }

        self.pts = pts;
        self.num_points = num_points;
        self.total_points = total_points;
        self.pixel_buf = pixel_buf;
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if self.needs_clear.get() {
            clear_buffer(buffer, Color::new(255, 0, 0, 0));
            self.needs_clear.set(false);
        }
        for &(x, y, color) in &self.pixel_buf {
            put_pixel(buffer, width, height, x, y, color);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.ncolors = config.ncolors;
        self.lasthalf = 0;
        self.cur_level = 0;
        self.alt = false;
        self.num_points = 0;
        self.total_points = 0;
        self.pixel_buf.clear();
        self.needs_clear.set(true);

        // xlockmore default count=20; clamp to minimum 1 as C code does
        let count = if config.count == 0 { DEF_COUNT } else { config.count };
        self.max_levels = count.max(1) as usize;

        // xlockmore default cycles=10000
        self.cycles = if config.cycles <= 0 {
            DEF_CYCLES
        } else {
            config.cycles as u32
        };

        if self.ncolors > 2 {
            let np = self.ncolors as u32;
            self.pixcol = self.halfrandom(np) as i32;
            self.color = Self::pixcol_to_color(self.pixcol, self.ncolors);
        } else {
            self.color = Color::new(255, 255, 255, 255);
        }

        // C uses NRAND(MAXKINDS) here, not halfrandom — must not disturb lasthalf.
        self.variation = rand::rng().random_range(0..MAXKINDS);
    }


    fn frame_delay_us(&self) -> u64 {
        DEF_DELAY_US
    }
}
