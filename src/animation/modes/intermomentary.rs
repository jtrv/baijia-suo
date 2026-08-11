/*
 *  InterMomentary (dragorn@kismetwireless.net)
 *  Directly ported code from complexification.net InterMomentary art
 *  http://www.complexification.net/gallery/machines/interMomentary/applet_l/interMomentary_l.pde
 *
 * Intersecting Circles, Instantaneous
 * J. Tarbell                              + complexification.net
 * Albuquerque, New Mexico
 * May, 2004
 *
 * a REAS collaboration for the            + groupc.net
 * Whitney Museum of American Art ARTPORT  + artport.whitney.org
 * Robert Hodgin                           + flight404.com
 * William Ngan                            + metaphorical.net
 *
 * 1.0  Oct 10 2004  dragorn  Completed first port
 *
 * Based, of course, on other hacks in:
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
 * Rust port of xscreensaver's intermomentary.c for baijia-suo.
 */

use crate::animation::primitives::{hsv_to_rgb, put_pixel, rgb16, Color};
use crate::animation::{AnimConfig, Animation};
use rand::RngExt;

/* defaults table */
const DRAW_DELAY: u64 = 30_000;
const NUM_DISCS: usize = 85;
const MAX_RIDERS: usize = 40;
const MAX_RADIUS: f32 = 100.0;
const NCOLORS: usize = 256;

fn frand(rng: &mut impl RngExt, up: f32) -> f32 {
    rng.random::<f32>() * up
}

fn fill_rect(buffer: &mut [u8], w: u32, h: u32, x: i32, y: i32, size: i32, color: Color) {
    for yy in y..y + size {
        for xx in x..x + size {
            put_pixel(buffer, w, h, xx, yy, color);
        }
    }
}

/// Pixel rider
#[derive(Clone, Copy)]
struct PxRider {
    t: f32,
    vt: f32,
    mycharge: f32,
}

/// disc of light
struct Disc {
    x: f32,
    y: f32,
    r: f32,
    dr: f32,
    vx: f32,
    vy: f32,
    numr: usize,
    px_riders: Vec<PxRider>,
}

pub struct Intermomentary {
    width: u32,
    height: u32,
    discs: Vec<Disc>,
    colors: Vec<Color>,
    ncolors: usize,
    pscale: i32,
    off_alpha: Vec<u8>,
    buffer: Vec<u8>,
    delay_us: u64,
}

impl Intermomentary {
    fn make_disc(&mut self, rng: &mut impl RngExt, x: f32, y: f32, vx: f32, vy: f32, r: f32) {
        let numr = ((frand(rng, r) / 2.62) as usize).min(MAX_RIDERS);
        let px_riders = (0..MAX_RIDERS)
            .map(|_| PxRider {
                vt: 0.0,
                t: frand(rng, std::f32::consts::PI * 2.0),
                mycharge: 0.0,
            })
            .collect();
        self.discs.push(Disc {
            x,
            y,
            vx,
            vy,
            dr: r,
            r: frand(rng, r) / 3.0,
            numr,
            px_riders,
        });
    }

    /// alpha blended point plotting into the charge map (C trans_point).
    /// The C version does this with unsigned arithmetic whose wraparound is UB;
    /// we use the intended signed blend.
    fn trans_point(&mut self, x1: i32, y1: i32, myc: i32, a: f32) -> i32 {
        if x1 >= 0 && x1 < self.width as i32 && y1 >= 0 && y1 < self.height as i32 {
            let idx = y1 as usize * self.width as usize + x1 as usize;
            if a >= 1.0 {
                self.off_alpha[idx] = myc as u8;
            } else {
                let c = self.off_alpha[idx] as i32;
                let c = (c as f32 + (myc - c) as f32 * a) as i32;
                self.off_alpha[idx] = c.clamp(0, 255) as u8;
                return c;
            }
        }
        0
    }

    fn get_pixel(&self, v: i32) -> Color {
        let idx = (v.clamp(0, 255) as usize) * (self.ncolors - 1) / 255;
        self.colors[idx]
    }

