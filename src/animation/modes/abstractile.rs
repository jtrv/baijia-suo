/*
 * Copyright (c) 2004-2009 Steve Sundstrom
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Rust port of xscreensaver's abstractile.c for baijia-suo.
 */

use crate::animation::primitives::{clear_buffer, hsv_to_rgb, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};
use rand::Rng;
use std::f64::consts::PI;

const DIR_NONE: i32 = 0;
const DIR_UP: i32 = 1;
const DIR_DOWN: i32 = 2;
const DIR_LEFT: i32 = 3;
const DIR_RIGHT: i32 = 4;

const LINE_FORCE: i32 = 1;
const LINE_NEW: i32 = 2;
const LINE_BRIN: i32 = 3;
const LINE_BROUT: i32 = 4;

const D3D_NONE: i32 = 0;
const D3D_BLOCK: i32 = 1;
const D3D_NEON: i32 = 2;
const D3D_TILED: i32 = 3;

const TILE_RANDOM: i32 = 0;
const TILE_FLAT: i32 = 1;
const TILE_THIN: i32 = 2;
const TILE_OUTLINE: i32 = 3;
const TILE_BLOCK: i32 = 4;
const TILE_NEON: i32 = 5;
const TILE_TILED: i32 = 6;

const BASECOLORS: usize = 30;
const MAXCOLORS: usize = 40;
const LAYERS: usize = 4;
const PATTERNS: i32 = 40;
const SHAPES: i32 = 18;
const DRAWORDERS: i32 = 40;
const COLORMAPS: i32 = 20;
const WAVES: i32 = 6;
const STRETCHES: i32 = 8;

// defaults from abstractile.c: *sleep: 3, *speed: 3, *tile: random,
// .background: black, .foreground: white
const SLEEP: i32 = 3;
const SPEED: i32 = 3;
const TILE: i32 = TILE_RANDOM;
const NEWCOLS: bool = false;

const BG: Color = Color { a: 255, r: 0, g: 0, b: 0 };

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Create,
    Erase,
    Draw,
}

#[derive(Clone, Copy, Default)]
struct Line {
    x: i32,
    y: i32,
    len: i32,
    obj: usize,
    color: i32,
    ndol: usize,
    deo: i32,
    hv: bool,
}

#[derive(Clone, Copy, Default)]
struct GridCell {
    line: usize,
    hl: usize,
    hr: usize,
    vu: usize,
    vd: usize,
    dhl: usize,
    dhr: usize,
    dvu: usize,
    dvd: usize,
}

/* ---- ports of utils/hsv.c and the needed parts of utils/colors.c ---- */

fn rgb_to_hsv16(r: i32, g: i32, b: i32) -> (i32, f64, f64) {
    let rr = r as f64 / 65535.0;
    let gg = g as f64 / 65535.0;
    let bb = b as f64 / 65535.0;
    let (mut cmax, mut cmin, mut imax) = (rr, gg, 1);
    if cmax < gg {
        cmax = gg;
        cmin = rr;
        imax = 2;
    }
    if cmax < bb {
        cmax = bb;
        imax = 3;
    }
    if cmin > bb {
        cmin = bb;
    }
    let cmm = cmax - cmin;
    let v = cmax;
    let (h, s) = if cmm == 0.0 {
        (0.0, 0.0)
    } else {
        let s = cmm / cmax;
        let mut h = match imax {
            1 => (gg - bb) / cmm,
            2 => 2.0 + (bb - rr) / cmm,
            _ => 4.0 + (rr - gg) / cmm,
        };
        if h < 0.0 {
            h += 6.0;
        }
        (h, s)
    };
    ((h * 60.0) as i32, s, v)
}

fn make_color_ramp16(
    h1: i32,
    s1: f64,
    v1: f64,
    h2: i32,
    s2: f64,
    v2: f64,
    total: usize,
    closed: bool,
) -> Vec<(u16, u16, u16)> {
    let n = if closed { total / 2 + 1 } else { total };
    let dh = (h2 - h1) as f64 / n as f64;
    let ds = (s2 - s1) / n as f64;
    let dv = (v2 - v1) / n as f64;
    let mut out = vec![(0u16, 0u16, 0u16); total];
    for i in 0..n.min(total) {
        out[i] = hsv_to_rgb(
            (h1 as f64 + i as f64 * dh) as i32,
            s1 + i as f64 * ds,
            v1 + i as f64 * dv,
        );
    }
    if closed {
        for i in n..total {
            out[i] = out[total - i];
        }
    }
    out
}

/* abstractile.c's make_color_ramp_rgb ignores its closed_p argument and
always builds an open ramp */
fn ramp_rgb16(
    r1: i32, g1: i32, b1: i32,
    r2: i32, g2: i32, b2: i32,
    total: usize,
) -> Vec<(u16, u16, u16)> {
    let (h1, s1, v1) = rgb_to_hsv16(r1, g1, b1);
    let (h2, s2, v2) = rgb_to_hsv16(r2, g2, b2);
    make_color_ramp16(h1, s1, v1, h2, s2, v2, total, false)
}

/* port of utils/colors.c make_color_path (color computation only) */
fn make_color_path16(h: &[i32], s: &[f64], v: &[f64], total: usize) -> Vec<(u16, u16, u16)> {
    let npoints = h.len();
    if npoints == 0 || total == 0 {
        return Vec::new();
    }
    if npoints == 2 {
        return make_color_ramp16(h[0], s[0], v[0], h[1], s[1], v[1], total, true);
    }

    let mut dhd = vec![0.0f64; npoints]; // DH: hue distance the short way round
    for i in 0..npoints {
        let j = (i + 1) % npoints;
        let mut d = ((h[i] - h[j]) as f64 / 360.0).abs();
        if d > 0.5 {
            d = 0.5 - (d - 0.5);
        }
        dhd[i] = d;
    }
    let mut edge = vec![0.0f64; npoints];
    let mut circum = 0.0;
    for i in 0..npoints {
        let j = (i + 1) % npoints;
        edge[i] = (dhd[i] * dhd[j] + (s[j] - s[i]).powi(2) + (v[j] - v[i]).powi(2)).sqrt();
        circum += edge[i];
    }
    if circum < 0.0001 {
        return Vec::new();
    }
    let mut npix = vec![0usize; npoints];
    for i in 0..npoints {
        npix[i] = (total as f64 * (edge[i] / circum)) as usize;
    }
    let mut dh = vec![0.0f64; npoints];
    let mut ds = vec![0.0f64; npoints];
    let mut dv = vec![0.0f64; npoints];
    for i in 0..npoints {
        let j = (i + 1) % npoints;
        if npix[i] > 0 {
            dh[i] = 360.0 * (dhd[i] / npix[i] as f64);
            ds[i] = (s[j] - s[i]) / npix[i] as f64;
            dv[i] = (v[j] - v[i]) / npix[i] as f64;
        }
    }

    let mut out = vec![(0u16, 0u16, 0u16); total];
    let mut k = 0;
    for i in 0..npoints {
        let distance = h[(i + 1) % npoints] - h[i];
        let mut direction = if distance >= 0 { -1.0 } else { 1.0 };
        if (-180..=180).contains(&distance) {
            direction = -direction;
        }
        for j in 0..npix[i] {
            if k >= total {
                break;
            }
            let mut hh = h[i] as f64 + j as f64 * dh[i] * direction;
            if hh < 0.0 {
                hh += 360.0;
            }
            out[k] = hsv_to_rgb(hh as i32, s[i] + j as f64 * ds[i], v[i] + j as f64 * dv[i]);
            k += 1;
        }
    }
    /* floating-point round-off: pad by duplicating the last color */
    if k == 0 {
        return Vec::new();
    }
    for i in k..total {
        out[i] = out[i - 1];
    }
    out
}

fn to_color(c: (u16, u16, u16)) -> Color {
    Color::new(255, (c.0 >> 8) as u8, (c.1 >> 8) as u8, (c.2 >> 8) as u8)
}

/* ---- drawing helpers (XFillRectangle / XFillPolygon equivalents) ---- */

fn fill_rect(buf: &mut [u8], bw: u32, bh: u32, x: i32, y: i32, w: i32, h: i32, c: Color) {
    if w <= 0 || h <= 0 {
        return;
    }
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(bw as i32);
    let y1 = (y + h).min(bh as i32);
    for yy in y0..y1 {
        for xx in x0..x1 {
            put_pixel(buf, bw, bh, xx, yy, c);
        }
    }
}

fn fill_poly(buf: &mut [u8], bw: u32, bh: u32, verts: &[(i32, i32)], c: Color) {
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
    let min_y = min_y.max(0);
    let max_y = max_y.min(bh as i32 - 1);
    for sy in min_y..=max_y {
        let mut xs: Vec<i32> = Vec::new();
        for i in 0..n {
            let (x0, y0) = verts[i];
            let (x1, y1) = verts[(i + 1) % n];
            if (y0 <= sy && sy < y1) || (y1 <= sy && sy < y0) {
                xs.push(x0 + (x1 - x0) * (sy - y0) / (y1 - y0));
            }
        }
        xs.sort_unstable();
        let mut i = 0;
        while i + 1 < xs.len() {
            for x in xs[i].max(0)..=xs[i + 1].min(bw as i32 - 1) {
                put_pixel(buf, bw, bh, x, sy, c);
            }
            i += 2;
        }
    }
}

pub struct Abstractile {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    colors: Vec<Color>,

    dline: Vec<Line>,
    eline: Vec<Line>,
    grid: Vec<GridCell>,
    zlist: Vec<usize>,
    fdol: Vec<usize>,

