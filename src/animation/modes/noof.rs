//! Flowery, circular shapes.
//
// noof, Copyright (c) 2004-2018 Bill Torzewski <billt@worksitez.com>
//
// Permission to use, copy, modify, distribute, and sell this software and its
// documentation for any purpose is hereby granted without fee, provided that
// the above copyright notice appear in all copies and that both that
// copyright notice and this permission notice appear in supporting
// documentation.  No representations are made about the suitability of this
// software for any purpose.  It is provided "as is" without express or
// implied warranty.
//
// Rust port of xscreensaver/hacks/glx/noof.c.

use rand::Rng;
use std::cell::Cell;
use std::f32::consts::PI;
use crate::animation::primitives::{clear_buffer, draw_line, Color};
use crate::animation::{AnimConfig, Animation};

const N_SHAPES: usize = 7;
// noof.c's default delay is 10000us; the port originally ran at 1000us (10x fast).
const DEFAULT_DELAY_US: u64 = 10_000;

#[derive(Clone, Default)]
struct Shape {
    pos: [f32; 3],
    dir: [f32; 3],
    acc: [f32; 3],
    col: [f32; 3],
    hsv: [f32; 3],
    hpr: [f32; 3],
    ang: f32,
    spn: f32,
    sca: f32,
    geep: f32,
    peep: f32,
    speedsq: f32,
    blad: usize,
}

pub struct Noof {
    shapes: [Shape; N_SHAPES],
    tko: u32,
    width: u32,
    height: u32,
    needs_clear: Cell<bool>,
    wd: f32,
    ht: f32,
}

