//! Animated mandelbrot sets.
//
// Copyright (c) 1997 Dan Stromberg <strombrg@nis.acs.uci.edu>
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
// Rust port of xlockmore/modes/mandelbrot.c.

use std::cell::Cell;
use std::f64::consts::LN_2;
use rand::Rng;
use crate::animation::primitives::{clear_buffer, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

const ESCAPE: f64 = 13.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Interior {
    None = 0,
    Lyapunov,
    Alpha,
    Index,
}

#[derive(Clone, Copy, Debug)]
struct Complex {
    real: f64,
    imag: f64,
}

impl Complex {
    #[inline]
    fn add(&mut self, b: Complex) {
        self.real += b.real;
        self.imag += b.imag;
    }

    #[inline]
    fn mult(&mut self, b: Complex) {
        let tr = self.real * b.real - self.imag * b.imag;
        let ti = self.real * b.imag + self.imag * b.real;
        self.real = tr;
        self.imag = ti;
    }
}

fn cln(a: &mut Complex) {
    let tr = (a.real * a.real + a.imag * a.imag).sqrt();
    let ti = if a.real == 0.0 && a.imag == 0.0 {
        0.0
    } else {
        a.imag.atan2(a.real)
    };
    a.real = tr;
    a.imag = ti;
}

fn complex_exp(a: &mut Complex) {
    let tr = a.real.exp();
    let old_imag = a.imag;
    a.real = tr * old_imag.cos();
    a.imag = tr * old_imag.sin();
}

fn complex_pow(a: Complex, b: Complex) -> Complex {
    let mut c = a;
    let mut d = b;
    cln(&mut c);
    d.mult(c);
    complex_exp(&mut d);
    d
}

fn complex_sin(a: Complex) -> Complex {
    let mut c = a;
    let mut d = a;
    let mut i = Complex { real: 0.0, imag: 1.0 };

    c.mult(i);
    i.imag = -i.imag;
    d.mult(i);
    complex_exp(&mut c);
    complex_exp(&mut d);
    d.real = -d.real;
    d.imag = -d.imag;
    c.add(d);
    c.real /= 2.0;
    c.imag /= 2.0;
    c.mult(i);
    c
}

fn ipow(a: &mut Complex, n: i32) {
    if n <= 1 {
        return;
    }
    if n == 2 {
        let b = *a;
        a.mult(b);
        return;
    }
    let mut a2 = *a;
    let t2 = n / 2;
    ipow(&mut a2, t2);
    let a2_copy = a2;
    a2.mult(a2_copy);
    if t2 * 2 != n {
        a2.mult(*a);
    }
    *a = a2;
}

fn reps(
    c: Complex,
    p: f64,
    r: i32,
    binary: bool,
    interior: Interior,
    demrange: f64,
    zpow: bool,
    zsin: bool,
) -> i32 {
    let mut rep = 0;
    let mut escaped = 0;
    let mut t = c;
    let escape = if demrange == 0.0 {
        ESCAPE
    } else {
        ESCAPE * ESCAPE * ESCAPE * ESCAPE
    };
    let mut t1 = Complex { real: 0.0, imag: 0.0 };
    let mut dt = Complex { real: 1.0, imag: 0.0 };
    let mut l_val = 0.0;
    let mut dl2 = 1.0;
    let mut alpha2 = ESCAPE;
    let mut index = 0;

    let log_top = (r as f64).ln();

    for _ in 0..r {
        t1 = t;
        ipow(&mut t, p as i32);
        t.add(c);
        if zpow {
            t.add(complex_pow(t1, t1));
        }
        if zsin {
            t.add(complex_sin(t1));
        }

        let l2 = t.real * t.real + t.imag * t.imag;
        if l2 <= alpha2 {
            alpha2 = l2;
            index = rep;
        }

        if l2 >= escape {
            escaped = 1;
            break;
        } else if interior == Interior::Lyapunov {
            l_val += l2.sqrt().ln();
        }

        if demrange != 0.0 {
            dt.real *= p;
            dt.imag *= p;
            if p > 2.0 {
                let mut tmp = t1;
                ipow(&mut tmp, (p - 1.0) as i32);
                dt.mult(tmp);
            }
            dt.real += 1.0;
            dl2 = dt.real * dt.real + dt.imag * dt.imag;
            if dl2 >= 1e300 {
                escaped = 2;
                break;
            }
        }
        rep += 1;
    }

    if escaped != 0 {
        if demrange != 0.0 {
            let mt = (t1.real * t1.real + t1.imag * t1.imag).sqrt();
            let dist = 0.5 * mt * mt.ln() / dl2.sqrt();
            let base = if interior != Interior::None { 0 } else { 1 };
            rep = base + (10.0 * r as f64 * dist / demrange) as i32;
            if rep > r - 1 {
                rep = r - 1;
            }
        }
        if binary && t.imag > 0.0 {
            rep = (r + rep / 2) % r;
        }

        // USE_LOG logic
        if rep > 0 {
            rep = (r as f64 * (rep as f64).ln() / log_top) as i32;
        }

        rep
    } else if interior == Interior::Lyapunov {
        (-(l_val / LN_2) as i32) % r
    } else if interior == Interior::Index {
        1 + index
    } else if interior == Interior::Alpha {
        (r as f64 * alpha2.sqrt()) as i32
    } else {
        r
    }
}

pub struct Mandelbrot {
    counter: i32,
    power: f64,
    column: i32,
    backwards: bool,
    ncolors: usize,
    extreme_ul: Complex,
    extreme_lr: Complex,
    ul: Complex,
    lr: Complex,
    screen_width: u32,
    screen_height: u32,
    reptop: i32,
    dem: bool,
    pow: bool,
    sin: bool,
    binary: bool,
    interior: Interior,

    cycles: i32,
    delay_us: u64,

    ops: Vec<(i32, i32, Color)>,
    needs_clear: Cell<bool>,
}

impl Mandelbrot {
    fn select(&mut self) {
        let mut rng = rand::rng();
        let mut found = false;

        while !found {
            let exp = -9.0 + rng.random::<f64>() * (-18.0 - (-9.0));
            let precision = 2.0_f64.powf(exp);

            for _tries in 0..10000 {
                let mut temp = Complex {
                    real: self.extreme_ul.real
                        + rng.random::<f64>() * (self.extreme_lr.real - self.extreme_ul.real),
                    imag: self.extreme_ul.imag
                        + rng.random::<f64>() * (self.extreme_lr.imag - self.extreme_ul.imag),
                };

                self.ul.real = temp.real - precision * (self.screen_width as f64) / 2.0;
                self.lr.real = temp.real + precision * (self.screen_width as f64) / 2.0;
                self.ul.imag = temp.imag - precision * (self.screen_height as f64) / 2.0;
                self.lr.imag = temp.imag + precision * (self.screen_height as f64) / 2.0;

                let sample_step = 4;
                let mut inside = 0;
                let mut uninteresting = 0;

                for row in 0..sample_step {
                    for column in 0..sample_step {
                        temp.imag = self.ul.imag
                            + (self.ul.imag - self.lr.imag) * (row as f64 / sample_step as f64);
                        temp.real = self.ul.real
                            + (self.ul.real - self.lr.real) * (column as f64 / sample_step as f64);

                        let r = reps(
                            temp,
                            self.power,
                            self.reptop,
                            false,
                            Interior::None,
                            0.0,
                            self.pow,
                            self.sin,
                        );
                        if r == self.reptop {
                            inside += 1;
                        }
                        if r < 2 {
                            uninteresting += 1;
                        }
                    }
                }

                let s2 = (sample_step * sample_step) as f64;
                if (inside as f64) >= (s2 / 10.0).ceil()
                    && (inside as f64) <= s2 * 6.0 / 10.0
                    && (uninteresting as f64) <= s2 / 10.0
                {
                    found = true;
                    break;
                }
            }
        }
    }
}

impl Animation for Mandelbrot {
    fn new(config: &AnimConfig) -> Self {
        let mut m = Mandelbrot {
            counter: 0,
            power: 2.0,
            column: 0,
            backwards: false,
            ncolors: 2,
            extreme_ul: Complex { real: -3.0, imag: -3.0 },
            extreme_lr: Complex { real: 3.0, imag: 3.0 },
            ul: Complex { real: 0.0, imag: 0.0 },
            lr: Complex { real: 0.0, imag: 0.0 },
            screen_width: config.width,
            screen_height: config.height,
            reptop: 300,
            dem: false,
            pow: false,
            sin: false,
            binary: false,
            interior: Interior::None,
            cycles: 20000,
            delay_us: config.delay_us,
            ops: Vec::new(),
            needs_clear: Cell::new(true),
        };
        m.reset(config);
        m
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        self.ops.clear();

        if (!self.backwards && self.column >= 3 * self.screen_width as i32)
            || (self.backwards && self.column < -2 * self.screen_width as i32)
        {
            self.backwards = rng.random::<bool>();
            if self.backwards {
                self.column = self.screen_width as i32 - 1;
            } else {
                self.column = 0;
            }
            self.power = rng.random_range(0..3) as f64 + 2.0;
            self.select();
        } else if self.column >= self.screen_width as i32 || self.column < 0 {
            if self.backwards {
                self.column -= 1;
            } else {
                self.column += 1;
            }
            self.counter += 1;
            return;
        }

        let demrange = if self.dem {
            (self.ul.real - self.lr.real).abs() / 2.0
        } else {
            0.0
        };

        for h in 0..self.screen_height as i32 {
            let mut c = Complex { real: 0.0, imag: 0.0 };
            c.real = self.ul.real
                + (self.ul.real - self.lr.real) * (self.column as f64 / self.screen_width as f64);
            c.imag = self.ul.imag
                + (self.ul.imag - self.lr.imag) * (h as f64 / self.screen_height as f64);

            let result = reps(
                c,
                self.power,
                self.reptop,
                self.binary,
                self.interior,
                demrange,
                self.pow,
                self.sin,
            );

            let color = if result < 0 || result >= self.reptop {
                Color::new(255, 0, 0, 0)
            } else {
                let color_idx =
                    (self.ncolors as f32 * result as f32 / self.reptop as f32) as usize;
                Color::from_hsl(color_idx as f32 / self.ncolors as f32, 1.0, 0.5)
            };

            self.ops.push((self.column, h, color));
        }

        if self.backwards {
            self.column -= 1;
        } else {
            self.column += 1;
        }

        self.counter += 1;
        if self.counter > self.cycles {
            let config = AnimConfig {
                width: self.screen_width,
                height: self.screen_height,
                count: 0,
                cycles: self.cycles,
                size: 0,
                ncolors: self.ncolors as i32,
                max_fps: 0, // internal re-init config; only the player reads max_fps
                delay_us: self.delay_us,
            };
            self.reset(&config);
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if self.needs_clear.get() {
            clear_buffer(buffer, Color::new(255, 0, 0, 0));
            self.needs_clear.set(false);
        }

        for op in &self.ops {
            put_pixel(buffer, width, height, op.0, op.1, op.2);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.screen_width = config.width;
        self.screen_height = config.height;
        self.ncolors = if config.ncolors < 2 {
            2
        } else {
            config.ncolors as usize
        };

        self.cycles = if config.cycles <= 0 { 20000 } else { config.cycles };
        self.delay_us = config.delay_us;

        self.backwards = rng.random::<bool>();
        if self.backwards {
            self.column = self.screen_width as i32 - 1;
        } else {
            self.column = 0;
        }

        self.power = rng.random_range(0..3) as f64 + 2.0;
        self.counter = 0;
        self.needs_clear.set(true);

        self.binary = rng.random::<bool>();
        self.dem = rng.random::<bool>();
        self.interior = match rng.random_range(0..4) {
            1 => Interior::Lyapunov,
            2 => Interior::Alpha,
            3 => Interior::Index,
            _ => Interior::None,
        };

        self.pow = false;
        self.sin = false;
        self.reptop = 300;

        self.extreme_ul = Complex { real: -3.0, imag: -3.0 };
        self.extreme_lr = Complex { real: 3.0, imag: 3.0 };

        self.select();
    }

    fn clears_each_frame(&self) -> bool {
        false
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
