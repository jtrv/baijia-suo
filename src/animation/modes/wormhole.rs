/* xscreensaver, Copyright (c) 1992-2011 Jamie Zawinski <jwz@jwz.org>
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * wormhole:
 * Animation of moving through a wormhole. Based on my own code written
 * a few years ago.
 * author: Jon Rafkind <jon@rafkind.com>
 * date: 1/19/04
 *
 * Rust port of xscreensaver's wormhole.c.
 */

use rand::Rng;
use std::f64::consts::PI;

use crate::animation::primitives::{clear_buffer, draw_line, Color};
use crate::animation::{AnimConfig, Animation};

// Hack defaults: *delay: 10000, *zspeed: 10, *stars: 20
const DELAY_US: u64 = 10_000;
const Z_SPEED: i32 = 10;
const MAKE_STARS: i32 = 20;

const SHADE_MAX: i32 = 2048;
const SHADE_USE: i32 = 128;

const BLACK: Color = Color { a: 255, r: 0, g: 0, b: 0 };

fn rnd(rng: &mut impl Rng, q: i32) -> i32 {
    rng.random_range(0..q.max(1))
}

fn gang(x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    let mut tang;
    if x1 == x2 {
        tang = if y1 < y2 { 90 } else { 270 };
    } else if y1 == y2 {
        tang = if x1 < x2 { 0 } else { 180 };
    } else {
        tang = (0.5 + (-(y2 - y1) as f64).atan2((x2 - x1) as f64) * 180.0 / PI) as i32;
    }
    while tang < 0 {
        tang += 360;
    }
    tang % 360
}

// The original converts degrees with `a * 180.0 / M_PI` (backwards); the
// resulting pseudo-random directions are part of the hack's look, so keep it.
fn xcos(a: i32) -> f64 {
    (a as f64 * 180.0 / PI).cos()
}

fn xsin(a: i32) -> f64 {
    (a as f64 * 180.0 / PI).sin()
}

// 16-bit XColor components, as in initXColor()
fn init_color(rng: &mut impl Rng) -> (i32, i32, i32) {
    (
        rnd(rng, 50000) + 10000,
        rnd(rng, 50000) + 10000,
        rnd(rng, 50000) + 10000,
    )
}

struct ColorChanger {
    shade: Vec<Color>,
    min: i32,
    min_want: i32,
}

impl ColorChanger {
    fn new(rng: &mut impl Rng) -> ColorChanger {
        let mut shade = vec![BLACK; SHADE_MAX as usize];
        let mut old_color = init_color(rng);
        let mut new_color = init_color(rng);
        let mut q = 0;
        while q < SHADE_MAX {
            Self::blend_palette(&mut shade[q as usize..(q + SHADE_USE) as usize], old_color, new_color);
            old_color = new_color;
            new_color = init_color(rng);
            q += SHADE_USE;
        }
        ColorChanger {
            shade,
            min: 0,
            min_want: rnd(rng, SHADE_MAX - SHADE_USE),
        }
    }

    fn blend_palette(pal: &mut [Color], sc: (i32, i32, i32), ec: (i32, i32, i32)) {
        let max = pal.len();
        for (q, p) in pal.iter_mut().enumerate() {
            let j = q as f32 / max as f32;
            let f_r = (0.5 + sc.0 as f32 + (ec.0 - sc.0) as f32 * j) as i32;
            let f_g = (0.5 + sc.1 as f32 + (ec.1 - sc.1) as f32 * j) as i32;
            let f_b = (0.5 + sc.2 as f32 + (ec.2 - sc.2) as f32 * j) as i32;
            *p = Color::new(255, (f_r >> 8) as u8, (f_g >> 8) as u8, (f_b >> 8) as u8);
        }
    }

    fn step(&mut self, rng: &mut impl Rng) {
        if self.min < self.min_want {
            self.min += 1;
        }
        if self.min > self.min_want {
            self.min -= 1;
        }
        if self.min == self.min_want {
            self.min_want = rnd(rng, SHADE_MAX - SHADE_USE);
        }
    }
}

#[derive(Clone, Copy)]
struct Star {
    x: i32,
    y: i32,
    calc_x: i32,
    calc_y: i32,
    z: i32,
    center_x: i32,
    center_y: i32,
}

impl Star {
    fn calc(&mut self) {
        if self.center_x == 0 || self.center_y == 0 {
            self.z = 0;
            return;
        }
        if self.z <= 0 {
            self.calc_x = (self.x << 10) / self.center_x;
            self.calc_y = (self.y << 10) / self.center_y;
        } else {
            self.calc_x = (self.x << 10) / self.z + self.center_x;
            self.calc_y = (self.y << 10) / self.z + self.center_y;
        }
    }
}

#[derive(Clone, Copy)]
struct StarLine {
    begin: Star,
    end: Star,
}

impl StarLine {
    // returns true when the star should be discarded
    fn advance(&mut self, z_speed: i32) -> bool {
        self.begin.z -= z_speed;
        self.end.z -= z_speed;
        self.begin.calc();
        self.end.calc();
        self.begin.z <= 0 || self.end.z <= 0
    }
}

fn dist(x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    let xs = x1 - x2;
    let ys = y1 - y2;
    ((xs * xs + ys * ys) as f64).sqrt() as i32
}

pub struct Wormhole {
    screen_x: i32,
    screen_y: i32,

