/* Copyright (c) 2003 Levi Burton <donburton@sbcglobal.net>
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Rust port of xscreensaver's popsquares.c for baijia-suo.
 */

use crate::animation::primitives::{hsv_to_rgb, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};
use rand::Rng;

// defaults from popsquares.c: *delay: 25000, *subdivision: 5, *border: 1,
// *ncolors: 128, *twitch: False, .background: #0000FF, .foreground: #00008B
const DELAY_US: u64 = 25_000;
const SUBDIVISION: i32 = 5;
const BORDER: i32 = 1;
const NCOLORS: usize = 128;
const TWITCH: bool = false;
const FG: (u16, u16, u16) = (0x0000, 0x0000, 0x8B8B); // #00008B
const BG: (u16, u16, u16) = (0x0000, 0x0000, 0xFFFF); // #0000FF

fn rgb_to_hsv(r: u16, g: u16, b: u16) -> (i32, f64, f64) {
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

/* port of utils/colors.c make_color_ramp (color computation only) */
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
    let n = if closed { total / 2 + 1 } else { total };
    let dh = (h2 - h1) as f64 / n as f64;
    let ds = (s2 - s1) / n as f64;
    let dv = (v2 - v1) / n as f64;
    let mut out = vec![Color::new(255, 0, 0, 0); total];
    for i in 0..n.min(total) {
        let (r, g, b) = hsv_to_rgb(
            (h1 as f64 + i as f64 * dh) as i32,
            s1 + i as f64 * ds,
            v1 + i as f64 * dv,
        );
        out[i] = Color::new(255, (r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8);
    }
    if closed {
        for i in n..total {
            out[i] = out[total - i];
        }
    }
    out
}

#[derive(Clone, Copy)]
struct Square {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: usize,
}

pub struct PopSquares {
    gw: i32,
    gh: i32,
    squares: Vec<Square>,
    colors: Vec<Color>,
    ncolors: usize,
    bg: Color,
}

fn randomize_square_colors(squares: &mut [Square], ncolors: usize, rng: &mut impl Rng) {
    for s in squares.iter_mut() {
        s.color = rng.random_range(0..ncolors);
    }
}

impl PopSquares {
    /* port of popsquares_reshape */
    fn reshape(&mut self, width: u32, height: u32) {
        let mut rng = rand::rng();
        let w = width as i32;
        let h = height as i32;
        let mut s = SUBDIVISION;

        if w < 100 || h < 100 {
            /* tiny window */
            let ss = w.min(h);
            s = (ss / 15).max(1);
        }

        let (subx, suby) = if w > h * 5 || h > w * 5 {
            /* weird aspect ratio */
            let r = w as f64 / h as f64;
            if r > 1.0 {
                ((s as f64 * r) as i32, s)
            } else {
                (s, (s as f64 / r) as i32)
            }
        } else {
            (s, s)
        };

        let sw = w / subx.max(1);
        let sh = h / suby.max(1);
        self.gw = if sw != 0 { w / sw } else { 0 };
        self.gh = if sh != 0 { h / sh } else { 0 };
        let nsquares = (self.gw * self.gh).max(0) as usize;

        self.squares = Vec::with_capacity(nsquares);
        for y in 0..self.gh {
            for x in 0..self.gw {
                self.squares.push(Square {
                    x: x * sw,
                    y: y * sh,
                    w: sw,
                    h: sh,
                    color: 0,
                });
            }
        }
        randomize_square_colors(&mut self.squares, self.ncolors, &mut rng);
    }
}

impl Animation for PopSquares {
    fn new(config: &AnimConfig) -> Self {
        let (h1, s1, v1) = rgb_to_hsv(FG.0, FG.1, FG.2);
        let (h2, s2, v2) = rgb_to_hsv(BG.0, BG.1, BG.2);
        let colors = make_color_ramp(h1, s1, v1, h2, s2, v2, NCOLORS, true);
        let mut ps = PopSquares {
            gw: 0,
            gh: 0,
            squares: Vec::new(),
            colors,
            ncolors: NCOLORS,
            bg: Color::new(255, 0x00, 0x00, 0xFF),
        };
        ps.reshape(config.width, config.height);
        ps
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();
        // popsquares_draw advances each square's color after painting it
        for i in 0..self.squares.len() {
            self.squares[i].color += 1;
            if self.squares[i].color == self.ncolors {
                if TWITCH && rng.random_range(0..4_u32) == 0 {
                    randomize_square_colors(&mut self.squares, self.ncolors, &mut rng);
                } else {
                    self.squares[i].color = rng.random_range(0..self.ncolors);
                }
            }
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        // the window background (#0000FF) shows through the 1px borders
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                put_pixel(buffer, width, height, x, y, self.bg);
            }
        }
        for s in &self.squares {
            let color = self.colors[s.color.min(self.ncolors - 1)];
            let w = if BORDER != 0 { s.w - BORDER } else { s.w };
            let h = if BORDER != 0 { s.h - BORDER } else { s.h };
            for y in s.y..(s.y + h).min(height as i32) {
                for x in s.x..(s.x + w).min(width as i32) {
                    put_pixel(buffer, width, height, x, y, color);
                }
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.reshape(config.width, config.height);
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        DELAY_US
    }
}
