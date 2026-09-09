/* xscreensaver, Copyright (c) 1992-2013 Jamie Zawinski <jwz@jwz.org>
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Simulation of a pair of quasi-gravitational fields, maybe sorta kinda
 * a little like the strong and weak electromagnetic forces.  Derived from
 * a Lispm screensaver by John Pezaris <pz@mit.edu>.  Viscosity added by
 * Philip Edward Cutone, III <pc2d+@andrew.cmu.edu>.
 *
 * Rust port of xscreensaver's attraction.c (with the needed parts of
 * utils/spline.c -- Copyright (c) 1987-1989 Stanford University, from the
 * InterViews distribution -- and utils/hsv.c / utils/colors.c ported inline).
 */

use crate::rng::RngExt;
use std::f64::consts::PI;

use crate::animation::primitives::{
    draw_line, draw_thick_line, hsv_to_rgb, make_smooth_colormap, put_pixel, rgb16, Color, Spline,
};
use crate::animation::{AnimConfig, Animation};

const MAX_SIZE: i32 = 16;

const BLACK: Color = Color {
    a: 255,
    r: 0,
    g: 0,
    b: 0,
};

/* ---- utils/colors.c (make_random_colormap) ---- */

fn make_random_colors(n: usize, rng: &mut impl RngExt) -> Vec<Color> {
    // bright_p variant
    (0..n)
        .map(|_| {
            let h = rng.random_range(0..360); /* range 0-360    */
            let s = (rng.random_range(0..70) + 30) as f64 / 100.0; /* range 30%-100% */
            let v = (rng.random_range(0..34) + 66) as f64 / 100.0; /* range 66%-100% */
            let (r, g, b) = hsv_to_rgb(h, s, v);
            rgb16(r, g, b)
        })
        .collect()
}

/* ---- drawing helpers ---- */

fn draw_polyline(buffer: &mut [u8], width: u32, height: u32, pts: &[(i32, i32)], color: Color) {
    for w in pts.windows(2) {
        draw_line(buffer, width, height, w[0].0, w[0].1, w[1].0, w[1].1, color);
    }
}

// XFillPolygon (Complex, even-odd scanline fill).
fn fill_polygon(buffer: &mut [u8], width: u32, height: u32, verts: &[(i32, i32)], color: Color) {
    let n = verts.len();
    if n < 3 {
        return;
    }
    let (Some(min_y), Some(max_y)) = (
        verts.iter().map(|&(_, y)| y).min(),
        verts.iter().map(|&(_, y)| y).max(),
    ) else {
        return;
    };
    let min_y = min_y.max(0) as u32;
    let max_y = match max_y.min(height as i32 - 1) {
        v if v < 0 => return,
        v => v as u32,
    };
    let mut xs: Vec<i32> = Vec::with_capacity(8);
    for scan_y in min_y..=max_y {
        let sy = scan_y as i32;
        xs.clear();
        for i in 0..n {
            let (x0, y0) = verts[i];
            let (x1, y1) = verts[(i + 1) % n];
            if (y0 <= sy && sy < y1) || (y1 <= sy && sy < y0) {
                let x = x0 + (x1 - x0) * (sy - y0) / (y1 - y0);
                xs.push(x);
            }
        }
        xs.sort_unstable();
        let mut i = 0;
        while i + 1 < xs.len() {
            let x0 = xs[i].max(0);
            let x1 = xs[i + 1].min(width as i32 - 1);
            for x in x0..=x1 {
                put_pixel(buffer, width, height, x, sy, color);
            }
            i += 2;
        }
    }
}

