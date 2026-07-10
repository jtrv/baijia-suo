//! Spiraling dots.
//
// Copyright (c) 1994 by Darrick Brown.
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
// Rust port of xlockmore/modes/spiral.c.

use rand::Rng;
use std::collections::VecDeque;
use std::f32::consts::PI;
use crate::animation::primitives::{put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

const TWOPI: f32 = 2.0 * PI;
const JAGGINESS: u32 = 4;
const SPEED: f32 = 2.0;
const MINDOTS: usize = 1;

#[derive(Clone)]
struct TrailDot {
    hx: f32,
    hy: f32,
    ha: f32,
    hr: f32,
    color: Color,
}

pub struct Spiral {
    trail: VecDeque<TrailDot>,
    nlength: usize,
    cx: f32,
    cy: f32,
    angle: f32,
    radius: f32,
    dr: f32,
    da: f32,
    dx: f32,
    dy: f32,
    // Fractional palette position, advances by ncolors/(2*nlength) per frame
    colors: f32,
    ncolors: usize,
    dots: usize,
    width: u32,
    height: u32,
    // Virtual coordinate bounds: x in [0, right], y in [0, top=10000]
    right: f32,
    top: f32,
    delay_us: u64,
}

impl Spiral {
    #[inline]
    fn tf_x(&self, x: f32) -> i32 {
        ((x / self.right) * self.width as f32) as i32
    }

    #[inline]
    fn tf_y(&self, y: f32) -> i32 {
        ((y / self.top) * self.height as f32) as i32
    }

    fn draw_ring(&self, buffer: &mut [u8], width: u32, height: u32, dot: &TrailDot) {
        let inc = TWOPI / self.dots as f32;
        let mut i = 0.0_f32;
        while i < TWOPI {
            let x = dot.hx + (i + dot.ha).cos() * dot.hr;
            let y = dot.hy + (i + dot.ha).sin() * dot.hr;
            put_pixel(buffer, width, height, self.tf_x(x), self.tf_y(y), dot.color);
            i += inc;
        }
    }
}

impl Animation for Spiral {
    fn new(config: &AnimConfig) -> Self {
        let mut s = Spiral {
            trail: VecDeque::new(),
            nlength: 2,
            cx: 0.0,
            cy: 0.0,
            angle: 0.0,
            radius: 0.0,
            dr: 0.0,
            da: 0.0,
            dx: 0.0,
            dy: 0.0,
            colors: 0.0,
            ncolors: 64,
            dots: MINDOTS,
            width: config.width,
            height: config.height,
            right: 10000.0,
            top: 10000.0,
            delay_us: config.delay_us,
        };
        s.reset(config);
        s
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        self.cx += self.dx;
        if self.cx > 9000.0 || self.cx < 1000.0 {
            self.dx *= -1.0;
        }

        self.cy += self.dy;
        if self.cy > 9000.0 || self.cy < 1000.0 {
            self.dy *= -1.0;
        }

        self.radius += self.dr;
        if (self.radius > 2500.0 && self.dr > 0.0) || (self.radius < 50.0 && self.dr < 0.0) {
            self.dr *= -1.0;
        }

        // Random direction kick when centre is away from walls
        if rng.random_range(0..3000_u32) < JAGGINESS
            && self.cx > 2000.0 && self.cx < 8000.0
            && self.cy > 2000.0 && self.cy < 8000.0
        {
            self.dx = (10 - rng.random_range(0..20_i32)) as f32 * SPEED;
            self.dy = (10 - rng.random_range(0..20_i32)) as f32 * SPEED;
        }

        // Random dr variation, clamped to [4, 18]
        if rng.random_range(0..3000_u32) < JAGGINESS {
            if rng.random::<bool>() {
                self.dr += rng.random_range(1..=3_i32) as f32;
            } else {
                self.dr -= rng.random_range(1..=3_i32) as f32;
            }
            self.dr = self.dr.clamp(4.0, 18.0);
        }

        // Random da magnitude
        if rng.random_range(0..3000_u32) < JAGGINESS {
            self.da = rng.random_range(0..360_u32) as f32 / 7200.0 + 0.01;
        }

        // Random da sign flip
        if rng.random_range(0..3000_u32) < JAGGINESS {
            self.da *= -1.0;
        }

        self.angle += self.da;
        if self.angle > TWOPI {
            self.angle -= TWOPI;
        } else if self.angle < 0.0 {
            self.angle += TWOPI;
        }

        self.colors += self.ncolors as f32 / (2.0 * self.nlength as f32);
        if self.colors >= self.ncolors as f32 {
            self.colors = 0.0;
        }
        let color = Color::from_hsl(self.colors / self.ncolors as f32, 1.0, 0.5);

        self.trail.push_back(TrailDot {
            hx: self.cx,
            hy: self.cy,
            ha: self.angle,
            hr: self.radius,
            color,
        });

        if self.trail.len() > self.nlength {
            self.trail.pop_front();
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for dot in &self.trail {
            self.draw_ring(buffer, width, height, dot);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.trail.clear();
        self.width = config.width;
        self.height = config.height;
        self.top = 10000.0;
        self.right = config.width as f32 / config.height as f32 * 10000.0;

        // xlockmore default cycles=350; fall back to it when none configured
        self.nlength = if config.cycles <= 0 { 350 } else { config.cycles.max(2) as usize };
        self.ncolors = config.ncolors.max(2) as usize;
        self.delay_us = config.delay_us;

        self.cx = (5000.0 - rng.random_range(0..2000_u32) as f32) / 10000.0 * self.right;
        self.cy = 5000.0 - rng.random_range(0..2000_u32) as f32;
        self.radius = rng.random_range(200..400_u32) as f32;
        self.angle = 0.0;
        self.dx = (10 - rng.random_range(0..20_i32)) as f32 * SPEED;
        self.dy = (10 - rng.random_range(0..20_i32)) as f32 * SPEED;
        let dr_mag = (rng.random_range(0..10_u32) + 4) as f32;
        self.dr = if rng.random::<bool>() { dr_mag } else { -dr_mag };
        self.da = rng.random_range(0..360_u32) as f32 / 7200.0 + 0.01;
        self.colors = rng.random_range(0..self.ncolors as u32) as f32;

        // dots from count — negative means random in [MINDOTS, |count|]
        // xlockmore default count=-40 (random 1-40 dots); 0 means "use that default"
        let count = if config.count == 0 { -40 } else { config.count };
        self.dots = if count < -(MINDOTS as i32) {
            let hi = (-count) as usize;
            rng.random_range(MINDOTS..=hi)
        } else {
            (count.unsigned_abs() as usize).max(MINDOTS)
        };
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
