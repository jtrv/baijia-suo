/* Copyright © Chris Le Sueur and Robby Griffin, 2005-2006
 *
 * Permission is hereby granted, free of charge, to any person obtaining
 * a copy of this software and associated documentation files (the
 * "Software"), to deal in the Software without restriction, including
 * without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to
 * permit persons to whom the Software is furnished to do so, subject to
 * the following conditions:
 *
 * The above copyright notice and this permission notice shall be included
 * in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
 * OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
 * IN NO EVENT SHALL THE X CONSORTIUM BE LIABLE FOR ANY CLAIM, DAMAGES OR
 * OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
 * ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
 * OTHER DEALINGS IN THE SOFTWARE.
 *
 * Ultimate thanks go to Massimino Pascal, who created the original
 * xscreensaver hack, and inspired me with it's swirly goodness. This
 * version adds things like variable quality, number of functions and also
 * a groovier colouring mode.
 *
 * This version by Chris Le Sueur <thefishface@gmail.com>, Feb 2005
 * Many improvements by Robby Griffin <rmg@terc.edu>, Mar 2006
 * Multi-coloured mode added by Jack Grahl <j.grahl@ucl.ac.uk>, Jan 2007
 *
 * Rust port of xscreensaver's ifs.c for baijia-suo.
 */

use crate::animation::primitives::{hsv_to_rgb, put_pixel, rgb16, Color};
use crate::animation::{AnimConfig, Animation};
use crate::rng::RngExt;

/* defaults table */
const LENSNUM: usize = 3;
const LENGTH: i32 = 9;
const NCOLOURS: usize = 200;
const DELAY: u64 = 20_000;
const TRANSLATE: bool = true;
const SCALE: bool = true;
const ROTATE: bool = true;
const RECURSE: bool = false;
const MULTI: bool = true;

const BLACK: Color = Color { a: 255, r: 0, g: 0, b: 0 };

fn myrandom(rng: &mut impl RngExt, up: f64) -> f64 {
    rng.random::<f64>() * up
}


/// Port of utils/colors.c make_color_ramp (no allocation).
fn make_color_ramp(
    h1: i32,
    s1: f64,
    v1: f64,
    h2: i32,
    s2: f64,
    v2: f64,
    total: usize,
    closed: bool,
) -> Vec<Color> {
    let ncolors = if closed { total / 2 + 1 } else { total };
    let dh = (h2 - h1) as f64 / ncolors as f64;
    let ds = (s2 - s1) / ncolors as f64;
    let dv = (v2 - v1) / ncolors as f64;
    let mut colors: Vec<Color> = (0..ncolors)
        .map(|i| {
            let (r, g, b) = hsv_to_rgb(
                (h1 as f64 + i as f64 * dh) as i32,
                s1 + i as f64 * ds,
                v1 + i as f64 * dv,
            );
            rgb16(r, g, b)
        })
        .collect();
    if closed {
        for i in ncolors..total {
            colors.push(colors[total - i]);
        }
    }
    colors
}

