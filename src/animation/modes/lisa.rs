//! Animated full-loop lisajous figures.
//
// Copyright (c) 1997 by Caleb Cullen.
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
// Rust port of xlockmore/modes/lisa.c.

use rand::Rng;
use std::f64::consts::PI;
use crate::animation::{AnimConfig, Animation, RenderPolicy};
use crate::animation::primitives::{draw_line, Color};

const XVMAX: i32 = 10;
const YVMAX: i32 = 10;
const NUMSTDFUNCS: usize = 10;
const MAXCYCLES: usize = 3;

struct LisaFunc {
    xcoeff: [f64; 2],
    ycoeff: [f64; 2],
    nx: usize,
    ny: usize,
    indx: usize,
}

const FUNCTIONS: [LisaFunc; NUMSTDFUNCS] = [
    LisaFunc { xcoeff: [1.0, 2.0], ycoeff: [1.0, 2.0], nx: 2, ny: 2, indx: 0 },
    LisaFunc { xcoeff: [1.0, 2.0], ycoeff: [1.0, 1.0], nx: 2, ny: 2, indx: 1 },
    LisaFunc { xcoeff: [1.0, 3.0], ycoeff: [1.0, 2.0], nx: 2, ny: 2, indx: 2 },
    LisaFunc { xcoeff: [1.0, 3.0], ycoeff: [1.0, 3.0], nx: 2, ny: 2, indx: 3 },
    LisaFunc { xcoeff: [2.0, 4.0], ycoeff: [1.0, 2.0], nx: 2, ny: 2, indx: 4 },
    LisaFunc { xcoeff: [1.0, 4.0], ycoeff: [1.0, 3.0], nx: 2, ny: 2, indx: 5 },
    LisaFunc { xcoeff: [1.0, 4.0], ycoeff: [1.0, 4.0], nx: 2, ny: 2, indx: 6 },
    LisaFunc { xcoeff: [1.0, 5.0], ycoeff: [1.0, 5.0], nx: 2, ny: 2, indx: 7 },
    LisaFunc { xcoeff: [2.0, 5.0], ycoeff: [2.0, 5.0], nx: 2, ny: 2, indx: 8 },
    LisaFunc { xcoeff: [1.0, 0.0], ycoeff: [1.0, 0.0], nx: 1, ny: 1, indx: 9 },
];

struct Lisajous {
    color_index: usize,
    radius: f64,
    dx: f64,
    dy: f64,
    nsteps: usize,
    nfuncs: usize,
    melting: usize,
    pistep: f64,
    center_x: f64,
    center_y: f64,
    functions: [usize; 2],
    points: Vec<(f64, f64)>,
}

pub struct Lisa {
    lisajous: Vec<Lisajous>,
    loopcount: usize,
    maxcycles: usize,
    width: u32,
    height: u32,
    ncolors: usize,
    delay_us: u64,
    additive: bool,
    config_size: i32,
}

impl Animation for Lisa {
    fn new(config: &AnimConfig) -> Self {
        let mut l = Lisa {
            lisajous: Vec::new(),
            loopcount: 0,
            maxcycles: 0,
            width: config.width,
            height: config.height,
            ncolors: config.ncolors.max(2) as usize,
            delay_us: config.delay_us,
            additive: true,
            config_size: config.size,
        };
        l.reset(config);
        l
    }

