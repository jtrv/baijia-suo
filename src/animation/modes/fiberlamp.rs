//! A Fiber Optic Lamp.
//
// Copyright (c) 2005 by Tim Auckland <tda10.geo AT yahoo.com>
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
// Rust port of xlockmore/modes/fiberlamp.c.

use rand::RngExt;
use std::f64::consts::PI;

use crate::animation::primitives::{draw_line, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const SPREAD: f64 = 30.0;
const NODES: usize = 20;
const DT: f64 = 0.5;
const PY: f64 = 0.12;
const DAMPING: f64 = 0.055;

#[inline(always)]
fn len(a: usize) -> f64 {
    if a < NODES - 3 {
        1.0 / (NODES as f64 - 2.5)
    } else {
        0.25 / (NODES as f64 - 2.5)
    }
}

#[derive(Clone, Default)]
struct NodeStruct {
    phi: f64,
    phidash: f64,
    eta: f64,
    etadash: f64,
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Default)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Clone)]
struct FiberStruct {
    node: Vec<NodeStruct>, // length NODES
    draw: Vec<Point>,      // length NODES
}

#[derive(Clone)]
struct DrawOp {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: Color,
}

pub struct Fiberlamp {
    width: u32,
    height: u32,
    psi: f64,
    dpsi: f64,
    count: i32,
    cycles: i32,
    nfibers: usize,
    cx: f64,
    fibers: Vec<FiberStruct>,
    delay_us: u64,

    ncolors: i32,
    bright: Color,
    medium: Color,
    dim: Color,

    ops: Vec<DrawOp>,
}

impl Fiberlamp {
    fn change(&mut self, rng: &mut impl RngExt) {
        // Knock the lamp
        self.cx = rng.random_range(0.0..0.25) - 0.125;
        self.count = 0;
    }
}

