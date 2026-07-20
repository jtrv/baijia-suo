//! Undulating, throbbing, star-like shapes.
//
// xscreensaver, Copyright (c) 1997-2015 Jamie Zawinski <jwz@jwz.org>
//
// Permission to use, copy, modify, distribute, and sell this software and its
// documentation for any purpose is hereby granted without fee, provided that
// the above copyright notice appear in all copies and that both that
// copyright notice and this permission notice appear in supporting
// documentation.  No representations are made about the suitability of this
// software for any purpose.  It is provided "as is" without express or
// implied warranty.
//
// Rust port of xscreensaver/hacks/starfish.c.

use rand::Rng;
use std::f64::consts::PI;
use crate::animation::primitives::{put_pixel, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const SCALE: f64 = 1000.0;

#[derive(PartialEq)]
enum StarfishMode {
    Pulse,
    Zoom,
}

pub struct Starfish {
    mode: StarfishMode,
    blob_p: bool,
    skip: usize,
    x: f64,
    y: f64,
    th: f64,
    rotv: f64,
    rota: f64,
    elasticity: f64,
    rot_max: f64,
    min_r: f64,
    max_r: f64,
    npoints: usize,
    r: Vec<f64>,
    controls: Vec<(f64, f64)>,
    current_spline: Vec<(f64, f64)>,
    prev: Vec<(f64, f64)>,

    colors: f32,
    ncolors: usize,
    
    size: f64,
    winwidth: u32,
    winheight: u32,
    counter: usize,
    thickness: f64,
    delay_us: u64,
    cycles: usize,
}

fn mid_point(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
    ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
}

fn third_point(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
    ((2.0 * x0 + x1) / 3.0, (2.0 * y0 + y1) / 3.0)
}

fn can_approx_with_line(x0: f64, y0: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> bool {
    let mut triangle_area = x0 * y2 - x2 * y0 + x2 * y3 - x3 * y2 + x3 * y0 - x0 * y3;
    triangle_area *= triangle_area;
    let dx = x3 - x0;
    let dy = y3 - y0;
    let side_squared = dx * dx + dy * dy;
    triangle_area <= side_squared
}

fn add_line(points: &mut Vec<(f64, f64)>, x0: f64, y0: f64, x1: f64, y1: f64) {
    if points.is_empty() {
        points.push((x0, y0));
    }
    points.push((x1, y1));
}

fn add_bezier_arc(
    points: &mut Vec<(f64, f64)>,
    x0: f64, y0: f64,
    x1: f64, y1: f64,
    x2: f64, y2: f64,
    x3: f64, y3: f64,
) {
    let (midx01, midy01) = mid_point(x0, y0, x1, y1);
    let (midx12, midy12) = mid_point(x1, y1, x2, y2);
    let (midx23, midy23) = mid_point(x2, y2, x3, y3);
    let (midlsegx, midlsegy) = mid_point(midx01, midy01, midx12, midy12);
    let (midrsegx, midrsegy) = mid_point(midx12, midy12, midx23, midy23);
    let (cx, cy) = mid_point(midlsegx, midlsegy, midrsegx, midrsegy);

    if can_approx_with_line(x0, y0, midlsegx, midlsegy, cx, cy) {
        add_line(points, x0, y0, cx, cy);
    } else if midx01 != x1 || midy01 != y1 || midlsegx != x2 || midlsegy != y2 || cx != x3 || cy != y3 {
        add_bezier_arc(points, x0, y0, midx01, midy01, midlsegx, midlsegy, cx, cy);
    }

    if can_approx_with_line(cx, cy, midx23, midy23, x3, y3) {
        add_line(points, cx, cy, x3, y3);
    } else if cx != x0 || cy != y0 || midrsegx != x1 || midrsegy != y1 || midx23 != x2 || midy23 != y2 {
        add_bezier_arc(points, cx, cy, midrsegx, midrsegy, midx23, midy23, x3, y3);
    }
}

fn calc_section(
    points: &mut Vec<(f64, f64)>,
    cm1: (f64, f64),
    c: (f64, f64),
    cp1: (f64, f64),
    cp2: (f64, f64),
) {
    let (p1x, p1y) = third_point(c.0, c.1, cp1.0, cp1.1);
    let (p2x, p2y) = third_point(cp1.0, cp1.1, c.0, c.1);
    let (tmpx, tmpy) = third_point(c.0, c.1, cm1.0, cm1.1);
    let (p0x, p0y) = mid_point(tmpx, tmpy, p1x, p1y);
    let (tmpx2, tmpy2) = third_point(cp1.0, cp1.1, cp2.0, cp2.1);
    let (p3x, p3y) = mid_point(tmpx2, tmpy2, p2x, p2y);
    add_bezier_arc(points, p0x, p0y, p1x, p1y, p2x, p2y, p3x, p3y);
}

/// Appends into a caller-owned scratch vec — this runs on starfish's 2ms
/// simulation clock, so a fresh allocation per tick is real churn.
fn compute_closed_spline(controls: &[(f64, f64)], points: &mut Vec<(f64, f64)>) {
    points.clear();
    let n = controls.len();
    if n < 3 {
        return;
    }

    calc_section(points, controls[n - 1], controls[0], controls[1], controls[2]);
    for i in 1..n - 2 {
        calc_section(points, controls[i - 1], controls[i], controls[i + 1], controls[i + 2]);
    }
    calc_section(points, controls[n - 3], controls[n - 2], controls[n - 1], controls[0]);
    calc_section(points, controls[n - 2], controls[n - 1], controls[0], controls[1]);
}

fn fill_polygon(buffer: &mut [u8], width: u32, height: u32, points: &[(f64, f64)], color: Color) {
    if points.len() < 3 {
        return;
    }
    let mut min_y = points[0].1;
    let mut max_y = points[0].1;
    for &(_, y) in points.iter() {
        if y < min_y { min_y = y; }
        if y > max_y { max_y = y; }
    }
    let start_y = (min_y.floor() as i32).max(0);
    let end_y = (max_y.ceil() as i32).min(height as i32 - 1);

    let mut nodes: Vec<f64> = Vec::with_capacity(8);
    for y in start_y..=end_y {
        let y_f = y as f64 + 0.5;
        nodes.clear();
        let mut j = points.len() - 1;
        for i in 0..points.len() {
            let (xi, yi) = points[i];
            let (xj, yj) = points[j];

            if (yi < y_f && yj >= y_f) || (yj < y_f && yi >= y_f) {
                let intersect = xi + (y_f - yi) * (xj - xi) / (yj - yi);
                nodes.push(intersect);
            }
            j = i;
        }
        nodes.sort_by(|a, b| a.total_cmp(b));

        for chunk in nodes.chunks(2) {
            if chunk.len() == 2 {
                let x1 = (chunk[0].round() as i32).max(0);
                let x2 = (chunk[1].round() as i32).min(width as i32 - 1);
                for x in x1..=x2 {
                    put_pixel(buffer, width, height, x, y, color);
                }
            }
        }
    }
}

impl Starfish {
    fn throb_starfish(&mut self) {
        let frac = (PI + PI) / (self.npoints as f64);

        for i in 0..self.npoints {
            let mut r = self.r[i];
            let mut ra = if r > 0.0 { r } else { -r };
            let th = if self.th > 0.0 { self.th } else { -self.th };

            let x = self.x + ra * ((i as f64) * frac + th).cos();
            let y = self.y + ra * ((i as f64) * frac + th).sin();

            self.controls[i] = (x / SCALE, y / SCALE);

            if self.mode == StarfishMode::Zoom && (i % self.skip == 0) {
                continue;
            }

            let mut elasticity = self.elasticity;
            let mut ratio = ra / (self.max_r - self.min_r);
            if ratio > 0.5 {
                ratio = 1.0 - ratio;
            }
            ratio *= 2.0;
            ratio = (ratio * 0.9) + 0.1;
            elasticity *= ratio;

            ra += if r >= 0.0 { elasticity } else { -elasticity };
            if i % self.skip == 0 {
                ra += elasticity / 2.0;
            }

            r = ra * (if r >= 0.0 { 1.0 } else { -1.0 });

            if (ra > self.max_r && r >= 0.0) || (ra < self.min_r && r < 0.0) {
                r = -r;
            }
            self.r[i] = r;
        }
    }

    fn spin_starfish(&mut self) {
        let mut rng = rand::rng();
        let mut th = self.th;

        if th < 0.0 {
            th = -(th + self.rotv);
        } else {
            th += self.rotv;
        }

        if th > PI + PI {
            th -= PI + PI;
        } else if th < 0.0 {
            th += PI + PI;
        }

        self.th = if self.th > 0.0 { th } else { -th };
        self.rotv += self.rota;

        if self.rotv > self.rot_max || self.rotv < -self.rot_max {
            self.rota = -self.rota;
        } else if self.rotv < 0.0 {
            if rng.random::<bool>() {
                self.rotv = 0.0;
                if self.rota < 0.0 {
                    self.rota = -self.rota;
                }
            } else {
                self.rotv = -self.rotv;
                self.rota = -self.rota;
                self.th = -self.th;
            }
        }

        if rng.random_range(0..120) == 0 {
            self.rota = -self.rota;
        }

        if rng.random_range(0..200) == 0 {
            if rng.random::<bool>() {
                self.rota *= 1.2;
            } else {
                self.rota *= 0.8;
            }
        }
    }

    fn init_starfish(&mut self) {
        let mut rng = rand::rng();

        self.elasticity = SCALE * self.thickness;
        if self.elasticity == 0.0 {
            self.elasticity =
                (rng.random_range(0..5) + rng.random_range(0..5) + rng.random_range(0..5)) as f64 * SCALE;
        }

        if self.rotv == -1.0 {
            self.rotv = 4.0 * (rng.random::<f64>() + rng.random::<f64>() + rng.random::<f64>());
        }
        self.rotv /= 360.0;

        if self.blob_p {
            self.elasticity *= 3.0;
            self.rotv *= 3.0;
        }

        self.rot_max = self.rotv * 2.0;
        self.rota = 0.0004 + 0.0002 * rng.random::<f64>();

        if rng.random_range(0..20) == 5 {
            self.size *= 0.35 * (rng.random::<f64>() + rng.random::<f64>()) + 0.3;
        }

        let skips = [2, 2, 2, 2, 3, 3, 3, 6, 6, 12];
        self.skip = skips[rng.random_range(0..skips.len())];

        let limit = if self.skip == 2 { 3 } else { 12 };
        if rng.random_range(0..limit) == 0 {
            self.mode = StarfishMode::Zoom;
        } else {
            self.mode = StarfishMode::Pulse;
        }

        let winwidth = (self.winwidth as f64) * SCALE;
        let winheight = (self.winheight as f64) * SCALE;
        self.size *= SCALE;

        self.max_r = self.size;
        self.min_r = SCALE;

        self.x = winwidth / 2.0;
        self.y = winheight / 2.0;

        let sign = if rng.random::<bool>() { 1.0 } else { -1.0 };
        self.th = 2.0 * PI * rng.random::<f64>() * sign;

        let sizes = [
            3, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 8, 8, 8, 10, 35,
        ];
        let mut nsizes = sizes.len();
        if self.skip > 3 {
            nsizes -= 4;
        }
        self.npoints = self.skip * sizes[rng.random_range(0..nsizes)];

        self.r = vec![0.0; self.npoints];
        self.controls = vec![(0.0, 0.0); self.npoints];

        for i in 0..self.npoints {
            self.r[i] = if i % self.skip == 0 { 0.0 } else { self.size };
        }
    }
}

impl Animation for Starfish {
    fn new(config: &AnimConfig) -> Self {
        let mut s = Starfish {
            mode: StarfishMode::Pulse,
            blob_p: false,
            skip: 2,
            x: 0.0,
            y: 0.0,
            th: 0.0,
            rotv: -1.0,
            rota: 0.0,
            elasticity: 0.0,
            rot_max: 0.0,
            min_r: 0.0,
            max_r: 0.0,
            npoints: 0,
            r: Vec::new(),
            controls: Vec::new(),
            current_spline: Vec::new(),
            prev: Vec::new(),
            colors: 0.0,
            ncolors: 64,
            size: 0.0,
            winwidth: 0,
            winheight: 0,
            counter: 0,
            thickness: 20.0,
            delay_us: 2000,
            cycles: 1000,
        };
        s.reset(config);
        s
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.blob_p = rng.random_ratio(1, 10);
        
        self.rotv = -1.0; // default
        // thickness in xscreensaver starfish is "-20" by default, meaning random
        self.thickness = 20.0 * rng.random::<f64>();

        self.size = (config.width.min(config.height)) as f64;
        if self.blob_p {
            self.size /= 2.0;
        } else {
            self.size *= 1.3;
        }

        self.winwidth = config.width;
        self.winheight = config.height;
        self.prev.clear();
        self.current_spline.clear();
        self.counter = 0;

        self.cycles = if config.cycles <= 0 {
            1000
        } else {
            config.cycles as usize
        };
        self.ncolors = config.ncolors.max(2) as usize;
        self.delay_us = config.delay_us;

        self.init_starfish();
    }

    fn tick(&mut self) {
        self.counter += 1;
        if self.counter > self.cycles {
            let config = AnimConfig {
                width: self.winwidth,
                height: self.winheight,
                delay_us: self.delay_us,
                max_fps: 0, // internal re-init config; only the player reads max_fps
                cycles: self.cycles as i32,
                ncolors: self.ncolors as i32,
                count: 0,
                size: 1,
            };
            self.reset(&config);
            return;
        }

        self.throb_starfish();
        self.spin_starfish();

        // Rotate buffers: the outgoing `prev` allocation becomes the new
        // spline's scratch, so steady state allocates nothing.
        let mut new_spline = std::mem::take(&mut self.prev);
        compute_closed_spline(&self.controls, &mut new_spline);
        self.prev = std::mem::take(&mut self.current_spline);
        self.current_spline = new_spline;

        self.colors += 1.0;
        if self.colors >= self.ncolors as f32 {
            self.colors = 0.0;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if self.prev.is_empty() {
            return;
        }

        let mut points = Vec::with_capacity(self.current_spline.len() + self.prev.len());
        points.extend_from_slice(&self.current_spline);
        points.extend_from_slice(&self.prev);

        let color = Color::from_hsl(self.colors / self.ncolors as f32, 1.0, 0.5);

        fill_polygon(buffer, width, height, &points, color);
    }

    fn render_policy(&self) -> RenderPolicy {
        if self.blob_p {
            RenderPolicy::ClearThenRender
        } else {
            RenderPolicy::Incremental
        }
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
