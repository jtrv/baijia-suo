//! A journey into deep space.
//
// Copyright (c) 1998 by Vincent Caron [Vincent.Caron@ecl1999.ec-lyon.fr]
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
// Rust port of xlockmore/modes/space.c.

use crate::rng::RngExt;
use crate::animation::primitives::{put_pixel, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const X_LIMIT: f32 = 400.0;
const Y_LIMIT: f32 = 300.0;
const Z_MAX: f32 = 450.0;
const Z_MIN: f32 = -330.0;
const Z_L2: f32 = 200.0;
const Z_L3: f32 = -150.0;
const DIST: f32 = 400.0;
const TRANS_X_MAX: f32 = 3.0;
const TRANS_Y_MAX: f32 = 3.0;
const TRANS_Z_MAX: f32 = 7.0;
const ROT_X_MAX: f32 = 60.0;
const ROT_Y_MAX: f32 = 80.0;
const ROT_Z_MAX: f32 = 50.0;
const TIME_MIN: i32 = 500;
const TIME_AMP: u32 = 800;
const DEGREE: f32 = 0.01 / 100.0;

#[derive(Clone, Copy)]
struct Star {
    x: f32,
    y: f32,
    z: f32,
}

pub struct Space {
    stars: Vec<Star>,
    origin_x: i32,
    origin_y: i32,
    zoom: f32,
    dx: f32,
    dy: f32,
    dz: f32,
    ddx: f32,
    ddy: f32,
    ddz: f32,
    ddxn: i32,
    ddyn: i32,
    ddzn: i32,
    ax: f32,
    ay: f32,
    az: f32,
    dax: f32,
    day: f32,
    daz: f32,
    daxn: i32,
    dayn: i32,
    dazn: i32,
    is_small: bool,
    delay_us: u64,
}

impl Animation for Space {
    fn new(config: &AnimConfig) -> Self {
        let mut s = Space {
            stars: Vec::new(),
            origin_x: 0,
            origin_y: 0,
            zoom: 0.0,
            dx: 0.0,
            dy: 0.0,
            dz: 3.0,
            ddx: 0.0,
            ddy: 0.0,
            ddz: 0.0,
            ddxn: 0,
            ddyn: 0,
            ddzn: 0,
            ax: 0.0,
            ay: 0.0,
            az: 0.0,
            dax: 0.0,
            day: 0.0,
            daz: 0.0,
            daxn: 0,
            dayn: 0,
            dazn: 0,
            is_small: false,
            delay_us: config.delay_us,
        };
        s.reset(config);
        s
    }

    fn tick(&mut self) {
        let mut rng = crate::rng::rng();

        let cos_x = self.ax.cos();
        let sin_x = self.ax.sin();
        let cos_y = self.ay.cos();
        let sin_y = self.ay.sin();
        let cos_z = self.az.cos();
        let sin_z = self.az.sin();

        for star in &mut self.stars {
            let mut _x = star.x;
            let mut _y = star.y;
            let mut _z = star.z;

            // X, Y & Z axis rotations
            let mut k = _y * cos_x + _z * sin_x;
            _z = _z * cos_x - _y * sin_x;
            _y = k;

            k = _x * cos_y + _z * sin_y;
            _z = _z * cos_y - _x * sin_y;
            _x = k;

            k = _x * cos_z + _y * sin_z;
            _y = _y * cos_z - _x * sin_z;
            _x = k;

            // Translations + space boundary overflow
            _x += self.dx;
            if _x < -X_LIMIT {
                _x = X_LIMIT;
            } else if _x > X_LIMIT {
                _x = -X_LIMIT;
            }

            _y += self.dy;
            if _y < -Y_LIMIT {
                _y = Y_LIMIT;
            } else if _y > Y_LIMIT {
                _y = -Y_LIMIT;
            }

            _z -= self.dz;
            if _z < Z_MIN {
                _z = Z_MAX;
            } else if _z > Z_MAX {
                _z = Z_MIN;
            }

            star.x = _x;
            star.y = _y;
            star.z = _z;
        }

        // Update translation parameters
        if self.ddxn == 0 {
            let k = (rng.random_range(0..((TRANS_X_MAX as i32) * 2)) as f32) - TRANS_X_MAX;
            self.ddxn = rng.random_range(0..TIME_AMP) as i32 + TIME_MIN;
            self.ddx = (k - self.dx) / (self.ddxn as f32);
        } else {
            self.dx += self.ddx;
            self.ddxn -= 1;
        }

        if self.ddyn == 0 {
            let k = (rng.random_range(0..((TRANS_Y_MAX as i32) * 2)) as f32) - TRANS_Y_MAX;
            self.ddyn = rng.random_range(0..TIME_AMP) as i32 + TIME_MIN;
            self.ddy = (k - self.dy) / (self.ddyn as f32);
        } else {
            self.dy += self.ddy;
            self.ddyn -= 1;
        }

        if self.ddzn == 0 {
            let k = (rng.random_range(0..((TRANS_Z_MAX as i32) * 2)) as f32) - TRANS_Z_MAX;
            self.ddzn = rng.random_range(0..TIME_AMP) as i32 + TIME_MIN;
            self.ddz = (k - self.dz) / (self.ddzn as f32);
        } else {
            self.dz += self.ddz;
            self.ddzn -= 1;
        }

        // Update rotation parameters
        if self.daxn == 0 {
            let k = ((rng.random_range(0..((ROT_X_MAX as i32) * 2)) as f32) - ROT_X_MAX) * DEGREE;
            self.daxn = rng.random_range(0..TIME_AMP) as i32 + TIME_MIN;
            self.dax = (k - self.ax) / (self.daxn as f32);
        } else {
            self.ax += self.dax;
            self.daxn -= 1;
        }

        if self.dayn == 0 {
            let k = ((rng.random_range(0..((ROT_Y_MAX as i32) * 2)) as f32) - ROT_Y_MAX) * DEGREE;
            self.dayn = rng.random_range(0..TIME_AMP) as i32 + TIME_MIN;
            self.day = (k - self.ay) / (self.dayn as f32);
        } else {
            self.ay += self.day;
            self.dayn -= 1;
        }

        if self.dazn == 0 {
            let k = ((rng.random_range(0..((ROT_Z_MAX as i32) * 2)) as f32) - ROT_Z_MAX) * DEGREE;
            self.dazn = rng.random_range(0..TIME_AMP) as i32 + TIME_MIN;
            self.daz = (k - self.az) / (self.dazn as f32);
        } else {
            self.az += self.daz;
            self.dazn -= 1;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let white = Color::new(255, 255, 255, 255);

        for star in &self.stars {
            let z = star.z;
            let k = self.zoom / (z + DIST);
            let y = self.origin_y - (k * star.y) as i32;
            let x = self.origin_x + (k * star.x) as i32;

            put_pixel(buffer, width, height, x, y, white);

            if z < Z_L2 {
                put_pixel(buffer, width, height, x + 1, y, white);
                put_pixel(buffer, width, height, x - 1, y, white);
                put_pixel(buffer, width, height, x, y + 1, white);
                put_pixel(buffer, width, height, x, y - 1, white);
            }

            if z < Z_L3 && !self.is_small {
                put_pixel(buffer, width, height, x - 1, y + 1, white);
                put_pixel(buffer, width, height, x - 1, y - 1, white);
                put_pixel(buffer, width, height, x + 1, y + 1, white);
                put_pixel(buffer, width, height, x + 1, y - 1, white);
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = crate::rng::rng();
        
        let count = if config.count <= 0 { 100 } else { config.count as usize };
        self.stars.clear();
        for _ in 0..count {
            let x = ((rng.random_range(0..10000_u32) as f32) / 5000.0 - 1.0) * X_LIMIT;
            let y = ((rng.random_range(0..10000_u32) as f32) / 5000.0 - 1.0) * Y_LIMIT;
            let z = (rng.random_range(0..10000_u32) as f32) / 10000.0 * (Z_MAX - Z_MIN) + Z_MIN;
            self.stars.push(Star { x, y, z });
        }

        self.origin_x = (config.width / 2) as i32;
        self.origin_y = (config.height / 2) as i32;
        // Zoom applies to both axes; raw width balloons the field on
        // ultrawide. Clamp the basis to a 16:9 width (same fix as life3d).
        self.zoom = config.width.min(config.height * 16 / 9) as f32 * 0.54 + 40.0;
        self.is_small = (self.origin_x * self.origin_y) < (160 * 100);

        self.dx = 0.0;
        self.ddxn = 0;
        self.dy = 0.0;
        self.ddyn = 0;
        self.dz = 3.0;
        self.ddzn = 0;
        self.ax = 0.0;
        self.daxn = 0;
        self.ay = 0.0;
        self.dayn = 0;
        self.az = 0.0;
        self.dazn = 0;
        self.delay_us = config.delay_us;
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
