/*
 * Binary Horizon
 * Copyright (c) 2020 Patrick Leiser <emilio.deltessa@gmail.com>
 *
 *  Directly ported code from complexification.net Binary Ring art
 *  http://www.complexification.net/gallery/machines/binaryRing/appletm/BinaryRing_m.pde
 *
 *  Directly Based on:
 *  Binary Ring code:
 *    j.tarbell   June, 2004
 *    Albuquerque, New Mexico
 *    complexification.net
 *
 * Directly based the hacks of:
 *
 * xscreensaver, Copyright (c) 1997, 1998, 2002 Jamie Zawinski <jwz@jwz.org>
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Rust port of xscreensaver's binaryhorizon.c for baijia-suo.
 */

use crate::animation::{AnimConfig, Animation, RenderPolicy};
use rand::Rng;
use std::time::Instant;

const BLACK: usize = 0;
const WHITE: usize = 1;

/* defaults table */
const GROWTH_DELAY: u64 = 10_000;
const PARTICLES_NUMBER: usize = 5000;
const MAX_AGE: i32 = 400;
const DURATION: f64 = 30.0;
const OPT_COLOR: bool = true;
const OPT_BICOLOR: bool = true;
const OPT_FADE: bool = true;
const CURLINESS: f32 = 0.5;

type Rgb = (i32, i32, i32);

/// frand1(): random float in [-1, 1)
fn frand1(rng: &mut impl Rng) -> f32 {
    rng.random::<f32>() * 2.0 - 1.0
}

struct Particle {
    x: f32,
    y: f32,
    xx: f32,
    yy: f32,
    vx: f32,
    vy: f32,
    color: Rgb,
    age: i32,
}

/// Alpha-blend a point into the persistent buffer (C draw_point).
fn draw_point(buffer: &mut [u8], width: i32, x: i32, y: i32, (r, g, b): Rgb, a: f32) {
    let idx = (y as usize * width as usize + x as usize) * 4;
    if idx + 3 >= buffer.len() {
        return;
    }
    let ob = buffer[idx] as i32;
    let og = buffer[idx + 1] as i32;
    let or = buffer[idx + 2] as i32;
    // C: nr = or + (r - or) * a, float truncated to int
    let nr = (or as f32 + (r - or) as f32 * a) as i32;
    let ng = (og as f32 + (g - og) as f32 * a) as i32;
    let nb = (ob as f32 + (b - ob) as f32 * a) as i32;
    buffer[idx] = nb as u8;
    buffer[idx + 1] = ng as u8;
    buffer[idx + 2] = nr as u8;
    buffer[idx + 3] = 255;
}

fn dla_plot(buffer: &mut [u8], width: i32, height: i32, x: i32, y: i32, col: Rgb, mut br: f32) {
    if x >= 0 && x < width && y >= 0 && y < height {
        if br > 1.0 {
            br = 1.0;
        }
        draw_point(buffer, width, x, y, col, br);
    }
}

fn ipart(x: f32) -> i32 {
    x as i32
}
fn fpart(x: f32) -> f32 {
    x - ipart(x) as f32
}
fn rfpart(x: f32) -> f32 {
    1.0 - fpart(x)
}
fn round_(x: f32) -> i32 {
    (x + 0.5) as i32
}

