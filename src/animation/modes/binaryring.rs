/*
 * Binary Ring
 * Copyright (c) 2006-2014 Emilio Del Tessandoro <emilio.deltessa@gmail.com>
 *
 *  Directly ported code from complexification.net Binary Ring art
 *  http://www.complexification.net/gallery/machines/binaryRing/appletm/BinaryRing_m.pde
 *
 *  Binary Ring code:
 *  j.tarbell   June, 2004
 *  Albuquerque, New Mexico
 *  complexification.net
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
 * Rust port of xscreensaver's binaryring.c.
 */

use rand::RngExt;
use std::f32::consts::PI;

use crate::animation::{AnimConfig, Animation, RenderPolicy};

const BLACK: usize = 0;
const WHITE: usize = 1;

// frand1() in the C: uniform in (-1, 1).
fn frand1(rng: &mut impl RngExt) -> f32 {
    rng.random::<f32>() * 2.0 - 1.0
}

// Pixels are stored 0x00RRGGBB (the depth-24 path of the C code).
fn point2rgb(c: u32) -> (i32, i32, i32) {
    (
        ((c & 0xff0000) >> 16) as i32,
        ((c & 0xff00) >> 8) as i32,
        (c & 0xff) as i32,
    )
}

fn rgb2point(r: i32, g: i32, b: i32) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

fn clamp(value: i32, l: i32, h: i32) -> i32 {
    value.max(l).min(h)
}

fn next_color(current: u32, rng: &mut impl RngExt) -> u32 {
    let (mut r, mut g, mut b) = point2rgb(current);
    r += rng.random_range(0..5) - 2;
    g += rng.random_range(0..5) - 2;
    b += rng.random_range(0..5) - 2;
    rgb2point(clamp(r, 0, 255), clamp(g, 0, 255), clamp(b, 0, 255))
}

// Blend a point into the BGRA canvas with coverage `a`. Same channel math
// as the old packed-u32 version, on bytes directly (alpha untouched).
fn draw_point(buffer: &mut [u8], width: i32, x: i32, y: i32, myc: u32, a: f32) {
    let idx = ((y * width + x) as usize) * 4;
    if idx + 3 >= buffer.len() {
        return;
    }
    let (or_, og, ob) = (
        buffer[idx + 2] as i32,
        buffer[idx + 1] as i32,
        buffer[idx] as i32,
    );
    let (r, g, b) = point2rgb(myc);
    let nr = (or_ as f32 + (r - or_) as f32 * a) as i32;
    let ng = (og as f32 + (g - og) as f32 * a) as i32;
    let nb = (ob as f32 + (b - ob) as f32 * a) as i32;
    buffer[idx] = nb as u8;
    buffer[idx + 1] = ng as u8;
    buffer[idx + 2] = nr as u8;
}