/// Port of utils/colors.c make_color_path (no allocation).
fn make_color_path(npoints: usize, h: &[i32], s: &[f64], v: &[f64], total: usize) -> Vec<Color> {
    if npoints == 2 {
        return make_color_ramp(h[0], s[0], v[0], h[1], s[1], v[1], total, true);
    }

    // Distance between H values in the shortest direction around the circle
    let mut dh_short = [0.0f64; 5];
    for i in 0..npoints {
        let j = (i + 1) % npoints;
        let mut d = ((h[i] - h[j]) as f64 / 360.0).abs();
        if d > 0.5 {
            d = 0.5 - (d - 0.5);
        }
        dh_short[i] = d;
    }

    // lengths of edges in unit HSV space (DH[i]*DH[j] is faithful to the C)
    let mut edge = [0.0f64; 5];
    let mut circum = 0.0;
    for i in 0..npoints {
        let j = (i + 1) % npoints;
        edge[i] = (dh_short[i] * dh_short[j]
            + (s[j] - s[i]) * (s[j] - s[i])
            + (v[j] - v[i]) * (v[j] - v[i]))
            .sqrt();
        circum += edge[i];
    }
    if circum < 0.0001 {
        // unreachable with make_smooth_colormap's 0.2 spacing constraint
        let (r, g, b) = hsv_to_rgb(h[0], s[0], v[0]);
        return vec![rgb16(r, g, b); total];
    }

    // space the colors evenly along the circumference
    let mut ncolors = [0usize; 5];
    let mut dh = [0.0f64; 5];
    let mut ds = [0.0f64; 5];
    let mut dv = [0.0f64; 5];
    for i in 0..npoints {
        ncolors[i] = (total as f64 * (edge[i] / circum)) as usize;
        let j = (i + 1) % npoints;
        if ncolors[i] > 0 {
            dh[i] = 360.0 * (dh_short[i] / ncolors[i] as f64);
            ds[i] = (s[j] - s[i]) / ncolors[i] as f64;
            dv[i] = (v[j] - v[i]) / ncolors[i] as f64;
        }
    }

    let mut colors = Vec::with_capacity(total);
    for i in 0..npoints {
        let distance = h[(i + 1) % npoints] - h[i];
        let mut direction = if distance >= 0 { -1.0 } else { 1.0 };
        if (-180..=180).contains(&distance) {
            direction = -direction;
        }
        for j in 0..ncolors[i] {
            let mut hh = h[i] as f64 + j as f64 * dh[i] * direction;
            if hh < 0.0 {
                hh += 360.0;
            }
            let (r, g, b) = hsv_to_rgb(hh as i32, s[i] + j as f64 * ds[i], v[i] + j as f64 * dv[i]);
            colors.push(rgb16(r, g, b));
        }
    }

    // Floating-point round-off can make us decide to use fewer colors;
    // pad things out by duplicating the last color.
    if colors.is_empty() {
        let (r, g, b) = hsv_to_rgb(h[0], s[0], v[0]);
        colors.push(rgb16(r, g, b));
    }
    if let Some(&fill) = colors.last() {
        while colors.len() < total {
            colors.push(fill);
        }
    }
    colors
}

/// Port of utils/colors.c make_smooth_colormap (no allocation).
fn make_smooth_colormap(rng: &mut impl RngExt, ncolors: usize) -> Vec<Color> {
    let npoints = match rng.random_range(0..20) {
        0..=5 => 2,   // 30% of the time
        6..=15 => 3,  // 50% of the time
        16..=18 => 4, // 15% of the time
        _ => 5,       //  5% of the time
    };

    let mut h = [0i32; 5];
    let mut s = [0.0f64; 5];
    let mut v = [0.0f64; 5];
    // Note: the C accumulates total_s/total_v across full repicks; kept as-is.
    let mut total_s = 0.0;
    let mut total_v = 0.0;
    'repick_all: loop {
        for i in 0..npoints {
            loop {
                h[i] = rng.random_range(0..360);
                s[i] = rng.random::<f64>();
                v[i] = rng.random::<f64>() * 0.8 + 0.2;

                // Make sure that no two adjacent colors are *too* close together.
                if i > 0 {
                    let j = if i + 1 == npoints { 0 } else { i - 1 };
                    let hi = h[i] as f64 / 360.0;
                    let hj = h[j] as f64 / 360.0;
                    let mut dh = (hj - hi).abs();
                    if dh > 0.5 {
                        dh = 0.5 - (dh - 0.5);
                    }
                    let distance =
                        (dh * dh + (s[j] - s[i]).powi(2) + (v[j] - v[i]).powi(2)).sqrt();
                    if distance < 0.2 {
                        continue;
                    }
                }
                break;
            }
            total_s += s[i];
            total_v += v[i];
        }

        // If the average saturation or intensity are too low, repick the colors,
        // so that we don't end up with a black-and-white or too-dark map.
        if total_s / (npoints as f64) < 0.2 {
            continue 'repick_all;
        }
        if total_v / (npoints as f64) < 0.3 {
            continue 'repick_all;
        }
        break;
    }

    make_color_path(npoints, &h, &s, &v, ncolors)
}

struct Lens {
    r: f32,
    s: f32,
    tx: f32,
    ty: f32, // Rotation, Scale, Translation X & Y
    ro: f32,
    rt: f32,
    rc: f32, // Old Rotation, Rotation Target, Rotation Counter
    so: f32,
    st: f32,
    sc: f32, // Old Scale, Scale Target, Scale Counter
    txa: f32,
    tya: f32, // Translation change