    ii: u32,
    fi: usize,
    bi: usize,
    li: usize,
    eli: usize,
    oi: usize,
    zi: usize,

    gridx: i32,
    gridy: i32,
    gridn: usize,
    lwid: i32,
    narray: usize,
    max_wxh: u32,
    elwid: i32,
    elpu: usize,
    egridx: i32,
    egridy: i32,

    bnratio: i32,
    maxlen: i32,
    forcemax: i32,
    olen: i32,
    bln: i32,

    ncolors: i32,
    shades: i32,
    rco: [i32; MAXCOLORS],
    cmap: i32,
    layers: i32,

    dmap: i32,
    emap: i32,
    dvar: i32,
    evar: i32,
    ddir: i32,
    edir: i32,
    lpu: usize,
    d3d: i32,
    round: i32,
    outline: i32,

    pattern: [i32; LAYERS],
    shape: [i32; LAYERS],
    mix: [i32; LAYERS],
    csw: [i32; LAYERS],
    wsx: [i32; LAYERS],
    wsy: [i32; LAYERS],
    sec: [i32; LAYERS],
    cs1: [i32; LAYERS],
    cs2: [i32; LAYERS],
    cs3: [i32; LAYERS],
    cs4: [i32; LAYERS],
    wave: [i32; LAYERS],
    waveh: [i32; LAYERS],
    wavel: [i32; LAYERS],
    rx1: [i32; LAYERS],
    rx2: [i32; LAYERS],
    rx3: [i32; LAYERS],
    ry1: [i32; LAYERS],
    ry2: [i32; LAYERS],
    ry3: [i32; LAYERS],

    mode: Mode,
    dialog: i32,
    grid_full: bool,
    resized: bool,
    /* progress through the sliced Mode::Create work (see create_step) */
    cphase: u32,
    cprog: usize,
    next_delay_us: u64,
}

impl Abstractile {
    fn dist(&self, x1: i32, x2: i32, y1: i32, y2: i32, s: i32) -> i32 {
        let xd = (x1 - x2) as f64;
        let yd = (y1 - y2) as f64;
        match s {
            0 => (xd * xd + yd * yd).sqrt() as i32,
            1 => (xd * xd * (self.cs1[0] * 2) as f64 + yd * yd).sqrt() as i32,
            2 => (xd * xd + yd * yd * (self.cs2[0] * 2) as f64).sqrt() as i32,
            _ => (xd * xd * self.cs1[0] as f64 / self.cs2[0] as f64
                + yd * yd * self.cs3[0] as f64 / self.cs4[0] as f64)
                .sqrt() as i32,
        }
    }

    fn wave_fn(&self, x: i32, h: i32, l: i32, wave: i32) -> i32 {
        let l = l + 1;
        match wave {
            /* cos wave */
            0 => ((x as f64 * PI / l as f64).cos() * h as f64) as i32,
            /* double wave */
            1 | 2 => {
                ((x as f64 * PI / l as f64).cos() * h as f64) as i32
                    + ((x as f64 * PI / l as f64 / self.cs1[1].max(1) as f64).sin() * h as f64)
                        as i32
            }
            /* zig zag */
            3 => (x % (l * 2) - l).abs() * h / l,
            /* giant zig zag */
            4 => (x % (l * 4) - l * 2).abs() * h * 3 / l,
            /* sawtooth */
            5 => (x % l) * h / l,
            /* no wave */
            _ => 0,
        }
    }

    fn triangle_fn(&self, x: i32, y: i32, rx: i32, ry: i32, t: i32) -> i32 {
        let gx = self.gridx;
        let gy = self.gridy;
        match t {
            1 => (x + y + rx - (gx / 2)).min(gx - x + y).min((gy - y + (ry / 2)) * 3 / 2),
            2 => (x - rx).min(y - ry).min((rx + ry - x - y) * 2 / 3),
            3 => (gx - x - rx).min(y - ry).min((rx + ry - gx + x - y) * 2 / 3),
            4 => (x - rx).min(gy - y - ry).min((rx + ry - x - gy + y) * 2 / 3),
            _ => (gx - x - rx)
                .min(gy - y - ry)
                .min((rx + ry - gx + x - gy + y) * 2 / 3),
        }
    }

    fn shape_fn(&self, x: i32, y: i32, rx: i32, ry: i32, n: usize) -> i32 {
        let gx = self.gridx;
        let gy = self.gridy;
        match self.shape[n] {
            /* square/rectangle */
            0..=2 => {
                1 + ((x - rx).abs() * self.cs1[n] / self.cs2[n])
                    .max((y - ry).abs() * self.cs3[n] / self.cs4[n])
            }
            /* diamond */
            3 | 4 => {
                1 + ((x - rx).abs() * self.cs1[n] / self.cs2[n]
                    + (y - ry).abs() * self.cs3[n] / self.cs4[n])
            }
            /* 8 point star */
            5 => {
                1 + ((x - rx).abs().max((y - ry).abs()) * 3 / 2)
                    .min((x - rx).abs() + (y - ry).abs())
            }
            /* circle/oval */
            6..=8 => 1 + self.dist(x, rx, y, ry, self.cs1[n]),
            /* black hole circle */
            9 => 1 + (gx * gy / (1 + self.dist(x, rx, y, ry, self.cs2[n]))),
            /* sun */
            10 => {
                1 + ((x - rx).abs() * gx / ((y - ry).abs() + 1))
                    .min((y - ry).abs() * gx / ((x - rx).abs() + 1))
            }
            /* 2 circles+inverted circle */
            11 => {
                1 + (self.dist(x, rx, y, ry, self.cs1[n])
                    * self.dist(x, (rx * 3) % gx, y, (ry * 5) % gy, self.cs1[n])
                    / (1 + self.dist(x, (rx * 4) % gx, y, (ry * 7) % gy, self.cs1[n])))
            }
            /* star */
            12 => 1 + ((((x - rx) * (y - ry)).abs() as f64).sqrt() as i32),
            /* centered ellipse */
            13 => 1 + self.dist(x, rx, y, ry, 0) + self.dist(x, gx - rx, y, gy - ry, 0),
            /* triangle */
            _ => 1 + self.triangle_fn(x, y, rx, ry, self.cs4[n]),
        }
    }

