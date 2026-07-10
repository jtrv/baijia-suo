//! Continuously varying Julia set.
//
// Copyright (c) 1995 Sean McCullough <bankshot@mailhost.nmt.edu>.
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
// Rust port of xlockmore/modes/julia.c.

use crate::animation::primitives::{put_pixel, Color};
use crate::animation::{AnimConfig, Animation};
use rand::Rng;
use std::f64::consts::PI;

pub struct Julia {
    centerx: i32,
    centery: i32,
    width: u32,
    height: u32,

    cr: f64,
    ci: f64,
    depth: i32,
    inc: i32,
    circsize: i32,

    pix: i32,
    ncolors: i32,

    point_buffers: Vec<Vec<(i32, i32)>>,
    buffer_colors: Vec<Color>,
    buffer_idx: usize,
    nbuffers: usize,

    delay_us: u64,
}

impl Julia {
    fn fill_circle(
        buffer: &mut [u8],
        width: u32,
        height: u32,
        cx: i32,
        cy: i32,
        radius: i32,
        color: Color,
    ) {
        if radius <= 0 {
            put_pixel(buffer, width, height, cx, cy, color);
            return;
        }
        for y in -radius..=radius {
            for x in -radius..=radius {
                if x * x + y * y <= radius * radius {
                    put_pixel(buffer, width, height, cx + x, cy + y, color);
                }
            }
        }
    }

    fn apply(&self, xr: f64, xi: f64, d: i32, buffer: &mut Vec<(i32, i32)>) {
        let px = (0.5 * xr * self.centerx as f64 + self.centerx as f64) as i32;
        let py = (0.5 * xi * self.centery as f64 + self.centery as f64) as i32;
        buffer.push((px, py));

        if d > 0 {
            let nxi = xi - self.ci;
            let nxr = xr - self.cr;

            let theta = if nxi == 0.0 && nxr == 0.0 {
                0.0
            } else {
                nxi.atan2(nxr) / 2.0
            };
            let r = (nxi * nxi + nxr * nxr).sqrt().sqrt();

            let nnxr = r * theta.cos();
            let nnxi = r * theta.sin();

            self.apply(nnxr, nnxi, d - 1, buffer);
            self.apply(-nnxr, -nnxi, d - 1, buffer);
        }
    }
}

impl Animation for Julia {
    fn new(config: &AnimConfig) -> Self {
        let mut julia = Julia {
            centerx: 0,
            centery: 0,
            width: config.width,
            height: config.height,
            cr: 0.0,
            ci: 0.0,
            depth: 10,
            inc: 0,
            circsize: 0,
            pix: 0,
            ncolors: 200,
            point_buffers: Vec::new(),
            buffer_colors: Vec::new(),
            buffer_idx: 0,
            nbuffers: 0,
            delay_us: config.delay_us,
        };
        julia.reset(config);
        julia
    }

    fn tick(&mut self) {
        self.cr = 1.5
            * ((PI * (self.inc as f64 / 300.0)).sin() * (self.inc as f64 * PI / 200.0).sin());
        self.ci = 1.5
            * ((PI * (self.inc as f64 / 300.0)).cos() * (self.inc as f64 * PI / 200.0).cos());
        self.cr += 0.5 * (PI * self.inc as f64 / 400.0).cos();
        self.ci += 0.5 * (PI * self.inc as f64 / 400.0).sin();

        self.inc += 1;

        let color = if self.ncolors > 2 {
            let c = Color::from_hsl(self.pix as f32 / self.ncolors as f32, 1.0, 0.5);
            self.pix += 1;
            if self.pix >= self.ncolors {
                self.pix = 0;
            }
            c
        } else {
            Color::new(255, 255, 255, 255)
        };
        self.buffer_colors[self.buffer_idx] = color;

        let mut rng = rand::rng();
        let mut xr = 0.0_f64;
        let mut xi = 0.0_f64;

        for _ in 0..64 {
            xi -= self.ci;
            xr -= self.cr;

            let theta = if xi == 0.0 && xr == 0.0 {
                0.0
            } else {
                xi.atan2(xr) / 2.0
            };
            let r = (xi * xi + xr * xr).sqrt().sqrt();

            xr = r * theta.cos();
            xi = r * theta.sin();

            if rng.random::<bool>() {
                xi = -xi;
                xr = -xr;
            }
        }

        let mut current_buffer = std::mem::take(&mut self.point_buffers[self.buffer_idx]);
        current_buffer.clear();
        self.apply(xr, xi, self.depth, &mut current_buffer);
        self.point_buffers[self.buffer_idx] = current_buffer;

        self.buffer_idx += 1;
        if self.buffer_idx >= self.nbuffers {
            self.buffer_idx = 0;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for i in 0..self.nbuffers {
            let pts = &self.point_buffers[i];
            if pts.is_empty() {
                continue;
            }
            let color = self.buffer_colors[i];
            for &(px, py) in pts {
                put_pixel(buffer, width, height, px, py, color);
            }
        }

        let cx = (self.centerx as f64 * self.cr / 2.0) as i32 + self.centerx;
        let cy = (self.centery as f64 * self.ci / 2.0) as i32 + self.centery;
        let radius = self.circsize / 2;

        let circle_color = Color::new(255, 255, 255, 255);
        Self::fill_circle(buffer, width, height, cx, cy, radius, circle_color);
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();
        self.width = config.width;
        self.height = config.height;
        self.centerx = (self.width / 2) as i32;
        self.centery = (self.height / 2) as i32;

        let depth = if config.count <= 0 { 10 } else { config.count };
        self.depth = depth.clamp(7, 10);

        let min_center = self.centerx.min(self.centery);
        self.circsize = (min_center / 96) * 2 + 1;

        self.ncolors = if config.ncolors <= 0 {
            200
        } else {
            config.ncolors
        };
        self.pix = if self.ncolors > 2 {
            rng.random_range(0..self.ncolors)
        } else {
            0
        };

        self.inc = rng.random_range(-200..=200);

        let cycles = if config.cycles <= 0 { 20 } else { config.cycles };
        self.nbuffers = (cycles + 1) as usize;

        self.point_buffers = vec![Vec::new(); self.nbuffers];
        self.buffer_colors = vec![Color::new(0, 0, 0, 0); self.nbuffers];
        self.buffer_idx = 0;
        self.delay_us = config.delay_us;
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