    ua: i32,
    ub: i32,
    utx: i32, // Precomputed combined r,s,t values
    uc: i32,
    ud: i32,
    uty: i32,
}

pub struct Ifs {
    width: i32,
    widthb: i32,
    height: i32,
    width8: i32,
    height8: i32,
    board: Vec<u32>,
    xmin: i32,
    xmax: i32,
    ymin: i32,
    ymax: i32,
    x: i32,
    y: i32,
    pscale: i32,

    colours: Vec<Color>,
    ncolours: usize,
    ccolour: usize,

    lensnum: usize,
    lenses: Vec<Lens>,
    length: i32,
    recurse: bool,
    multi: bool,
    translate: bool,
    scale: bool,
    rotate: bool,

    buffer: Vec<u8>,
    delay_us: u64,
}

/* fixed-point matrix step: extra factor of 2^10 in the u* values and 2^8 in
 * the coordinates.  Wrapping matches C int overflow in practice; out-of-range
 * results are discarded by sp()'s bounds check. */
fn step_x(l: &Lens, x: i32, y: i32) -> i32 {
    l.ua.wrapping_mul(x)
        .wrapping_add(l.ub.wrapping_mul(y))
        .wrapping_add(l.utx)
        >> 10
}

fn step_y(l: &Lens, x: i32, y: i32) -> i32 {
    l.uc.wrapping_mul(x)
        .wrapping_add(l.ud.wrapping_mul(y))
        .wrapping_add(l.uty)
        >> 10
}

impl Ifs {
    fn getdot(&self, x: i32, y: i32) -> bool {
        self.board[(y * self.widthb + (x >> 5)) as usize] & (1 << (x & 31)) != 0
    }

    fn setdot(&mut self, x: i32, y: i32) {
        self.board[(y * self.widthb + (x >> 5)) as usize] |= 1 << (x & 31);
    }

    /// Set a point to be drawn, if it hasn't been already.
    /// Expects coordinates in 256ths of a pixel.
    fn sp(&mut self, x: i32, y: i32, colour: Color) {
        if x < 0 || x >= self.width8 || y < 0 || y >= self.height8 {
            return;
        }

        let x = x >> 8;
        let y = y >> 8;

        if self.getdot(x, y) {
            return;
        }
        self.setdot(x, y);

        if x < self.xmin {
            self.xmin = x;
        }
        if x > self.xmax {
            self.xmax = x;
        }
        if y < self.ymin {
            self.ymin = y;
        }
        if y > self.ymax {
            self.ymax = y;
        }

        // The C queues points and draws them in batches; we draw immediately.
        self.fill_rect(x, y, self.pscale, self.pscale, colour);
    }

    fn fill_rect(&mut self, x: i32, y: i32, rw: i32, rh: i32, colour: Color) {
        let (w, h) = (self.width as u32, self.height as u32);
        for yy in y..y + rh {
            for xx in x..x + rw {
                put_pixel(&mut self.buffer, w, h, xx, yy, colour);
            }
        }
    }

    fn create_lens(&self, nr: f32, ns: f32, nx: f32, ny: f32) -> Lens {
        let mut l = Lens {
            r: 0.0,
            s: 0.5,
            tx: nx,
            ty: ny,
            ro: 0.0,
            rt: 0.0,
            rc: 0.0,
            so: 0.0,
            st: 0.0,
            sc: 0.0,
            txa: 0.0,
            tya: 0.0,
            ua: 0,
            ub: 0,
            utx: 0,
            uc: 0,
            ud: 0,
            uty: 0,
        };
        if self.rotate {
            l.r = nr;
            l.ro = nr;
            l.rt = nr;
            l.rc = 1.0;
        }
        if self.scale {
            l.s = ns;
            l.so = ns;
            l.st = ns;
            l.sc = 1.0;
        }
        lensmatrix(self.width, self.height, &mut l);
        l
    }

