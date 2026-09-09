//! Fireworks.
//
// 1991, Pezhman Givy.
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
// Rust port of xlockmore/modes/pyro2.c.

#![allow(dead_code, unused_variables, unused_assignments, unused_imports)]
use crate::rng::RngExt;
use std::sync::OnceLock;

use crate::animation::primitives::{draw_line, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const EXP_MAX_GENERATION: i32 = 30;
const EXP_MAX_TTL: i32 = 200;
const EXP_MIN_SPARKS: usize = 20;
const EXP_RND_SPARKS: usize = 20;

const RHO: f64 = 5.0;
const FACTOR: f64 = 2.0;

const PYRO_NHUES: usize = 7;
const PYRO_NSHADES: usize = 20;

const TYPES: &[u32] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
    12, 12, 12, 12, // fallback for text 'R' 'u' 's' 't'
    10, 11
];

include!("heart.rs");

#[derive(Clone, PartialEq, Eq)]
enum PyroStat {
    Wait,
    Parabel,
    Explosion,
    Ready,
}

#[derive(Clone)]
struct Para {
    init: bool,
    t: f64,
    th: f64,
    v0: f64,
    phi: f64,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

#[derive(Clone)]
struct Spark {
    time: f64,
    v0: f64,
    delta_t: f64,
    angle: f64,
    m: f64,
    generation: i32,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    ttl: i32,
    color: usize,
}

#[derive(Clone)]
struct Expl {
    init: bool,
    x0: i32,
    y0: i32,
    sparks: Vec<Spark>,
    current_generation: i32,
    etype: u32,
    firsttime: i32,
    typedata_integer: i32,
}

#[derive(Clone)]
struct Pyro {
    stat: PyroStat,
    wait: i32,
    etype: u32,
    para: Para,
    expl: Expl,
    color1: usize,
    color2: isize,
}

fn color_for_shade(hue_index: usize, shade: i32) -> Color {
    let is_white = hue_index == 0;
    let h = if hue_index > 0 { (hue_index - 1) as f32 * 60.0 } else { 0.0 } / 360.0;

    let s = if is_white {
        0.0
    } else if shade < 2 {
        (shade as f32 / 1.0) * 0.8
    } else {
        0.8
    };

    let l = if shade < 2 {
        1.0 - s / 2.0
    } else {
        let b = 1.0 - ((shade - 2) as f32 / 17.0);
        b * (1.0 - s / 2.0)
    };

    Color::from_hsl(h, s, l)
}

// (hue_index, ttl) maps onto at most PYRO_NHUES * PYRO_NSHADES distinct
// colors, so precompute them all once instead of re-deriving via HSL math
// for every live spark on every frame.
static COLOR_TABLE: OnceLock<[[Color; PYRO_NSHADES]; PYRO_NHUES]> = OnceLock::new();

fn get_color(hue_index: usize, ttl: i32) -> Color {
    let ttl_ratio = (ttl as f32 / EXP_MAX_TTL as f32).clamp(0.0, 1.0);
    let mut shade = 19 - (ttl_ratio * 20.0) as i32;
    shade = shade.clamp(0, 19);

    let table = COLOR_TABLE.get_or_init(|| {
        let mut table = [[Color::new(0, 0, 0, 0); PYRO_NSHADES]; PYRO_NHUES];
        for (hue_index, row) in table.iter_mut().enumerate() {
            for (shade, cell) in row.iter_mut().enumerate() {
                *cell = color_for_shade(hue_index, shade as i32);
            }
        }
        table
    });

    table[hue_index][shade as usize]
}

impl Pyro {
    fn parabel(&mut self, width: u32, height: u32) {
        let mut rng = crate::rng::rng();
        if !self.para.init {
            self.para.phi = 80.0 + rng.random_range(0..20) as f64;
            self.para.v0 = 23.0;
            self.para.th = self.para.v0 * self.para.phi.to_radians().sin() / 9.81;
            self.para.t = 0.0;
        }

        let x = width as i32 / 2 + (self.para.v0 * self.para.phi.to_radians().cos() * self.para.t * width as f64 / 35.0) as i32;
        let y = height as i32 - ((self.para.v0 * self.para.phi.to_radians().sin() * self.para.t - 0.5 * 9.81 * self.para.t * self.para.t) * height as f64 / 35.0) as i32;

        let mut x_final = x;
        let mut y_final = y;
        if x < 0 || x >= width as i32 || y < 0 || y >= height as i32 {
            x_final = -1;
            y_final = -1;
        }

        if !self.para.init {
            self.para.x1 = x_final;
            self.para.y1 = y_final;
            self.para.x2 = -1;
            self.para.y2 = -1;
        }

        self.para.x1 = self.para.x2;
        self.para.y1 = self.para.y2;
        self.para.x2 = x_final;
        self.para.y2 = y_final;

        if !self.para.init {
            self.para.init = true;
        }

        self.para.t += 0.02;
        if self.para.t > self.para.th {
            self.stat = PyroStat::Explosion;
            self.para.init = false;
        }
    }

