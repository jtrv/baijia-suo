/* drift --- drifting recursive fractal cosmic flames */

/*-
 * Copyright (c) 1991 by Patrick J. Naughton.
 *
 * Permission to use, copy, modify, and distribute this software and its
 * documentation for any purpose and without fee is hereby granted,
 * provided that the above copyright notice appear in all copies and that
 * both that copyright notice and this permission notice appear in
 * supporting documentation.
 *
 * This file is provided AS IS with no warranties of any kind.  The author
 * shall have no liability with respect to the infringement of copyrights,
 * trade secrets or any patents by this file or any part thereof.  In no
 * event will the author be liable for any lost revenue or profits or
 * other special, indirect and consequential damages.
 *
 * Rust port of xscreensaver/xlockmore's drift.c.
 */

use rand::RngExt;
use std::f64::consts::PI;

use crate::animation::primitives::{clear_buffer, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

// Hack defaults: *delay: 10000, *count: 30, *ncolors: 200
const DELAY_US: u64 = 10_000;
const DEF_COUNT: i32 = 30;

const FUSE: i32 = 10; // discard this many initial iterations
const NMAJORVARS: usize = 7;
const MAXLEV: usize = 10;

const BLACK: Color = Color { a: 255, r: 0, g: 0, b: 0 };
const WHITE: Color = Color { a: 255, r: 255, g: 255, b: 255 };

/// Exact port of hsv_to_rgb from utils/hsv.c (output scaled to 8 bits).
// 8-bit-direct rounding ((x*255.0) as u8) differs from primitives::hsv_to_rgb's
// 16-bit path (e.g. v=0.999 -> 254 here vs 255 there); kept for port fidelity.
fn hsv_to_rgb(h: i32, s: f64, v: f64) -> Color {
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let hh = h.rem_euclid(360) as f64 / 60.0;
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
    Color::new(255, (r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

// LRAND(): a 31-bit random value
fn lrand() -> u32 {
    rand::rng().random::<u32>() & 0x7fff_ffff
}

pub struct Drift {
    width: i32,
    height: i32,
    pixels: Vec<u8>,
    npixels: usize,
    palette: Vec<Color>,
    count: i32,

    /* shape of current flame */
    nxforms: usize,
    f: [[[f64; MAXLEV]; 3]; 2], // a bunch of non-homogeneous xforms
    variation: [usize; MAXLEV], // for each xform

    /* animation */
    df: [[[f64; MAXLEV]; 3]; 2],

    /* high-level control */
    nfractals: i32, // draw this many fractals
    major_variation: usize,
    fractal_len: i32, // pts/fractal
    color: bool,
    rainbow: bool, // more than one color per fractal

    /* draw info about current flame */
    fuse: i32,         // iterate this many before drawing
    total_points: i32, // draw this many pts before fractal ends
    pixcol: Color,

    x: f64,
    y: f64,
    c: f64,
    liss_time: i32,
    grow: bool,
    liss: bool,

    lasthalf: u32,
    saved_random_bits: u32,
    nbits: i32,

    erase_countdown: i32,
}

impl Drift {
    fn build(config: &AnimConfig) -> Drift {
        let npixels = config.ncolors.max(2) as usize;
        let palette = (0..npixels)
            .map(|i| hsv_to_rgb((i * 360 / npixels) as i32, 1.0, 1.0))
            .collect();

        let mut dp = Drift {
            width: config.width as i32,
            height: config.height as i32,
            pixels: vec![0u8; (config.width * config.height * 4) as usize],
            npixels,
            palette,
            count: if config.count == 0 { DEF_COUNT } else { config.count },
            nxforms: 2,
            f: [[[0.0; MAXLEV]; 3]; 2],
            variation: [0; MAXLEV],
            df: [[[0.0; MAXLEV]; 3]; 2],
            nfractals: 0,
            major_variation: 0,
            fractal_len: 0,
            color: npixels > 2,
            rainbow: false,
            fuse: 0,
            total_points: 0,
            pixcol: WHITE,
            x: 0.0,
            y: 0.0,
            c: 0.0,
            liss_time: 0,
            grow: false,
            liss: false,
            lasthalf: 0,
            saved_random_bits: 0,
            nbits: 0,
            erase_countdown: 0,
        };

        // MI_IS_FULLRANDOM is always true in the standalone build
        if lrand() % 3 == 0 {
            dp.grow = true;
        } else {
            dp.grow = false;
            dp.liss = lrand() & 1 != 0;
        }

        dp.initmode(1);
        dp.initfractal();
        dp
    }

    fn halfrandom(&mut self, mv: u32) -> u32 {
        let r = if self.lasthalf != 0 {
            let r = self.lasthalf;
            self.lasthalf = 0;
            r
        } else {
            let r = lrand();
            self.lasthalf = r >> 16;
            r
        };
        r % mv
    }

    fn frandom(&mut self, n: u32) -> usize {
        if self.nbits < 3 {
            self.saved_random_bits = lrand();
            self.nbits = 31;
        }
        match n {
            2 => {
                let result = self.saved_random_bits & 1;
                self.saved_random_bits >>= 1;
                self.nbits -= 1;
                result as usize
            }
            3 => {
                let result = self.saved_random_bits & 3;
                self.saved_random_bits >>= 2;
                self.nbits -= 2;
                if result == 3 {
                    self.frandom(3)
                } else {
                    result as usize
                }
            }
            4 => {
                let result = self.saved_random_bits & 3;
                self.saved_random_bits >>= 2;
                self.nbits -= 2;
                result as usize
            }
            5 => {
                let result = self.saved_random_bits & 7;
                self.saved_random_bits >>= 3;
                self.nbits -= 3;
                if result > 4 {
                    self.frandom(5)
                } else {
                    result as usize
                }
            }
            _ => 0,
        }
    }

    fn distrib_a(&mut self) -> i32 {
        self.halfrandom(7000) as i32 + 9000
    }

    fn distrib_b(&mut self) -> i32 {
        ((self.frandom(3) + 1) * (self.frandom(3) + 1) * 120_000) as i32
    }

    fn initmode(&mut self, mode: i32) {
        const VARIATION_LEN: usize = 14;

        let mv = self.halfrandom(VARIATION_LEN as u32) as usize;
        // 0, 0, 1, 1, 2, 2, 3, 4, 4, 5, 5, 6, 6, 6
        self.major_variation = if (VARIATION_LEN >> 1..VARIATION_LEN - 1).contains(&mv) {
            (mv + 1) >> 1
        } else {
            mv >> 1
        };

        if self.grow {
            self.rainbow = false;
            if mode != 0 {
                if !self.color || self.halfrandom(8) != 0 {
                    self.nfractals = self.halfrandom(30) as i32 + 5;
                    self.fractal_len = self.distrib_a();
                } else {
                    self.nfractals = self.halfrandom(5) as i32 + 5;
                    self.fractal_len = self.distrib_b();
                }
            } else {
                self.rainbow = self.color;
                self.nfractals = 1;
                self.fractal_len = self.distrib_b();
            }
        } else {
            self.nfractals = 1;
            self.rainbow = self.color;
            self.fractal_len = 2_000_000;
        }
        self.fractal_len = ((self.fractal_len as i64 * self.count as i64) / 20) as i32;

        clear_buffer(&mut self.pixels, BLACK); // MI_CLEARWINDOW
    }

    fn pick_df_coefs(&mut self) {
        for i in 0..self.nxforms {
            let mut r = 1e-6;
            for j in 0..2 {
                for k in 0..3 {
                    self.df[j][k][i] = self.halfrandom(1000) as f64 / 500.0 - 1.0;
                    r += self.df[j][k][i] * self.df[j][k][i];
                }
            }
            let r = (3 + self.halfrandom(5)) as f64 * 0.01 / r.sqrt();
            for j in 0..2 {
                for k in 0..3 {
                    self.df[j][k][i] *= r;
                }
            }
        }
    }

    fn initfractal(&mut self) {
        const XFORM_LEN: usize = 9;

        self.fuse = FUSE;
        self.total_points = 0;

        let n = self.halfrandom(XFORM_LEN as u32) as usize;
        // 2, 2, 2, 3, 3, 3, 4, 4, 5
        self.nxforms = (n >= XFORM_LEN - 1) as usize + n / 3 + 2;

        self.c = 0.0;
        self.x = 0.0;
        self.y = 0.0;
        if self.liss && self.halfrandom(10) == 0 {
            self.liss_time = 0;
        }
        if !self.grow {
            self.pick_df_coefs();
        }
        for i in 0..self.nxforms {
            self.variation[i] = if NMAJORVARS == self.major_variation {
                self.halfrandom(NMAJORVARS as u32) as usize
            } else {
                self.major_variation
            };
            for j in 0..2 {
                for k in 0..3 {
                    self.f[j][k][i] = if self.liss {
                        (self.liss_time as f64 * self.df[j][k][i]).sin()
                    } else {
                        self.halfrandom(1000) as f64 / 500.0 - 1.0
                    };
                }
            }
        }
        self.pixcol = if self.color {
            let i = self.halfrandom(self.npixels as u32) as usize;
            self.palette[i]
        } else {
            WHITE
        };
    }

    fn iter(&mut self) {
        let i = self.frandom(self.nxforms as u32);

        let nc = if i != 0 { (self.c + 1.0) / 2.0 } else { self.c / 2.0 };

        let mut nx = self.f[0][0][i] * self.x + self.f[0][1][i] * self.y + self.f[0][2][i];
        let mut ny = self.f[1][0][i] * self.x + self.f[1][1][i] * self.y + self.f[1][2][i];

        match self.variation[i] {
            1 => {
                // sinusoidal
                nx = nx.sin();
                ny = ny.sin();
            }
            2 => {
                // complex
                let r2 = nx * nx + ny * ny + 1e-6;
                nx /= r2;
                ny /= r2;
            }
            3 => {
                // bent
                if nx < 0.0 {
                    nx *= 2.0;
                }
                if ny < 0.0 {
                    ny /= 2.0;
                }
            }
            4 => {
                // swirl
                let r = nx * nx + ny * ny;
                let c1 = r.sin();
                let c2 = r.cos();
                let t = nx;

                if !(-1e4..=1e4).contains(&nx) || !(-1e4..=1e4).contains(&ny) {
                    ny = 1e4;
                } else {
                    ny = c2 * t + c1 * ny;
                }
                // C quirk: nx is computed from the already-updated ny
                nx = c1 * nx - c2 * ny;
            }
            5 => {
                // horseshoe
                let r = if nx == 0.0 && ny == 0.0 { 0.0 } else { nx.atan2(ny) };
                let c1 = r.sin();
                let c2 = r.cos();
                let t = nx;

                nx = c1 * nx - c2 * ny;
                ny = c2 * t + c1 * ny;
            }
            6 => {
                // drape
                let t = if nx == 0.0 && ny == 0.0 {
                    0.0
                } else {
                    nx.atan2(ny) / PI
                };

                if !(-1e4..=1e4).contains(&nx) || !(-1e4..=1e4).contains(&ny) {
                    ny = 1e4;
                } else {
                    ny = (nx * nx + ny * ny).sqrt() - 1.0;
                }
                nx = t;
            }
            _ => {}
        }

        if !(-1e4..=1e4).contains(&nx) {
            nx = self.halfrandom(1000) as f64 / 500.0 - 1.0;
            ny = self.halfrandom(1000) as f64 / 500.0 - 1.0;
            self.fuse = FUSE;
        }
        self.x = nx;
        self.y = ny;
        self.c = nc;
    }

    // The C version batches points purely to reduce X round trips;
    // plotting each point directly produces identical pixels.
    fn draw(&mut self) {
        if self.fuse != 0 {
            self.fuse -= 1;
            return;
        }
        let (x, y) = (self.x, self.y);
        if !(x > -1.0 && x < 1.0 && y > -1.0 && y < 1.0) {
            return;
        }

        let fixed_x = ((self.width / 2) as f64 * (x + 1.0)) as i32;
        let fixed_y = ((self.height / 2) as f64 * (y + 1.0)) as i32;

        let color = if !self.rainbow {
            self.pixcol
        } else {
            let c = ((self.c * self.npixels as f64) as i32).clamp(0, self.npixels as i32 - 1);
            self.palette[c as usize]
        };
        put_pixel(
            &mut self.pixels,
            self.width as u32,
            self.height as u32,
            fixed_x,
            fixed_y,
            color,
        );
    }
}

impl Animation for Drift {
    fn new(config: &AnimConfig) -> Self {
        Self::build(config)
    }

    fn tick(&mut self) {
        if self.erase_countdown != 0 {
            self.erase_countdown -= 1;
            if self.erase_countdown == 0 {
                let mode = self.frandom(2) as i32;
                self.initmode(mode);
                self.initfractal();
            }
            return;
        }

        let mut timer = 3000;
        while timer != 0 {
            self.iter();
            self.draw();
            let total_points = self.total_points;
            self.total_points += 1;
            if total_points > self.fractal_len {
                self.nfractals -= 1;
                if self.nfractals == 0 {
                    // 4 seconds' worth of idle frames (MI_PAUSE == the delay)
                    self.erase_countdown = (4_000_000 / DELAY_US) as i32;
                    return;
                }
                self.initfractal();
            }
            timer -= 1;
        }

        if !self.grow {
            if self.liss {
                self.liss_time += 1;
            }
            for i in 0..self.nxforms {
                for j in 0..2 {
                    for k in 0..3 {
                        if self.liss {
                            self.f[j][k][i] = (self.liss_time as f64 * self.df[j][k][i]).sin();
                        } else {
                            self.f[j][k][i] += self.df[j][k][i];
                            let t = self.f[j][k][i];
                            if !(-1.0..=1.0).contains(&t) {
                                self.df[j][k][i] *= -1.0;
                            }
                        }
                    }
                }
            }
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if width as i32 == self.width && height as i32 == self.height {
            let n = buffer.len().min(self.pixels.len());
            buffer[..n].copy_from_slice(&self.pixels[..n]);
        } else {
            let w = (width as usize).min(self.width as usize);
            let h = (height as usize).min(self.height as usize);
            for row in 0..h {
                let src = row * self.width as usize * 4;
                let dst = row * width as usize * 4;
                buffer[dst..dst + w * 4].copy_from_slice(&self.pixels[src..src + w * 4]);
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        *self = Self::build(config);
    }

    fn frame_delay_us(&self) -> u64 {
        DELAY_US
    }
}