    fn mutate(&mut self, i: usize, rng: &mut impl RngExt) {
        let (width, height) = (self.width, self.height);
        let l = &mut self.lenses[i];
        if self.rotate {
            if l.rc >= 1.0 {
                l.rc = 0.0;
                l.ro = l.rt;
                l.rt = (myrandom(rng, 4.0) - 2.0) as f32;
            }
            let factor =
                (((-std::f64::consts::PI / 2.0) + std::f64::consts::PI * l.rc as f64).sin() + 1.0)
                    / 2.0;
            l.r = l.ro + (l.rt - l.ro) * factor as f32;
            l.rc += 0.01;
        }
        if self.scale {
            if l.sc >= 1.0 {
                // Reset counter, obtain new target value
                l.sc = 0.0;
                l.so = l.st;
                l.st = (myrandom(rng, 2.0) - 1.0) as f32;
            }
            // Take average of old target and new target, using factor to
            // weight. It's computed sinusoidally, resulting in smooth,
            // rhythmic transitions.
            let factor =
                (((-std::f64::consts::PI / 2.0) + std::f64::consts::PI * l.sc as f64).sin() + 1.0)
                    / 2.0;
            l.s = l.so + (l.st - l.so) * factor as f32;
            l.sc += 0.01;
        }
        if self.translate {
            l.txa += (myrandom(rng, 0.004) - 0.002) as f32;
            l.tya += (myrandom(rng, 0.004) - 0.002) as f32;
            l.tx += l.txa;
            l.ty += l.tya;
            if l.tx > 6.0 {
                l.txa -= 0.004;
            }
            if l.ty > 6.0 {
                l.tya -= 0.004;
            }
            if l.tx < -6.0 {
                l.txa += 0.004;
            }
            if l.ty < -6.0 {
                l.tya += 0.004;
            }
            if l.txa > 0.05 || l.txa < -0.05 {
                l.txa /= 1.7;
            }
            if l.tya > 0.05 || l.tya < -0.05 {
                l.tya /= 1.7;
            }
        }
        if self.rotate || self.scale || self.translate {
            lensmatrix(width, height, l);
        }
    }

    /// Calls itself <lensnum> times - with results from each lens/function.
    /// After <length> calls to itself, it stops iterating and draws a point.
    fn recurse(&mut self, x: i32, y: i32, length: i32, p: usize, colour: Color) {
        if length == 0 {
            if p == 0 {
                self.sp(x, y, colour);
            } else {
                let l = &self.lenses[p];
                let (nx, ny) = (step_x(l, x, y), step_y(l, x, y));
                self.sp(nx, ny, colour);
            }
        } else {
            for i in 0..self.lensnum {
                let l = &self.lenses[i];
                let (nx, ny) = (step_x(l, x, y), step_y(l, x, y));
                self.recurse(nx, ny, length - 1, p, colour);
            }
        }
    }

    /// Performs <count> random lens transformations, drawing a point at each
    /// iteration after the first 10.
    fn iterate(&mut self, count: i32, p: usize, colour: Color, rng: &mut impl RngExt) {
        let mut x = self.x;
        let mut y = self.y;

        let mut i = 0;
        while i < 10 {
            let l = &self.lenses[rng.random_range(0..self.lensnum)];
            let tx = step_x(l, x, y);
            y = step_y(l, x, y);
            x = tx;
            i += 1;
        }

        while i < count {
            let l = &self.lenses[rng.random_range(0..self.lensnum)];
            let tx = step_x(l, x, y);
            y = step_y(l, x, y);
            x = tx;
            if p == 0 {
                self.sp(x, y, colour);
            } else {
                let l = &self.lenses[p];
                let (nx, ny) = (step_x(l, x, y), step_y(l, x, y));
                self.sp(nx, ny, colour);
            }
            i += 1;
        }

        self.x = x;
        self.y = y;
    }

    /// C's ifs_reshape: recompute size-dependent state and clear the backbuffer.
    fn reshape(&mut self, w: u32, h: u32) {
        self.width = w as i32;
        self.widthb = (w as i32 + 31) >> 5;
        self.height = h as i32;
        self.width8 = (w as i32) << 8;
        self.height8 = (h as i32) << 8;

        self.xmin = self.width + 1;
        self.xmax = -1;
        self.ymax = -1;
        self.ymin = self.height + 1;

        self.buffer = vec![0u8; w as usize * h as usize * 4];
        for px in self.buffer.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }

        self.board = vec![0u32; (self.widthb * self.height) as usize];
    }
}

