/* apollonian --- Apollonian Circles */

/*-
 * Copyright (c) 2000, 2001 by Allan R. Wilks <allan@research.att.com>.
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
 * 25-Jun-2001: Converted from C and Postscript code by David Bagley
 *              Original code by Allan R. Wilks <allan@research.att.com>.
 *
 * Rust port of xscreensaver's apollonian.c.  X fonts are replaced by a
 * built-in 5x7 pixel font (rendered at 2x) so the depth numbers and the
 * space labels stay visible.
 */

use rand::Rng;

use crate::animation::primitives::{clear_buffer, draw_circle, draw_line, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

const K: f64 = 2.15470053837925152902; /* 1+2/sqrt(3) */
const MAXBEND: i32 = 100; /* Do not want configurable by user since it will
                             take too much time if increased. */
const BIG: f64 = 7.0;

const EUCLIDEAN: usize = 0;
const SPHERICAL: usize = 1;
const HYPERBOLIC: usize = 2;

const SPACE_STRING: [&str; 3] = ["euclidean", "spherical", "hyperbolic"];

#[derive(Debug, Clone, Copy, Default)]
struct Circle {
    e: f64,      /* euclidean bend */
    s: f64,      /* spherical bend */
    h: f64,      /* hyperbolic bend */
    x: f64,      /* euclidean bend times euclidean position */
    y: f64,
}

const fn c5(e: f64, s: f64, h: f64, x: f64, y: f64) -> Circle {
    Circle { e, s, h, x, y }
}

const DELTA: f64 = 2.154700538; /* ((3+2*sqrt(3))/3) */
const ALPHA: f64 = 2.618033989; /* ((3+sqrt(5))/2) */
const BETA: f64 = 2.890053638; /* (PHI+sqrt(PHI)) */

static EXAMPLES: [[Circle; 4]; 4] = [
    /* double semi-bounded */
    [
        c5(0.0, 0.0, 0.0, 0.0, 1.0),
        c5(0.0, 0.0, 0.0, 0.0, -1.0),
        c5(1.0, 1.0, 1.0, -1.0, 0.0),
        c5(1.0, 1.0, 1.0, 1.0, 0.0),
    ],
    /* 3 fold symmetric bounded (x, y calculated later) */
    [
        c5(-1.0, -1.0, -1.0, 0.0, 0.0),
        c5(DELTA, DELTA, DELTA, 1.0, 0.0),
        c5(DELTA, DELTA, DELTA, 1.0, -1.0),
        c5(DELTA, DELTA, DELTA, -1.0, 1.0),
    ],
    /* semi-bounded (x, y calculated later) */
    [
        c5(1.0, 1.0, 1.0, 0.0, 0.0),
        c5(0.0, 0.0, 0.0, 0.0, -1.0),
        c5(
            1.0 / (ALPHA * ALPHA),
            1.0 / (ALPHA * ALPHA),
            1.0 / (ALPHA * ALPHA),
            -1.0,
            0.0,
        ),
        c5(1.0 / ALPHA, 1.0 / ALPHA, 1.0 / ALPHA, -1.0, 0.0),
    ],
    /* unbounded (x, y calculated later) */
    [
        c5(1.0, 1.0, 1.0, 0.0, 0.0),
        c5(
            1.0 / (BETA * BETA * BETA),
            1.0 / (BETA * BETA * BETA),
            1.0 / (BETA * BETA * BETA),
            1.0,
            0.0,
        ),
        c5(
            1.0 / (BETA * BETA),
            1.0 / (BETA * BETA),
            1.0 / (BETA * BETA),
            1.0,
            0.0,
        ),
        c5(1.0 / BETA, 1.0 / BETA, 1.0 / BETA, 1.0, 0.0),
    ],
];

const PREDEF_CIRCLE_GAMES: usize = 4;

#[derive(Debug, Clone, Copy)]
struct Quadruple {
    a: i32,
    b: i32,
    c: i32,
    d: i32,
}

fn gcd(mut a: i32, mut b: i32) -> i32 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

fn isqrt(n: i32) -> i32 {
    if n < 0 {
        return -1;
    }
    let y = ((n as f64).sqrt() + 0.5) as i32;
    if n == y * y {
        y
    } else {
        -1
    }
}

/* Enumerate root integer Descartes quadruples (see the header comment of
   the C source for the number theory). */
fn dquad(n: usize) -> Vec<Quadruple> {
    let mut quad = Vec::with_capacity(n);
    for a in 0..MAXBEND {
        let bb = (K * a as f64) as i32;
        for b in (a + 1)..=bb {
            let cc = (((a + b) * (a + b)) as f64 / (4.0 * (b - a) as f64)) as i32;
            for c in b..=cc {
                let d = isqrt(b * c - a * (b + c));
                if d >= 0 && gcd(a, gcd(b, c)) <= 1 {
                    quad.push(Quadruple {
                        a: -a,
                        b,
                        c,
                        d: -a + b + c - 2 * d,
                    });
                    if quad.len() >= n {
                        return quad;
                    }
                }
            }
        }
    }
    while quad.len() < n {
        quad.push(Quadruple { a: -1, b: 2, c: 2, d: 3 });
    }
    quad
}

fn is_quad(a: i32, b: i32, c: i32, d: i32) -> bool {
    let s = a + b + c + d;
    2 * (a * a + b * b + c * c + d * d) == s * s
}

fn is_tangent(e1: i32, p1: i32, q1: i32, e2: i32, p2: i32, q2: i32) -> bool {
    let dx = p1 * e2 - p2 * e1;
    let dy = q1 * e2 - q2 * e1;
    let s = e1 + e2;
    dx * dx + dy * dy == s * s
}

fn iflor(a: i32, b: i32) -> i32 {
    if b == 0 {
        return 0;
    }
    if a % b == 0 {
        return a / b;
    }
    let q = a.abs() / b.abs();
    if (a < 0) ^ (b < 0) {
        -q - 1
    } else {
        q
    }
}

fn iceil(a: i32, b: i32) -> i32 {
    if b == 0 {
        return 0;
    }
    if a % b == 0 {
        return a / b;
    }
    let q = a.abs() / b.abs();
    if (a < 0) ^ (b < 0) {
        -q
    } else {
        1 + q
    }
}

fn geom(geometry: usize, e: i32, p: i32, q: i32) -> f64 {
    let g = match geometry {
        SPHERICAL => -1,
        HYPERBOLIC => 1,
        _ => 0,
    };
    if g != 0 {
        return ((e * e) as f64 + (1.0 - (p * p + q * q) as f64) * g as f64) / (2.0 * e as f64);
    }
    e as f64
}

/* H(z): parity condition; UNIT(z): outer circle center in the unit square. */
fn h_odd(e: i32, p: i32, q: i32) -> bool {
    (e * e + p * p + q * q) % 2 != 0
}

fn unit(e: i32, p: i32, q: i32) -> bool {
    (e.abs() - 1) * (e.abs() - 1) >= p * p + q * q
}

/*
 * Given a Descartes quadruple of bends (a,b,c,d), with a<0, find integer
 * centers so that the packing can be labelled with integer spherical and
 * hyperbolic labels.  Exhaustive search (macros FOR/H/UNIT/T/B expanded).
 */
fn cquad(c1: &mut Circle, c2: &mut Circle, c3: &mut Circle, c4: &mut Circle) {
    let ea = c1.e as i32;
    let eb = c2.e as i32;
    let ec = c3.e as i32;
    let ed = c4.e as i32;

    if ea >= 0 || !is_quad(ea, eb, ec, ed) {
        return; /* C prints a diagnostic here */
    }
    let (lopa, hipa) = (ea, 0);
    let (loqa, hiqa) = (ea, 0);
    for pa in lopa..=hipa {
        for qa in loqa..=hiqa {
            /* B(b); B(c); B(d) */
            let lopb = iceil(eb * (pa + 1), ea) - 1;
            let hipb = iflor(eb * (pa - 1), ea) - 1;
            let loqb = iceil(eb * (qa + 1), ea) - 1;
            let hiqb = iflor(eb * (qa - 1), ea) - 1;
            let lopc = iceil(ec * (pa + 1), ea) - 1;
            let hipc = iflor(ec * (pa - 1), ea) - 1;
            let loqc = iceil(ec * (qa + 1), ea) - 1;
            let hiqc = iflor(ec * (qa - 1), ea) - 1;
            let lopd = iceil(ed * (pa + 1), ea) - 1;
            let hipd = iflor(ed * (pa - 1), ea) - 1;
            let loqd = iceil(ed * (qa + 1), ea) - 1;
            let hiqd = iflor(ed * (qa - 1), ea) - 1;
            if h_odd(ea, pa, qa) && unit(ea, pa, qa) {
                for pb in lopb..=hipb {
                    for qb in loqb..=hiqb {
                        if h_odd(eb, pb, qb) && is_tangent(ea, pa, qa, eb, pb, qb) {
                            for pc in lopc..=hipc {
                                for qc in loqc..=hiqc {
                                    if h_odd(ec, pc, qc)
                                        && is_tangent(ea, pa, qa, ec, pc, qc)
                                        && is_tangent(eb, pb, qb, ec, pc, qc)
                                    {
                                        for pd in lopd..=hipd {
                                            for qd in loqd..=hiqd {
                                                if h_odd(ed, pd, qd)
                                                    && is_tangent(ea, pa, qa, ed, pd, qd)
                                                    && is_tangent(eb, pb, qb, ed, pd, qd)
                                                    && is_tangent(ec, pc, qc, ed, pd, qd)
                                                {
                                                    c1.s = geom(SPHERICAL, ea, pa, qa);
                                                    c1.h = geom(HYPERBOLIC, ea, pa, qa);
                                                    c2.s = geom(SPHERICAL, eb, pb, qb);
                                                    c2.h = geom(HYPERBOLIC, eb, pb, qb);
                                                    c3.s = geom(SPHERICAL, ec, pc, qc);
                                                    c3.h = geom(HYPERBOLIC, ec, pc, qc);
                                                    c4.s = geom(SPHERICAL, ed, pd, qd);
                                                    c4.h = geom(HYPERBOLIC, ed, pd, qd);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn randomize_c(randomize: u32, c: &mut Circle) {
    if randomize / 2 != 0 {
        std::mem::swap(&mut c.x, &mut c.y);
    }
    if randomize % 2 != 0 {
        c.x = -c.x;
        c.y = -c.y;
    }
}

/* ---- tiny built-in font (5x7 bitmaps drawn at 2x), replacing Xft ---- */

const FONT_SCALE: i32 = 2;
const FONT_ASCENT: i32 = 7 * FONT_SCALE; /* stand-in for cp->font->ascent */
const FONT_DESCENT: i32 = 2 * FONT_SCALE; /* stand-in for cp->font->descent */
const CHAR_WIDTH: i32 = 6 * FONT_SCALE;

fn glyph(ch: char) -> Option<[u8; 7]> {
    Some(match ch {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        'a' => [0x00, 0x00, 0x0E, 0x01, 0x0F, 0x11, 0x0F],
        'b' => [0x10, 0x10, 0x16, 0x19, 0x11, 0x11, 0x1E],
        'c' => [0x00, 0x00, 0x0E, 0x10, 0x10, 0x11, 0x0E],
        'd' => [0x01, 0x01, 0x0D, 0x13, 0x11, 0x11, 0x0F],
        'e' => [0x00, 0x00, 0x0E, 0x11, 0x1F, 0x10, 0x0E],
        'h' => [0x10, 0x10, 0x16, 0x19, 0x11, 0x11, 0x11],
        'i' => [0x04, 0x00, 0x0C, 0x04, 0x04, 0x04, 0x0E],
        'l' => [0x0C, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'n' => [0x00, 0x00, 0x16, 0x19, 0x11, 0x11, 0x11],
        'o' => [0x00, 0x00, 0x0E, 0x11, 0x11, 0x11, 0x0E],
        'p' => [0x00, 0x00, 0x1E, 0x11, 0x1E, 0x10, 0x10],
        'r' => [0x00, 0x00, 0x16, 0x19, 0x10, 0x10, 0x10],
        's' => [0x00, 0x00, 0x0E, 0x10, 0x0E, 0x01, 0x1E],
        'u' => [0x00, 0x00, 0x11, 0x11, 0x11, 0x13, 0x0D],
        'y' => [0x00, 0x00, 0x11, 0x11, 0x0F, 0x01, 0x0E],
        _ => return None,
    })
}

/* Draws text with the baseline at (x, y), like XftDrawStringUtf8. */
fn draw_text(buffer: &mut [u8], width: u32, height: u32, x: i32, y: i32, text: &str, color: Color) {
    let mut cx = x;
    for ch in text.chars() {
        if let Some(rows) = glyph(ch) {
            for (ry, row) in rows.iter().enumerate() {
                for bx in 0..5 {
                    if row & (0x10 >> bx) != 0 {
                        for sy in 0..FONT_SCALE {
                            for sx in 0..FONT_SCALE {
                                put_pixel(
                                    buffer,
                                    width,
                                    height,
                                    cx + bx * FONT_SCALE + sx,
                                    y - FONT_ASCENT + ry as i32 * FONT_SCALE + sy,
                                    color,
                                );
                            }
                        }
                    }
                }
            }
        }
        cx += CHAR_WIDTH;
    }
}

fn text_width(text: &str) -> i32 {
    text.chars().count() as i32 * CHAR_WIDTH
}

/* sprintf(string, "%g", g) for the (integral in practice) labels */
fn fmt_g(g: f64) -> String {
    if g == g.trunc() && g.abs() < 1e15 {
        format!("{}", g as i64)
    } else {
        format!("{}", g)
    }
}

fn fill_circle(buffer: &mut [u8], width: u32, height: u32, cx: i32, cy: i32, r: i32, color: Color) {
    for dy in -r..=r {
        let dx = ((r * r - dy * dy) as f64).sqrt() as i32;
        for x in (cx - dx)..=(cx + dx) {
            put_pixel(buffer, width, height, x, cy + dy, color);
        }
    }
}

pub struct Apollonian {
    buffer: Vec<u8>,
    width: u32,
    height: u32,
    delay_us: u64,

    size: i32,
    offset_x: i32,
    offset_y: i32,
    geometry: usize,
    c1: Circle,
    c2: Circle,
    c3: Circle,
    c4: Circle,
    color_offset: i32,
    count: usize,
    label: bool,
    altgeom: bool,
    quad: Vec<Quadruple>,
    time: i32,
    game: usize,
    cycles: i32,
    ncolors: i32,
}

impl Apollonian {
    fn pixel(&self, i: i32) -> Color {
        let n = self.ncolors.max(2);
        Color::from_hsl(i.rem_euclid(n) as f32 / n as f32, 1.0, 0.5)
    }

    /* p(): draw one circle plus its labels */
    fn p(&mut self, c: Circle) {
        let g0 = match self.geometry {
            SPHERICAL => c.s,
            HYPERBOLIC => c.h,
            _ => c.e,
        };
        let size = self.size as f64;
        let (w, h) = (self.width, self.height);
        let off = self.color_offset as f64;
        let n = self.ncolors.max(2);

        if c.e < 0.0 {
            /* outer bounding circle: outline */
            let g = if g0 < 0.0 { -g0 } else { g0 };
            let pix = self.pixel((((g + off) * g) as i32).rem_euclid(n));
            let xi = (size * (-self.c1.e) * (c.x - 1.0) / (-2.0 * c.e)
                + size / 2.0
                + self.offset_x as f64) as i32;
            let yi = (size * (-self.c1.e) * (c.y - 1.0) / (-2.0 * c.e)
                + size / 2.0
                + self.offset_y as f64) as i32;
            let di = (self.c1.e * size / c.e) as i32;
            draw_circle(&mut self.buffer, w, h, xi + di / 2, yi + di / 2, di / 2, pix);
            if !self.label {
                return;
            }
            let string = fmt_g(if g == 0.0 { 0.0 } else { -g });
            draw_text(
                &mut self.buffer,
                w,
                h,
                (size * c.x / (2.0 * c.e)) as i32 + self.offset_x + FONT_ASCENT * 2,
                (size * c.y / (2.0 * c.e)) as i32 + FONT_ASCENT * 4,
                &string,
                pix,
            );
            draw_text(
                &mut self.buffer,
                w,
                h,
                (size * c.x / (2.0 * c.e) + self.offset_x as f64) as i32 + FONT_ASCENT * 2,
                (size * c.y / (2.0 * c.e) + h as f64 - (FONT_ASCENT * 4) as f64) as i32,
                SPACE_STRING[self.geometry],
                pix,
            );
            return;
        }

        let pix = self.pixel((((g0 + off) * g0) as i32).rem_euclid(n));
        if c.e == 0.0 {
            /* a straight line (circle of curvature 0) */
            if c.x == 0.0 && c.y != 0.0 {
                let y = ((c.y + 1.0) * size / 2.0 + self.offset_y as f64) as i32;
                draw_line(&mut self.buffer, w, h, 0, y, w as i32, y, pix);
            } else if c.y == 0.0 && c.x != 0.0 {
                let x = ((c.x + 1.0) * size / 2.0 + self.offset_x as f64) as i32;
                draw_line(&mut self.buffer, w, h, x, 0, x, h as i32, pix);
            }
            return;
        }

        let e = if self.c1.e >= 0.0 { 1.0 } else { -self.c1.e };
        let xi =
            (size * e * (c.x - 1.0) / (2.0 * c.e) + size / 2.0 + self.offset_x as f64) as i32;
        let yi =
            (size * e * (c.y - 1.0) / (2.0 * c.e) + size / 2.0 + self.offset_y as f64) as i32;
        let di = (e * size / c.e) as i32;
        fill_circle(&mut self.buffer, w, h, xi + di / 2, yi + di / 2, di / 2, pix);
        if !self.label {
            return;
        }
        let label_pix = self.pixel(((((g0 + off) * g0) as i32) + n / 2).rem_euclid(n));
        if c.e < e * size / ((FONT_ASCENT + FONT_DESCENT) * 2) as f64 && g0 < 1000.0 {
            let string = fmt_g(g0);
            draw_text(
                &mut self.buffer,
                w,
                h,
                (size * e * c.x / (2.0 * c.e) + size / 2.0 + self.offset_x as f64) as i32
                    - text_width(&string) / 2,
                (size * e * c.y / (2.0 * c.e) + size / 2.0 + self.offset_y as f64) as i32
                    + FONT_ASCENT / 2,
                &string,
                label_pix,
            );
        }
    }

    /* f(): recursively add the circle tangent to c1, c2, c3 other than c4 */
    fn f(&mut self, c1: Circle, c2: Circle, c3: Circle, c4: Circle) {
        let e = (if self.c1.e >= 0.0 { 1.0 } else { -self.c1.e }) as i32;
        let c = Circle {
            e: 2.0 * (c1.e + c2.e + c3.e) - c4.e,
            s: 2.0 * (c1.s + c2.s + c3.s) - c4.s,
            h: 2.0 * (c1.h + c2.h + c3.h) - c4.h,
            x: 2.0 * (c1.x + c2.x + c3.x) - c4.x,
            y: 2.0 * (c1.y + c2.y + c3.y) - c4.y,
        };
        if c.e == 0.0
            || c.e > (self.size * e) as f64
            || c.x / c.e > BIG
            || c.y / c.e > BIG
            || c.x / c.e < -BIG
            || c.y / c.e < -BIG
        {
            return;
        }
        self.p(c);
        self.f(c2, c3, c, c1);
        self.f(c1, c3, c, c2);
        self.f(c1, c2, c, c3);
    }

    /* init_apollonian */
    fn init(&mut self) {
        let mut rng = rand::rng();

        self.size = ((self.width.min(self.height) as i32) - 1).max(1);
        self.offset_x = (self.width as i32 - self.size) / 2;
        self.offset_y = (self.height as i32 - self.size) / 2;
        self.color_offset = rng.random_range(0..self.ncolors.max(2));

        self.label = true; /* DEF_LABEL */
        self.altgeom = self.label; /* DEF_ALTGEOM, anded with label */

        self.game = rng.random_range(0..PREDEF_CIRCLE_GAMES + self.count);
        self.geometry = if self.game != 0 && self.altgeom {
            rng.random_range(0..3)
        } else {
            EUCLIDEAN
        };

        if self.game < PREDEF_CIRCLE_GAMES {
            self.c1 = EXAMPLES[self.game][0];
            self.c2 = EXAMPLES[self.game][1];
            self.c3 = EXAMPLES[self.game][2];
            self.c4 = EXAMPLES[self.game][3];
            /* do not label non int */
            self.label = self.label && self.c4.e == (self.c4.e as i32) as f64;
        } else {
            /* uses results of dquad, all int */
            let i = self.game - PREDEF_CIRCLE_GAMES;
            self.c1.e = self.quad[i].a as f64;
            self.c2.e = self.quad[i].b as f64;
            self.c3.e = self.quad[i].c as f64;
            self.c4.e = self.quad[i].d as f64;
            if self.geometry != EUCLIDEAN {
                let (mut c1, mut c2, mut c3, mut c4) = (self.c1, self.c2, self.c3, self.c4);
                cquad(&mut c1, &mut c2, &mut c3, &mut c4);
                self.c1 = c1;
                self.c2 = c2;
                self.c3 = c3;
                self.c4 = c4;
            }
        }
        self.time = 0;
        clear_buffer(&mut self.buffer, Color::new(255, 0, 0, 0));
        if self.game != 0 {
            if self.c1.e == 0.0 || self.c1.e == -self.c2.e {
                return;
            }
            self.c1.x = 0.0;
            self.c1.y = 0.0;
            self.c2.x = -(self.c1.e + self.c2.e) / self.c1.e;
            self.c2.y = 0.0;
            let mut q123 = (self.c1.e * self.c2.e
                + self.c1.e * self.c3.e
                + self.c2.e * self.c3.e)
                .sqrt();
            self.c3.x =
                (self.c1.e * self.c1.e - q123 * q123) / (self.c1.e * (self.c1.e + self.c2.e));
            self.c3.y = -2.0 * q123 / (self.c1.e + self.c2.e);
            q123 += -self.c1.e - self.c2.e;
            self.c4.x =
                (self.c1.e * self.c1.e - q123 * q123) / (self.c1.e * (self.c1.e + self.c2.e));
            self.c4.y = -2.0 * q123 / (self.c1.e + self.c2.e);
        }
        if rng.random::<bool>() {
            self.c3.y = -self.c3.y;
            self.c4.y = -self.c4.y;
        }
        let i = rng.random_range(0..4u32);
        randomize_c(i, &mut self.c1);
        randomize_c(i, &mut self.c2);
        randomize_c(i, &mut self.c3);
        randomize_c(i, &mut self.c4);
    }
}

impl Animation for Apollonian {
    fn new(config: &AnimConfig) -> Self {
        let mut cp = Apollonian {
            buffer: Vec::new(),
            width: 0,
            height: 0,
            delay_us: 1_000_000, /* hack default *delay: 1000000 */
            size: 1,
            offset_x: 0,
            offset_y: 0,
            geometry: EUCLIDEAN,
            c1: Circle::default(),
            c2: Circle::default(),
            c3: Circle::default(),
            c4: Circle::default(),
            color_offset: 0,
            count: 0,
            label: true,
            altgeom: true,
            quad: Vec::new(),
            time: 0,
            game: 0,
            cycles: 20,
            ncolors: 64,
        };
        cp.reset(config);
        cp
    }

    fn tick(&mut self) {
        if self.time < 5 {
            match self.time {
                0 => {
                    let (c1, c2, c3, c4) = (self.c1, self.c2, self.c3, self.c4);
                    self.p(c1);
                    self.p(c2);
                    self.p(c3);
                    self.p(c4);
                }
                1 => self.f(self.c1, self.c2, self.c3, self.c4),
                2 => self.f(self.c1, self.c2, self.c4, self.c3),
                3 => self.f(self.c1, self.c3, self.c4, self.c2),
                _ => self.f(self.c2, self.c3, self.c4, self.c1),
            }
        }
        self.time += 1;
        if self.time > self.cycles {
            self.init();
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if width == self.width && height == self.height && buffer.len() == self.buffer.len() {
            buffer.copy_from_slice(&self.buffer);
        } else {
            /* size mismatch (shouldn't happen; reset handles resizes) */
            let src_stride = (self.width * 4) as usize;
            let dst_stride = (width * 4) as usize;
            let rows = height.min(self.height) as usize;
            let row_bytes = src_stride.min(dst_stride);
            for y in 0..rows {
                buffer[y * dst_stride..y * dst_stride + row_bytes]
                    .copy_from_slice(&self.buffer[y * src_stride..y * src_stride + row_bytes]);
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.buffer = vec![0u8; (config.width * config.height * 4) as usize];
        self.delay_us = 1_000_000;
        self.ncolors = if config.ncolors > 1 { config.ncolors } else { 64 };
        self.cycles = if config.cycles > 0 { config.cycles } else { 20 };

        /* MI_COUNT default 64; the quadruple table is only (re)computed
           when the requested count changes, as in the C init path */
        let count = if config.count != 0 {
            config.count.unsigned_abs() as usize
        } else {
            64
        };
        if count != self.count {
            self.count = count;
            self.quad = dquad(count);
        }

        self.init();
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