    diameter: i32,
    diameter_change: i32,
    actualx: i32,
    actualy: i32,
    virtualx: f64,
    virtualy: f64,
    speed: f64,
    ang: i32,
    want_x: i32,
    want_y: i32,
    max_z: i32,
    spiral: i32,
    changer: ColorChanger,
    stars: Vec<Option<StarLine>>,
}

impl Wormhole {
    fn build(config: &AnimConfig) -> Wormhole {
        let mut rng = rand::rng();
        let screen_x = config.width as i32;
        let screen_y = config.height as i32;
        let actualx = screen_x / 2;
        let actualy = screen_y / 2;
        let want_x = rnd(&mut rng, screen_x - 50) + 25;
        let want_y = rnd(&mut rng, screen_y - 50) + 25;
        Wormhole {
            screen_x,
            screen_y,
            diameter: rnd(&mut rng, 10) + 15,
            diameter_change: rnd(&mut rng, 10) + 15,
            actualx,
            actualy,
            virtualx: actualx as f64,
            virtualy: actualy as f64,
            speed: screen_x as f64 / 180.0,
            ang: gang(actualx, actualy, want_x, want_y),
            want_x,
            want_y,
            max_z: 600,
            spiral: 0,
            changer: ColorChanger::new(&mut rng),
            stars: vec![None; 64],
        }
    }

    fn init_star(&self, z: i32, ang: i32) -> Star {
        let mut s = Star {
            x: (xcos(ang) * self.diameter as f64) as i32,
            y: (xsin(ang) * self.diameter as f64) as i32,
            calc_x: 0,
            calc_y: 0,
            z,
            center_x: self.actualx,
            center_y: self.actualy,
        };
        s.calc();
        s
    }

    fn add_star(&mut self, rng: &mut impl Rng) {
        let ang = rnd(rng, 360);
        let star_new = StarLine {
            begin: self.init_star(self.max_z, ang),
            end: self.init_star(self.max_z + rnd(rng, 6) + 4, ang),
        };
        // the C version doubles a fixed array; Vec growth is equivalent
        match self.stars.iter_mut().find(|s| s.is_none()) {
            Some(slot) => *slot = Some(star_new),
            None => self.stars.push(Some(star_new)),
        }
    }
}

impl Animation for Wormhole {
    fn new(config: &AnimConfig) -> Self {
        Self::build(config)
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();
        let min_dist = 100;
        let mut find = false;

        let dx = xcos(self.ang) * self.speed;
        let dy = xsin(self.ang) * self.speed;
        self.virtualx += dx;
        self.virtualy += dy;
        self.actualx = self.virtualx as i32;
        self.actualy = self.virtualy as i32;

        if self.spiral != 0 {
            if self.spiral % 5 == 0 {
                self.ang = (self.ang + 1) % 360;
            }
            self.spiral -= 1;
            if self.spiral <= 0 {
                find = true;
            }
        } else {
            if dist(self.actualx, self.actualy, self.want_x, self.want_y) < 20 {
                find = true;
            }
            if rnd(&mut rng, 20) == rnd(&mut rng, 20) {
                find = true;
            }
            if self.actualx < min_dist {
                self.actualx = min_dist;
                self.virtualx = self.actualx as f64;
                find = true;
            }
            if self.actualy < min_dist {
                self.actualy = min_dist;
                self.virtualy = self.actualy as f64;
                find = true;
            }
            if self.actualx > self.screen_x - min_dist {
                self.actualx = self.screen_x - min_dist;
                self.virtualx = self.actualx as f64;
                find = true;
            }
            if self.actualy > self.screen_y - min_dist {
                self.actualy = self.screen_y - min_dist;
                self.virtualy = self.actualy as f64;
                find = true;
            }
            if rnd(&mut rng, 500) == rnd(&mut rng, 500) {
                self.spiral = rnd(&mut rng, 30) + 50;
            }
        }

        if find {
            self.want_x = rnd(&mut rng, self.screen_x - min_dist * 2) + min_dist;
            self.want_y = rnd(&mut rng, self.screen_y - min_dist * 2) + min_dist;
            self.ang = gang(self.actualx, self.actualy, self.want_x, self.want_y);
        }

        for slot in self.stars.iter_mut() {
            if let Some(star) = slot {
                if star.advance(Z_SPEED) {
                    *slot = None;
                }
            }
        }

        self.changer.step(&mut rng);

        if self.diameter < self.diameter_change {
            self.diameter += 1;
        }
        if self.diameter > self.diameter_change {
            self.diameter -= 1;
        }
        if rnd(&mut rng, 30) == rnd(&mut rng, 30) {
            self.diameter_change = rnd(&mut rng, 35) + 5;
        }

        for _ in 0..MAKE_STARS {
            self.add_star(&mut rng);
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        clear_buffer(buffer, BLACK);
        for star in self.stars.iter().flatten() {
            let z = star.begin.z;
            let color = z * SHADE_USE / self.max_z;
            let idx = ((self.changer.min + color).max(0) as usize).min(self.changer.shade.len() - 1);
            draw_line(
                buffer,
                width,
                height,
                star.begin.calc_x,
                star.begin.calc_y,
                star.end.calc_x,
                star.end.calc_y,
                self.changer.shade[idx],
            );
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        *self = Self::build(config);
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        DELAY_US
    }
}