    fn pattern_fn(&self, x: i32, y: i32, n: usize) -> i32 {
        let gx = self.gridx;
        let gy = self.gridy;
        let mut x = x;
        let mut y = y;
        let ox = x;
        match self.wsx[n] {
            /* slants */
            0 => x += y / (1 + self.cs4[n]),
            1 => x += (gy - y) / (1 + self.cs4[n]),
            /* curves */
            2 => x += self.wave_fn(y, gx / (1 + self.cs1[n]), gy, 0),
            3 => x += self.wave_fn(gy - y, gy / (1 + self.cs1[n]), gy, 0),
            /* U curves */
            4 => x += self.wave_fn(y, self.cs1[n] * self.csw[n] / 2, (gy as f64 * 2.0 / PI) as i32, 0),
            5 => x -= self.wave_fn(y, self.cs1[n] * self.csw[n] / 2, (gy as f64 * 2.0 / PI) as i32, 0),
            _ => {}
        }
        match self.wsy[0] {
            /* slants */
            0 => y += ox / (1 + self.cs1[n]),
            1 => y += (gx - ox) / (1 + self.cs1[n]),
            /* curves */
            2 => y += self.wave_fn(ox, gx / (1 + self.cs1[n]), gx, 0),
            3 => y += self.wave_fn(gx - ox, gx / (1 + self.cs1[n]), gx, 0),
            /* U curves */
            4 => y += self.wave_fn(ox, self.cs1[n] * self.csw[n] / 2, (gy as f64 * 2.0 / PI) as i32, 0),
            5 => y -= self.wave_fn(ox, self.cs1[n] * self.csw[n] / 2, (gy as f64 * 2.0 / PI) as i32, 0),
            _ => {}
        }
        let csw = self.csw[n];
        let mut v = match self.pattern[n] {
            /* horizontal stripes */
            0 => y,
            /* vertical stripes */
            1 => x,
            /* diagonal stripes */
            2 => x + (y * self.cs1[n] / self.cs2[n]),
            /* reverse diagonal stripes */
            3 => x - (y * self.cs1[n] / self.cs2[n]),
            /* checkerboard */
            4 => (y / csw * 3 + x / csw) * csw,
            /* diagonal checkerboard */
            5 => ((x + y) / 2 / csw + (x + gy - y) / 2 / csw * 3) * csw,
            /* + cross */
            6 => gx + ((x - self.rx3[n]).abs().min((y - self.ry3[n]).abs()) * 2),
            /* double + cross */
            7 => {
                ((x - self.rx2[n]).abs().min((y - self.ry2[n]).abs()))
                    .min((x - self.rx1[n]).abs().min((y - self.ry1[n]).abs()))
                    * 2
            }
            /* X cross */
            8 => {
                gx + (((x - self.rx3[n]).abs() * self.cs1[n] / self.cs2[n]
                    + (y - self.ry2[n]).abs() * self.cs3[n] / self.cs4[n])
                    .min(
                        (x - self.rx3[n]).abs() * self.cs1[n] / self.cs2[n]
                            - (y - self.ry3[n]).abs() * self.cs3[n] / self.cs4[n],
                    )
                    * 2)
            }
            /* double X cross */
            9 => {
                (((x - self.rx2[n]).abs() + (y - self.ry2[n]).abs())
                    .min((x - self.rx2[n]).abs() - (y - self.ry2[n]).abs()))
                .min(
                    ((x - self.rx1[n]).abs() + (y - self.ry1[n]).abs())
                        .min((x - self.rx1[n]).abs() - (y - self.ry1[n]).abs()),
                ) * 2
            }
            /* horizontal stripes/waves */
            10 => gy + (y + self.wave_fn(x, self.waveh[n], self.wavel[n], self.wave[n])),
            /* vertical stripes/waves */
            11 => gx + (x + self.wave_fn(y, self.waveh[n], self.wavel[n], self.wave[n])),
            /* diagonal stripes/waves */
            12 => {
                gx + (x + (y * self.cs1[n] / self.cs2[n])
                    + self.wave_fn(x, self.waveh[n], self.wavel[n], self.wave[n]))
            }
            13 => {
                gx + (x - (y * self.cs1[n] / self.cs2[n])
                    + self.wave_fn(y, self.waveh[n], self.wavel[n], self.wave[n]))
            }
            /* horizontal spikey waves */
            14 => {
                y + (csw * self.cs4[n] / self.cs3[n])
                    + self.wave_fn(
                        x + ((y / self.cs3[n]) * self.edir),
                        csw / 2 * self.cs1[n] / self.cs2[n],
                        csw / 2 * self.cs2[n] / self.cs1[n],
                        self.wave[n],
                    )
            }
            /* vertical spikey waves */
            15 => {
                x + (csw * self.cs1[n] / self.cs2[n])
                    + self.wave_fn(
                        y + ((x / self.cs3[n]) * self.edir),
                        csw / 2 * self.cs1[n] / self.cs2[n],
                        csw / 2 * self.cs3[n] / self.cs4[n],
                        self.wave[n],
                    )
            }
            /* big slanted hwaves */
            16 => {
                gy - y - (x * self.cs1[n] / self.cs3[n]) + (csw * self.cs1[n] * self.cs2[n])
                    + self.wave_fn(
                        x,
                        csw / 3 * self.cs1[n] * self.cs2[n],
                        csw / 3 * self.cs3[n] * self.cs2[n],
                        self.wave[n],
                    )
            }
            /* big slanted vwaves */
            17 => {
                x - (y * self.cs1[n] / self.cs3[n]) + (csw * self.cs1[n] * self.cs2[n])
                    + self.wave_fn(
                        y,
                        csw / 3 * self.cs1[n] * self.cs2[n],
                        csw / 3 * self.cs3[n] * self.cs2[n],
                        self.wave[n],
                    )
            }
            /* double hwave */
            18 => {
                y + (y + csw * self.cs3[n])
                    + self.wave_fn(x, csw / 3 * self.cs3[n], csw / 3 * self.cs2[n], self.wave[n])
                    + self.wave_fn(
                        x,
                        csw / 3 * self.cs4[n],
                        csw / 3 * self.cs1[n] * 3 / 2,
                        self.wave[n],
                    )
            }
            /* double vwave */
            19 => {
                x + (x + csw * self.cs1[n])
                    + self.wave_fn(y, csw / 3 * self.cs1[n], csw / 3 * self.cs3[n], self.wave[n])
                    + self.wave_fn(
                        y,
                        csw / 3 * self.cs2[n],
                        csw / 3 * self.cs4[n] * 3 / 2,
                        self.wave[n],
                    )
            }
            /* one shape */
            20..=22 => self.shape_fn(x, y, self.rx3[n], self.ry3[n], n),
            /* two shapes */
            23..=25 => self
                .shape_fn(x, y, self.rx1[n], self.ry1[n], n)
                .min(self.shape_fn(x, y, self.rx2[n], self.ry2[n], n)),
            /* two shapes opposites (C uses gridy-rx2 for the y coord) */
            26 | 27 => self
                .shape_fn(x, y, self.rx2[n], self.ry2[n], n)
                .min(self.shape_fn(x, y, gx - self.rx2[n], gy - self.rx2[n], n)),
            /* two shape checkerboard */
            28 | 29 => {
                ((self.shape_fn(x, y, self.rx1[n], self.ry1[n], n) / csw)
                    + (self.shape_fn(x, y, self.rx2[n], self.ry2[n], n) / csw))
                    * csw
            }
            /* two shape blob */
            30 | 31 => {
                (self.shape_fn(x, y, self.rx1[n], self.ry1[n], n)
                    + self.shape_fn(x, y, self.rx2[n], self.ry2[n], n))
                    / 2
            }
            /* inverted two shape blob */
            32 | 33 => {
                (self.shape_fn(x, y, self.rx1[n], self.ry1[n], n)
                    + self.shape_fn(gx - x, gy - y, self.rx1[n], self.ry1[n], n))
                    / 2
            }
            /* three shapes */
            34 | 35 => self.shape_fn(x, y, self.rx3[n], self.ry3[n], n).min(
                self.shape_fn(x, y, self.rx1[n], self.ry1[n], n)
                    .min(self.shape_fn(x, y, self.rx2[n], self.ry2[n], n)),
            ),
            /* three shape blob */
            36 | 37 => {
                (self.shape_fn(x, y, self.rx1[n], self.ry1[n], n)
                    + self.shape_fn(x, y, self.rx2[n], self.ry2[n], n)
                    + self.shape_fn(x, y, self.rx3[n], self.ry3[n], n))
                    / 3
            }
            /* 4 shapes -- C's comma operator discards the first _min */
            38 => self
                .shape_fn(x, y, gx - self.rx2[n], self.ry2[n], n)
                .min(self.shape_fn(x, y, self.rx2[n], gy - self.ry2[n], n)),
            /* four rainbows -- C's comma operator discards the first _min */
            39 => self
                .shape_fn(x, y, self.rx2[n] / 2, gy - csw, n)
                .min(self.shape_fn(x, y, gx - csw, gy - (self.ry2[n] / 2), n)),
            _ => 0,
        };
        /* stretch or contract stripe */
        match self.sec[n] {
            0 => {
                v = ((((v.abs() as i64 * gx as i64) as f64).sqrt() as i64 * gx as i64) as f64)
                    .sqrt() as i32
            }
            1 => v = ((v as i64 * v as i64) / gx.max(1) as i64) as i32,
            _ => {}
        }
        v.abs()
    }

    fn getcolor(&self, x: i32, y: i32) -> i32 {
        let mut cv = [0i32; LAYERS];
        for n in 0..self.layers as usize {
            cv[n] = self.pattern_fn(x, y, n);
            cv[0] = if n == 0 {
                /* first wave/shape */
                cv[0] / self.csw[0]
            } else if self.mix[n] < 5 {
                /* checkerboard+1 */
                (cv[0] * self.csw[0] + cv[n]) / self.csw[n]
            } else if self.mix[n] < 12 {
                /* checkerboard+ncol/2 */
                cv[0] + (cv[n] / self.csw[n] * self.ncolors / 2)
            } else if self.mix[n] < 16 {
                /* add mix */
                cv[0] + (cv[n] / self.csw[n])
            } else if self.mix[n] < 18 {
                /* subtract mix */
                cv[0] - (cv[n] / self.csw[n])
            } else if self.mix[n] == 18 {
                /* r to l morph mix */
                ((cv[0] * x) + (cv[n] * (self.gridx - x) / self.csw[n])) / self.gridx
            } else {
                /* u to d morph mix */
                ((cv[0] * y) + (cv[n] * (self.gridy - y) / self.csw[n])) / self.gridy
            };
        }
        cv[0]
    }

    fn hv_fn(&self, x: i32, y: i32, d1: i32, d2: i32, pn: i32, de: usize, line_hv: bool) -> i32 {
        let v1 = match d1 {
            0 => {
                if de == 1 {
                    self.egridx - x
                } else {
                    self.gridx - x
                }
            }
            1 => y,
            2 => x,
            _ => {
                if de == 1 {
                    self.egridy - y
                } else {
                    self.gridy - y
                }
            }
        };
        let v2 = match d2 {
            0 => {
                if de == 1 {
                    self.egridx - x
                } else {
                    self.gridx - x
                }
            }
            1 => y,
            2 => x,
            _ => {
                if de == 1 {
                    self.egridy - y
                } else {
                    self.gridy - y
                }
            }
        };
        if line_hv {
            (v1 + 10000) * pn
        } else {
            (v2 + 10000) * (-pn)
        }
    }

