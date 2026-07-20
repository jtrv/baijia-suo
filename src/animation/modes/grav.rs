/* grav --- planets spinning around a pulsar
 *
 * Copyright (c) 1993 by Greg Boewring <gb@pobox.com>
 *
 * Permission to use, copy, modify, and distribute this software and its
 * documentation for any purpose and without fee is hereby granted,
 * provided that the above copyright notice appear in all copies and that
 * both that copyright notice and this permission notice appear in
 * supporting documentation.
 *
 * Rust port of xlockmore/modes/grav.c (5.00 2000/11/01).
 */

use std::cell::Cell;

use rand::Rng;
use crate::animation::primitives::{clear_buffer, draw_circle, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

const GRAV: f64 = -0.02;
const DIST: f64 = 16.0;
const COLLIDE: f64 = 0.0001;
const ALMOST: f64 = 15.99;
const HALF: f64 = 0.5;
const VR: f64 = 0.04;
const DAMP: f64 = 0.999999;
const MAX_A: f64 = 0.1;
const XR: f64 = HALF * ALMOST;
const YR: f64 = HALF * ALMOST;
const ZR: f64 = HALF * ALMOST;
// xlockmore grav defaults: delay 10000, count -12; decay/trail default off.
const DEF_DELAY_US: u64 = 10_000;
const DEF_COUNT: i32 = -12;

struct Planet {
    p: [f64; 3],
    v: [f64; 3],
    a: [f64; 3],
    xi: i32,
    yi: i32,
    ri: i32,
    color: Color,
}

pub struct Grav {
    width: u32,
    height: u32,
    sr: i32,
    nplanets: usize,
    starcolor: Color,
    planets: Vec<Planet>,
    decay: bool,
    trail: bool,
    ncolors: usize,
    // Draw commands accumulated in tick(), replayed in render().
    draw_ops: Vec<DrawOp>,
    star_ops: Vec<DrawOp>,
    // MI_CLEARWINDOW on init; cleared inside render(), Cell allows mutation through &self.
    needs_clear: Cell<bool>,
}

#[derive(Clone)]
struct DrawOp {
    x: i32,
    y: i32,
    ri: i32,
    color: Color,
}

const BLACK: Color = Color { a: 255, r: 0, g: 0, b: 0 };

impl Grav {
    fn star_radius_max(&self) -> i32 {
        (self.height as f64 / (2.0 * DIST)) as u32 as i32
    }

    fn planet_radius(height: u32, pz: f64) -> i32 {
        // INTRINSIC_RADIUS is (float)(height/5) — integer division first, as in C.
        let ir = (height / 5) as f64;
        (ir / (pz + DIST)) as u32 as i32
    }

    // MI_PIXEL(NRAND(npixels)) when npixels > 2, else white — matching C.
    fn random_color(rng: &mut impl Rng, ncolors: usize) -> Color {
        if ncolors > 2 {
            let idx = rng.random_range(0..ncolors);
            Color::from_hsl(idx as f32 / ncolors as f32, 1.0, 0.5)
        } else {
            Color::new(255, 255, 255, 255)
        }
    }

    // xlockmore Planet macro: clips with <= (inclusive on right/bottom edge, matching C).
    fn planet_op(x: i32, y: i32, ri: i32, width: u32, height: u32, color: Color) -> Option<DrawOp> {
        // xlockmore checks >= 0 && <= width && <= height (inclusive upper bound)
        if x >= 0 && y >= 0 && x <= width as i32 && y <= height as i32 {
            Some(DrawOp { x, y, ri, color })
        } else {
            None
        }
    }

    // XFillArc equivalent: filled circle of diameter r*2 centred on (cx, cy).
    fn fill_circle(buffer: &mut [u8], width: u32, height: u32, cx: i32, cy: i32, r: i32, color: Color) {
        for dy in -r..=r {
            let y = cy + dy;
            if y < 0 || y >= height as i32 {
                continue;
            }
            let half = ((r * r - dy * dy) as f64).sqrt() as i32;
            let x0 = (cx - half).max(0);
            let x1 = (cx + half).min(width as i32 - 1);
            for x in x0..=x1 {
                put_pixel(buffer, width, height, x, y, color);
            }
        }
    }

    fn apply_op(buffer: &mut [u8], width: u32, height: u32, op: &DrawOp) {
        if op.ri < 2 {
            put_pixel(buffer, width, height, op.x, op.y, op.color);
        } else {
            // C uses XFillArc for planets (filled), XDrawArc for the star (outline).
            Self::fill_circle(buffer, width, height, op.x, op.y, op.ri / 2, op.color);
        }
    }

    fn apply_star(buffer: &mut [u8], width: u32, height: u32, cx: i32, cy: i32, sr: i32, color: Color) {
        draw_circle(buffer, width, height, cx, cy, sr / 2, color);
    }

    fn init_planet(rng: &mut impl Rng, width: u32, height: u32, ncolors: usize) -> Planet {
        let color = Self::random_color(rng, ncolors);

        let px = rng.random::<f64>() * (2.0 * XR) - XR;
        let py = rng.random::<f64>() * (2.0 * YR) - YR;
        let pz = rng.random::<f64>() * (2.0 * ZR) - ZR;

        let (xi, yi) = if pz > -ALMOST {
            let xi = (width as f64 * (HALF + px / (pz + DIST))) as i32;
            let yi = (height as f64 * (HALF + py / (pz + DIST))) as i32;
            (xi, yi)
        } else {
            (-1, -1)
        };

        let ri = Self::planet_radius(height, pz);

        let vx = rng.random::<f64>() * (2.0 * VR) - VR;
        let vy = rng.random::<f64>() * (2.0 * VR) - VR;
        let vz = rng.random::<f64>() * (2.0 * VR) - VR;

        Planet {
            p: [px, py, pz],
            v: [vx, vy, vz],
            a: [0.0, 0.0, 0.0],
            xi,
            yi,
            ri,
            color,
        }
    }
}

impl Animation for Grav {
    fn new(config: &AnimConfig) -> Self {
        let mut g = Grav {
            width: config.width,
            height: config.height,
            sr: 0,
            nplanets: 0,
            starcolor: BLACK,
            planets: Vec::new(),
            decay: false,
            trail: false,
            ncolors: config.ncolors.max(2) as usize,
            draw_ops: Vec::new(),
            star_ops: Vec::new(),
            needs_clear: Cell::new(true),
        };
        g.reset(config);
        g
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        self.draw_ops.clear();
        self.star_ops.clear();

        let cx = self.width as i32 / 2;
        let cy = self.height as i32 / 2;

        // Erase centrepoint
        self.star_ops.push(DrawOp { x: cx, y: cy, ri: self.sr, color: BLACK });

        // Resize centrepoint
        let star_max = self.star_radius_max();
        match rng.random_range(0..4u32) {
            0
                if self.sr < star_max => {
                    self.sr += 1;
                }
            1
                if self.sr > 2 => {
                    self.sr -= 1;
                }
            _ => {}
        }

        // Draw centrepoint
        self.star_ops.push(DrawOp { x: cx, y: cy, ri: self.sr, color: self.starcolor });

        let width = self.width;
        let height = self.height;
        let decay = self.decay;
        let trail = self.trail;

        for planet in self.planets.iter_mut() {
            let d2 = planet.p[0] * planet.p[0]
                + planet.p[1] * planet.p[1]
                + planet.p[2] * planet.p[2];
            let mut d = if d2 < COLLIDE { COLLIDE } else { d2 };
            d = d.sqrt();
            d = d * d * d;

            for c in 0..3usize {
                planet.a[c] = planet.p[c] * GRAV / d;
                if decay {
                    planet.a[c] = planet.a[c].clamp(-MAX_A, MAX_A);
                    planet.v[c] += planet.a[c];
                    planet.v[c] *= DAMP;
                } else {
                    planet.v[c] += planet.a[c];
                }
                planet.p[c] += planet.v[c];
            }

            let old_xi = planet.xi;
            let old_yi = planet.yi;

            if planet.p[2] > -ALMOST {
                planet.xi = (width as f64 * (HALF + planet.p[0] / (planet.p[2] + DIST))) as i32;
                planet.yi = (height as f64 * (HALF + planet.p[1] / (planet.p[2] + DIST))) as i32;
            } else {
                planet.xi = -1;
                planet.yi = -1;
            }

            // Erase old position
            if let Some(op) = Self::planet_op(old_xi, old_yi, planet.ri, width, height, BLACK) {
                self.draw_ops.push(op);
            }

            if trail {
                // Leave a dot at the old position in the planet's color
                if let Some(op) = Self::planet_op(old_xi, old_yi, 0, width, height, planet.color) {
                    self.draw_ops.push(op);
                }
            }

            planet.ri = Self::planet_radius(height, planet.p[2]);

            // Draw new position
            if let Some(op) = Self::planet_op(planet.xi, planet.yi, planet.ri, width, height, planet.color) {
                self.draw_ops.push(op);
            }
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if self.needs_clear.get() {
            clear_buffer(buffer, BLACK);
            self.needs_clear.set(false);
        }
        for op in &self.star_ops {
            Self::apply_star(buffer, width, height, op.x, op.y, op.ri, op.color);
        }
        for op in &self.draw_ops {
            Self::apply_op(buffer, width, height, op);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;
        self.ncolors = config.ncolors.max(2) as usize;
        self.draw_ops.clear();
        self.star_ops.clear();
        self.needs_clear.set(true);

        self.sr = self.star_radius_max();

        // Default count=-12: NRAND(12)+1 -> random 1..=12 planets
        let raw_count = if config.count == 0 { DEF_COUNT } else { config.count };
        self.nplanets = if raw_count < 0 {
            rng.random_range(0..(-raw_count) as usize) + 1
        } else {
            raw_count.max(1) as usize
        };

        self.starcolor = Self::random_color(&mut rng, self.ncolors);

        self.planets = (0..self.nplanets)
            .map(|_| Self::init_planet(&mut rng, self.width, self.height, self.ncolors))
            .collect();
    }


    fn frame_delay_us(&self) -> u64 {
        DEF_DELAY_US
    }
}