/// Precompute integer values for matrix multiplication and vector
/// addition. The matrix multiplication will go like this (see iterate()):
///   |x2|     |ua ub|   |x|     |utx|
///   |  |  =  |     | * | |  +  |   |
///   |y2|     |uc ud|   |y|     |uty|
///
/// There is an extra factor of 2^10 in these values, and an extra factor of
/// 2^8 in the coordinates, in order to implement fixed-point arithmetic.
fn lensmatrix(width: i32, height: i32, l: &mut Lens) {
    l.ua = (1024.0 * l.s as f64 * (l.r as f64).cos()) as i32;
    l.ub = (-1024.0 * l.s as f64 * (l.r as f64).sin()) as i32;
    l.uc = -l.ub;
    l.ud = l.ua;
    l.utx = (131072.0
        * width as f64
        * (l.s as f64 * ((l.r as f64).sin() - (l.r as f64).cos()) + l.tx as f64 / 16.0 + 1.0))
        as i32;
    l.uty = (-131072.0
        * height as f64
        * (l.s as f64 * ((l.r as f64).sin() + (l.r as f64).cos()) + l.ty as f64 / 16.0 - 1.0))
        as i32;
}

impl Animation for Ifs {
    fn new(config: &AnimConfig) -> Self {
        let mut rng = crate::rng::rng();

        let lensnum = LENSNUM;
        let ncolours = NCOLOURS.max(lensnum).max(1);

        let mut st = Ifs {
            width: 0,
            widthb: 0,
            height: 0,
            width8: 0,
            height8: 0,
            board: Vec::new(),
            xmin: 0,
            xmax: 0,
            ymin: 0,
            ymax: 0,
            x: 0,
            y: 0,
            pscale: if config.width > 2560 || config.height > 2560 {
                3
            } else {
                1
            },
            colours: make_smooth_colormap(&mut rng, ncolours),
            ncolours,
            ccolour: 0,
            lensnum,
            lenses: Vec::new(),
            length: LENGTH,
            recurse: RECURSE,
            multi: MULTI,
            translate: TRANSLATE,
            scale: SCALE,
            rotate: ROTATE,
            buffer: Vec::new(),
            delay_us: DELAY,
        };
        st.reshape(config.width, config.height);

        st.lenses = (0..lensnum)
            .map(|_| {
                st.create_lens(
                    (myrandom(&mut rng, 1.0) - 0.5) as f32,
                    myrandom(&mut rng, 1.0) as f32,
                    (myrandom(&mut rng, 4.0) - 2.0) as f32,
                    (myrandom(&mut rng, 4.0) + 2.0) as f32,
                )
            })
            .collect();

        st
    }

    fn tick(&mut self) {
        let mut rng = crate::rng::rng();

        // erase whatever was drawn in the previous frame
        if self.xmin <= self.xmax && self.ymin <= self.ymax {
            let (xmin, ymin) = (self.xmin, self.ymin);
            let (rw, rh) = (
                self.xmax - self.xmin + self.pscale,
                self.ymax - self.ymin + self.pscale,
            );
            self.fill_rect(xmin, ymin, rw, rh, BLACK);
            self.xmin = self.width + 1;
            self.xmax = -1;
            self.ymax = -1;
            self.ymin = self.height + 1;
        }

        self.ccolour = (self.ccolour + 1) % self.ncolours;

        // calculate and draw points for this frame
        let x = self.width << 7;
        let y = self.height << 7;

        if self.multi {
            for i in 0..self.lensnum {
                let partcolor = (self.ccolour * (i + 1)) % self.ncolours;
                let colour = self.colours[partcolor];
                self.board.fill(0);
                if self.recurse {
                    self.recurse(x, y, self.length - 1, i, colour);
                } else {
                    let count = (self.lensnum as f64).powi(self.length - 1) as i32;
                    self.x = x;
                    self.y = y;
                    self.iterate(count, i, colour, &mut rng);
                }
            }
        } else {
            let colour = self.colours[self.ccolour];
            self.board.fill(0);
            if self.recurse {
                self.recurse(x, y, self.length, 0, colour);
            } else {
                let count = (self.lensnum as f64).powi(self.length) as i32;
                self.x = x;
                self.y = y;
                self.iterate(count, 0, colour, &mut rng);
            }
        }

        for i in 0..self.lensnum {
            self.mutate(i, &mut rng);
        }
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        let n = buffer.len().min(self.buffer.len());
        buffer[..n].copy_from_slice(&self.buffer[..n]);
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.pscale = if config.width > 2560 || config.height > 2560 {
            3
        } else {
            1
        };
        self.reshape(config.width, config.height);
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