    /* de: 1 while drawing (dline), 0 while erasing (eline);
    `line` is the line whose draw/erase order is being computed */
    fn getdeo(&self, x: i32, y: i32, map: i32, de: usize, line: &Line, rng: &mut impl Rng) -> i32 {
        match map {
            /* horizontal one side */
            0 => x,
            /* vertical one side */
            1 => y,
            /* horizontal two side */
            2 => x.min(self.gridx - x) + 1,
            /* vertical two side */
            3 => y.min(self.gridy - y) + 1,
            /* square */
            4 => (x - self.rx3[de]).abs().max((y - self.ry3[de]).abs()) + 1,
            /* two squares */
            5 => {
                ((x - (self.rx3[de] / 2)).abs().max((y - self.ry3[de]).abs())).min(
                    (x - (self.gridx - (self.rx2[de] / 2)))
                        .abs()
                        .max((y - self.ry2[de]).abs()),
                ) + 1
            }
            /* horizontal rectangle */
            6 => {
                (x - self.rx3[de])
                    .abs()
                    .max((y - self.ry3[de]).abs() * self.cs1[de])
                    + 1
            }
            /* vertical rectangle */
            7 => {
                ((x - self.rx3[de]).abs() * self.cs1[de]).max((y - self.ry3[de]).abs()) + 1
            }
            /* + cross */
            8 => (x - self.rx3[de]).abs().min((y - self.ry3[de]).abs()) + 1,
            /* diagonal */
            9 => (x * 3 / 4 + y) + 1,
            /* opposite diagonal */
            10 => (x * 3 / 4 + self.gridy - y) + 1,
            /* diamond */
            11 => ((x - self.rx3[de]).abs() + (y - self.ry3[de]).abs()) / 2 + 1,
            /* two diamonds */
            12 => {
                ((x - (self.rx3[de] / 2)).abs() + (y - self.ry3[de]).abs()).min(
                    (x - (self.gridx - (self.rx2[de] / 2))).abs() + (y - self.ry2[de]).abs(),
                ) / 2
                    + 1
            }
            /* circle */
            13 => self.dist(x, self.rx3[de], y, self.ry3[de], 0) + 1,
            /* horizontal ellipse */
            14 => self.dist(x, self.rx3[de], y, self.ry3[de], 1) + 1,
            /* vertical ellipse */
            15 => self.dist(x, self.rx3[de], y, self.ry3[de], 2) + 1,
            /* two circles */
            16 => {
                self.dist(x, self.rx3[de] / 2, y, self.ry3[de], 0)
                    .min(self.dist(x, self.gridx - (self.rx2[de] / 2), y, self.ry2[de], 0))
                    + 1
            }
            /* horizontal straight wave */
            17 => {
                x + self.wave_fn(
                    self.gridy + y,
                    self.csw[0] * self.cs1[0],
                    self.csw[0] * self.cs2[0],
                    self.wave[de],
                )
            }
            /* vertical straight wave */
            18 => {
                y + self.wave_fn(
                    self.gridx + x,
                    self.csw[0] * self.cs1[0],
                    self.csw[0] * self.cs2[0],
                    self.wave[de],
                )
            }
            /* horizontal wavey wave */
            19 => {
                x + self.wave_fn(
                    self.gridy + y + ((x / 5) * self.edir),
                    self.csw[de] * self.cs1[de],
                    self.csw[de] * self.cs2[de],
                    self.wave[de],
                ) + 1
            }
            /* vertical wavey wave */
            20 => {
                y + self.wave_fn(
                    self.gridx + x + ((y / 5) * self.edir),
                    self.csw[de] * self.cs1[de],
                    self.csw[de] * self.cs2[de],
                    self.wave[de],
                ) + 1
            }
            /* simultaneous directional */
            21 => self.hv_fn(x, y, self.cs1[0] % 2, self.cs2[0] % 2, 1, de, line.hv),
            /* reverse directional */
            22 => self.hv_fn(x, y, self.cs1[0] % 2, self.cs2[0] % 2, -1, de, line.hv),
            /* length */
            23 => line.len * 1000 + rng.random_range(0..5000),
            /* object */
            24..=27 => (line.obj as i32) * 100,
            /* color */
            _ => {
                let mut cr = line.color;
                if map < 34 {
                    cr = self.rco[(cr as usize) % MAXCOLORS];
                }
                if map % 6 < 4 || de == 0 {
                    /* by color */
                    cr *= 1000;
                    cr += rng.random_range(0..1000);
                } else if map % 6 == 4 {
                    /* by color horizontally */
                    cr *= self.gridx;
                    cr += x + rng.random_range(0..(self.gridx / 2).max(1));
                } else {
                    /* by color vertically */
                    cr *= self.gridy;
                    cr += y + rng.random_range(0..(self.gridy / 2).max(1));
                }
                cr
            }
        }
    }

    fn init_zlist(&mut self, rng: &mut impl Rng) {
        self.gridx = self.width as i32 / self.lwid;
        self.gridy = self.height as i32 / self.lwid;
        /* C aborts here; clamp instead of crashing the locker */
        if self.gridx <= 0 {
            self.gridx = 1;
        }
        if self.gridy <= 0 {
            self.gridy = 1;
        }
        self.gridn = (self.gridx * self.gridy) as usize;
        for z in 0..self.gridn {
            self.grid[z] = GridCell::default();
            self.zlist[z] = z;
        }
        /* rather than pull x,y points randomly and wait to hit final empty
        cells, a list of all points is created and mixed so empty cells do
        get hit last */
        for z in 0..self.gridn {
            let y = rng.random_range(0..self.gridn);
            self.zlist.swap(y, z);
        }
    }

    fn init_colors(&mut self, rng: &mut impl Rng) {
        let mut basecol: [[i32; 3]; BASECOLORS] = [
            /* darks */
            [0x3333, 0x3333, 0x3333], // dgray
            [0x6666, 0x3333, 0x0000], // dbrown
            [0x9999, 0x0000, 0x0000], // dred
            [0xFFFF, 0x6666, 0x0000], // orange
            [0xFFFF, 0xCCCC, 0x0000], // gold
            [0x6666, 0x6666, 0x0000], // olive
            [0x0000, 0x6666, 0x0000], // ivy
            [0x0000, 0x9999, 0x0000], // dgreen
            [0x3333, 0x6666, 0x6666], // bluegray
            [0x0000, 0x0000, 0x9999], // dblue
            [0x3333, 0x3333, 0xFFFF], // blue
            [0x6666, 0x0000, 0xCCCC], // dpurple
            [0x6666, 0x3333, 0xFFFF], // purple
            [0x9999, 0x3333, 0x9999], // violet
            [0xCCCC, 0x3333, 0xCCCC], // magenta
            /* lights */
            [0x3333, 0x3333, 0x3333], // gray
            [0x9999, 0x6666, 0x3333], // brown
            [0xCCCC, 0x9999, 0x3333], // tan
            [0xFFFF, 0x0000, 0x0000], // red
            [0xFFFF, 0x9999, 0x0000], // lorange
            [0xFFFF, 0xFFFF, 0x0000], // yellow
            [0x9999, 0x9999, 0x0000], // lolive
            [0x3333, 0xCCCC, 0x0000], // green
            [0x3333, 0xFFFF, 0x3333], // lgreen
            [0x0000, 0xCCCC, 0xCCCC], // cyan
            [0x3333, 0xFFFF, 0xFFFF], // sky
            [0x3333, 0x6666, 0xFFFF], // marine
            [0x3333, 0xCCCC, 0xFFFF], // lblue
            [0x9999, 0x9999, 0xFFFF], // lpurple
            [0xFFFF, 0x9999, 0xFFFF], // pink
        ];

        self.colors = vec![BG; 255];

        if self.d3d != D3D_NONE {
            self.shades = if self.d3d == D3D_TILED {
                5
            } else {
                self.lwid / 2 + 1
            };
            self.ncolors = 4 + rng.random_range(0..4);
            if self.cmap > 0 {
                /* tint the basecolors a bit */
                for row in basecol.iter_mut() {
                    for c in row.iter_mut().take(2) {
                        if *c == 0 {
                            *c += rng.random_range(0..16000);
                        } else if *c == 0xFFFF {
                            *c -= rng.random_range(0..16000);
                        } else {
                            *c -= 8000;
                            *c += rng.random_range(0..16000);
                        }
                    }
                }
            }
            let nc = self.ncolors as usize;
            let mut col = [0usize; BASECOLORS];
            match self.cmap % 4 {
                /* all */
                0 => {
                    for c in col.iter_mut().take(nc) {
                        *c = rng.random_range(0..BASECOLORS);
                    }
                }
                /* darks */
                1 => {
                    for c in col.iter_mut().take(nc) {
                        *c = rng.random_range(0..15);
                    }
                }
                /* semi consecutive darks */
                2 => {
                    col[0] = rng.random_range(0..15);
                    for c1 in 1..nc {
                        col[c1] = (col[c1 - 1] + 1 + rng.random_range(0..2)) % 15;
                    }
                }
                /* consecutive darks */
                _ => {
                    col[0] = rng.random_range(0..15 - nc);
                    for c1 in 1..nc {
                        col[c1] = col[c1 - 1] + 1;
                    }
                }
            }
            let shades = self.shades as usize;
            for c1 in 0..nc {
                /* shift colors already set to make room at the front */
                for h1 in (0..c1 * shades).rev() {
                    self.colors[h1 + shades] = self.colors[h1];
                }
                let b = basecol[col[c1]];
                let ramp = ramp_rgb16(b[0], b[1], b[2], 0xFFFF, 0xFFFF, 0xFFFF, shades);
                for (i, c) in ramp.iter().enumerate() {
                    self.colors[i] = to_color(*c);
                }
            }
            return;
        }

        /* not 3d */
        self.shades = 1;
        let (r1, g1, b1, r2, g2, b2, r3, g3, b3);
        if self.cmap % 2 != 0 {
            /* basecolors */
            let (c1i, c2i, c3i);
            if rng.random_range(0..3) != 0 {
                c1i = rng.random_range(0..15usize);
                c2i = (c1i + 3 + rng.random_range(0..5)) % 15;
                c3i = (c2i + 3 + rng.random_range(0..5)) % 15;
            } else {
                c1i = rng.random_range(0..BASECOLORS);
                c2i = (c1i + 5 + rng.random_range(0..10)) % BASECOLORS;
                c3i = (c2i + 5 + rng.random_range(0..10)) % BASECOLORS;
            }
            r1 = basecol[c1i][0];
            g1 = basecol[c1i][1];
            b1 = basecol[c1i][2];
            r2 = basecol[c2i][0];
            g2 = basecol[c2i][1];
            b2 = basecol[c2i][2];
            r3 = basecol[c3i][0];
            g3 = basecol[c3i][1];
            b3 = basecol[c3i][2];
        } else {
            /* random rgb's */
            r1 = rng.random_range(0..65535);
            g1 = rng.random_range(0..65535);
            b1 = rng.random_range(0..65535);
            r2 = (r1 + 16384 + rng.random_range(0..32768)) % 65535;
            g2 = (g1 + 16384 + rng.random_range(0..32768)) % 65535;
            b2 = (b1 + 16384 + rng.random_range(0..32768)) % 65535;
            r3 = (r2 + 16384 + rng.random_range(0..32768)) % 65535;
            g3 = (g2 + 16384 + rng.random_range(0..32768)) % 65535;
            b3 = (b2 + 16384 + rng.random_range(0..32768)) % 65535;
        }
        match self.cmap {
            /* make_color_ramp color->color / color->white */
            0..=3 => {
                self.ncolors = 5 + rng.random_range(0..5);
                let (r2, g2, b2) = if self.cmap > 1 {
                    (0xFFFF, 0xFFFF, 0xFFFF)
                } else {
                    (r2, g2, b2)
                };
                /* C passes random()%2 as closed_p, which its ramp_rgb ignores */
                let _ = rng.random_range(0..2);
                let ramp = ramp_rgb16(r1, g1, b1, r2, g2, b2, self.ncolors as usize);
                for (i, c) in ramp.iter().enumerate() {
                    self.colors[i] = to_color(*c);
                }
            }
            /* 3 color make_color_loop */
            4..=7 => {
                self.ncolors = 8 + rng.random_range(0..12);
                let (h1, s1, v1) = rgb_to_hsv16(r1, g1, b1);
                let (h2, s2, v2) = rgb_to_hsv16(r2, g2, b2);
                let (h3, s3, v3) = rgb_to_hsv16(r3, g3, b3);
                let path = make_color_path16(
                    &[h1, h2, h3],
                    &[s1, s2, s3],
                    &[v1, v2, v3],
                    self.ncolors as usize,
                );
                for (i, c) in path.iter().enumerate() {
                    self.colors[i] = to_color(*c);
                }
                if path.is_empty() {
                    self.ncolors = 0;
                }
            }
            /* random smooth */
            8 | 9 => {
                self.ncolors = rng.random_range(0..4) * 6 + 12;
                let (h, s, v) = smooth_points(rng);
                let path = make_color_path16(&h, &s, &v, self.ncolors as usize);
                for (i, c) in path.iter().enumerate() {
                    self.colors[i] = to_color(*c);
                }
                if path.is_empty() {
                    self.ncolors = 0;
                }
            }
            /* rainbow */
            10 => {
                self.ncolors = rng.random_range(0..4) * 6 + 12;
                /* make_uniform_colormap: full-hue ramp, S/V in 66%-100% */
                let s = (rng.random_range(0..34) + 66) as f64 / 100.0;
                let v = (rng.random_range(0..34) + 66) as f64 / 100.0;
                let ramp = make_color_ramp16(0, s, v, 359, s, v, self.ncolors as usize, false);
                for (i, c) in ramp.iter().enumerate() {
                    self.colors[i] = to_color(*c);
                }
            }
            /* dark to light blend */
            11..=14 => {
                let t1 = ramp_rgb16(r1, g1, b1, 0xFFFF, 0xFFFF, 0xFFFF, 7);
                let t2 = ramp_rgb16(r2, g2, b2, 0xFFFF, 0xFFFF, 0xFFFF, 7);
                if self.cmap < 13 {
                    for c1 in 0..=4 {
                        self.colors[c1 * 2] = to_color(t1[c1]);
                        self.colors[c1 * 2 + 1] = to_color(t2[c1]);
                    }
                    self.ncolors = 10;
                } else {
                    let t3 = ramp_rgb16(r3, g3, b3, 0xFFFF, 0xFFFF, 0xFFFF, 7);
                    for c1 in 0..=4 {
                        self.colors[c1 * 3] = to_color(t1[c1]);
                        self.colors[c1 * 3 + 1] = to_color(t2[c1]);
                        self.colors[c1 * 3 + 2] = to_color(t3[c1]);
                    }
                    self.ncolors = 15;
                }
            }
            /* random (make_random_colormap with bright_p=False) */
            _ => {
                self.ncolors = rng.random_range(0..4) * 6 + 12;
                for i in 0..self.ncolors as usize {
                    self.colors[i] = Color::new(
                        255,
                        (rng.random_range(0..0xFFFF_u32) >> 8) as u8,
                        (rng.random_range(0..0xFFFF_u32) >> 8) as u8,
                        (rng.random_range(0..0xFFFF_u32) >> 8) as u8,
                    );
                }
            }
        }
        if self.ncolors <= 0 {
            self.ncolors = 1;
        }

        /* set random color order for drawing and erasing */
        for (c1, r) in self.rco.iter_mut().enumerate() {
            *r = c1 as i32;
        }
        for c1 in 0..MAXCOLORS {
            let c3 = rng.random_range(0..MAXCOLORS);
            self.rco.swap(c1, c3);
        }
    }

