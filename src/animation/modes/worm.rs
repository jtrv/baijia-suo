//! Draw wiggly worms.
//
// Copyright (c) 1991 by Patrick J. Naughton.
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
// Rust port of xlockmore/modes/worm.c.

use rand::Rng;
use std::f32::consts::PI;
use crate::animation::primitives::{Color, put_pixel};
use crate::animation::{AnimConfig, Animation};

const SEGMENTS: usize = 36;
const MINSIZE: i32 = 1;
const MINWORMS: i32 = 1;
const REDRAWSTEP: usize = 3;

static SINTAB: std::sync::OnceLock<[f32; SEGMENTS]> = std::sync::OnceLock::new();
static COSTAB: std::sync::OnceLock<[f32; SEGMENTS]> = std::sync::OnceLock::new();

fn get_sintab() -> &'static [f32; SEGMENTS] {
    SINTAB.get_or_init(|| {
        let mut t = [0.0f32; SEGMENTS];
        for i in 0..SEGMENTS {
            t[i] = (i as f32 * 2.0 * PI / SEGMENTS as f32).sin();
        }
        t
    })
}

fn get_costab() -> &'static [f32; SEGMENTS] {
    COSTAB.get_or_init(|| {
        let mut t = [0.0f32; SEGMENTS];
        for i in 0..SEGMENTS {
            t[i] = (i as f32 * 2.0 * PI / SEGMENTS as f32).cos();
        }
        t
    })
}

#[inline]
fn irint(x: f32) -> i32 {
    if x > 0.0 { (x + 0.5) as i32 } else { (x - 0.5) as i32 }
}

/// Fill a circsize×circsize square at (x, y) with color.
#[inline]
fn fill_rect(buffer: &mut [u8], width: u32, height: u32, x: i32, y: i32, size: i32, color: Color) {
    for dy in 0..size {
        for dx in 0..size {
            put_pixel(buffer, width, height, x + dx, y + dy, color);
        }
    }
}

#[derive(Clone)]
struct WormPoint {
    x: i32,
    y: i32,
}

struct WormStuff {
    circ: Vec<WormPoint>,
    dir: usize,
    tail: usize,
    x: i32,
    y: i32,
    redrawing: bool,
    redrawpos: usize,
}

pub struct Worm {
    xsize: i32,
    ysize: i32,
    wormlength: usize,
    nc: usize,
    nw: usize,
    circsize: i32,
    worms: Vec<WormStuff>,
    chromo: usize,
    colors: Vec<Color>,
    delay_us: u64,
    // Pending draw operations accumulated during tick: (color_idx, x, y)
    pending_draws: Vec<(usize, i32, i32)>,
    // Pending erase operations accumulated during tick: (x, y)
    pending_erases: Vec<(i32, i32)>,
}

impl Worm {
    fn make_colors(nc: usize) -> Vec<Color> {
        (0..nc)
            .map(|i| Color::from_hsl(i as f32 / nc as f32, 1.0, 0.5))
            .collect()
    }

    fn init_state(config: &AnimConfig) -> Self {
        let mut rng = rand::rng();

        let nc = config.ncolors.max(2) as usize;

        let count = if config.count == 0 { -20 } else { config.count };
        let nw = if count < -MINWORMS {
            rng.random_range(MINWORMS..=(-count - MINWORMS + 1)) as usize
        } else if count < MINWORMS {
            MINWORMS as usize
        } else {
            count as usize
        };

        let size = if config.size == 0 { -3 } else { config.size };
        let circsize = if size < -MINSIZE {
            rng.random_range(MINSIZE..=(-size - MINSIZE + 1))
        } else if size < MINSIZE {
            MINSIZE
        } else {
            size
        };

        let cycles = if config.cycles == 0 { 10 } else { config.cycles };
        let xsize = config.width as i32;
        let ysize = config.height as i32;
        let wormlength = (((xsize + ysize) as f64).sqrt() * cycles as f64 / 8.0) as usize;
        let wormlength = wormlength.max(1);

        let chromo = if nc > 2 { rng.random_range(0..nc) } else { 0 };

        let worms = (0..nw)
            .map(|_| {
                let circ = vec![WormPoint { x: xsize / 2, y: ysize / 2 }; wormlength];
                WormStuff {
                    circ,
                    dir: rng.random_range(0..SEGMENTS),
                    tail: 0,
                    x: xsize / 2,
                    y: ysize / 2,
                    redrawing: false,
                    redrawpos: 0,
                }
            })
            .collect();

        let colors = Self::make_colors(nc);

        Worm {
            xsize,
            ysize,
            wormlength,
            nc,
            nw,
            circsize,
            worms,
            chromo,
            colors,
            delay_us: config.delay_us,
            pending_draws: Vec::new(),
            pending_erases: Vec::new(),
        }
    }