/// Xiaolin-Wu style antialiased line, exact port of draw_line_antialias().
#[allow(clippy::too_many_arguments)]
fn draw_line_antialias(
    buffer: &mut [u8],
    width: i32,
    height: i32,
    mut x1: i32,
    mut y1: i32,
    mut x2: i32,
    mut y2: i32,
    color: Rgb,
    alpha: f32,
) {
    // hard clipping, as in the C (this routine has problems with negative coords)
    if x1 < 0
        || x1 > width
        || x2 < 0
        || x2 > width
        || y1 < 0
        || y1 > height
        || y2 < 0
        || y2 > height
    {
        return;
    }
    // C computes 0/0 = NaN here (UB when converted); a zero-length line draws nothing useful
    if x1 == x2 && y1 == y2 {
        return;
    }

    let dx = (x2 - x1) as f32;
    let dy = (y2 - y1) as f32;

    let plot = |x: i32, y: i32, d: f32, buffer: &mut [u8]| {
        dla_plot(buffer, width, height, x, y, color, d * alpha);
    };

    if dx.abs() > dy.abs() {
        if x2 < x1 {
            std::mem::swap(&mut x1, &mut x2);
            std::mem::swap(&mut y1, &mut y2);
        }
        let gradient = dy / dx;
        let mut xend = round_(x1 as f32) as f32;
        let mut yend = y1 as f32 + gradient * (xend - x1 as f32);
        let mut xgap = rfpart(x1 as f32 + 0.5);
        let xpxl1 = xend as i32;
        let ypxl1 = ipart(yend);
        plot(xpxl1, ypxl1, rfpart(yend) * xgap, buffer);
        plot(xpxl1, ypxl1 + 1, fpart(yend) * xgap, buffer);
        let mut intery = yend + gradient;

        xend = round_(x2 as f32) as f32;
        yend = y2 as f32 + gradient * (xend - x2 as f32);
        xgap = fpart(x2 as f32 + 0.5);
        let xpxl2 = xend as i32;
        let ypxl2 = ipart(yend);
        plot(xpxl2, ypxl2, rfpart(yend) * xgap, buffer);
        plot(xpxl2, ypxl2 + 1, fpart(yend) * xgap, buffer);

        for x in (xpxl1 + 1)..=(xpxl2 - 1) {
            plot(x, ipart(intery), rfpart(intery), buffer);
            plot(x, ipart(intery) + 1, fpart(intery), buffer);
            intery += gradient;
        }
    } else {
        if y2 < y1 {
            std::mem::swap(&mut x1, &mut x2);
            std::mem::swap(&mut y1, &mut y2);
        }
        let gradient = dx / dy;
        let mut yend = round_(y1 as f32) as f32;
        let mut xend = x1 as f32 + gradient * (yend - y1 as f32);
        let mut ygap = rfpart(y1 as f32 + 0.5);
        let ypxl1 = yend as i32;
        let xpxl1 = ipart(xend);
        plot(xpxl1, ypxl1, rfpart(xend) * ygap, buffer);
        plot(xpxl1, ypxl1 + 1, fpart(xend) * ygap, buffer);
        let mut interx = xend + gradient;

        yend = round_(y2 as f32) as f32;
        xend = x2 as f32 + gradient * (yend - y2 as f32);
        ygap = fpart(y2 as f32 + 0.5);
        let ypxl2 = yend as i32;
        let xpxl2 = ipart(xend);
        plot(xpxl2, ypxl2, rfpart(xend) * ygap, buffer);
        plot(xpxl2, ypxl2 + 1, fpart(xend) * ygap, buffer);

        for y in (ypxl1 + 1)..=(ypxl2 - 1) {
            plot(ipart(interx), y, rfpart(interx), buffer);
            plot(ipart(interx) + 1, y, fpart(interx), buffer);
            interx += gradient;
        }
    }
}

pub struct BinaryHorizon {
    epoch: usize,
    max_age: i32,
    duration: u64, // seconds, 0 = never reset
    start_time: Instant,
    curliness: f32,
    particles: Vec<Particle>,
    width: i32,
    height: i32,
    line_height: i32,
    colors: [Rgb; 2],
    color: bool,
    bicolor: bool,
    fade: bool,
    buffer: Vec<u8>,
    delay_us: u64,
}

impl BinaryHorizon {
    fn next_color(&self, current: Rgb, rng: &mut impl Rng) -> Rgb {
        if self.fade {
            let (mut r, mut g, mut b) = current;
            r += rng.random_range(0..5) - 2;
            g += rng.random_range(0..5) - 2;
            b += rng.random_range(0..5) - 2;
            (r.clamp(0, 255), g.clamp(0, 255), b.clamp(0, 255))
        } else {
            (
                rng.random_range(0..255),
                rng.random_range(0..255),
                rng.random_range(0..255),
            )
        }
    }

    fn init_particle(p: &mut Particle, dx: f32, dy: f32, direction: f32, color: Rgb, max_age: i32, rng: &mut impl Rng) {
        let max_initial_velocity = 2.0f32;
        p.x = -dx;
        p.y = -dy;
        p.xx = 0.0;
        p.yy = 0.0;
        p.vx = max_initial_velocity * direction.cos();
        p.vy = max_initial_velocity * direction.sin();
        p.age = rng.random_range(0..max_age.max(1));
        p.color = color;
    }

    fn create_particles(&mut self, rng: &mut impl Rng) {
        let n = self.particles.len();
        for i in 0..n {
            let emitx = self.width as f32 * (i as f32 / n as f32);
            let emity = 0.0;
            let direction = std::f32::consts::PI * i as f32 / n as f32;

            if self.epoch == WHITE && self.color {
                self.colors[WHITE] = self.next_color(self.colors[WHITE], rng);
            }
            let color = self.colors[WHITE];
            let max_age = self.max_age;
            Self::init_particle(&mut self.particles[i], emitx, emity, direction, color, max_age, rng);
        }
    }