fn dla_plot(buffer: &mut [u8], width: i32, height: i32, x: i32, y: i32, col: u32, br: f32) {
    if x >= 0 && x < width && y >= 0 && y < height {
        let br = if br > 1.0 { 1.0 } else { br };
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

#[allow(clippy::too_many_arguments)]
fn draw_line_antialias(
    buffer: &mut [u8],
    width: i32,
    height: i32,
    mut x1: i32,
    mut y1: i32,
    mut x2: i32,
    mut y2: i32,
    color: u32,
    alpha: f32,
) {
    let dx = (x2 - x1) as f32;
    let dy = (y2 - y1) as f32;

    // Hard clipping, because this routine has some problems with negative
    // coordinates (kept from the C).
    if (x1 < 0 || x1 > width)
        || (x2 < 0 || x2 > width)
        || (y1 < 0 || y1 > height)
        || (y2 < 0 || y2 > height)
    {
        return;
    }

    macro_rules! plot {
        ($x:expr, $y:expr, $d:expr) => {
            dla_plot(buffer, width, height, $x, $y, color, ($d) * alpha)
        };
    }

    if dx.abs() > dy.abs() {
        if x2 < x1 {
            std::mem::swap(&mut x1, &mut x2);
            std::mem::swap(&mut y1, &mut y2);
        }
        let gradient = dy / dx;
        let mut xend = (x1 as f32 + 0.5) as i32 as f32;
        let mut yend = y1 as f32 + gradient * (xend - x1 as f32);
        let xgap = rfpart(x1 as f32 + 0.5);
        let xpxl1 = xend as i32;
        let ypxl1 = ipart(yend);
        plot!(xpxl1, ypxl1, rfpart(yend) * xgap);
        plot!(xpxl1, ypxl1 + 1, fpart(yend) * xgap);
        let mut intery = yend + gradient;

        xend = (x2 as f32 + 0.5) as i32 as f32;
        yend = y2 as f32 + gradient * (xend - x2 as f32);
        let xgap = fpart(x2 as f32 + 0.5);
        let xpxl2 = xend as i32;
        let ypxl2 = ipart(yend);
        plot!(xpxl2, ypxl2, rfpart(yend) * xgap);
        plot!(xpxl2, ypxl2 + 1, fpart(yend) * xgap);

        for x in (xpxl1 + 1)..=(xpxl2 - 1) {
            plot!(x, ipart(intery), rfpart(intery));
            plot!(x, ipart(intery) + 1, fpart(intery));
            intery += gradient;
        }
    } else {
        if y2 < y1 {
            std::mem::swap(&mut x1, &mut x2);
            std::mem::swap(&mut y1, &mut y2);
        }
        let gradient = dx / dy;
        let mut yend = (y1 as f32 + 0.5) as i32 as f32;
        let mut xend = x1 as f32 + gradient * (yend - y1 as f32);
        let ygap = rfpart(y1 as f32 + 0.5);
        let ypxl1 = yend as i32;
        let xpxl1 = ipart(xend);
        plot!(xpxl1, ypxl1, rfpart(xend) * ygap);
        plot!(xpxl1, ypxl1 + 1, fpart(xend) * ygap);
        let mut interx = xend + gradient;

        yend = (y2 as f32 + 0.5) as i32 as f32;
        xend = x2 as f32 + gradient * (yend - y2 as f32);
        let ygap = fpart(y2 as f32 + 0.5);
        let ypxl2 = yend as i32;
        let xpxl2 = ipart(xend);
        plot!(xpxl2, ypxl2, rfpart(xend) * ygap);
        plot!(xpxl2, ypxl2 + 1, fpart(xend) * ygap);

        for y in (ypxl1 + 1)..=(ypxl2 - 1) {
            plot!(ipart(interx), y, rfpart(interx));
            plot!(ipart(interx) + 1, y, fpart(interx));
            interx += gradient;
        }
    }
}

struct Particle {
    x: f32,
    y: f32,
    xx: f32,
    yy: f32,
    vx: f32,
    vy: f32,
    color: u32,
    age: i32, // age from 0 to max_age
}

pub struct BinaryRing {
    epoch: usize,
    growth_delay: u64,
    ring_radius: i32,
    max_age: i32,
    curliness: f32,
    particles: Vec<Particle>,

    width: i32,
    height: i32,
    /// Persistent canvas in BGRA byte order (alpha pre-set at reset), so
    /// render() is a plain memcpy instead of a per-pixel u32 conversion.
    buffer: Vec<u8>,
    colors: [u32; 2],
    color: bool,
}

impl BinaryRing {
    fn init_particle(
        &mut self,
        i: usize,
        dx: f32,
        dy: f32,
        direction: f32,
        color: u32,
        rng: &mut impl RngExt,
    ) {
        let max_initial_velocity = 2.0f32;
        let age = rng.random_range(0..self.max_age);
        let p = &mut self.particles[i];
        p.x = -dx;
        p.y = -dy;
        p.xx = 0.0;
        p.yy = 0.0;
        p.vx = max_initial_velocity * direction.cos();
        p.vy = max_initial_velocity * direction.sin();
        p.age = age;
        p.color = color;
    }

    fn create_particles(&mut self, rng: &mut impl RngExt) {
        let n = self.particles.len();
        for i in 0..n {
            let emitx = self.ring_radius as f32 * (PI * 2.0 * (i as f32 / n as f32)).sin();
            let emity = self.ring_radius as f32 * (PI * 2.0 * (i as f32 / n as f32)).cos();
            let direction = (PI * i as f32) / n as f32;

            if self.epoch == WHITE && self.color {
                self.colors[WHITE] = next_color(self.colors[WHITE], rng);
            }
            let color = self.colors[WHITE];
            self.init_particle(i, emitx, emity, direction, color, rng);
        }
    }

    /// Randomly move one particle and draw it.
    fn move_particle(&mut self, i: usize, rng: &mut impl RngExt) {
        let w = self.width / 2;
        let h = self.height / 2;
        let max_dv = 1.0f32;

        let p = &mut self.particles[i];
        p.xx = p.x;
        p.yy = p.y;
        p.x += p.vx;
        p.y += p.vy;
        p.vx += frand1(rng) * self.curliness * max_dv;
        p.vy += frand1(rng) * self.curliness * max_dv;
        let (x, y, xx, yy, color) = (p.x, p.y, p.xx, p.yy, p.color);

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

        let p = &mut self.particles[i];
        p.age += 1;
        // If this is too old, die and be reborn.
        if p.age > self.max_age {
            let dir = frand1(rng) * 2.0 * PI;
            p.x = self.ring_radius as f32 * dir.sin();
            p.y = self.ring_radius as f32 * dir.cos();
            p.xx = 0.0;
            p.yy = 0.0;
            p.vx = 0.0;
            p.vy = 0.0;
            p.age = 0;

            if self.epoch == WHITE && self.color {
                self.colors[WHITE] = next_color(self.colors[WHITE], rng);
            }
            self.particles[i].color = self.colors[self.epoch];
        }
    }
}

impl Animation for BinaryRing {
    fn new(config: &AnimConfig) -> Self {
        let mut b = BinaryRing {
            epoch: WHITE,
            growth_delay: 10_000,
            ring_radius: 40,
            max_age: 400,
            curliness: 0.5,
            particles: Vec::new(),
            width: config.width as i32,
            height: config.height as i32,
            buffer: Vec::new(),
            colors: [0, 0],
            color: true,
        };
        b.reset(config);
        b
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();
        for i in 0..self.particles.len() {
            self.move_particle(i, &mut rng);
        }
        // Randomly switch ageColor periods.
        if rng.random_range(0..10000) > 9950 {
            self.epoch = if self.epoch == WHITE { BLACK } else { WHITE };
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let w = (self.width as u32).min(width) as usize * 4;
        let h = (self.height as u32).min(height) as usize;
        for y in 0..h {
            let src = y * self.width as usize * 4;
            let dst = y * width as usize * 4;
            buffer[dst..dst + w].copy_from_slice(&self.buffer[src..src + w]);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width as i32;
        self.height = config.height as i32;
        self.epoch = WHITE;
        self.growth_delay = 10_000;
        self.ring_radius = 40;
        self.max_age = 400;
        self.color = true;
        self.curliness = 0.5;
        self.colors[0] = rgb2point(0, 0, 0);
        self.colors[1] = rgb2point(255, 255, 255);

        let particles_number = if config.count > 0 {
            config.count as usize
        } else {
            5000
        };
        self.particles = (0..particles_number)
            .map(|_| Particle {
                x: 0.0,
                y: 0.0,
                xx: 0.0,
                yy: 0.0,
                vx: 0.0,
                vy: 0.0,
                color: 0,
                age: 0,
            })
            .collect();
        self.create_particles(&mut rng);

        self.buffer = vec![0u8; (self.width * self.height) as usize * 4];
        // Opaque black: alpha bytes are set once here and never touched by
        // the blend path.
        for px in self.buffer.chunks_exact_mut(4) {
            px[3] = 0xff;
        }
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::CompleteFrame
    }

    fn frame_delay_us(&self) -> u64 {
        self.growth_delay
    }
}