    fn move_disc(&mut self, dnum: usize) {
        let (w, h) = (self.width as f32, self.height as f32);
        let d = &mut self.discs[dnum];

        // add velocity to position
        d.x += d.vx;
        d.y += d.vy;

        // bound check
        if d.x + d.r < 0.0 {
            d.x += w + d.r + d.r;
        }
        if d.x - d.r > w {
            d.x -= w + d.r + d.r;
        }
        if d.y + d.r < 0.0 {
            d.y += h + d.r + d.r;
        }
        if d.y - d.r > h {
            d.y -= h + d.r + d.r;
        }

        // increase to destination radius
        if d.r < d.dr {
            d.r += 0.1;
        }
    }

    fn draw_glowpoint(&mut self, px: f32, py: f32) {
        for i in -2i32..3 {
            for j in -2i32..3 {
                let a = 0.8 - i as f32 * i as f32 * 0.1 - j as f32 * j as f32 * 0.1;
                let c = self.trans_point((px + i as f32) as i32, (py + j as f32) as i32, 255, a);
                let color = self.get_pixel(c);
                fill_rect(
                    &mut self.buffer,
                    self.width,
                    self.height,
                    (px + i as f32) as i32,
                    (py + j as f32) as i32,
                    self.pscale,
                    color,
                );
            }
        }
    }

    fn moverender_rider(
        &mut self,
        rng: &mut impl RngExt,
        mut rid: PxRider,
        x: f32,
        y: f32,
        r: f32,
    ) -> PxRider {
        use std::f32::consts::PI;

        // add velocity to theta
        rid.t = (rid.t + rid.vt + PI) % (2.0 * PI) - PI;
        rid.vt += frand(rng, 0.002) - 0.001;

        // apply friction brakes
        if rid.vt.abs() > 0.02 {
            rid.vt *= 0.9;
        }

        // draw
        let px = x + r * rid.t.cos();
        let py = y + r * rid.t.sin();

        if px < 0.0 || px >= self.width as f32 || py < 0.0 || py >= self.height as f32 {
            return rid;
        }

        // max brightness seems to be 0.003845
        let c = self.off_alpha[py as usize * self.width as usize + px as usize] as i32;
        let cv = c as f64 / 255.0;

        // guestimated - 40 is 18% of 255, so scale this to 0.0 to 0.003845
        if cv > 0.0006921 {
            self.draw_glowpoint(px, py);
            rid.mycharge = 0.003845;
        } else {
            rid.mycharge *= 0.98;
            let c = (255.0 * rid.mycharge) as i32;
            self.trans_point(px as i32, py as i32, c, 0.5);
            let color = self.get_pixel(c);
            fill_rect(
                &mut self.buffer,
                self.width,
                self.height,
                px as i32,
                py as i32,
                self.pscale,
                color,
            );
        }
        rid
    }

    fn render_disc(&mut self, rng: &mut impl RngExt, dnum: usize) {
        let (dix, diy, dir_, numr) = {
            let d = &self.discs[dnum];
            (d.x, d.y, d.r, d.numr)
        };

        // Find intersecting points with all ascending discs
        for n in dnum + 1..self.discs.len() {
            let (nx, ny, nr) = {
                let d = &self.discs[n];
                (d.x, d.y, d.r)
            };
            let dx = nx - dix;
            let dy = ny - diy;
            let d2 = dx * dx + dy * dy;
            let sum_r = nr + dir_;

            // intersection test (radii are non-negative, so this is
            // equivalent to `d < sum_r` without paying for a sqrt on
            // every disc pair)
            if d2 < sum_r * sum_r {
                let d = d2.sqrt();
                // complete containment test
                if d > (nr - dir_).abs() {
                    // find solutions
                    let a = (dir_ * dir_ - nr * nr + d * d) / (2.0 * d);
                    let p2x = dix + a * (nx - dix) / d;
                    let p2y = diy + a * (ny - diy) / d;

                    let h = (dir_ * dir_ - a * a).sqrt();

                    let p3ax = p2x + h * (ny - diy) / d;
                    let p3ay = p2y - h * (nx - dix) / d;

                    let p3bx = p2x - h * (ny - diy) / d;
                    let p3by = p2y + h * (nx - dix) / d;

                    // bounds check
                    if p3ax < 0.0
                        || p3ax >= self.width as f32
                        || p3ay < 0.0
                        || p3ay >= self.height as f32
                        || p3bx < 0.0
                        || p3bx >= self.width as f32
                        || p3by < 0.0
                        || p3by >= self.height as f32
                    {
                        continue;
                    }

                    let c = self.trans_point(p3ax as i32, p3ay as i32, 255, 0.75);
                    let color = self.get_pixel(c);
                    fill_rect(
                        &mut self.buffer,
                        self.width,
                        self.height,
                        p3ax as i32,
                        p3ay as i32,
                        self.pscale,
                        color,
                    );

                    let c = self.trans_point(p3bx as i32, p3by as i32, 255, 0.75);
                    let color = self.get_pixel(c);
                    fill_rect(
                        &mut self.buffer,
                        self.width,
                        self.height,
                        p3bx as i32,
                        p3by as i32,
                        self.pscale,
                        color,
                    );
                }
            }
        }

        // Render all the pixel riders
        for m in 0..numr {
            let rid = self.discs[dnum].px_riders[m];
            let rid = self.moverender_rider(rng, rid, dix, diy, dir_);
            self.discs[dnum].px_riders[m] = rid;
        }
    }