    /* one bounded slice of the screen (re)build per tick. The C does all of
    this in a single blocking draw_abstractile call and hides the stall by
    subtracting the elapsed time from its multi-second sleep; we can't, so
    the same work is resumed across ticks. No pixels are touched while
    creating, so every rendered frame is identical to the blocking version. */
    fn create_step(&mut self, rng: &mut impl Rng) {
        /* ~3-6ms slices at 1920x1080 in release builds */
        const DEO_CHUNK: usize = 100_000;
        const LINE_CHUNK: usize = 25_000;
        match self.cphase {
            /* allocate memory in case of resize; swap dline and eline to
            resort and erase */
            0 => {
                if self.resized {
                    self.max_wxh = self.width * self.height;
                    self.narray =
                        (self.width as usize + 1) * (self.height as usize + 1) / 4 + 1;
                    self.dline = vec![Line::default(); self.narray];
                    self.eline = vec![Line::default(); self.narray];
                    self.grid = vec![GridCell::default(); self.narray];
                    self.zlist = vec![0; self.narray];
                    self.fdol = vec![0; self.narray];
                    self.dialog = if self.width < 500 { 1 } else { 0 };
                    self.resized = false;
                }
                if self.ii > 0 {
                    std::mem::swap(&mut self.dline, &mut self.eline);
                    self.eli = self.li;
                    self.elwid = self.lwid;
                    self.elpu = self.lpu;
                    self.egridx = self.gridx;
                    self.egridy = self.gridy;
                    self.cphase = 1;
                    self.cprog = 1;
                } else {
                    self.cphase = 3;
                }
            }
            /* create new erase order */
            1 => {
                let end = (self.eli + 1).min(self.cprog + DEO_CHUNK);
                for li in self.cprog..end {
                    let l = self.eline[li];
                    let deo = (self.getdeo(l.x, l.y, self.emap, 0, &l, rng)
                        + rng.random_range(0..self.evar.max(1))
                        + rng.random_range(0..self.evar.max(1)))
                        * self.edir;
                    self.eline[li].deo = deo;
                }
                self.cprog = end;
                if end > self.eli {
                    self.cphase = 2;
                }
            }
            2 => {
                self.eline[..=self.eli].sort_unstable_by_key(|l| l.deo);
                self.cphase = 3;
            }
            /* set random screen variables */
            3 => {
                self.init_screen(rng);
                self.cphase = 4;
            }
            /* create the grid of lines */
            4 => {
                let end = (self.zi + LINE_CHUNK).min(self.gridn);
                while !self.grid_full && self.zi < end {
                    self.newline(rng);
                }
                if self.grid_full || self.zi >= self.gridn {
                    self.cphase = 5;
                }
            }
            /* sort the draw order and start erasing */
            _ => {
                self.create_screen();
                self.cphase = 0;
            }
        }
    }

