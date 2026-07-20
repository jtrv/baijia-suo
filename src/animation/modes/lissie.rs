//! The Lissajous worm.
//
// Copyright (c) 1996 by Alexander Jolk <ub9x@rz.uni-karlsruhe.de>
//
// Permission to use, copy, modify, and distribute this software and its
// documentation for any purpose and without fee is hereby granted,
// provided that the above copyright notice appear in all copies and that
// both that copyright notice and this permission notice appear in
// supporting documentation.
//
// This file is provided AS IS with no warranties of any kind.  The author
// shall have no liability with respect to the infringement of copyrights,
// trade secrets or any patents by this file or any part thereof.  In no
// event will the author be liable for any lost revenue or profits or
// other special, indirect and consequential damages.
//
// Rust port of xlockmore/modes/lissie.c.

use rand::Rng;
use std::f64::consts::PI;
use crate::animation::primitives::{draw_circle, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

const MAXLISSIELEN: usize = 100;
const MINLISSIELEN: usize = 10;
const MINLISSIES: i32 = 1;
const MINSIZE: i32 = 1;
const MINDT: f64 = 0.01;
const MAXDT: f64 = 0.15;
const REDRAWSTEP: usize = 3;

#[derive(Clone, Copy, Default)]
struct XPoint {
    x: i32,
    y: i32,
}

#[derive(Clone)]
struct LissieState {
    tx: f64,
    ty: f64,
    dtx: f64,
    dty: f64,
    xi: i32,
    yi: i32,
    ri: i32,
    rx: i32,
    ry: i32,
    len: usize,
    pos: usize,
    redrawing: bool,
    redrawpos: usize,
    loc: [XPoint; MAXLISSIELEN],
    color: usize,
}

impl Default for LissieState {
    fn default() -> Self {
        Self {
            tx: 0.0,
            ty: 0.0,
            dtx: 0.0,
            dty: 0.0,
            xi: 0,
            yi: 0,
            ri: 0,
            rx: 0,
            ry: 0,
            len: 0,
            pos: 0,
            redrawing: false,
            redrawpos: 0,
            loc: [XPoint::default(); MAXLISSIELEN],
            color: 0,
        }
    }
}

pub struct Lissie {
    width: u32,
    height: u32,
    nlissies: usize,
    lissies: Vec<LissieState>,
    loopcount: i32,
    cycles: i32,
    ncolors: usize,
    size: i32,
    delay_us: u64,
    // Pending draw operations accumulated by tick(), flushed by render()
    pending: Vec<DrawOp>,
}

#[derive(Clone)]
enum DrawOp {
    Erase { x: i32, y: i32, ri: i32 },
    Draw { x: i32, y: i32, ri: i32, color: usize },
}

impl Lissie {
    fn init_lissie(
        lissie: &mut LissieState,
        width: u32,
        height: u32,
        size: i32,
        ncolors: usize,
        rng: &mut impl Rng,
    ) {
        lissie.color = rng.random_range(0..ncolors);

        // Compute ri from size parameter, matching xlockmore verbatim.
        let min_dim = width.min(height) as i32;
        if size < -MINSIZE {
            let hi = (-size).min(MINSIZE.max(min_dim / 4));
            let range = (hi - MINSIZE + 1).max(1) as u32;
            lissie.ri = rng.random_range(0..range) as i32 + MINSIZE;
        } else if size < MINSIZE {
            if size == 0 {
                lissie.ri = MINSIZE.max(min_dim / 4);
            } else {
                lissie.ri = MINSIZE;
            }
        } else {
            lissie.ri = size.min(MINSIZE.max(min_dim / 4));
        }

        let w = width as i32;
        let h = height as i32;

        let xi_lo = w / 4 + lissie.ri;
        let xi_hi = w * 3 / 4 - lissie.ri;
        lissie.xi = if xi_lo < xi_hi {
            rng.random_range(xi_lo..=xi_hi)
        } else {
            xi_lo
        };

        let yi_lo = h / 4 + lissie.ri;
        let yi_hi = h * 3 / 4 - lissie.ri;
        lissie.yi = if yi_lo < yi_hi {
            rng.random_range(yi_lo..=yi_hi)
        } else {
            yi_lo
        };

        // xlockmore: rx = INTRAND(w/4, MIN(w - xi, xi)) - 2*ri — the 2*ri
        // shifts the PICKED value, not just the upper bound. Subtracting it
        // only from the bound (an earlier port bug) forced rx >= w/4, which
        // on wide screens stretched every worm into flat, screen-spanning
        // ellipses.
        let rx_lo = w / 4;
        let rx_hi = (w - lissie.xi).min(lissie.xi);
        lissie.rx = if rx_lo < rx_hi {
            rng.random_range(rx_lo..=rx_hi)
        } else {
            rx_lo
        } - 2 * lissie.ri;

        let ry_lo = h / 4;
        let ry_hi = (h - lissie.yi).min(lissie.yi);
        lissie.ry = if ry_lo < ry_hi {
            rng.random_range(ry_lo..=ry_hi)
        } else {
            ry_lo
        } - 2 * lissie.ri;

        lissie.len = rng.random_range(MINLISSIELEN..MAXLISSIELEN);
        lissie.pos = 0;
        lissie.redrawing = false;

        lissie.tx = rng.random::<f64>() * 2.0 * PI;
        lissie.ty = rng.random::<f64>() * 2.0 * PI;
        lissie.dtx = MINDT + rng.random::<f64>() * (MAXDT - MINDT);
        lissie.dty = MINDT + rng.random::<f64>() * (MAXDT - MINDT);

        for pt in lissie.loc.iter_mut() {
            pt.x = 0;
            pt.y = 0;
        }
    }

    fn draw_lissie_step(
        lissie: &mut LissieState,
        width: u32,
        height: u32,
        ncolors: usize,
        rng: &mut impl Rng,
        ops: &mut Vec<DrawOp>,
    ) {
        lissie.pos = (lissie.pos + 1) % MAXLISSIELEN;
        let p = lissie.pos;
        // xlockmore: oldp = (pos - len + MAXLISSIELEN) % MAXLISSIELEN
        // pos was already incremented, so this uses the post-increment value
        let oldp = (p + MAXLISSIELEN - lissie.len) % MAXLISSIELEN;

        lissie.tx += lissie.dtx;
        lissie.ty += lissie.dty;
        if lissie.tx > 2.0 * PI {
            lissie.tx -= 2.0 * PI;
        }
        if lissie.ty > 2.0 * PI {
            lissie.ty -= 2.0 * PI;
        }

        // Vary both speeds by up to 1%
        let fx: f64 = 0.99 + rng.random::<f64>() * 0.02;
        let fy: f64 = 0.99 + rng.random::<f64>() * 0.02;
        lissie.dtx *= fx;
        lissie.dty *= fy;
        lissie.dtx = lissie.dtx.clamp(MINDT, MAXDT);
        lissie.dty = lissie.dty.clamp(MINDT, MAXDT);

        lissie.loc[p].x = lissie.xi + (lissie.tx.sin() * lissie.rx as f64) as i32;
        lissie.loc[p].y = lissie.yi + (lissie.ty.sin() * lissie.ry as f64) as i32;

        // Erase tail point (oldp) in black
        let op = &lissie.loc[oldp];
        // xlockmore bounds check: x > 0 && y > 0 && x <= width && y <= height
        // This uses 1-based inclusive bounds from X11 convention; preserved verbatim.
        if op.x > 0 && op.y > 0 && op.x <= width as i32 && op.y <= height as i32 {
            ops.push(DrawOp::Erase {
                x: op.x,
                y: op.y,
                ri: lissie.ri,
            });
        }

        // Draw new head point (p) in current color
        let np = &lissie.loc[p];
        if np.x > 0 && np.y > 0 && np.x <= width as i32 && np.y <= height as i32 {
            ops.push(DrawOp::Draw {
                x: np.x,
                y: np.y,
                ri: lissie.ri,
                color: lissie.color,
            });
        }
        lissie.color = (lissie.color + 1) % ncolors;

        // Redraw mode: fill in the worm body incrementally (used after a reinit)
        if lissie.redrawing {
            lissie.redrawpos += 1;
            for _ in 0..REDRAWSTEP {
                let idx = (p + MAXLISSIELEN - lissie.redrawpos) % MAXLISSIELEN;
                let rp = &lissie.loc[idx];
                if rp.x > 0 && rp.y > 0 && rp.x <= width as i32 && rp.y <= height as i32 {
                    ops.push(DrawOp::Draw {
                        x: rp.x,
                        y: rp.y,
                        ri: lissie.ri,
                        color: lissie.color,
                    });
                }
                lissie.redrawpos += 1;
                if lissie.redrawpos >= lissie.len {
                    lissie.redrawing = false;
                    break;
                }
            }
        }
    }

    fn apply_op(
        buffer: &mut [u8],
        width: u32,
        height: u32,
        op: &DrawOp,
        ncolors: usize,
    ) {
        match op {
            DrawOp::Erase { x, y, ri } => {
                let black = Color::new(255, 0, 0, 0);
                if *ri < 2 {
                    put_pixel(buffer, width, height, *x, *y, black);
                } else {
                    draw_circle(buffer, width, height, *x, *y, *ri / 2, black);
                }
            }
            DrawOp::Draw { x, y, ri, color } => {
                let c = Color::from_hsl(*color as f32 / ncolors as f32, 1.0, 0.5);
                if *ri < 2 {
                    put_pixel(buffer, width, height, *x, *y, c);
                } else {
                    draw_circle(buffer, width, height, *x, *y, *ri / 2, c);
                }
            }
        }
    }
}

impl Animation for Lissie {
    fn new(config: &AnimConfig) -> Self {
        let mut l = Lissie {
            width: config.width,
            height: config.height,
            nlissies: 0,
            lissies: Vec::new(),
            loopcount: 0,
            cycles: 0,
            ncolors: 0,
            size: 0,
            delay_us: config.delay_us,
            pending: Vec::new(),
        };
        l.reset(config);
        l
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        self.loopcount += 1;
        if self.loopcount > self.cycles {
            // Reinitialize all worms — mirrors xlockmore's init_lissie call from draw_lissie
            let w = self.width;
            let h = self.height;
            let size = self.size;
            let ncolors = self.ncolors;
            for lissie in self.lissies.iter_mut() {
                Self::init_lissie(lissie, w, h, size, ncolors, &mut rng);
            }
            self.loopcount = 0;
            return;
        }

        let w = self.width;
        let h = self.height;
        let ncolors = self.ncolors;
        let mut ops = std::mem::take(&mut self.pending);
        ops.clear();
        for lissie in self.lissies.iter_mut() {
            Self::draw_lissie_step(lissie, w, h, ncolors, &mut rng, &mut ops);
        }
        self.pending = ops;
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for op in &self.pending {
            Self::apply_op(buffer, width, height, op, self.ncolors);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;
        self.delay_us = config.delay_us;
        self.loopcount = 0;

        // xlockmore DEFAULTS: ncolors=200
        self.ncolors = if config.ncolors <= 2 { 200 } else { config.ncolors as usize };

        // xlockmore DEFAULTS: cycles=20000
        self.cycles = if config.cycles <= 0 { 20000 } else { config.cycles };

        // xlockmore DEFAULTS: size=-200
        self.size = if config.size == 0 { -200 } else { config.size };

        // xlockmore DEFAULTS: count=1; negative count → random in [MINLISSIES, |count|]
        let raw_count = if config.count == 0 { 1 } else { config.count };
        self.nlissies = if raw_count < -MINLISSIES {
            let hi = (-raw_count - MINLISSIES + 1).max(1) as u32;
            (rng.random_range(0..hi) as i32 + MINLISSIES) as usize
        } else if raw_count < MINLISSIES {
            MINLISSIES as usize
        } else {
            raw_count as usize
        };

        self.lissies = vec![LissieState::default(); self.nlissies];
        self.pending = Vec::new();

        let w = self.width;
        let h = self.height;
        let size = self.size;
        let ncolors = self.ncolors;
        for lissie in self.lissies.iter_mut() {
            Self::init_lissie(lissie, w, h, size, ncolors, &mut rng);
        }
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