    fn explosion(&mut self, width: u32, height: u32) {
        // Spark speed is radial; basing it on raw width balloons/crops on
        // ultrawide. Clamp the basis to a 16:9 width (same fix as life3d).
        let width = width.min(height * 16 / 9);
        let mut rng = crate::rng::rng();

        if !self.expl.init {
            self.expl.sparks.clear();
            self.expl.current_generation = 0;
            self.expl.firsttime = 0;
            self.expl.x0 = self.para.x2;
            self.expl.y0 = self.para.y2;
            self.expl.init = true;

            if self.etype == 4 {
                self.expl.typedata_integer = 1;
            } else {
                self.expl.typedata_integer = 0;
            }
        } else {
            self.expl.current_generation += 1;
        }

        let mut sumttl = 0;
        for s in &self.expl.sparks {
            if s.ttl > 0 {
                sumttl += s.ttl;
            }
        }

        if !self.expl.sparks.is_empty() && sumttl < 1 {
            self.stat = PyroStat::Ready;
            self.expl.init = false;
            return;
        }

        for s in &mut self.expl.sparks {
            s.ttl -= 1;

            if s.ttl < 1 {
                continue;
            }

            let phi = s.angle;
            let v0 = s.v0;
            let mut t = s.time;
            let dt = s.delta_t;
            let m = s.m;

            let x1 = m * v0 * phi.to_radians().cos() / RHO * (1.0 - (-RHO / m * t).exp());
            let y1 = m / RHO * (v0 * phi.to_radians().sin() + m * 9.81 / RHO) * (1.0 - (-RHO / m * t).exp()) - m * 9.81 / RHO * t;
            let xk1 = self.expl.x0 + x1 as i32;
            let yk1 = self.expl.y0 - y1 as i32;

            t += dt;

            let x2 = m * v0 * phi.to_radians().cos() / RHO * (1.0 - (-RHO / m * t).exp());
            let y2 = m / RHO * (v0 * phi.to_radians().sin() + m * 9.81 / RHO) * (1.0 - (-RHO / m * t).exp()) - m * 9.81 / RHO * t;
            let xk2 = self.expl.x0 + x2 as i32;
            let yk2 = self.expl.y0 - y2 as i32;

            s.x1 = xk1;
            s.y1 = yk1;
            s.x2 = xk2;
            s.y2 = yk2;

            s.time = t;
            s.delta_t += 0.001;
        }

        if self.expl.current_generation >= EXP_MAX_GENERATION {
            return;
        }
        self.expl.firsttime = 1;

        let sparks = EXP_MIN_SPARKS + rng.random_range(0..EXP_RND_SPARKS);
        let step = 360.0 / sparks as f64;
        let mut phi = 1.0 + rng.random_range(0..10) as f64;

        for i in 0..sparks {
            let mut color = if (self.expl.sparks.len() & 1) != 0 && self.color2 != -1 {
                self.color2 as usize
            } else {
                self.color1
            };

            let mut ttl = EXP_MAX_TTL / 2 + rng.random_range(0..EXP_MAX_TTL);
            let mut v0 = 0.0;
            let m = 2.0 + rng.random_range(0..2) as f64 / 2.0;

            match self.etype {
                0 => {
                    let tmp = self.expl.current_generation as f64 / EXP_MAX_GENERATION as f64;
                    ttl = EXP_MAX_TTL / 10 + rng.random_range(0..EXP_MAX_TTL / 2);
                    v0 = rng.random_range(0..30) as f64 + (tmp * 90.0).to_radians().sin() * width as f64 / 10.0 * FACTOR;
                }
                1 => {
                    let tmp = self.expl.current_generation as f64 / EXP_MAX_GENERATION as f64;
                    v0 = rng.random_range(0..30) as f64 + (tmp * 360.0).to_radians().sin() * width as f64 / 10.0 * FACTOR;
                }
                2 => {
                    let tmp = self.expl.current_generation as f64 / EXP_MAX_GENERATION as f64;
                    v0 = rng.random_range(0..30) as f64 + (tmp * 180.0).to_radians().cos() * width as f64 / 5.0 * FACTOR;
                }
                3 => {
                    let tmp = self.expl.current_generation as f64 / EXP_MAX_GENERATION as f64;
                    v0 = rng.random_range(0..30) as f64 + (tmp * 360.0).to_radians().sin() * (tmp * 360.0).to_radians().cos() * width as f64 / 5.0 * FACTOR;
                }
                4 => {
                    if i == 0 {
                        self.expl.typedata_integer = -self.expl.typedata_integer;
                    }
                    v0 = rng.random_range(0..30) as f64 + (i as i32 + 2) as f64 * self.expl.typedata_integer as f64 * width as f64 / 200.0 * FACTOR;
                }
                5 => {
                    let tmp_deg = (self.expl.current_generation as f64 / EXP_MAX_GENERATION as f64 * 360.0) as i32;
                    let tmp2 = self.expl.current_generation as f64 / EXP_MAX_GENERATION as f64;
                    v0 = rng.random_range(0..30) as f64 + ((tmp_deg % 90) as f64).to_radians().cos() * (tmp2 * 360.0).to_radians().cos() * width as f64 / 5.0 * FACTOR;
                }
                6 => {
                    let tmp = self.expl.current_generation as f64 / EXP_MAX_GENERATION as f64;
                    v0 = rng.random_range(0..30) as f64 + ((i as i32 - sparks as i32 / 2) as f64 * (phi * 10.0).to_radians().sin() * (tmp * 360.0).to_radians().sin()) * width as f64 / 100.0 * FACTOR;
                }
                7 => {
                    let tmp_deg = (self.expl.current_generation as f64 / EXP_MAX_GENERATION as f64 * 360.0) as i32;
                    v0 = rng.random_range(0..30) as f64 + ((i as i32 - sparks as i32 / 2) as f64 * (phi * 10.0).to_radians().sin() * ((tmp_deg % 90) as f64).to_radians().sin()) * width as f64 / 50.0 * FACTOR;
                }
                8 => {
                    v0 = rng.random_range(0..30) as f64 + (((i as f64 + 1.0) / (EXP_MIN_SPARKS as f64 + EXP_RND_SPARKS as f64) * 360.0).to_radians().sin() * (1.0 * i as f64)) * width as f64 / 100.0 * FACTOR;
                }
                9 => {
                    let tmp = self.expl.current_generation as f64 / EXP_MAX_GENERATION as f64;
                    v0 = rng.random_range(0..30) as f64 + (tmp * 360.0 * 4.0).to_radians().sin() * width as f64 / 10.0 * FACTOR;
                }
                10 => {
                    ttl = EXP_MAX_TTL / 2 + rng.random_range(0..EXP_MAX_TTL) + 5;
                    let idx = ((i as f64 / sparks as f64) * 360.0) as usize % 360;
                    v0 = HEART[idx] * (self.expl.current_generation as f64) * width as f64 / 150.0 * FACTOR;
                    color = PYRO_NSHADES;
                }
                11 => {
                    let tmp = i as f64 / sparks as f64;
                    v0 = rng.random_range(0..30) as f64 + (tmp * 90.0).to_radians().sin() * (tmp * 180.0).to_radians().cos() * i as f64 * (if i & 1 != 0 { 1.0 } else { -1.0 }) * width as f64 / 100.0 * FACTOR;
                }
                13 => {
                    v0 = if rng.random_range(0..2) != 0 { i as f64 * 20.0 } else { (sparks - i) as f64 * 20.0 };
                }
                14 => {
                    v0 = 5.0 + (if rng.random_range(0..20) == 3 { i } else { sparks - i }) as f64 * 20.0;
                }
                _ => {
                    let tmp = self.expl.current_generation as f64 / EXP_MAX_GENERATION as f64;
                    let sin_val = if tmp <= 0.3 {
                        (tmp * 90.0).to_radians().sin()
                    } else if tmp <= 0.6 {
                        ((tmp - 0.3) * 90.0).to_radians().sin()
                    } else {
                        ((tmp - 0.6) * 90.0).to_radians().sin()
                    };
                    v0 = rng.random_range(0..10) as f64 + 15.0 + sin_val * (tmp + 0.5) * width as f64 / 5.0 * FACTOR;
                }
            }

            self.expl.sparks.push(Spark {
                time: 0.0,
                v0,
                delta_t: 0.0,
                angle: phi,
                m,
                generation: self.expl.current_generation,
                x1: 0,
                y1: 0,
                x2: 0,
                y2: 0,
                ttl,
                color,
            });

            phi += step;
        }
    }
}

pub struct Pyro2 {
    pyros: Vec<Pyro>,
    width: u32,
    height: u32,
    delay_us: u64,
}

impl Pyro2 {
    fn init_pyros(&mut self) {
        let mut rng = crate::rng::rng();
        for (i, p) in self.pyros.iter_mut().enumerate() {
            p.stat = PyroStat::Wait;
            p.wait = (i * 100) as i32;
            p.color1 = rng.random_range(0..PYRO_NHUES) * PYRO_NSHADES;
            p.color2 = if rng.random_bool(0.5) {
                (rng.random_range(0..PYRO_NHUES) * PYRO_NSHADES) as isize
            } else {
                -1
            };

            p.para.init = false;
            p.expl.init = false;
            p.expl.sparks.clear();
        }
    }
}

impl Animation for Pyro2 {
    fn new(config: &AnimConfig) -> Self {
        let mut p = Pyro2 {
            pyros: Vec::new(),
            width: config.width,
            height: config.height,
            delay_us: 30_000,
        };
        p.reset(config);
        p
    }

