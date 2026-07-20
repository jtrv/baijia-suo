/* euler2d --- 2 Dimensional Incompressible Inviscid Fluid Flow */

/*
 * Copyright (c) 2000 by Stephen Montgomery-Smith <stephen@math.missouri.edu>
 *
 * Permission to use, copy, modify, and distribute this software and its
 * documentation for any purpose and without fee is hereby granted,
 * provided that the above copyright notice appear in all copies and that
 * both that copyright notice and this permission notice appear in
 * supporting documentation.
 *
 * This file is provided AS IS with no warranties of any kind.  The author
 * shall have no liability with respect to the infringement of copyrights,
 * trade secrets or any patents by this file or any part thereof.  In no
 * event will the author be liable for any lost revenue or profits or
 * other special, indirect and consequential damages.
 *
 * Rust port of xscreensaver/xlockmore's euler2d.c.
 */

use rand::Rng;
use std::f64::consts::PI;

use crate::animation::primitives::{clear_buffer, draw_circle, draw_line, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

/* xlockmore ModStruct defaults: delay 10000, count 1024, cycles 3000,
 * ncolors 64.  Option defaults: -eulertail 10, -eulerpower 1. */
const DEF_DELAY_US: u64 = 10_000;
const DEF_COUNT: usize = 1024;
const DEF_CYCLES: i32 = 3000;
const DEF_NCOLORS: usize = 64;
const DEF_EULERTAIL: usize = 10;
const POWER: f64 = 1.0;

const NUMBER_OF_VORTEX_POINTS: usize = 20;
const N_BOUND_P: usize = 500;
const DEG_P: usize = 6;

const BLACK: Color = Color { a: 255, r: 0, g: 0, b: 0 };
const WHITE: Color = Color { a: 255, r: 255, g: 255, b: 255 };

type Seg = (i16, i16, i16, i16);

/* xlockmore SMOOTH_COLORS hue-wheel palette. */
fn pixel(col: usize, ncolors: usize) -> Color {
    Color::from_hsl(col as f32 / ncolors as f32, 1.0, 0.5)
}

fn fill_disc(buffer: &mut [u8], width: u32, height: u32, cx: i32, cy: i32, r: i32, color: Color) {
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                put_pixel(buffer, width, height, cx + dx, cy + dy, color);
            }
        }
    }
}

/* On Retina-sized windows the C sets a 3px round-capped line width on the
 * shared GC; this affects segments, erase lines and the boundary alike. */