impl Animation for Fiberlamp {
    fn new(config: &AnimConfig) -> Self {
        let mut f = Fiberlamp {
            width: config.width,
            height: config.height,
            psi: 0.0,
            dpsi: 0.0,
            count: 0,
            cycles: 0,
            nfibers: 0,
            cx: 0.0,
            fibers: Vec::new(),
            delay_us: 0,
            ncolors: 0,
            bright: Color::new(255, 0, 0, 0),
            medium: Color::new(255, 0, 0, 0),
            dim: Color::new(255, 0, 0, 0),
            ops: Vec::new(),
        };
        f.reset(config);
        f
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        let cx = (self.width / 2) as i32;
        let cy = self.height as i32;

        self.psi += self.dpsi; // turn colorwheel

        // One bubble pass is enough as the order only changes slowly
        for i in 1..self.nfibers {
            if self.fibers[i - 1].node[NODES - 1].z > self.fibers[i].node[NODES - 1].z {
                self.fibers.swap(i - 1, i);
            }
        }

        for f in 0..self.nfibers {
            self.fibers[f].node[0].eta += DT * self.fibers[f].node[0].etadash;
            self.fibers[f].node[0].x = self.cx;

            // Handle window move.
            // Since we don't track real window movement, rx - x = 0 and ry - y = 0.
            let rx_x_diff = 0.0;
            let ry_y_diff = 0.0;
            self.fibers[f].node[NODES - 2].x *= 0.1 * ry_y_diff;
            self.fibers[f].node[NODES - 2].x += 0.05 * rx_x_diff;

            // 2nd order diff equation
            for i in 1..NODES {
                let mut pload = 0.0;
                let mut eload = 0.0;
                let pstress = (self.fibers[f].node[i].phi - self.fibers[f].node[i - 1].phi) * PY;
                let estress = (self.fibers[f].node[i].eta - self.fibers[f].node[i - 1].eta) * PY;
                let dxi = self.fibers[f].node[i].x - self.fibers[f].node[i - 1].x;
                let dzi = self.fibers[f].node[i].z - self.fibers[f].node[i - 1].z;
                let li = (dxi * dxi + dzi * dzi).sqrt() / len(i);
                let drag = DAMPING * len(i) * len(i) * (NODES as f64) * (NODES as f64);

                if li > 0.0 {
                    for j in (i + 1)..NODES {
                        let dxj = self.fibers[f].node[j].x - self.fibers[f].node[i].x;
                        let dzj = self.fibers[f].node[j].z - self.fibers[f].node[i].z;

                        pload += len(j) * (dxi * dxj + dzi * dzj) / li; // Radial load
                        eload += len(j) * (dxi * dzj - dzi * dxj) / li; // Transverse load
                    }
                }

                self.fibers[f].node[i].phidash += DT * (pload - pstress - drag * self.fibers[f].node[i].phidash) / len(i);
                self.fibers[f].node[i].phi += DT * self.fibers[f].node[i].phidash;

                self.fibers[f].node[i].etadash += DT * (eload - estress - drag * self.fibers[f].node[i].etadash) / len(i);
                self.fibers[f].node[i].eta += DT * self.fibers[f].node[i].etadash;

                let sp = self.fibers[f].node[i - 1].phi.sin();
                let cp = self.fibers[f].node[i - 1].phi.cos();
                let se = self.fibers[f].node[i - 1].eta.sin();
                let ce = self.fibers[f].node[i - 1].eta.cos();

                self.fibers[f].node[i].x = self.fibers[f].node[i - 1].x + len(i - 1) * ce * sp;
                self.fibers[f].node[i].y = self.fibers[f].node[i - 1].y - len(i - 1) * cp;
                self.fibers[f].node[i].z = self.fibers[f].node[i - 1].z + len(i - 1) * se * sp;

                // Width-only projection crops the fan on ultrawide; clamp the
                // basis to a 16:9 width (same fix as life3d).
                let basis = self.width.min(self.height * 16 / 9) as f64 / 2.0;
                self.fibers[f].draw[i - 1].x = cx + (basis * self.fibers[f].node[i].x).round() as i32;
                self.fibers[f].draw[i - 1].y = cy + (basis * self.fibers[f].node[i].y).round() as i32;
            }
        }

        self.ops.clear();

        for f in 0..self.nfibers {
            let x = self.fibers[f].node[1].x - self.cx + 0.025;
            let y = self.fibers[f].node[1].z + 0.02;
            let angle = y.atan2(x) + self.psi;

            let color_idx = (((self.ncolors as f64 * angle / (2.0 * PI)) + self.ncolors as f64) as i32) % self.ncolors;
            let tipcolor = if self.ncolors > 2 {
                Color::from_hsl(color_idx as f32 / self.ncolors as f32, 1.0, 0.5)
            } else {
                Color::new(255, 255, 255, 255)
            };

            let fibercolor: Color;
            let tiplen: usize;

            if self.fibers[f].node[1].z < 0.0 {
                // Back
                tiplen = 2;
                fibercolor = self.dim;
            } else if self.fibers[f].node[NODES - 1].z < 0.7 {
                // Middle
                tiplen = 3;
                fibercolor = self.medium;
            } else {
                // Front
                tiplen = 3;
                fibercolor = self.bright;
            }

            // Draw line segments for the fiber body
            for i in 0..(NODES - tiplen - 1) {
                self.ops.push(DrawOp {
                    x0: self.fibers[f].draw[i].x,
                    y0: self.fibers[f].draw[i].y,
                    x1: self.fibers[f].draw[i + 1].x,
                    y1: self.fibers[f].draw[i + 1].y,
                    color: fibercolor,
                });
            }

            // Draw tip
            let start = NODES - 1 - tiplen;
            for i in 0..(tiplen - 1) {
                self.ops.push(DrawOp {
                    x0: self.fibers[f].draw[start + i].x,
                    y0: self.fibers[f].draw[start + i].y,
                    x1: self.fibers[f].draw[start + i + 1].x,
                    y1: self.fibers[f].draw[start + i + 1].y,
                    color: tipcolor,
                });
            }
        }

        self.count += 1;
        if self.count > self.cycles {
            self.change(&mut rng);
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for op in &self.ops {
            draw_line(buffer, width, height, op.x0, op.y0, op.x1, op.y1, op.color);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;
        self.delay_us = if config.delay_us == 0 { 10000 } else { config.delay_us };
        self.cycles = if config.cycles <= 0 { 10000 } else { config.cycles };
        self.ncolors = if config.ncolors <= 0 { 64 } else { config.ncolors };
        let count = if config.count <= 0 { 500 } else { config.count };
        self.nfibers = count as usize;

        self.fibers.clear();
        for _ in 0..self.nfibers {
            let mut nodes = vec![NodeStruct::default(); NODES];
            let phi = (PI / 180.0) * rng.random_range(0.0..SPREAD);
            let eta = rng.random_range(0.0..(2.0 * PI)) - PI;

            for i in 0..NODES {
                nodes[i].phi = phi;
                nodes[i].phidash = 0.0;
                nodes[i].eta = eta;
                nodes[i].etadash = 0.0;
            }
            nodes[0].etadash = 0.002 / DT;
            nodes[0].y = 0.0;
            nodes[0].z = 0.0;

            self.fibers.push(FiberStruct {
                node: nodes,
                draw: vec![Point::default(); NODES],
            });
        }

        self.psi = rng.random_range(0.0..(2.0 * PI));
        self.dpsi = 0.01;

        if self.ncolors > 2 {
            self.bright = Color::new(255, 0xE0, 0xE0, 0xC0);
            self.medium = Color::new(255, 0x80, 0x80, 0x70);
            self.dim = Color::new(255, 0x40, 0x40, 0x20);
        } else {
            self.bright = Color::new(255, 255, 255, 255);
            self.medium = Color::new(255, 255, 255, 255);
            self.dim = Color::new(255, 0, 0, 0);
        }

        self.change(&mut rng);
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        10_000 // match xlockmore default
    }
}