    fn init_screen(&mut self, rng: &mut impl Rng) {
        self.ii += 1;

        /* clear arrays and other counters */
        self.fi = 0;
        self.li = 0;
        self.oi = 0;
        self.zi = 0;
        self.grid_full = false;
        /* li starts from 1; keep index 0 first after sorting so di is never null */
        self.dline[0] = Line {
            deo: -999_999_999,
            ..Line::default()
        };

        /* set random screen variables */
        self.lwid = if self.ii == 1 {
            3
        } else {
            2 + (rng.random_range(0..6) % 4)
        };
        self.d3d = if TILE == TILE_FLAT || TILE == TILE_THIN || TILE == TILE_OUTLINE {
            D3D_NONE
        } else if TILE == TILE_BLOCK {
            D3D_BLOCK
        } else if TILE == TILE_NEON {
            D3D_NEON
        } else if TILE == TILE_TILED {
            D3D_TILED
        } else if self.ii == 1 && !NEWCOLS {
            /* force TILE_D3D on first screen to properly load all shades */
            D3D_TILED
        } else {
            rng.random_range(0..5) % 4
        };
        self.outline = if TILE == TILE_OUTLINE {
            1
        } else if TILE != TILE_RANDOM || rng.random_range(0..5) != 0 {
            0
        } else {
            1
        };
        self.round = if self.d3d == D3D_NEON {
            1
        } else if self.d3d == D3D_BLOCK || self.outline != 0 || rng.random_range(0..6) != 0 {
            0
        } else {
            1
        };
        if self.d3d != D3D_NONE || self.outline != 0 || self.round != 0 {
            self.lwid += 2;
        }
        if self.d3d == D3D_NONE && self.round == 0 && self.outline == 0 && self.lwid > 3 {
            self.lwid -= 2;
        }
        if self.d3d == D3D_TILED {
            self.lwid += 1;
        }
        if TILE == TILE_THIN {
            self.lwid = 2;
        }
        if self.width > 2560 || self.height > 2560 {
            self.lwid *= 3; /* Retina displays */
        }

        self.init_zlist(rng);

        self.maxlen = if self.lwid > 6 {
            2 + rng.random_range(0..4)
        } else if self.lwid > 4 {
            2 + rng.random_range(0..8) % 6
        } else if self.lwid > 2 {
            2 + rng.random_range(0..12) % 8
        } else {
            2 + rng.random_range(0..15) % 10
        };
        self.bnratio = 4 + rng.random_range(0..4) + rng.random_range(0..4);
        self.forcemax = if rng.random_range(0..6) != 0 { 0 } else { 1 };

        if self.ii == 1 || NEWCOLS {
            self.init_colors(rng);
        }

        /* C computes this dmap and then immediately overwrites it */
        self.dmap = (self.emap + 5 + rng.random_range(0..5)) % DRAWORDERS;
        self.dmap = 20 + rng.random_range(0..20);

        self.dvar = if self.dmap > 22 {
            100
        } else {
            10 + self.csw[0] * rng.random_range(0..5)
        };
        self.ddir = if rng.random_range(0..2) != 0 { 1 } else { -1 };

        self.emap = (self.dmap + 10 + rng.random_range(0..10)) % 20;
        self.evar = if self.emap > 22 {
            100
        } else {
            10 + self.csw[0] * rng.random_range(0..5)
        };
        self.edir = if rng.random_range(0..2) != 0 { 1 } else { -1 };

        self.layers = if rng.random_range(0..2) != 0 {
            2
        } else if rng.random_range(0..2) != 0 {
            1
        } else if rng.random_range(0..2) != 0 {
            3
        } else {
            4
        };
        self.cmap = (self.cmap + 5 + rng.random_range(0..10)) % COLORMAPS;

        for x in 0..LAYERS {
            self.pattern[x] = rng.random_range(0..PATTERNS);
            self.shape[x] = rng.random_range(0..SHAPES);
            self.mix[x] = rng.random_range(0..20);
            let nstr = match self.lwid {
                2 => 20 + rng.random_range(0..12),
                3 => 16 + rng.random_range(0..8),
                4 => 12 + rng.random_range(0..6),
                5 => 10 + rng.random_range(0..5),
                6 => 8 + rng.random_range(0..4),
                _ => 5 + rng.random_range(0..5),
            };
            self.csw[x] = 5.max(self.gridy / nstr);
            self.wsx[x] = (self.wsx[x] + 3 + rng.random_range(0..3)) % STRETCHES;
            self.wsy[x] = (self.wsy[x] + 3 + rng.random_range(0..3)) % STRETCHES;
            self.sec[x] = rng.random_range(0..5);
            if self.dialog == 0 && self.sec[x] < 2 {
                self.csw[x] /= 2;
            }
            self.cs1[x] = if self.dialog != 0 {
                1 + rng.random_range(0..3)
            } else {
                2 + rng.random_range(0..5)
            };
            self.cs2[x] = if self.dialog != 0 {
                1 + rng.random_range(0..3)
            } else {
                2 + rng.random_range(0..5)
            };
            self.cs3[x] = if self.dialog != 0 {
                1 + rng.random_range(0..3)
            } else {
                2 + rng.random_range(0..5)
            };
            self.cs4[x] = if self.dialog != 0 {
                1 + rng.random_range(0..3)
            } else {
                2 + rng.random_range(0..5)
            };
            self.wave[x] = rng.random_range(0..WAVES);
            self.wavel[x] = self.csw[x] * (2 + rng.random_range(0..6));
            self.waveh[x] = self.csw[x] * (1 + rng.random_range(0..3));
            self.rx1[x] = self.gridx / 10 + rng.random_range(0..(self.gridx * 8 / 10).max(1));
            self.ry1[x] = self.gridy / 10 + rng.random_range(0..(self.gridy * 8 / 10).max(1));
            self.rx2[x] = self.gridx * 2 / 10 + rng.random_range(0..(self.gridx * 6 / 10).max(1));
            self.ry2[x] = self.gridy * 2 / 10 + rng.random_range(0..(self.gridy * 6 / 10).max(1));
            self.rx3[x] = self.gridx * 3 / 10 + rng.random_range(0..(self.gridx * 4 / 10).max(1));
            self.ry3[x] = self.gridy * 3 / 10 + rng.random_range(0..(self.gridy * 4 / 10).max(1));
        }
    }

    /* return value = line direction; sets self.olen (open space to edge or
    next blocking line) and self.bln (blocking line number, -1 if edge) */
    fn findopen(&mut self, x: i32, y: i32, z: usize, rng: &mut impl Rng) -> i32 {
        let gx = self.gridx as usize;
        if (self.grid[z].hl != 0 || self.grid[z].hr != 0)
            && (self.grid[z].vu != 0 || self.grid[z].vd != 0)
        {
            return DIR_NONE;
        }
        let mut od = [0i32; 4];
        let mut no = 0;
        if z > gx && self.grid[z].hl == 0 && self.grid[z].hr == 0 && self.grid[z - gx].line == 0 {
            od[no] = DIR_UP;
            no += 1;
        }
        if z < self.gridn - gx
            && self.grid[z].hl == 0
            && self.grid[z].hr == 0
            && self.grid[z + gx].line == 0
        {
            od[no] = DIR_DOWN;
            no += 1;
        }
        if x != 0 && self.grid[z].hl == 0 && self.grid[z].hr == 0 && self.grid[z - 1].line == 0 {
            od[no] = DIR_LEFT;
            no += 1;
        }
        if (z + 1) % gx != 0
            && self.grid[z].hl == 0
            && self.grid[z].hr == 0
            && self.grid[z + 1].line == 0
        {
            od[no] = DIR_RIGHT;
            no += 1;
        }
        if no == 0 {
            return DIR_NONE;
        }
        let dir = od[rng.random_range(0..no)];
        self.olen = 0;
        self.bln = 0;
        while self.olen <= self.maxlen && self.bln == 0 {
            self.olen += 1;
            let ol = self.olen;
            match dir {
                DIR_UP => {
                    self.bln = if y - ol < 0 {
                        -1
                    } else {
                        self.grid[z - ol as usize * gx].line as i32
                    }
                }
                DIR_DOWN => {
                    self.bln = if y + ol >= self.gridy {
                        -1
                    } else {
                        self.grid[z + ol as usize * gx].line as i32
                    }
                }
                DIR_LEFT => {
                    self.bln = if x - ol < 0 {
                        -1
                    } else {
                        self.grid[z - ol as usize].line as i32
                    }
                }
                _ => {
                    self.bln = if x + ol >= self.gridx {
                        -1
                    } else {
                        self.grid[z + ol as usize].line as i32
                    }
                }
            }
        }
        self.olen -= 1;
        dir
    }

    fn fillgrid(&mut self) {
        let li = self.li;
        let l = self.dline[li];
        let mut gridc = (self.gridx * l.y + l.x) as usize;
        let add = if l.hv { 1 } else { self.gridx as usize };
        for n in 0..=l.len {
            if n != 0 {
                gridc += add;
            }
            if gridc >= self.gridn {
                return; // defensive; C trusts its invariants
            }
            if self.grid[gridc].line == 0 {
                self.fi += 1;
                self.grid[gridc].line = li;
            }
            if l.hv {
                if n != 0 {
                    self.grid[gridc].hr = li;
                }
                if n < l.len {
                    self.grid[gridc].hl = li;
                }
            } else {
                if n != 0 {
                    self.grid[gridc].vd = li;
                }
                if n < l.len {
                    self.grid[gridc].vu = li;
                }
            }
            if self.fi >= self.gridn {
                self.grid_full = true;
                return;
            }
        }
    }

    fn newline(&mut self, rng: &mut impl Rng) {
        let mut bl = 0usize;
        let z = self.zlist[self.zi];
        let x = (z % self.gridx as usize) as i32;
        let y = (z / self.gridx as usize) as i32;
        self.zi += 1;
        let mut dir = self.findopen(x, y, z, rng);

        let lt;
        if self.grid[z].line == 0 {
            /* empty space: make a new line unless nothing is open around it */
            if dir == DIR_NONE {
                /* nothing is open, force a len 1 branch in any direction */
                lt = LINE_FORCE;
                while dir == DIR_NONE
                    || (dir == DIR_UP && y == 0)
                    || (dir == DIR_DOWN && y + 1 == self.gridy)
                    || (dir == DIR_LEFT && x == 0)
                    || (dir == DIR_RIGHT && x + 1 == self.gridx)
                {
                    dir = rng.random_range(0..4);
                }
                let bz = match dir {
                    DIR_UP => z - self.gridx as usize,
                    DIR_DOWN => z + self.gridx as usize,
                    DIR_LEFT => z - 1,
                    _ => z + 1,
                };
                bl = self.grid[bz].line;
            } else if self.bnratio > 1
                && self.bln > 0
                && self.olen < self.maxlen
                && rng.random_range(0..self.bnratio) != 0
            {
                /* branch into blocking line */
                lt = LINE_BRIN;
                bl = self.bln as usize;
            } else {
                /* make a new line and new object */
                lt = LINE_NEW;
                self.oi += 1;
            }
        } else {
            /* filled space: make a branch unless nothing is open around it */
            if dir == DIR_NONE {
                return;
            }
            /* make a branch out of this line */
            lt = LINE_BROUT;
            bl = self.grid[z].line;
        }
        self.li += 1;
        let li = self.li;
        let len = if lt == LINE_FORCE {
            1
        } else if lt == LINE_BRIN {
            self.olen + 1
        } else if self.forcemax == 0 {
            self.olen
        } else {
            1 + rng.random_range(0..self.olen.max(1))
        };
        self.dline[li].len = len;
        self.dline[li].x = x;
        if dir == DIR_LEFT {
            self.dline[li].x -= len;
        }
        self.dline[li].y = y;
        if dir == DIR_UP {
            self.dline[li].y -= len;
        }
        self.dline[li].hv = dir == DIR_LEFT || dir == DIR_RIGHT;
        self.dline[li].obj = if lt == LINE_NEW {
            self.oi
        } else {
            self.dline[bl].obj
        };
        if lt == LINE_NEW {
            let mut color = self.getcolor(x, y) % self.ncolors;
            if color < 0 {
                color += self.ncolors;
            }
            self.dline[li].color = color;
        } else {
            self.dline[li].color = self.dline[bl].color;
        }
        let l = self.dline[li];
        self.dline[li].deo = (self.getdeo(x, y, self.dmap, 1, &l, rng)
            + rng.random_range(0..self.dvar.max(1))
            + rng.random_range(0..self.dvar.max(1)))
            * self.ddir;
        self.dline[li].ndol = 0;
        self.fillgrid();
    }