    fn worm_doit(&mut self, which: usize, wcolor: usize) {
        let sintab = get_sintab();
        let costab = get_costab();
        let wormlength = self.wormlength;
        let xsize = self.xsize;
        let ysize = self.ysize;
        let circsize = self.circsize;
        let mut rng = rand::rng();

        // Advance tail pointer and record erase position for the slot being overwritten.
        {
            let ws = &mut self.worms[which];
            ws.tail += 1;
            if ws.tail == wormlength {
                ws.tail = 0;
            }
        }
        let (ex, ey) = {
            let ws = &self.worms[which];
            (ws.circ[ws.tail].x, ws.circ[ws.tail].y)
        };
        self.pending_erases.push((ex, ey));

        // Update direction and compute new head position.
        {
            let ws = &mut self.worms[which];
            if rng.random::<bool>() {
                ws.dir = (ws.dir + 1) % SEGMENTS;
            } else {
                ws.dir = (ws.dir + SEGMENTS - 1) % SEGMENTS;
            }
        }
        let (nx, ny) = {
            let ws = &self.worms[which];
            let nx = (ws.x + irint(circsize as f32 * costab[ws.dir]) + xsize) % xsize;
            let ny = (ws.y + irint(circsize as f32 * sintab[ws.dir]) + ysize) % ysize;
            (nx, ny)
        };
        {
            let ws = &mut self.worms[which];
            ws.circ[ws.tail].x = nx;
            ws.circ[ws.tail].y = ny;
            ws.x = nx;
            ws.y = ny;
        }
        self.pending_draws.push((wcolor, nx, ny));

        // Redraw incremental segments if a refresh was triggered.
        {
            let ws = &mut self.worms[which];
            if ws.redrawing {
                ws.redrawpos += 1;
            }
        }
        if self.worms[which].redrawing {
            let tail = self.worms[which].tail;
            let mut redrawpos = self.worms[which].redrawpos;
            for _ in 0..REDRAWSTEP {
                let k = (tail + wormlength - redrawpos) % wormlength;
                let rx = self.worms[which].circ[k].x;
                let ry = self.worms[which].circ[k].y;
                self.pending_draws.push((wcolor, rx, ry));

                redrawpos += 1;
                if redrawpos >= wormlength {
                    self.worms[which].redrawing = false;
                    break;
                }
            }
            self.worms[which].redrawpos = redrawpos;
        }
    }
}

impl Animation for Worm {
    fn new(config: &AnimConfig) -> Self {
        Self::init_state(config)
    }

    fn tick(&mut self) {
        self.pending_draws.clear();
        self.pending_erases.clear();

        for i in 0..self.nw {
            let wcolor = (i + self.chromo) % self.nc;
            self.worm_doit(i, wcolor);
        }

        self.chromo += 1;
        if self.chromo == self.nc {
            self.chromo = 0;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let black = Color::new(255, 0, 0, 0);
        for &(ex, ey) in &self.pending_erases {
            fill_rect(buffer, width, height, ex, ey, self.circsize, black);
        }
        for &(ci, nx, ny) in &self.pending_draws {
            let color = self.colors[ci];
            fill_rect(buffer, width, height, nx, ny, self.circsize, color);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        *self = Self::init_state(config);
    }

    fn clears_each_frame(&self) -> bool {
        false
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