fn draw_seg(buffer: &mut [u8], width: u32, height: u32, s: Seg, lw: i32, color: Color) {
    let (mut x0, mut y0, x1, y1) = (s.0 as i32, s.1 as i32, s.2 as i32, s.3 as i32);
    if lw <= 1 {
        draw_line(buffer, width, height, x0, y0, x1, y1, color);
        return;
    }
    let r = lw / 2;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        fill_disc(buffer, width, height, x0, y0, r, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/*
 * If variable_boundary, we make a variable boundary by mapping the unit
 * disk under a polynomial p, where p(z) = z + c_2 z^2 + ... + c_n z^n
 * with n = deg_p.  p_coef contains the complex numbers c_2, c_3, ... c_n.
 */

fn add(a1: &mut f64, a2: &mut f64, b1: f64, b2: f64) {
    *a1 += b1;
    *a2 += b2;
}

fn mult(a1: &mut f64, a2: &mut f64, b1: f64, b2: f64) {
    let temp = *a1 * b1 - *a2 * b2;
    *a2 = *a1 * b2 + *a2 * b1;
    *a1 = temp;
}

fn calc_p(p1: &mut f64, p2: &mut f64, z1: f64, z2: f64, p_coef: &[f64]) {
    *p1 = 0.0;
    *p2 = 0.0;
    for i in (2..=DEG_P).rev() {
        add(p1, p2, p_coef[(i - 2) * 2], p_coef[(i - 2) * 2 + 1]);
        mult(p1, p2, z1, z2);
    }
    add(p1, p2, 1.0, 0.0);
    mult(p1, p2, z1, z2);
}

/* Calculate |p'(z)|^2 */
fn calc_mod_dp2(z1: f64, z2: f64, p_coef: &[f64]) -> f64 {
    let mut mp1 = 0.0;
    let mut mp2 = 0.0;
    for i in (2..=DEG_P).rev() {
        let fi = i as f64;
        add(&mut mp1, &mut mp2, fi * p_coef[(i - 2) * 2], fi * p_coef[(i - 2) * 2 + 1]);
        mult(&mut mp1, &mut mp2, z1, z2);
    }
    add(&mut mp1, &mut mp2, 1.0, 0.0);
    mp1 * mp1 + mp2 * mp2
}

fn calc_all_mod_dp2(x: &[f64], n: usize, dead: &[bool], p_coef: &[f64], mod_dp2: &mut [f64]) {
    for j in 0..n {
        if dead[j] {
            continue;
        }
        let mut mp1 = 0.0;
        let mut mp2 = 0.0;
        let z1 = x[2 * j];
        let z2 = x[2 * j + 1];
        for i in (2..=DEG_P).rev() {
            let fi = i as f64;
            add(&mut mp1, &mut mp2, fi * p_coef[(i - 2) * 2], fi * p_coef[(i - 2) * 2 + 1]);
            mult(&mut mp1, &mut mp2, z1, z2);
        }
        add(&mut mp1, &mut mp2, 1.0, 0.0);
        mod_dp2[j] = mp1 * mp1 + mp2 * mp2;
    }
}

/* ret = x + k, killing points that move too far or leave the unit disk.
 * (The C's SUBTLE_PERTURB variant is compiled out, as in the original.) */
fn perturb(n: usize, dead: &mut [bool], ret: &mut [f64], x: &[f64], k: &[f64]) {
    for i in 0..n {
        if dead[i] {
            continue;
        }
        let x1 = x[2 * i];
        let x2 = x[2 * i + 1];
        let k1 = k[2 * i];
        let k2 = k[2 * i + 1];
        if k1 * k1 + k2 * k2 > 0.1 || x1 * x1 + x2 * x2 > 1.0 - 1e-5 {
            dead[i] = true;
        } else {
            ret[2 * i] = x1 + k1;
            ret[2 * i + 1] = x2 + k2;
        }
    }
}

/* Same as perturb(), but writes back into x in place: each index only ever
 * reads and writes its own pair, so no snapshot of x is needed. */
fn perturb_in_place(n: usize, dead: &mut [bool], x: &mut [f64], k: &[f64]) {
    for i in 0..n {
        if dead[i] {
            continue;
        }
        let x1 = x[2 * i];
        let x2 = x[2 * i + 1];
        let k1 = k[2 * i];
        let k2 = k[2 * i + 1];
        if k1 * k1 + k2 * k2 > 0.1 || x1 * x1 + x2 * x2 > 1.0 - 1e-5 {
            dead[i] = true;
        } else {
            x[2 * i] = x1 + k1;
            x[2 * i + 1] = x2 + k2;
        }
    }
}

pub struct Euler2D {
    /* real framebuffer size */
    buf_w: u32,
    buf_h: u32,
    buf: Vec<u8>,

    /* like C sp->width/height: logical size, shrunk for weird aspects */
    width: i32,
    height: i32,

    count: i32,
    xshift: f64,
    yshift: f64,
    scale: f64,
    xshift2: f64,
    yshift2: f64,
    radius: f64,

    n_points: usize,
    n_vortex: usize,

    /*  x[2i+0], x[2i+1] = coords of nth point; w[i] = vorticity */
    x: Vec<f64>,
    w: Vec<f64>,

    diffx: Vec<f64>,
    olddiffx: Vec<f64>,
    tempx: Vec<f64>,
    tempdiffx: Vec<f64>,
    /* xs = reflection of x about the unit circle */
    xs: Vec<f64>,
    x_is_zero: Vec<bool>,

    /* p = image of x under polynomial p; mod_dp2 = |p'(z)|^2 */
    p: Vec<f64>,
    mod_dp2: Vec<f64>,

    dead: Vec<bool>,

    old_segs: Vec<Vec<Seg>>,
    c_old_seg: usize,
    boundary_color: usize,
    hide_vortex: bool,
    lastx: Vec<i16>,

    p_coef: [f64; 2 * (DEG_P - 1)],
    boundary: Vec<Seg>,

    /* options */
    count_opt: usize,
    cycles: i32,
    colors: usize,
    tail_len: usize,
    variable_boundary: bool,
    delta_t: f64,
    line_width: i32,
}

impl Euler2D {
    fn calc_all_p(&mut self) {
        let start = if self.hide_vortex { self.n_vortex } else { 0 };
        for j in start..self.n_points {
            if self.dead[j] {
                continue;
            }
            let mut p1 = 0.0;
            let mut p2 = 0.0;
            let z1 = self.x[2 * j];
            let z2 = self.x[2 * j + 1];
            for i in (2..=DEG_P).rev() {
                add(&mut p1, &mut p2, self.p_coef[(i - 2) * 2], self.p_coef[(i - 2) * 2 + 1]);
                mult(&mut p1, &mut p2, z1, z2);
            }
            add(&mut p1, &mut p2, 1.0, 0.0);
            mult(&mut p1, &mut p2, z1, z2);
            self.p[2 * j] = p1;
            self.p[2 * j + 1] = p2;
        }
    }

    fn derivs(&mut self, use_tempx: bool) {
        if self.variable_boundary {
            /* C: calc_all_mod_dp2(sp->x, sp) -- always evaluated at the
             * committed positions, even for the midpoint substep on tempx. */
            calc_all_mod_dp2(&self.x, self.n_points, &self.dead, &self.p_coef, &mut self.mod_dp2);
        }

        /* Split-borrow: take x/tempx out so it can be read alongside the
         * other mutated fields below without cloning, then put it back. */
        let x = if use_tempx {
            std::mem::take(&mut self.tempx)
        } else {
            std::mem::take(&mut self.x)
        };

        for j in 0..self.n_vortex {
            if self.dead[j] {
                continue;
            }
            let nx = x[2 * j] * x[2 * j] + x[2 * j + 1] * x[2 * j + 1];
            if nx < 1e-10 {
                self.x_is_zero[j] = true;
            } else {
                self.x_is_zero[j] = false;
                self.xs[2 * j] = x[2 * j] / nx;
                self.xs[2 * j + 1] = x[2 * j + 1] / nx;
            }
        }

        self.diffx.fill(0.0);

        for i in 0..self.n_points {
            if self.dead[i] {
                continue;
            }
            let x1 = x[2 * i];
            let x2 = x[2 * i + 1];
            for j in 0..self.n_vortex {
                if self.dead[j] {
                    continue;
                }
                /*
                 * Biot-Savart kernel: effect of a vortex point at
                 * a = x[2j] on the point (x1,x2):  u = (x-a)/|x-a|^2,
                 * minus the reflected term (x-as)/|x-as|^2 with
                 * as = a/|a|^2, unless a is at the origin.
                 */
                let xij1 = x1 - x[2 * j];
                let xij2 = x2 - x[2 * j + 1];
                let nxij = if POWER == 1.0 {
                    xij1 * xij1 + xij2 * xij2
                } else {
                    (xij1 * xij1 + xij2 * xij2).powf((POWER + 1.0) / 2.0)
                };

                let (mut u1, mut u2) = if nxij >= 1e-4 {
                    (xij2 / nxij, -xij1 / nxij)
                } else {
                    (0.0, 0.0)
                };

                if !self.x_is_zero[j] {
                    let xij1 = x1 - self.xs[2 * j];
                    let xij2 = x2 - self.xs[2 * j + 1];
                    let nxij = if POWER == 1.0 {
                        xij1 * xij1 + xij2 * xij2
                    } else {
                        (xij1 * xij1 + xij2 * xij2).powf((POWER + 1.0) / 2.0)
                    };

                    if nxij < 1e-5 {
                        self.dead[i] = true;
                        u1 = 0.0;
                        u2 = 0.0;
                    } else {
                        u1 -= xij2 / nxij;
                        u2 += xij1 / nxij;
                    }
                }

                if !self.dead[i] {
                    self.diffx[2 * i] += u1 * self.w[j];
                    self.diffx[2 * i + 1] += u2 * self.w[j];
                }
            }

            if !self.dead[i] && self.variable_boundary {
                if self.mod_dp2[i] < 1e-5 {
                    self.dead[i] = true;
                } else {
                    self.diffx[2 * i] /= self.mod_dp2[i];
                    self.diffx[2 * i + 1] /= self.mod_dp2[i];
                }
            }
        }

        if use_tempx {
            self.tempx = x;
        } else {
            self.x = x;
        }
    }

    fn ode_solve(&mut self) {
        if self.count < 1 {
            /* midpoint method */
            self.derivs(false);
            self.olddiffx.copy_from_slice(&self.diffx);
            for i in 0..self.n_points {
                if !self.dead[i] {
                    self.tempdiffx[2 * i] = 0.5 * self.delta_t * self.diffx[2 * i];
                    self.tempdiffx[2 * i + 1] = 0.5 * self.delta_t * self.diffx[2 * i + 1];
                }
            }
            {
                let mut tempx = std::mem::take(&mut self.tempx);
                perturb(self.n_points, &mut self.dead, &mut tempx, &self.x, &self.tempdiffx);
                self.tempx = tempx;
            }
            self.derivs(true);
            for i in 0..self.n_points {
                if !self.dead[i] {
                    self.tempdiffx[2 * i] = self.delta_t * self.diffx[2 * i];
                    self.tempdiffx[2 * i + 1] = self.delta_t * self.diffx[2 * i + 1];
                }
            }
            {
                let mut x = std::mem::take(&mut self.x);
                let x_old = x.clone();
                perturb(self.n_points, &mut self.dead, &mut x, &x_old, &self.tempdiffx);
                self.x = x;
            }
        } else {
            /* Adams-Bashforth */
            self.derivs(false);
            for i in 0..self.n_points {
                if !self.dead[i] {
                    self.tempdiffx[2 * i] =
                        self.delta_t * (1.5 * self.diffx[2 * i] - 0.5 * self.olddiffx[2 * i]);
                    self.tempdiffx[2 * i + 1] = self.delta_t
                        * (1.5 * self.diffx[2 * i + 1] - 0.5 * self.olddiffx[2 * i + 1]);
                }
            }
            {
                let mut x = std::mem::take(&mut self.x);
                perturb_in_place(self.n_points, &mut self.dead, &mut x, &self.tempdiffx);
                self.x = x;
            }
            std::mem::swap(&mut self.olddiffx, &mut self.diffx);
        }
    }

    fn project(&self, b: usize) -> (i16, i16) {
        if self.variable_boundary {
            (
                (self.p[2 * b] * self.scale + self.xshift) as i16,
                (self.p[2 * b + 1] * self.scale + self.yshift) as i16,
            )
        } else {
            /* C: sp->width/2 and sp->height/2 are integer divisions */
            (
                (self.x[2 * b] * self.radius + (self.width / 2) as f64) as i16,
                (self.x[2 * b + 1] * self.radius + (self.height / 2) as f64) as i16,
            )
        }
    }

    fn init_euler2d(&mut self) {
        /* how many rotations to try to fill as much of screen as possible -
         * must be even */
        const NR_ROTATES: usize = 18;
        let mut rng = rand::rng();

        /* -eulerpower is fixed at its default 1.0; the C clamps power to
         * [0.5, 3.0] and does `variable_boundary &= power == 1.0`. */
        self.variable_boundary = POWER == 1.0;
        self.delta_t = 0.001;
        if POWER > 1.0 {
            self.delta_t *= 0.1_f64.powf(POWER - 1.0);
        }

        self.boundary_color = rng.random_range(0..self.colors);
        self.hide_vortex = rng.random_range(0..4_u32) != 0;

        self.count = 0;
        self.xshift2 = 0.0;
        self.yshift2 = 0.0;

        self.width = self.buf_w as i32;
        self.height = self.buf_h as i32;

        if self.width > self.height * 5 || /* window has weird aspect */
           self.height > self.width * 5
        {
            if self.width > self.height {
                self.height = (self.width as f64 * 0.8) as i32;
                self.yshift2 = (-self.height / 2) as f64;
            } else {
                self.width = (self.height as f64 * 0.8) as i32;
                self.xshift2 = (-self.width / 2) as f64;
            }
        }

        /* Retina displays */
        self.line_width = if self.width > 2560 || self.height > 2560 { 3 } else { 1 };

        self.n_points = self.count_opt + NUMBER_OF_VORTEX_POINTS;
        self.n_vortex = NUMBER_OF_VORTEX_POINTS;

        /* minimum tail 1, maximum tail MI_CYCLES */
        self.tail_len = DEF_EULERTAIL.max(1).min(self.cycles.max(1) as usize);

        /* Clear the background (MI_CLEARWINDOW). */
        self.buf = vec![0u8; (self.buf_w * self.buf_h * 4) as usize];
        clear_buffer(&mut self.buf, BLACK);

        self.old_segs = vec![Vec::new(); self.tail_len];
        self.c_old_seg = 0;

        self.x = vec![0.0; 2 * self.n_points];
        self.diffx = vec![0.0; 2 * self.n_points];
        self.olddiffx = vec![0.0; 2 * self.n_points];
        self.tempx = vec![0.0; 2 * self.n_points];
        self.tempdiffx = vec![0.0; 2 * self.n_points];
        self.w = vec![0.0; self.n_vortex];
        self.xs = vec![0.0; 2 * self.n_vortex];
        self.x_is_zero = vec![false; self.n_vortex];
        self.p = vec![0.0; 2 * self.n_points];
        self.mod_dp2 = vec![0.0; self.n_points];
        self.dead = vec![false; self.n_points];
        self.lastx = vec![0; 2 * self.n_points];

        if self.variable_boundary {
            /* Initialize polynomial p.  p(z) = z + c_2 z^2 + ... c_n z^n must
             * be a bijection of the unit disk onto its image; this is achieved
             * by insisting that sum_{k=2}^n k |c_k| = 1. */
            self.p_coef = [0.0; 2 * (DEG_P - 1)];
            let mut mag = 0.0;
            for k in 2..=DEG_P {
                let r = rng.random_range(0.0..1.0 / k as f64);
                let theta = rng.random_range(-PI..PI);
                self.p_coef[2 * (k - 2)] = r * theta.cos();
                self.p_coef[2 * (k - 2) + 1] = r * theta.sin();
                mag += k as f64 * r;
            }
            if mag > 0.0001 {
                for k in 2..=DEG_P {
                    self.p_coef[2 * (k - 2)] /= mag;
                    self.p_coef[2 * (k - 2) + 1] /= mag;
                }
            }

            /* Figure out the best rotation of the domain so that it fills as
             * much of the screen as possible, and the correct scaling. */
            let mut low = [1e5; NR_ROTATES];
            let mut high = [-1e5; NR_ROTATES];

            let nr = NR_ROTATES as f64;
            for k in 0..N_BOUND_P {
                let ang = |k: f64| k / N_BOUND_P as f64 * 2.0 * PI;
                let mut p1 = 0.0;
                let mut p2 = 0.0;
                calc_p(&mut p1, &mut p2, ang(k as f64).cos(), ang(k as f64).sin(), &self.p_coef);
                let mut pp1 = 0.0;
                let mut pp2 = 0.0;
                calc_p(&mut pp1, &mut pp2, ang(k as f64 - 1.0).cos(), ang(k as f64 - 1.0).sin(), &self.p_coef);
                let mut pn1 = 0.0;
                let mut pn2 = 0.0;
                calc_p(&mut pn1, &mut pn2, ang(k as f64 + 1.0).cos(), ang(k as f64 + 1.0).sin(), &self.p_coef);

                let mut angle1 = nr / PI * (p2 - pp2).atan2(p1 - pp1) - nr / 2.0;
                let mut angle2 = nr / PI * (pn2 - p2).atan2(pn1 - p1) - nr / 2.0;
                while angle1 < 0.0 {
                    angle1 += nr * 2.0;
                }
                while angle2 < 0.0 {
                    angle2 += nr * 2.0;
                }
                if angle1 > nr * 1.75 && angle2 < nr * 0.25 {
                    angle2 += nr * 2.0;
                }
                if angle1 < nr * 0.25 && angle2 > nr * 1.75 {
                    angle1 += nr * 2.0;
                }
                if angle2 < angle1 {
                    std::mem::swap(&mut angle1, &mut angle2);
                }
                for i in (angle1.floor() as i32)..(angle2.ceil() as i32) {
                    let dist = (i as f64 * PI / nr).cos() * p1 + (i as f64 * PI / nr).sin() * p2;
                    let idx = (i % (NR_ROTATES as i32 * 2)) as usize;
                    let bin = idx % NR_ROTATES;
                    if idx < NR_ROTATES {
                        if dist > high[bin] {
                            high[bin] = dist;
                        }
                        if dist < low[bin] {
                            low[bin] = dist;
                        }
                    } else {
                        if -dist > high[bin] {
                            high[bin] = -dist;
                        }
                        if -dist < low[bin] {
                            low[bin] = -dist;
                        }
                    }
                }
            }

            let mut bestscale = 0.0;
            let mut besti = 0;
            for i in 0..NR_ROTATES {
                let xscale = (self.width as f64 - 5.0) / (high[i] - low[i]);
                let yscale = (self.height as f64 - 5.0)
                    / (high[(i + NR_ROTATES / 2) % NR_ROTATES]
                        - low[(i + NR_ROTATES / 2) % NR_ROTATES]);
                let scale = if xscale > yscale { yscale } else { xscale };
                if scale > bestscale {
                    bestscale = scale;
                    besti = i;
                }
            }

            /* Do the rotation: replace p(z) by a^{-1} p(a z),
             * a = exp(i best_angle). */
            let mut p1 = 1.0;
            let mut p2 = 0.0;
            for k in 2..=DEG_P {
                mult(&mut p1, &mut p2, (besti as f64 * PI / nr).cos(), (besti as f64 * PI / nr).sin());
                let mut c1 = self.p_coef[2 * (k - 2)];
                let mut c2 = self.p_coef[2 * (k - 2) + 1];
                mult(&mut c1, &mut c2, p1, p2);
                self.p_coef[2 * (k - 2)] = c1;
                self.p_coef[2 * (k - 2) + 1] = c2;
            }

            self.scale = bestscale;
            self.xshift = -(low[besti] + high[besti]) / 2.0 * self.scale
                + (self.width / 2) as f64;
            if besti < NR_ROTATES / 2 {
                self.yshift = -(low[besti + NR_ROTATES / 2] + high[besti + NR_ROTATES / 2]) / 2.0
                    * self.scale
                    + (self.height / 2) as f64;
            } else {
                self.yshift = (low[besti - NR_ROTATES / 2] + high[besti - NR_ROTATES / 2]) / 2.0
                    * self.scale
                    + (self.height / 2) as f64;
            }

            self.xshift += self.xshift2;
            self.yshift += self.yshift2;

            /* Initialize boundary */
            self.boundary = vec![(0, 0, 0, 0); N_BOUND_P];
            for k in 0..N_BOUND_P {
                let mut p1 = 0.0;
                let mut p2 = 0.0;
                calc_p(
                    &mut p1,
                    &mut p2,
                    (k as f64 / N_BOUND_P as f64 * 2.0 * PI).cos(),
                    (k as f64 / N_BOUND_P as f64 * 2.0 * PI).sin(),
                    &self.p_coef,
                );
                self.boundary[k].0 = (p1 * self.scale + self.xshift) as i16;
                self.boundary[k].1 = (p2 * self.scale + self.yshift) as i16;
            }
            for k in 1..N_BOUND_P {
                self.boundary[k].2 = self.boundary[k - 1].0;
                self.boundary[k].3 = self.boundary[k - 1].1;
            }
            self.boundary[0].2 = self.boundary[N_BOUND_P - 1].0;
            self.boundary[0].3 = self.boundary[N_BOUND_P - 1].1;
        } else {
            self.boundary = Vec::new();
            if self.width > self.height {
                self.radius = self.height as f64 / 2.0 - 5.0;
            } else {
                self.radius = self.width as f64 / 2.0 - 5.0;
            }
        }

        /* Initialize point positions */
        for i in self.n_vortex..self.n_points {
            loop {
                let r = rng.random_range(0.0..1.0_f64).sqrt();
                let theta = rng.random_range(-PI..PI);
                self.x[2 * i] = r * theta.cos();
                self.x[2 * i + 1] = r * theta.sin();
                /* make sure the initial distribution of points is uniform */
                if !(self.variable_boundary
                    && calc_mod_dp2(self.x[2 * i], self.x[2 * i + 1], &self.p_coef)
                        < rng.random_range(0.0..4.0_f64))
                {
                    break;
                }
            }
        }

        let n = rng.random_range(0..4_usize) + 2;
        /* number of vortex points with negative vorticity */
        let np = if n % 2 != 0 {
            rng.random_range(0..n + 1)
        } else {
            /* if n is even make sure that np==n/2 is twice as likely as the
             * other possibilities. */
            let np = rng.random_range(0..n + 2);
            if np == n + 1 { n / 2 } else { np }
        };
        for k in 0..n {
            let r = rng.random_range(0.0..0.77_f64).sqrt();
            let theta = rng.random_range(-PI..PI);
            let x = r * theta.cos();
            let y = r * theta.sin();
            let r = 0.02 + rng.random_range(0.0..0.1_f64);
            let w = (2.0 * (k < np) as i32 as f64 - 1.0) * 2.0 / self.n_vortex as f64;
            for i in self.n_vortex * k / n..self.n_vortex * (k + 1) / n {
                let theta = rng.random_range(-PI..PI);
                self.x[2 * i] = x + r * theta.cos();
                self.x[2 * i + 1] = y + r * theta.sin();
                self.w[i] = w;
            }
        }
    }
}

impl Animation for Euler2D {
    fn new(config: &AnimConfig) -> Self {
        let mut s = Euler2D {
            buf_w: config.width,
            buf_h: config.height,
            buf: Vec::new(),
            width: 0,
            height: 0,
            count: 0,
            xshift: 0.0,
            yshift: 0.0,
            scale: 0.0,
            xshift2: 0.0,
            yshift2: 0.0,
            radius: 0.0,
            n_points: 0,
            n_vortex: 0,
            x: Vec::new(),
            w: Vec::new(),
            diffx: Vec::new(),
            olddiffx: Vec::new(),
            tempx: Vec::new(),
            tempdiffx: Vec::new(),
            xs: Vec::new(),
            x_is_zero: Vec::new(),
            p: Vec::new(),
            mod_dp2: Vec::new(),
            dead: Vec::new(),
            old_segs: Vec::new(),
            c_old_seg: 0,
            boundary_color: 0,
            hide_vortex: false,
            lastx: Vec::new(),
            p_coef: [0.0; 2 * (DEG_P - 1)],
            boundary: Vec::new(),
            count_opt: DEF_COUNT,
            cycles: DEF_CYCLES,
            colors: DEF_NCOLORS,
            tail_len: DEF_EULERTAIL,
            variable_boundary: true,
            delta_t: 0.001,
            line_width: 1,
        };
        s.reset(config);
        s
    }

    fn tick(&mut self) {
        self.ode_solve();
        if self.variable_boundary {
            self.calc_all_p();
        }

        let (bw, bh, lw) = (self.buf_w, self.buf_h, self.line_width);

        /* Reuse the buffer that's tail_len frames old: erase what it holds
         * (if any) first, then clear and refill it in place as this tick's
         * segs instead of allocating a fresh Vec every tick. */
        let mut csegs: Vec<Seg> = std::mem::take(&mut self.old_segs[self.c_old_seg]);
        if self.count != 0 {
            for s in &csegs {
                draw_seg(&mut self.buf, bw, bh, *s, lw, BLACK);
            }
        }
        csegs.clear();

        for b in self.n_vortex..self.n_points {
            if self.dead[b] {
                continue;
            }
            let (x2, y2) = self.project(b);
            csegs.push((self.lastx[2 * b], self.lastx[2 * b + 1], x2, y2));
            self.lastx[2 * b] = x2;
            self.lastx[2 * b + 1] = y2;
        }
        let n_non_vortex_segs = csegs.len();

        if !self.hide_vortex {
            for b in 0..self.n_vortex {
                if self.dead[b] {
                    continue;
                }
                let (x2, y2) = self.project(b);
                csegs.push((self.lastx[2 * b], self.lastx[2 * b + 1], x2, y2));
                self.lastx[2 * b] = x2;
                self.lastx[2 * b + 1] = y2;
            }
        }

        if self.count != 0 {
            if self.colors > 2 {
                /* render colour */
                for col in 0..self.colors {
                    let start = col * n_non_vortex_segs / self.colors;
                    let finish = (col + 1) * n_non_vortex_segs / self.colors;
                    let c = pixel(col, self.colors);
                    for s in &csegs[start..finish] {
                        draw_seg(&mut self.buf, bw, bh, *s, lw, c);
                    }
                }
                if !self.hide_vortex {
                    for s in &csegs[n_non_vortex_segs..] {
                        draw_seg(&mut self.buf, bw, bh, *s, lw, WHITE);
                    }
                }
            } else {
                /* render mono */
                for s in &csegs {
                    draw_seg(&mut self.buf, bw, bh, *s, lw, WHITE);
                }
            }

            let bc = if self.colors > 2 {
                pixel(self.boundary_color, self.colors)
            } else {
                WHITE
            };
            if self.variable_boundary {
                for s in &self.boundary {
                    draw_seg(&mut self.buf, bw, bh, *s, lw, bc);
                }
            } else {
                /* C: XDrawArc over a (2r+2)-sized box: circle of radius
                 * radius+1 centred on (width/2, height/2). */
                draw_circle(
                    &mut self.buf,
                    bw,
                    bh,
                    self.width / 2,
                    self.height / 2,
                    self.radius as i32 + 1,
                    bc,
                );
            }

            /* Copy to erase-list */
            self.old_segs[self.c_old_seg] = csegs;
            self.c_old_seg += 1;
            if self.c_old_seg >= self.tail_len {
                self.c_old_seg = 0;
            }
        }

        self.count += 1;
        if self.count > self.cycles {
            /* pick a new flow */
            self.init_euler2d();
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let w = (self.buf_w.min(width) as usize) * 4;
        let h = self.buf_h.min(height) as usize;
        for y in 0..h {
            let s = y * self.buf_w as usize * 4;
            let d = y * width as usize * 4;
            buffer[d..d + w].copy_from_slice(&self.buf[s..s + w]);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.buf_w = config.width;
        self.buf_h = config.height;
        self.count_opt = if config.count == 0 {
            DEF_COUNT
        } else {
            config.count.unsigned_abs() as usize
        };
        self.cycles = if config.cycles <= 0 { DEF_CYCLES } else { config.cycles };
        self.colors = if config.ncolors <= 0 {
            DEF_NCOLORS
        } else {
            config.ncolors as usize
        };
        self.init_euler2d();
    }


    fn frame_delay_us(&self) -> u64 {
        DEF_DELAY_US
    }
}