fn initshapes(shape: &mut Shape) {
    let mut rng = rand::rng();

    for k in 0..3 {
        shape.pos[k] = rng.random::<f32>();
        let f = rng.random::<f32>();
        shape.dir[k] = (f - 0.5) * 0.05;
        let f2 = rng.random::<f32>();
        shape.acc[k] = (f2 - 0.5) * 0.0002;
        shape.col[k] = rng.random::<f32>();
    }

    shape.speedsq = shape.dir[0] * shape.dir[0] + shape.dir[1] * shape.dir[1];
    shape.blad = 2 + (rng.random::<f32>() * 17.0) as usize;
    shape.ang = rng.random::<f32>();
    shape.spn = (rng.random::<f32>() - 0.5) * 40.0 / (10 + shape.blad) as f32;
    shape.sca = rng.random::<f32>() * 0.1 + 0.08;
    shape.dir[0] *= shape.sca;
    shape.dir[1] *= shape.sca;

    shape.hsv[0] = rng.random::<f32>() * 360.0;
    shape.hsv[1] = rng.random::<f32>() * 0.6 + 0.4;
    shape.hsv[2] = rng.random::<f32>() * 0.7 + 0.3;

    shape.hpr[0] = rng.random::<f32>() * 0.005 * 360.0;
    shape.hpr[1] = rng.random::<f32>() * 0.03;
    shape.hpr[2] = rng.random::<f32>() * 0.02;

    shape.geep = 0.0;
    shape.peep = 0.01 + rng.random::<f32>() * 0.2;
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let mut h = h;
    while h < 0.0 { h += 360.0; }
    while h >= 360.0 { h -= 360.0; }
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);

    if s <= 0.0 {
        return [v, v, v];
    }

    let h_norm = h / 60.0;
    let hi = h_norm as i32;
    let f = h_norm - hi as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));

    match hi {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

// Simple triangle fill with alpha blending
fn fill_triangle(buffer: &mut [u8], width: u32, height: u32, v0: (i32, i32), v1: (i32, i32), v2: (i32, i32), r: u8, g: u8, b: u8, a: u8) {
    let mut v = [v0, v1, v2];
    v.sort_by_key(|p| p.1);
    let (p0, p1, p2) = (v[0], v[1], v[2]);

    let min_x = p0.0.min(p1.0).min(p2.0).max(0);
    let max_x = p0.0.max(p1.0).max(p2.0).min(width as i32 - 1);
    let min_y = p0.1.min(p1.1).min(p2.1).max(0);
    let max_y = p0.1.max(p1.1).max(p2.1).min(height as i32 - 1);

    if min_x > max_x || min_y > max_y {
        return;
    }

    let edge = |a: (i32, i32), b: (i32, i32), c: (i32, i32)| -> i32 {
        (c.0 - a.0) * (b.1 - a.1) - (c.1 - a.1) * (b.0 - a.0)
    };

    let area = edge(p0, p1, p2);
    let (p1, p2) = if area < 0 { (p2, p1) } else { (p1, p2) };
    if area == 0 { return; }

    let mut w0_row = edge(p1, p2, (min_x, min_y));
    let mut w1_row = edge(p2, p0, (min_x, min_y));
    let mut w2_row = edge(p0, p1, (min_x, min_y));

    let dx0 = p1.1 - p2.1;
    let dy0 = p2.0 - p1.0;
    let dx1 = p2.1 - p0.1;
    let dy1 = p0.0 - p2.0;
    let dx2 = p0.1 - p1.1;
    let dy2 = p1.0 - p0.0;

    let alpha = a as f32 / 255.0;
    let inv_alpha = 1.0 - alpha;
    let stride = (width * 4) as usize;

    for py in min_y..=max_y {
        let mut w0 = w0_row;
        let mut w1 = w1_row;
        let mut w2 = w2_row;

        let mut idx = (py as usize) * stride + (min_x as usize) * 4;

        for _px in min_x..=max_x {
            if w0 >= 0 && w1 >= 0 && w2 >= 0 {
                let cb = buffer[idx] as f32;
                let cg = buffer[idx + 1] as f32;
                let cr = buffer[idx + 2] as f32;

                buffer[idx] = ((b as f32 * alpha) + (cb * inv_alpha)) as u8;
                buffer[idx + 1] = ((g as f32 * alpha) + (cg * inv_alpha)) as u8;
                buffer[idx + 2] = ((r as f32 * alpha) + (cr * inv_alpha)) as u8;
                buffer[idx + 3] = 255;
            }
            w0 += dx0;
            w1 += dx1;
            w2 += dx2;
            idx += 4;
        }
        w0_row += dy0;
        w1_row += dy1;
        w2_row += dy2;
    }
}

impl Animation for Noof {
    fn new(config: &AnimConfig) -> Self {
        let mut s = Noof {
            shapes: Default::default(),
            tko: 0,
            width: config.width,
            height: config.height,
            needs_clear: Cell::new(true),
            wd: 1.0,
            ht: 1.0,
        };
        s.reset(config);
        s
    }

    fn tick(&mut self) {
        // gravity
        let mut dirs = [[0.0; 3]; N_SHAPES];
        for a in 0..N_SHAPES {
            for b in 0..a {
                let mut t = self.shapes[b].pos[0] - self.shapes[a].pos[0];
                let mut d2 = t * t;
                t = self.shapes[b].pos[1] - self.shapes[a].pos[1];
                d2 += t * t;
                if d2 < 0.000001 {
                    d2 = 0.00001;
                }
                if d2 < 0.1 {
                    let v0 = self.shapes[b].pos[0] - self.shapes[a].pos[0];
                    let v1 = self.shapes[b].pos[1] - self.shapes[a].pos[1];
                    let z = 0.00000001 * -2.0 / d2;
                    
                    dirs[a][0] += v0 * z * self.shapes[b].sca;
                    dirs[b][0] += -v0 * z * self.shapes[a].sca;
                    dirs[a][1] += v1 * z * self.shapes[b].sca;
                    dirs[b][1] += -v1 * z * self.shapes[a].sca;
                }
            }
        }
        for a in 0..N_SHAPES {
            self.shapes[a].dir[0] += dirs[a][0];
            self.shapes[a].dir[1] += dirs[a][1];
        }

        // update
        let mut reset_indices = Vec::new();
        for i in 0..N_SHAPES {
            let shape = &mut self.shapes[i];
            
            // motion update
            if (shape.pos[0] < -shape.sca * self.wd && shape.dir[0] < 0.0)
                || (shape.pos[0] > (1.0 + shape.sca) * self.wd && shape.dir[0] > 0.0)
            {
                shape.dir[0] = -shape.dir[0];
            } else if (shape.pos[1] < -shape.sca * self.ht && shape.dir[1] < 0.0)
                || (shape.pos[1] > (1.0 + shape.sca) * self.ht && shape.dir[1] > 0.0)
            {
                shape.dir[1] = -shape.dir[1];
            }

            shape.pos[0] += shape.dir[0];
            shape.pos[1] += shape.dir[1];

            shape.ang += shape.spn;
            shape.geep += shape.peep;
            if shape.geep > 360.0 * 5.0 {
                shape.geep -= 360.0 * 5.0;
            }
            if shape.ang < 0.0 {
                shape.ang += 360.0;
            }
            if shape.ang > 360.0 {
                shape.ang -= 360.0;
            }

            // color update
            if shape.hsv[1] <= 0.5 && shape.hpr[1] < 0.0 {
                shape.hpr[1] = -shape.hpr[1];
            }
            if shape.hsv[1] >= 1.0 && shape.hpr[1] > 0.0 {
                shape.hpr[1] = -shape.hpr[1];
            }
            if shape.hsv[2] <= 0.4 && shape.hpr[2] < 0.0 {
                shape.hpr[2] = -shape.hpr[2];
            }
            if shape.hsv[2] >= 1.0 && shape.hpr[2] > 0.0 {
                shape.hpr[2] = -shape.hpr[2];
            }

            shape.hsv[0] += shape.hpr[0];
            shape.hsv[1] += shape.hpr[1];
            shape.hsv[2] += shape.hpr[2];

            shape.col = hsv_to_rgb(shape.hsv[0], shape.hsv[1], shape.hsv[2]);

            // check rebirth (from drawleaf in C)
            let geep_rad = shape.geep * PI / 180.0;
            let mut y = 0.10 * geep_rad.sin() + 0.099 * (geep_rad * 5.12).sin();
            if y < 0.0 {
                y = -y;
            }
            let mut x = 0.15 * geep_rad.cos() + 0.149 * (geep_rad * 5.12).cos();
            if x < 0.0 {
                x = -x;
            }

            if y < 0.001 && x > 0.000002 && (self.tko & 0x1) == 0 {
                reset_indices.push(i);
            }
        }
        for i in reset_indices {
            initshapes(&mut self.shapes[i]);
            self.tko += 1;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if self.needs_clear.get() {
            clear_buffer(buffer, Color::new(255, 0, 0, 0));
            self.needs_clear.set(false);
        }

        let bladeratio: [f32; 20] = [
            0.0, 0.0, 3.00000, 1.73205, 1.00000, 0.72654, 0.57735, 0.48157,
            0.41421, 0.36397, 0.19076, 0.29363, 0.26795, 0.24648,
            0.22824, 0.21256, 0.19891, 0.18693, 0.17633, 0.16687,
        ];

        for i in 0..N_SHAPES {
            let shape = &self.shapes[i];
            
            let geep_rad = shape.geep * PI / 180.0;
            let mut y = 0.10 * geep_rad.sin() + 0.099 * (geep_rad * 5.12).sin();
            if y < 0.0 {
                y = -y;
            }
            let mut x = 0.15 * geep_rad.cos() + 0.149 * (geep_rad * 5.12).cos();
            if x < 0.0 {
                x = -x;
            }

            let w1 = (geep_rad * 15.3).sin();
            let wobble = 3.0 + 2.00 * (geep_rad * 0.4).sin() + 3.94261 * w1;

            if shape.blad < 20 && y > x * bladeratio[shape.blad] {
                y = x * bladeratio[shape.blad];
            } else if shape.blad >= 20 {
                let ratio = bladeratio[19];
                if y > x * ratio { y = x * ratio; }
            }

            for b in 0..shape.blad {
                let angle_rad = (shape.ang + b as f32 * (360.0 / shape.blad as f32)).to_radians();
                let cos_a = angle_rad.cos();
                let sin_a = angle_rad.sin();
                let scale = wobble * shape.sca;

                let transform = |vx: f32, vy: f32| -> (f32, f32) {
                    let sx = vx * scale;
                    let sy = vy * scale;
                    let rx = sx * cos_a - sy * sin_a;
                    let ry = sx * sin_a + sy * cos_a;
                    (rx + shape.pos[0], ry + shape.pos[1])
                };

                let va = transform(x * shape.sca, 0.0);
                let vb = transform(x, y);
                let vc = transform(x, -y);
                let vd = transform(0.3, 0.0);

                let to_pixel = |v: (f32, f32)| -> (i32, i32) {
                    let px = (v.0 / self.wd * width as f32).round() as i32;
                    let py = height as i32 - 1 - (v.1 / self.ht * height as f32).round() as i32;
                    (px, py)
                };

                let pa = to_pixel(va);
                let pb = to_pixel(vb);
                let pc = to_pixel(vc);
                let pd = to_pixel(vd);

                fill_triangle(buffer, width, height, pa, pb, pc, 0, 0, 0, 0x60);
                fill_triangle(buffer, width, height, pb, pc, pd, 0, 0, 0, 0x60);

                let cr = (shape.col[0] * 255.0) as u8;
                let cg = (shape.col[1] * 255.0) as u8;
                let cb = (shape.col[2] * 255.0) as u8;
                let line_color = Color::new(255, cr, cg, cb);

                draw_line(buffer, width, height, pa.0, pa.1, pb.0, pb.1, line_color);
                draw_line(buffer, width, height, pb.0, pb.1, pd.0, pd.1, line_color);
                draw_line(buffer, width, height, pd.0, pd.1, pc.0, pc.1, line_color);
                draw_line(buffer, width, height, pc.0, pc.1, pa.0, pa.1, line_color);
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;

        if self.width <= self.height {
            self.wd = 1.0;
            self.ht = self.height as f32 / self.width as f32;
        } else {
            self.wd = self.width as f32 / self.height as f32;
            self.ht = 1.0;
        }

        for i in 0..N_SHAPES {
            initshapes(&mut self.shapes[i]);
        }
        self.tko = 0;
        self.needs_clear.set(true);
    }

    fn clears_each_frame(&self) -> bool {
        false
    }

    fn frame_delay_us(&self) -> u64 {
        // noof.c: "*delay: 10000". The old hardcoded 1_000 here overran the
        // budget (mean render ~1ms at 1080p), so the player perpetually fell
        // behind and burst catch-up ticks — multiple full renders in one
        // frame, i.e. visible stutter. The player's generic fallback would
        // hand us 16_666 via config, so return the authentic constant
        // directly (same pattern as goop.rs).
        DEFAULT_DELAY_US
    }
}