    fn create_screen(&mut self) {
        self.grid_full = true;
        self.dline[..=self.li].sort_unstable_by_key(|l| l.deo);
        /* draw 1/200th of the screen with each update (1/50th for small
        windows); the C comment explains this replaced a speed-tuned lpu */
        self.lpu = if self.dialog != 0 {
            self.li / 50
        } else {
            self.li / 200
        };
        if self.lpu == 0 {
            self.lpu = 1;
        }
        self.bi = 1;
        self.mode = Mode::Erase;
    }

    fn fill_outline(&mut self, di: usize) {
        if di == 0 {
            return;
        }
        let l = self.dline[di];
        let x = l.x * self.lwid + 1;
        let y = l.y * self.lwid + 1;
        let (w, h) = if l.hv {
            ((l.len + 1) * self.lwid - 3, self.lwid - 3)
        } else {
            (self.lwid - 3, (l.len + 1) * self.lwid - 3)
        };
        fill_rect(&mut self.pixels, self.width, self.height, x, y, w, h, BG);
    }

    fn xfill_rectangle(&mut self, di: usize, adj: i32, c: Color) {
        let l = self.dline[di];
        let lwid = self.lwid;
        let mut x = l.x * lwid;
        let mut y = l.y * lwid;
        let (mut w, mut h) = if l.hv {
            ((l.len + 1) * lwid - 1, lwid - 1)
        } else {
            (lwid - 1, (l.len + 1) * lwid - 1)
        };
        match self.d3d {
            D3D_NEON => {
                x += adj;
                y += adj;
                w -= adj * 2;
                h -= adj * 2;
            }
            D3D_BLOCK => {
                x += adj;
                y += adj;
                w -= lwid / 2 - 1;
                h -= lwid / 2 - 1;
            }
            _ => {}
        }
        if self.round == 0 {
            fill_rect(&mut self.pixels, self.width, self.height, x, y, w, h, c);
        } else if h < lwid {
            /* horizontal */
            let a = (h - 1) / 2;
            for b in 0..=a {
                fill_rect(
                    &mut self.pixels,
                    self.width,
                    self.height,
                    x + b,
                    y + a - b,
                    w - b * 2,
                    h - (a - b) * 2,
                    c,
                );
            }
        } else {
            /* vertical */
            let a = (w - 1) / 2;
            for b in 0..=a {
                fill_rect(
                    &mut self.pixels,
                    self.width,
                    self.height,
                    x + a - b,
                    y + b,
                    w - (a - b) * 2,
                    h - b * 2,
                    c,
                );
            }
        }
    }