    fn tick(&mut self) {
        let width = self.width;
        let height = self.height;

        for p in &mut self.pyros {
            match p.stat {
                PyroStat::Wait => {
                    p.wait -= 1;
                    if p.wait <= 0 {
                        p.stat = PyroStat::Parabel;
                    }
                }
                PyroStat::Parabel => {
                    p.parabel(width, height);
                }
                PyroStat::Explosion => {
                    p.explosion(width, height);
                }
                PyroStat::Ready => {}
            }
        }

        if self.pyros.iter().all(|p| p.stat == PyroStat::Ready) {
            self.init_pyros();
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for p in &self.pyros {
            match p.stat {
                PyroStat::Parabel
                    if p.para.t < p.para.th && p.para.x1 >= 0 && p.para.y1 >= 0 && p.para.x2 >= 0 && p.para.y2 >= 0 => {
                        draw_line(buffer, width, height, p.para.x1, p.para.y1, p.para.x2, p.para.y2, Color::new(255, 255, 255, 255));
                    }
                PyroStat::Explosion
                    if p.expl.firsttime == 1 => {
                        for s in &p.expl.sparks {
                            if s.ttl >= 0 {
                                let c = get_color(s.color / PYRO_NSHADES, s.ttl);
                                draw_line(buffer, width, height, s.x1, s.y1, s.x2, s.y2, c);
                            }
                        }
                    }
                _ => {}
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.delay_us = if config.delay_us == 0 { 30_000 } else { config.delay_us };

        self.pyros.clear();
        for &etype in TYPES {
            self.pyros.push(Pyro {
                stat: PyroStat::Ready,
                wait: 0,
                etype,
                para: Para {
                    init: false, t: 0.0, th: 0.0, v0: 0.0, phi: 0.0, x1: -1, y1: -1, x2: -1, y2: -1,
                },
                expl: Expl {
                    init: false, x0: 0, y0: 0, sparks: Vec::new(), current_generation: 0, etype, firsttime: 0, typedata_integer: 0,
                },
                color1: 0,
                color2: -1,
            });
        }
        self.init_pyros();
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