    /// randomly move one particle and draw it
    fn move_particle(&mut self, i: usize, rng: &mut impl Rng) {
        let w = self.width / 2;
        let h = self.height / 2;
        let max_dv = 1.0f32;

        {
            let p = &mut self.particles[i];
            p.xx = p.x;
            p.yy = p.y;
            p.x += p.vx;
            p.y += p.vy;
            p.vx += frand1(rng) * self.curliness * max_dv;
            p.vy += frand1(rng) * self.curliness * max_dv;
        }

        let (x, y, xx, yy, color) = {
            let p = &self.particles[i];
            (p.x, p.y, p.xx, p.yy, p.color)
        };
        // C passes floats to int parameters: truncation toward zero
        draw_line_antialias(
            &mut self.buffer,
            self.width,
            self.height,
            (w as f32 + xx) as i32,
            (h as f32 + yy) as i32,
            (w as f32 + x) as i32,
            (h as f32 + y) as i32,
            color,
            0.15,
        );
        draw_line_antialias(
            &mut self.buffer,
            self.width,
            self.height,
            (w as f32 - xx) as i32,
            (h as f32 + yy) as i32,
            (w as f32 - x) as i32,
            (h as f32 + y) as i32,
            color,
            0.15,
        );

        self.particles[i].age += 1;
        // if this is too old, die and reborn
        if self.particles[i].age > self.max_age {
            let dir = frand1(rng) * 2.0 * std::f32::consts::PI;
            if self.epoch == WHITE && self.color {
                self.colors[WHITE] = self.next_color(self.colors[WHITE], rng);
            }
            if self.epoch == BLACK && self.color && self.bicolor {
                self.colors[BLACK] = self.next_color(self.colors[BLACK], rng);
            }
            let p = &mut self.particles[i];
            p.x = self.width as f32 * dir.sin();
            p.y = self.line_height as f32;
            p.xx = 0.0;
            p.yy = 0.0;
            p.vx = 0.0;
            p.vy = 0.0;
            p.age = 0;
            p.color = self.colors[self.epoch];
        }
    }

    fn clear_buffer_black(&mut self) {
        self.buffer = vec![0u8; self.width as usize * self.height as usize * 4];
        for px in self.buffer.chunks_exact_mut(4) {
            px[3] = 255;
        }
    }
}

impl Animation for BinaryHorizon {
    fn new(config: &AnimConfig) -> Self {
        let mut rng = rand::rng();
        // dual screens not in lockstep: duration *= 1 + frand(0.3), int truncation
        let duration = (DURATION * (1.0 + rng.random::<f64>() * 0.3)) as u64;
        let mut st = BinaryHorizon {
            epoch: WHITE,
            max_age: MAX_AGE,
            duration,
            start_time: Instant::now(),
            curliness: CURLINESS,
            particles: Vec::new(),
            width: config.width as i32,
            height: config.height as i32,
            line_height: 0,
            colors: [(0, 0, 0), (255, 255, 255)],
            color: OPT_COLOR,
            bicolor: OPT_BICOLOR,
            fade: OPT_FADE,
            buffer: Vec::new(),
            delay_us: GROWTH_DELAY,
        };
        st.particles = (0..PARTICLES_NUMBER)
            .map(|_| Particle {
                x: 0.0,
                y: 0.0,
                xx: 0.0,
                yy: 0.0,
                vx: 0.0,
                vy: 0.0,
                color: (255, 255, 255),
                age: 0,
            })
            .collect();
        st.create_particles(&mut rng);
        st.clear_buffer_black();
        st
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        // Full reset every N seconds
        if self.duration != 0 && self.start_time.elapsed().as_secs() > self.duration {
            self.start_time = Instant::now();
            self.epoch = WHITE;
            self.create_particles(&mut rng);
            self.clear_buffer_black();
        }

        for i in 0..self.particles.len() {
            self.move_particle(i, &mut rng);
        }

        // randomly switch ageColor periods
        if rng.random_range(0..10000) > 9975 {
            self.epoch = if self.epoch == WHITE { BLACK } else { WHITE };
            self.line_height = -((frand1(&mut rng) * self.height as f32 / 2.0) as i32).abs();
            if self.epoch == WHITE {
                self.line_height = -self.line_height;
            }
        }
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        let n = buffer.len().min(self.buffer.len());
        buffer[..n].copy_from_slice(&self.buffer[..n]);
    }

    fn reset(&mut self, config: &AnimConfig) {
        // C reshape path: new size, epoch back to WHITE, fresh particles + buffers
        let mut rng = rand::rng();
        self.width = config.width as i32;
        self.height = config.height as i32;
        self.epoch = WHITE;
        self.create_particles(&mut rng);
        self.clear_buffer_black();
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::CompleteFrame
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