    fn xfill_triangle(&mut self, color: usize, x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32) {
        let c = self.colors[color.min(254)];
        fill_poly(
            &mut self.pixels,
            self.width,
            self.height,
            &[(x1, y1), (x2, y2), (x3, y3)],
            c,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn xfill_polygon4(
        &mut self,
        color: usize,
        x1: i32, y1: i32,
        x2: i32, y2: i32,
        x3: i32, y3: i32,
        x4: i32, y4: i32,
    ) {
        let c = self.colors[color.min(254)];
        fill_poly(
            &mut self.pixels,
            self.width,
            self.height,
            &[(x1, y1), (x2, y2), (x3, y3), (x4, y4)],
            c,
        );
    }

    fn draw_tiled(&mut self, di: usize, color: usize) {
        let l = self.dline[di];
        let a = if l.hv { 1 } else { self.gridx as usize };
        let mut z = (l.y * self.gridx + l.x) as usize;
        let m1 = (self.lwid - 1) / 2;
        let m2 = self.lwid / 2;
        let lr = self.lwid - 1;
        let nl = self.lwid;

        /* draw tiles one grid cell at a time */
        for c in 0..=l.len {
            if z >= self.gridn {
                break;
            }
            let (x, y);
            if l.hv {
                x = (l.x + c) * self.lwid;
                y = l.y * self.lwid;
                if c != 0 {
                    self.grid[z].dhr = di;
                }
                if c < l.len {
                    self.grid[z].dhl = di;
                }
            } else {
                x = l.x * self.lwid;
                y = (l.y + c) * self.lwid;
                if c != 0 {
                    self.grid[z].dvd = di;
                }
                if c < l.len {
                    self.grid[z].dvu = di;
                }
            }
            let mut d = 0;
            if self.grid[z].dhl != 0 {
                d += 8;
            }
            if self.grid[z].dhr != 0 {
                d += 4;
            }
            if self.grid[z].dvu != 0 {
                d += 2;
            }
            if self.grid[z].dvd != 0 {
                d += 1;
            }
            /* draw line base */
            match d {
                /* vertical */
                1 | 2 | 3 | 5 | 6 | 7 | 11 | 15 => {
                    let h = if d == 1 || d == 5 { lr } else { nl };
                    let c0 = self.colors[color.min(254)];
                    let c3 = self.colors[(color + 3).min(254)];
                    fill_rect(&mut self.pixels, self.width, self.height, x, y, m2, h, c0);
                    fill_rect(&mut self.pixels, self.width, self.height, x + m2, y, m1, h, c3);
                }
                /* horizontal */
                4 | 8 | 9 | 10 | 12 | 13 | 14 => {
                    let w = if d == 4 { lr } else { nl };
                    let c1 = self.colors[(color + 1).min(254)];
                    let c2 = self.colors[(color + 2).min(254)];
                    fill_rect(&mut self.pixels, self.width, self.height, x, y, w, m2, c1);
                    fill_rect(&mut self.pixels, self.width, self.height, x, y + m2, w, m1, c2);
                }
                _ => {}
            }
            /* draw angles */
            match d {
                /* bottom end ^ */
                1 => self.xfill_triangle(color + 2, x, y + lr, x + lr, y + lr, x + m2, y + m2),
                /* top end \/ */
                2 => self.xfill_triangle(color + 1, x, y, x + lr, y, x + m2, y + m2),
                /* right end < */
                4 => self.xfill_triangle(color + 3, x + lr, y, x + lr, y + lr, x + m2, y + m2),
                /* LR corner */
                5 => {
                    self.xfill_triangle(color + 1, x, y + m2, x + m2, y + m2, x, y);
                    self.xfill_polygon4(color + 2, x, y + m2, x + m2, y + m2, x + lr, y + lr, x, y + lr);
                }
                /* UR corner */
                6 => {
                    self.xfill_polygon4(color + 1, x, y + m2, x + m2, y + m2, x + lr, y, x, y);
                    self.xfill_triangle(color + 2, x, y + m2, x + m2, y + m2, x, y + lr);
                }
                /* T > into line */
                7 => {
                    self.xfill_triangle(color + 1, x, y + m2, x + m2, y + m2, x, y);
                    self.xfill_triangle(color + 2, x, y + m2, x + m2, y + m2, x, y + lr);
                }
                /* left end > */
                8 => self.xfill_triangle(color, x, y, x, y + lr, x + m2, y + m2),
                /* LL corner */
                9 => {
                    self.xfill_polygon4(color, x + m2, y, x + m2, y + m2, x, y + lr, x, y);
                    self.xfill_triangle(color + 3, x + m2, y, x + m2, y + m2, x + lr, y);
                }
                /* UL corner */
                10 => {
                    self.xfill_polygon4(color, x + m2, y + nl, x + m2, y + m2, x, y, x, y + nl);
                    self.xfill_polygon4(color + 3, x + m2, y + nl, x + m2, y + m2, x + lr, y + lr, x + lr, y + nl);
                }
                /* T < into line */
                11 => {
                    self.xfill_polygon4(color + 1, x + nl, y + m2, x + m2, y + m2, x + lr, y, x + nl, y);
                    self.xfill_polygon4(color + 2, x + nl, y + m2, x + m2, y + m2, x + lr, y + lr, x + nl, y + lr);
                }
                /* T \/ into line */
                13 => {
                    self.xfill_triangle(color, x + m2, y, x + m2, y + m2, x, y);
                    self.xfill_triangle(color + 3, x + m2, y, x + m2, y + m2, x + lr, y);
                }
                /* T ^ into line */
                14 => {
                    self.xfill_polygon4(color, x + m2, y + nl, x + m2, y + m2, x, y + lr, x, y + nl);
                    self.xfill_polygon4(color + 3, x + m2, y + nl, x + m2, y + m2, x + lr, y + lr, x + lr, y + nl);
                }
                /* X intersection */
                15 => {
                    self.xfill_triangle(color + 1, x, y + m2, x + m2, y + m2, x, y);
                    self.xfill_triangle(color + 2, x, y + m2, x + m2, y + m2, x, y + lr);
                    self.xfill_polygon4(color + 1, x + nl, y + m2, x + m2, y + m2, x + lr, y, x + nl, y);
                    self.xfill_polygon4(color + 2, x + nl, y + m2, x + m2, y + m2, x + lr, y + lr, x + nl, y + lr);
                }
                _ => {}
            }
            z += a;
        }
    }

    fn draw_lines(&mut self) {
        if self.bi == 1 {
            for a in 0..=self.oi.min(self.fdol.len() - 1) {
                self.fdol[a] = 0;
            }
        }
        let end = (self.li + 1).min(self.bi + self.lpu);
        for di in self.bi..end {
            let color = ((self.dline[di].color % self.ncolors) * self.shades) as usize;
            match self.d3d {
                D3D_NEON => {
                    /* draw the full object so far, one shade ring at a time */
                    let obj = self.dline[di].obj;
                    self.dline[di].ndol = self.fdol[obj];
                    self.fdol[obj] = di;
                    for sh in 0..self.lwid / 2 {
                        let c = self.colors[(color + sh as usize).min(254)];
                        let mut d = di;
                        while d > 0 {
                            self.xfill_rectangle(d, sh, c);
                            d = self.dline[d].ndol;
                        }
                    }
                }
                D3D_BLOCK => {
                    let obj = self.dline[di].obj;
                    self.dline[di].ndol = self.fdol[obj];
                    self.fdol[obj] = di;
                    for sh in 0..self.lwid / 2 {
                        let ci = color as i32 + self.lwid / 2 - sh - 1;
                        let c = self.colors[(ci.max(0) as usize).min(254)];
                        let mut d = di;
                        while d > 0 {
                            self.xfill_rectangle(d, sh, c);
                            d = self.dline[d].ndol;
                        }
                    }
                }
                D3D_TILED => self.draw_tiled(di, color),
                /* D3D_NONE */
                _ => {
                    let c = self.colors[color.min(254)];
                    self.xfill_rectangle(di, 0, c);
                    if self.outline != 0 {
                        self.fill_outline(di);
                        let l = self.dline[di];
                        let a = if l.hv { 1 } else { self.gridx as usize };
                        let mut z = (l.y * self.gridx + l.x) as usize;
                        for n in 0..=l.len {
                            if z >= self.gridn {
                                break;
                            }
                            let cell = self.grid[z];
                            self.fill_outline(cell.dhl);
                            self.fill_outline(cell.dhr);
                            self.fill_outline(cell.dvu);
                            self.fill_outline(cell.dvd);
                            if l.hv {
                                if n != 0 {
                                    self.grid[z].dhr = di;
                                }
                                if n < l.len {
                                    self.grid[z].dhl = di;
                                }
                            } else {
                                if n != 0 {
                                    self.grid[z].dvd = di;
                                }
                                if n < l.len {
                                    self.grid[z].dvu = di;
                                }
                            }
                            z += a;
                        }
                    }
                }
            }
        }
        if end > self.li {
            self.bi = 1;
            self.mode = Mode::Create;
        } else {
            self.bi += self.lpu;
        }
    }

    fn erase_lines(&mut self) {
        if self.ii == 0 {
            return;
        }
        let end = (self.eli + 1).min(self.bi + self.elpu.max(1));
        for di in self.bi..end {
            let l = self.eline[di];
            if l.hv {
                fill_rect(
                    &mut self.pixels,
                    self.width,
                    self.height,
                    l.x * self.elwid,
                    l.y * self.elwid,
                    (l.len + 1) * self.elwid,
                    self.elwid,
                    BG,
                );
            } else {
                fill_rect(
                    &mut self.pixels,
                    self.width,
                    self.height,
                    l.x * self.elwid,
                    l.y * self.elwid,
                    self.elwid,
                    (l.len + 1) * self.elwid,
                    BG,
                );
            }
            if di == self.eli {
                /* clear just in case */
                clear_buffer(&mut self.pixels, BG);
            }
        }
        if end > self.eli {
            self.bi = 1;
            self.mode = if self.resized { Mode::Create } else { Mode::Draw };
        } else {
            self.bi += self.elpu.max(1);
        }
    }
}

/* port of make_smooth_colormap's point picking */
fn smooth_points(rng: &mut impl Rng) -> (Vec<i32>, Vec<f64>, Vec<f64>) {
    let npoints = {
        let n = rng.random_range(0..20);
        if n <= 5 {
            2 /* 30% of the time */
        } else if n <= 15 {
            3 /* 50% of the time */
        } else if n <= 18 {
            4 /* 15% of the time */
        } else {
            5 /*  5% of the time */
        }
    };
    let mut h = vec![0i32; npoints];
    let mut s = vec![0.0f64; npoints];
    let mut v = vec![0.0f64; npoints];
    let mut guard = 0;
    loop {
        let mut total_s = 0.0;
        let mut total_v = 0.0;
        for i in 0..npoints {
            loop {
                guard += 1;
                h[i] = rng.random_range(0..360);
                s[i] = rng.random::<f64>();
                v[i] = rng.random::<f64>() * 0.8 + 0.2;
                /* make sure no two adjacent colors are *too* close together */
                if i > 0 && guard <= 10000 {
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
        /* repick if the average saturation or intensity is too low */
        if (total_s / npoints as f64 >= 0.2 && total_v / npoints as f64 >= 0.3) || guard > 10000 {
            break;
        }
    }
    (h, s, v)
}

impl Animation for Abstractile {
    fn new(config: &AnimConfig) -> Self {
        let mut pixels = vec![0u8; (config.width * config.height * 4) as usize];
        clear_buffer(&mut pixels, BG);
        /* allocate the big line/grid arrays here rather than on the first
        tick: zeroing ~25ms worth of memory would blow the frame budget */
        let narray = (config.width as usize + 1) * (config.height as usize + 1) / 4 + 1;
        Abstractile {
            width: config.width,
            height: config.height,
            pixels,
            colors: vec![BG; 255],
            dline: vec![Line::default(); narray],
            eline: vec![Line::default(); narray],
            grid: vec![GridCell::default(); narray],
            zlist: vec![0; narray],
            fdol: vec![0; narray],
            ii: 0,
            fi: 0,
            bi: 0,
            li: 0,
            eli: 0,
            oi: 0,
            zi: 0,
            gridx: 0,
            gridy: 0,
            gridn: 0,
            lwid: 0,
            narray,
            max_wxh: config.width * config.height,
            elwid: 0,
            elpu: 0,
            egridx: 0,
            egridy: 0,
            bnratio: 0,
            maxlen: 0,
            forcemax: 0,
            olen: 0,
            bln: 0,
            ncolors: 1,
            shades: 1,
            rco: [0; MAXCOLORS],
            cmap: 0,
            layers: 0,
            dmap: 0,
            emap: 0,
            dvar: 0,
            evar: 0,
            ddir: 0,
            edir: 0,
            lpu: 1,
            d3d: 0,
            round: 0,
            outline: 0,
            pattern: [0; LAYERS],
            shape: [0; LAYERS],
            mix: [0; LAYERS],
            csw: [0; LAYERS],
            wsx: [0; LAYERS],
            wsy: [0; LAYERS],
            sec: [0; LAYERS],
            cs1: [0; LAYERS],
            cs2: [0; LAYERS],
            cs3: [0; LAYERS],
            cs4: [0; LAYERS],
            wave: [0; LAYERS],
            waveh: [0; LAYERS],
            wavel: [0; LAYERS],
            rx1: [0; LAYERS],
            rx2: [0; LAYERS],
            rx3: [0; LAYERS],
            ry1: [0; LAYERS],
            ry2: [0; LAYERS],
            ry3: [0; LAYERS],
            mode: Mode::Create,
            dialog: if config.width < 500 { 1 } else { 0 },
            grid_full: false,
            resized: false,
            cphase: 0,
            cprog: 0,
            next_delay_us: 20_000,
        }
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();
        let was_creating = self.mode == Mode::Create;
        /* if the window is too small, do nothing, sorry! */
        if self.width > 20 && self.height > 20 {
            match self.mode {
                Mode::Create => self.create_step(&mut rng),
                Mode::Erase => self.erase_lines(),
                Mode::Draw => self.draw_lines(),
            }
        }
        /* C subtracts elapsed computation time from these targets; we return
        the full delay. speed=0-5, goal is 10,8,6,4,2,0 sec per screen */
        self.next_delay_us = if self.mode == Mode::Create && !was_creating {
            /* finished drawing: linger on the mosaic, as the C does */
            SLEEP as u64 * 1_000_000
        } else if self.mode == Mode::Create {
            /* mid-create slice; the C spends this time inside one blocking
            call, so keep the gaps between slices negligible */
            1_000
        } else {
            ((5 - SPEED) * (2 - self.dialog) * 100_000 / self.lpu.max(1) as i32).max(0) as u64
        };
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        if buffer.len() == self.pixels.len() {
            buffer.copy_from_slice(&self.pixels);
        } else {
            clear_buffer(buffer, BG);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        *self = Self::new(config);
    }

    fn clears_each_frame(&self) -> bool {
        false
    }

    fn frame_delay_us(&self) -> u64 {
        self.next_delay_us
    }
}