// XFillArc over a size x size bounding box at (x, y): a filled circle.
fn fill_arc(buffer: &mut [u8], width: u32, height: u32, x: i32, y: i32, size: i32, color: Color) {
    if size <= 0 {
        return;
    }
    let r = size as f32 / 2.0;
    let cx = x as f32 + r;
    let cy = y as f32 + r;
    for yy in y..y + size {
        for xx in x..x + size {
            let dx = xx as f32 + 0.5 - cx;
            let dy = yy as f32 + 0.5 - cy;
            if dx * dx + dy * dy <= r * r {
                put_pixel(buffer, width, height, xx, yy, color);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ObjectMode {
    Ball,
    Line,
    Polygon,
    Spline,
    SplineFilled,
    Tail,
}

struct Ball {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    dx: f64,
    dy: f64,
    mass: f64,
    size: i32,
    pixel_index: usize,
}

pub struct Attraction {
    balls: Vec<Ball>,
    npoints: usize,
    threshold: i32,
    global_size: i32,
    segments: i32,
    walls_p: bool,
    maxspeed_p: bool,
    cbounce_p: bool,
    point_stack: Vec<(i32, i32)>,
    point_stack_fp: usize,
    colors: Vec<Color>,
    ncolors: usize,
    fg_index: usize,
    color_shift: i32,
    color_tick: i32,
    xlim: i32,
    ylim: i32,
    no_erase_yet: bool, // for tail mode fix
    viscosity: f64,
    mode: ObjectMode,
    total_ticks: i32,
    spline: Spline,
    line_width: i32,

    width: u32,
    height: u32,
    buf: Vec<u8>,
    delay_us: u64,
}

impl Attraction {
    /// Compute the force of attraction/repulsion between one ball and all others.
    fn compute_force(&self, i: usize, rng: &mut impl RngExt) -> (f64, f64) {
        let mut dx_ret = 0.0;
        let mut dy_ret = 0.0;
        for j in 0..self.npoints {
            if i == j {
                continue;
            }
            let x_dist = self.balls[j].x - self.balls[i].x;
            let y_dist = self.balls[j].y - self.balls[i].y;
            let dist2 = (x_dist * x_dist) + (y_dist * y_dist);
            let dist = dist2.sqrt();

            if dist > 0.1 {
                // the balls are not overlapping
                let new_acc = (self.balls[j].mass / dist2)
                    * if dist < self.threshold as f64 { -1.0 } else { 1.0 };
                let new_acc_dist = new_acc / dist;
                dx_ret += new_acc_dist * x_dist;
                dy_ret += new_acc_dist * y_dist;
            } else {
                // the balls are overlapping; move randomly
                dx_ret += rng.random::<f64>() * 10.0 - 5.0;
                dy_ret += rng.random::<f64>() * 10.0 - 5.0;
            }
        }
        (dx_ret, dy_ret)
    }
}

impl Animation for Attraction {
    fn new(config: &AnimConfig) -> Self {
        let mut a = Attraction {
            balls: Vec::new(),
            npoints: 0,
            threshold: 200,
            global_size: 0,
            segments: 500,
            walls_p: true,
            maxspeed_p: true,
            cbounce_p: true,
            point_stack: Vec::new(),
            point_stack_fp: 0,
            colors: Vec::new(),
            ncolors: 2,
            fg_index: 0,
            color_shift: 3,
            color_tick: 0,
            xlim: config.width as i32,
            ylim: config.height as i32,
            no_erase_yet: true,
            viscosity: 1.0,
            mode: ObjectMode::Ball,
            total_ticks: 0,
            spline: Spline::new(0),
            line_width: 1,
            width: config.width,
            height: config.height,
            buf: Vec::new(),
            delay_us: 10_000,
        };
        a.reset(config);
        a
    }

    fn tick(&mut self) {
        let mut rng = crate::rng::rng();
        let last_point_stack_fp = self.point_stack_fp;
        let mut radius = self.global_size / 2;

        self.total_ticks += 1;

        if self.global_size == 0 {
            radius = MAX_SIZE / 3;
        }

        // graphmode defaults to "none"; the velocity meters are not ported.

        // compute the force of attraction/repulsion among all balls
        for i in 0..self.npoints {
            let (dx, dy) = self.compute_force(i, &mut rng);
            self.balls[i].dx = dx;
            self.balls[i].dy = dy;
        }

        // move the balls according to the forces now in effect
        for i in 0..self.npoints {
            let old_x = self.balls[i].x;
            let old_y = self.balls[i].y;
            let size = self.balls[i].size;

            self.balls[i].vx += self.balls[i].dx;
            self.balls[i].vy += self.balls[i].dy;

            // don't let them get too fast: impose a terminal velocity
            // (actually, make the medium have friction)
            if self.balls[i].vx.abs() > 10.0 && self.maxspeed_p {
                self.balls[i].vx *= 0.9;
                self.balls[i].dx = 0.0;
            }
            if self.viscosity != 1.0 {
                self.balls[i].vx *= self.viscosity;
            }

            if self.balls[i].vy.abs() > 10.0 && self.maxspeed_p {
                self.balls[i].vy *= 0.9;
                self.balls[i].dy = 0.0;
            }
            if self.viscosity != 1.0 {
                self.balls[i].vy *= self.viscosity;
            }

            self.balls[i].x += self.balls[i].vx;
            self.balls[i].y += self.balls[i].vy;

            // bounce off the walls if desired
            // note: a ball is actually its upper left corner
            if self.walls_p {
                if self.cbounce_p {
                    // with correct bouncing: so long as it's out of range,
                    // keep bouncing (at most 4 times)
                    let mut bounce_allowed = 4;
                    while bounce_allowed > 0
                        && (self.balls[i].x >= (self.xlim - self.balls[i].size) as f64
                            || self.balls[i].y >= (self.ylim - self.balls[i].size) as f64
                            || self.balls[i].x <= 0.0
                            || self.balls[i].y <= 0.0)
                    {
                        bounce_allowed -= 1;
                        if self.balls[i].x >= (self.xlim - self.balls[i].size) as f64 {
                            self.balls[i].x =
                                2.0 * (self.xlim - self.balls[i].size) as f64 - self.balls[i].x;
                            self.balls[i].vx = -self.balls[i].vx;
                        }
                        if self.balls[i].y >= (self.ylim - self.balls[i].size) as f64 {
                            self.balls[i].y =
                                2.0 * (self.ylim - self.balls[i].size) as f64 - self.balls[i].y;
                            self.balls[i].vy = -self.balls[i].vy;
                        }
                        if self.balls[i].x <= 0.0 {
                            self.balls[i].x = -self.balls[i].x;
                            self.balls[i].vx = -self.balls[i].vx;
                        }
                        if self.balls[i].y <= 0.0 {
                            self.balls[i].y = -self.balls[i].y;
                            self.balls[i].vy = -self.balls[i].vy;
                        }
                    }
                } else {
                    // with old bouncing
                    if self.balls[i].x >= (self.xlim - self.balls[i].size) as f64 {
                        self.balls[i].x = (self.xlim - self.balls[i].size - 1) as f64;
                        if self.balls[i].vx > 0.0 {
                            self.balls[i].vx = -self.balls[i].vx;
                        }
                    }
                    if self.balls[i].y >= (self.ylim - self.balls[i].size) as f64 {
                        self.balls[i].y = (self.ylim - self.balls[i].size - 1) as f64;
                        if self.balls[i].vy > 0.0 {
                            self.balls[i].vy = -self.balls[i].vy;
                        }
                    }
                    if self.balls[i].x <= 0.0 {
                        self.balls[i].x = 0.0;
                        if self.balls[i].vx < 0.0 {
                            self.balls[i].vx = -self.balls[i].vx;
                        }
                    }
                    if self.balls[i].y <= 0.0 {
                        self.balls[i].y = 0.0;
                        if self.balls[i].vy < 0.0 {
                            self.balls[i].vy = -self.balls[i].vy;
                        }
                    }
                }
            }

            let new_x = self.balls[i].x;
            let new_y = self.balls[i].y;

            if self.mode == ObjectMode::Ball {
                let color = self.colors[self.balls[i].pixel_index % self.ncolors];
                fill_arc(&mut self.buf, self.width, self.height, old_x as i32, old_y as i32, size, BLACK);
                fill_arc(&mut self.buf, self.width, self.height, new_x as i32, new_y as i32, size, color);
            } else {
                self.point_stack[self.point_stack_fp] = (new_x as i32, new_y as i32);
                self.point_stack_fp += 1;
            }
        }

        // draw the lines or polygons after computing all points
        if self.mode != ObjectMode::Ball {
            // close the polygon
            self.point_stack[self.point_stack_fp] = (self.balls[0].x as i32, self.balls[0].y as i32);
            self.point_stack_fp += 1;
            if self.point_stack_fp == self.point_stack.len() {
                self.point_stack_fp = 0;
            }
            let t = self.color_tick;
            self.color_tick += 1;
            if t == self.color_shift {
                self.color_tick = 0;
                self.fg_index = (self.fg_index + 1) % self.ncolors;
            }
        }

        let fg = self.colors[self.fg_index % self.ncolors];
        let n1 = self.npoints + 1;
        match self.mode {
            ObjectMode::Ball => {}
            ObjectMode::Line => {
                if self.segments > 0 {
                    let a = self.point_stack_fp;
                    draw_polyline(&mut self.buf, self.width, self.height, &self.point_stack[a..a + n1], BLACK);
                }
                let a = last_point_stack_fp;
                draw_polyline(&mut self.buf, self.width, self.height, &self.point_stack[a..a + n1], fg);
            }
            ObjectMode::Polygon => {
                if self.segments > 0 {
                    let a = self.point_stack_fp;
                    fill_polygon(&mut self.buf, self.width, self.height, &self.point_stack[a..a + n1], BLACK);
                }
                let a = last_point_stack_fp;
                fill_polygon(&mut self.buf, self.width, self.height, &self.point_stack[a..a + n1], fg);
            }
            ObjectMode::Tail => {
                let stack_size = self.point_stack.len();
                for i in 0..self.npoints {
                    let index = self.point_stack_fp + i;
                    let next_index = (index + n1) % stack_size;
                    let erase = if self.no_erase_yet {
                        if self.total_ticks >= self.segments {
                            self.no_erase_yet = false;
                            true
                        } else {
                            false
                        }
                    } else {
                        true
                    };
                    if erase {
                        let (x0, y0) = self.point_stack[index];
                        let (x1, y1) = self.point_stack[next_index];
                        draw_thick_line(
                            &mut self.buf,
                            self.width,
                            self.height,
                            x0 + radius,
                            y0 + radius,
                            x1 + radius,
                            y1 + radius,
                            self.line_width,
                            BLACK,
                        );
                    }
                    let index = last_point_stack_fp + i;
                    let mut next_index = index as i32 - n1 as i32;
                    next_index %= stack_size as i32;
                    if next_index < 0 {
                        next_index += stack_size as i32;
                    }
                    let next_index = next_index as usize;
                    if self.point_stack[next_index] == (0, 0) {
                        continue;
                    }
                    let (x0, y0) = self.point_stack[index];
                    let (x1, y1) = self.point_stack[next_index];
                    draw_thick_line(
                        &mut self.buf,
                        self.width,
                        self.height,
                        x0 + radius,
                        y0 + radius,
                        x1 + radius,
                        y1 + radius,
                        self.line_width,
                        fg,
                    );
                }
            }
            ObjectMode::Spline | ObjectMode::SplineFilled => {
                if self.segments > 0 {
                    for i in 0..self.npoints {
                        let p = self.point_stack[self.point_stack_fp + i];
                        self.spline.control_x[i] = p.0 as f64;
                        self.spline.control_y[i] = p.1 as f64;
                    }
                    self.spline.compute_closed_spline();
                    if self.mode == ObjectMode::SplineFilled {
                        fill_polygon(&mut self.buf, self.width, self.height, &self.spline.points, BLACK);
                    } else {
                        draw_polyline(&mut self.buf, self.width, self.height, &self.spline.points, BLACK);
                    }
                }
                for i in 0..self.npoints {
                    let p = self.point_stack[last_point_stack_fp + i];
                    self.spline.control_x[i] = p.0 as f64;
                    self.spline.control_y[i] = p.1 as f64;
                }
                self.spline.compute_closed_spline();
                if self.mode == ObjectMode::SplineFilled {
                    fill_polygon(&mut self.buf, self.width, self.height, &self.spline.points, fg);
                } else {
                    draw_polyline(&mut self.buf, self.width, self.height, &self.spline.points, fg);
                }
            }
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let w = (self.width.min(width) as usize) * 4;
        let h = self.height.min(height) as usize;
        for y in 0..h {
            let s = y * self.width as usize * 4;
            let d = y * width as usize * 4;
            buffer[d..d + w].copy_from_slice(&self.buf[s..s + w]);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = crate::rng::rng();

        self.width = config.width;
        self.height = config.height;
        self.xlim = config.width as i32;
        self.ylim = config.height as i32;
        let midx = self.xlim / 2;
        let midy = self.ylim / 2;
        self.walls_p = true;

        // if there aren't walls, don't set a limit on the radius
        // (radius resource defaults to 0 -> computed from the window size)
        let r = (self.xlim / 2).min(self.ylim / 2) - 50;
        let vx = 0.0;
        let vy = 0.0;

        self.npoints = if config.count > 0 {
            config.count as usize
        } else {
            3 + rng.random_range(0..5)
        };

        self.no_erase_yet = true; // for tail mode fix
        self.total_ticks = 0;
        self.segments = 500;
        self.threshold = 200;
        self.delay_us = 10_000;
        self.global_size = if config.size > 1 { config.size } else { 0 };
        self.maxspeed_p = true;
        self.cbounce_p = true;
        self.color_shift = 3;
        self.color_tick = 0;
        self.viscosity = 1.0;

        // The C default mode is "balls" with no way to change it at runtime;
        // pick one of the six object modes at random for variety instead
        // (glow, orbit, graphmode and the mouse ball keep their defaults off).
        self.mode = match rng.random_range(0..6_u32) {
            0 => ObjectMode::Ball,
            1 => ObjectMode::Line,
            2 => ObjectMode::Polygon,
            3 => ObjectMode::Spline,
            4 => ObjectMode::SplineFilled,
            _ => ObjectMode::Tail,
        };
        if self.mode == ObjectMode::Polygon && self.npoints < 3 {
            self.mode = ObjectMode::Line;
        }

        self.ncolors = config.ncolors.max(2) as usize;
        match self.mode {
            ObjectMode::Ball => {
                self.ncolors = self.npoints;
                self.colors = make_random_colors(self.ncolors, &mut rng);
            }
            _ => {
                self.colors = make_smooth_colormap(self.ncolors, &mut rng);
                self.ncolors = self.colors.len().max(2);
            }
        }
        self.fg_index = 0;

        if self.mode != ObjectMode::Ball {
            let size = if self.segments > 0 { self.segments } else { 1 };
            self.point_stack = vec![(0, 0); size as usize * (self.npoints + 1)];
            self.point_stack_fp = 0;
        } else {
            self.point_stack = Vec::new();
            self.point_stack_fp = 0;
        }

        self.line_width = if self.mode == ObjectMode::Tail {
            if self.global_size != 0 {
                self.global_size
            } else {
                MAX_SIZE * 2 / 3
            }
        } else {
            1
        };

        let size_scale = if config.width < 100 || config.height < 100 {
            0.75 // tiny windows
        } else {
            3.0
        };
        // let's make the balls bigger by default
        let rand_size = |rng: &mut crate::rng::Rng| -> i32 {
            (size_scale * (8 + rng.random_range(0..7)) as f64) as i32
        };

        let th = rng.random::<f64>() * (PI + PI);
        self.balls = (0..self.npoints)
            .map(|i| {
                let new_size = if self.global_size != 0 {
                    self.global_size
                } else {
                    rand_size(&mut rng)
                };
                Ball {
                    dx: 0.0,
                    dy: 0.0,
                    size: new_size,
                    mass: (new_size * new_size * 10) as f64,
                    x: midx as f64
                        + r as f64 * (i as f64 * ((PI + PI) / self.npoints as f64) + th).cos(),
                    y: midy as f64
                        + r as f64 * (i as f64 * ((PI + PI) / self.npoints as f64) + th).sin(),
                    vx: if vx != 0.0 {
                        vx
                    } else {
                        (6.0 - rng.random_range(0..11) as f64) / 8.0
                    },
                    vy: if vy != 0.0 {
                        vy
                    } else {
                        (6.0 - rng.random_range(0..11) as f64) / 8.0
                    },
                    pixel_index: if self.mode == ObjectMode::Ball {
                        rng.random_range(0..self.ncolors)
                    } else {
                        0
                    },
                }
            })
            .collect();

        // This lets modes where the points don't really have any size use the
        // whole window; otherwise they would be bounced somewhat early.
        if matches!(
            self.mode,
            ObjectMode::Line | ObjectMode::Spline | ObjectMode::SplineFilled | ObjectMode::Polygon
        ) {
            for b in self.balls.iter_mut().skip(1) {
                b.size = 0;
            }
        }

        self.spline = Spline::new(self.npoints);

        self.buf = vec![0u8; (config.width * config.height * 4) as usize];
        for px in self.buf.as_chunks_mut::<4>().0 {
            px[3] = 0xff;
        }
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