    fn tick(&mut self) {
        self.loopcount += 1;
        if self.loopcount > self.maxcycles {
            self.loopcount = 0;
            for l in &mut self.lisajous {
                l.functions[1] = (FUNCTIONS[l.functions[0]].indx + 1) % NUMSTDFUNCS;
                l.melting = l.nsteps - 1;
                l.nfuncs = 2;
            }
        }

        let mut rng = rand::rng();

        for l in &mut self.lisajous {
            l.center_x += l.dx;
            l.center_y += l.dy;

            Self::check_radius(&mut l.radius, l.center_x, l.center_y, self.width, self.height, self.config_size);

            if l.center_x - l.radius <= 0.0 {
                l.center_x = l.radius;
                l.dx = rng.random_range(0..XVMAX) as f64;
            } else if l.center_x + l.radius >= self.width as f64 {
                l.center_x = self.width as f64 - l.radius;
                l.dx = -(rng.random_range(0..XVMAX) as f64);
            }

            if l.center_y - l.radius <= 0.0 {
                l.center_y = l.radius;
                l.dy = rng.random_range(0..YVMAX) as f64;
            } else if l.center_y + l.radius >= self.height as f64 {
                l.center_y = self.height as f64 - l.radius;
                l.dy = -(rng.random_range(0..YVMAX) as f64);
            }

            Self::calc_points(l, self.loopcount, self.additive);

            l.color_index = (l.color_index + 1) % self.ncolors;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for l in &self.lisajous {
            let color = Color::from_hsl(l.color_index as f32 / self.ncolors as f32, 1.0, 0.5);
            for i in 0..l.nsteps {
                let p1 = l.points[i];
                let p2 = l.points[(i + 1) % l.nsteps];
                draw_line(
                    buffer,
                    width,
                    height,
                    p1.0 as i32,
                    p1.1 as i32,
                    p2.0 as i32,
                    p2.1 as i32,
                    color,
                );
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.ncolors = config.ncolors.max(2) as usize;
        self.delay_us = config.delay_us;
        self.config_size = config.size;
        self.loopcount = 0;

        let nlisajous = if config.count <= 0 { 1 } else { config.count as usize };
        let nsteps = if config.cycles <= 0 { 256 } else { config.cycles as usize };

        self.maxcycles = MAXCYCLES * nsteps - 1;
        
        let mut rng = rand::rng();
        
        self.lisajous.clear();
        for _ in 0..nlisajous {
            let mut radius = self.config_size as f64;
            let center_x = self.width as f64 / 2.0;
            let center_y = self.height as f64 / 2.0;

            Self::check_radius(&mut radius, center_x, center_y, self.width, self.height, self.config_size);

            let mut l = Lisajous {
                color_index: rng.random_range(0..self.ncolors),
                radius,
                dx: rng.random_range(0..XVMAX) as f64 + 1.0,
                dy: rng.random_range(0..YVMAX) as f64 + 1.0,
                nsteps,
                nfuncs: 1,
                melting: 0,
                pistep: 2.0 * PI / nsteps as f64,
                center_x,
                center_y,
                functions: [self.loopcount % NUMSTDFUNCS, 0],
                points: vec![(0.0, 0.0); nsteps],
            };
            
            // Calculate initial points
            Self::calc_points(&mut l, self.loopcount, self.additive);
            self.lisajous.push(l);
            self.loopcount += 1;
        }
        self.loopcount = 0;
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}

impl Lisa {
    fn get_radius(width: u32, height: u32) -> f64 {
        let min_dim = if width > height { height } else { width };
        (min_dim * 3 / 8) as f64
    }

    fn check_radius(radius: &mut f64, center_x: f64, center_y: f64, width: u32, height: u32, config_size: i32) {
        if config_size > 0 && (height / 2 > config_size as u32) && (width / 2 > config_size as u32) {
            *radius = config_size as f64;
        }
        if *radius < 0.0 || *radius > center_x || *radius > center_y {
            *radius = Self::get_radius(width, height);
        }
    }

    fn calc_points(l: &mut Lisajous, loopcount: usize, additive: bool) {
        let phase = loopcount % l.nsteps;
        if l.points.len() != l.nsteps {
            l.points = vec![(0.0, 0.0); l.nsteps];
        }

        for pctr in 0..l.nsteps {
            let phi = (pctr as f64 - phase as f64) * l.pistep;
            let theta = (pctr as f64 + phase as f64) * l.pistep;
            let mut xsum = 0.0;
            let mut ysum = 0.0;

            for fctr in (0..l.nfuncs).rev() {
                let func = &FUNCTIONS[l.functions[fctr]];
                let mut xprod = 0.0;
                let mut yprod = 0.0;

                if additive {
                    for xctr in (0..func.nx).rev() {
                        xprod += (func.xcoeff[xctr] * theta).sin();
                    }
                    for yctr in (0..func.ny).rev() {
                        yprod += (func.ycoeff[yctr] * phi).sin();
                    }

                    if l.melting > 0 {
                        if fctr > 0 {
                            xsum += xprod * (l.nsteps - l.melting) as f64 / l.nsteps as f64;
                            ysum += yprod * (l.nsteps - l.melting) as f64 / l.nsteps as f64;
                        } else {
                            xsum += xprod * l.melting as f64 / l.nsteps as f64;
                            ysum += yprod * l.melting as f64 / l.nsteps as f64;
                        }
                    } else {
                        xsum = xprod;
                        ysum = yprod;
                    }

                    if fctr == 0 {
                        xsum = xsum * l.radius / func.nx as f64;
                        ysum = ysum * l.radius / func.ny as f64;
                    }
                } else {
                    let mut x_p = if l.melting > 0 {
                        if fctr > 0 {
                            l.radius * (l.nsteps - l.melting) as f64 / l.nsteps as f64
                        } else {
                            l.radius * l.melting as f64 / l.nsteps as f64
                        }
                    } else {
                        l.radius
                    };
                    let mut y_p = x_p;

                    for xctr in (0..func.nx).rev() {
                        x_p *= (func.xcoeff[xctr] * theta).sin();
                    }
                    for yctr in (0..func.ny).rev() {
                        y_p *= (func.ycoeff[yctr] * phi).sin();
                    }
                    xsum += x_p;
                    ysum += y_p;
                }
            }

            if l.nfuncs > 1 && l.melting == 0 {
                xsum /= l.nfuncs as f64;
                ysum /= l.nfuncs as f64;
            }
            xsum += l.center_x;
            ysum += l.center_y;

            l.points[pctr] = (xsum.ceil(), ysum.ceil());
        }

        if l.melting > 0 {
            l.melting -= 1;
            if l.melting == 0 {
                l.nfuncs = 1;
                l.functions[0] = l.functions[1];
            }
        }
    }
}