    fn init_field(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;

        self.pscale = if config.width > 2560 || config.height > 2560 {
            3
        } else {
            1
        };

        // make_color_ramp from background (black, hsv 0/0/0) to foreground
        // (yellow, hsv 60/1/1), ncolors = 256 + 1, non-closed
        self.ncolors = NCOLORS + 1;
        let (h1, s1, v1) = (0.0f64, 0.0f64, 0.0f64);
        let (h2, s2, v2) = (60.0f64, 1.0f64, 1.0f64);
        let dh = (h2 - h1) / self.ncolors as f64;
        let ds = (s2 - s1) / self.ncolors as f64;
        let dv = (v2 - v1) / self.ncolors as f64;
        self.colors = (0..self.ncolors)
            .map(|i| {
                let (r, g, b) = hsv_to_rgb(
                    (h1 + i as f64 * dh) as i32,
                    s1 + i as f64 * ds,
                    v1 + i as f64 * dv,
                );
                rgb16(r, g, b)
            })
            .collect();

        self.off_alpha = vec![0u8; self.width as usize * self.height as usize];
        self.buffer = vec![0u8; self.width as usize * self.height as usize * 4];

        self.discs.clear();
        for tempx in 0..NUM_DISCS {
            // Arrange in anti-collapsing circle
            let fx = 0.4
                * self.width as f32
                * ((2.0 * std::f32::consts::PI) * tempx as f32 / NUM_DISCS as f32).cos();
            let fy = 0.4
                * self.height as f32
                * ((2.0 * std::f32::consts::PI) * tempx as f32 / NUM_DISCS as f32).sin();
            let x = frand(&mut rng, (self.width / 2) as f32) + fx;
            let y = frand(&mut rng, (self.height / 2) as f32) + fy;
            let r = 5.0 + frand(&mut rng, MAX_RADIUS);
            let bt = if rng.random_range(0..100) < 50 { -1.0f32 } else { 1.0 };

            self.make_disc(&mut rng, x, y, bt * fx / 1000.0, bt * fy / 1000.0, r);
        }
    }

    fn blank_img(&mut self) {
        self.off_alpha.fill(0);
        // background is black
        for px in self.buffer.chunks_exact_mut(4) {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
            px[3] = 255;
        }
    }
}

impl Animation for Intermomentary {
    fn new(config: &AnimConfig) -> Self {
        let mut st = Intermomentary {
            width: config.width,
            height: config.height,
            discs: Vec::new(),
            colors: Vec::new(),
            ncolors: 0,
            pscale: 1,
            off_alpha: Vec::new(),
            buffer: Vec::new(),
            delay_us: DRAW_DELAY,
        };
        st.init_field(config);
        st
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();
        self.blank_img();
        for tempx in 0..self.discs.len() {
            self.move_disc(tempx);
            self.render_disc(&mut rng, tempx);
        }
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        let n = buffer.len().min(self.buffer.len());
        buffer[..n].copy_from_slice(&self.buffer[..n]);
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.init_field(config);
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
