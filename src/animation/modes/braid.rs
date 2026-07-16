//! Random braids around a circle, cycling the colors rotationally.
//
// Copyright (c) 1995 by John Neil.
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
// Rust port of xlockmore/modes/braid.c.

use crate::animation::primitives::{draw_line, Color};
use crate::animation::{AnimConfig, Animation};
use rand::Rng;
use std::f32::consts::PI;

const MAXLENGTH: usize = 50;
const MINLENGTH: usize = 8;
const MAXSTRANDS: usize = 15;
const MINSTRANDS: usize = 3;
const SPINRATE: f32 = 12.0;

pub struct Braid {
    linewidth: i32,
    braidword: [i32; MAXLENGTH],
    components: [i32; MAXSTRANDS],
    nstrands: usize,
    braidlength: usize,
    startcolor: f32,
    center_x: f32,
    center_y: f32,
    min_radius: f32,
    max_radius: f32,
    age: i32,
    color_direction: i32,

    width: u32,
    height: u32,
    delay_us: u64,
    cycles: i32,
    ncolors: usize,
    count: i32,
    size: i32,
}

impl Braid {
    fn applyword(&self, string: i32, position: usize) -> i32 {
        let mut c = string;
        for i in position..self.braidlength {
            if c == self.braidword[i].abs() {
                c -= 1;
            } else if c == self.braidword[i].abs() - 1 {
                c += 1;
            }
        }
        for i in 0..position {
            if c == self.braidword[i].abs() {
                c -= 1;
            } else if c == self.braidword[i].abs() - 1 {
                c += 1;
            }
        }
        c
    }

    fn applywordbackto(&self, string: i32, position: usize) -> i32 {
        let mut c = string;
        for i in (0..position).rev() {
            if c == self.braidword[i].abs() {
                c -= 1;
            } else if c == self.braidword[i].abs() - 1 {
                c += 1;
            }
        }
        c
    }

    fn init_braid(&mut self) {
        let mut rng = rand::rng();

        self.center_x = self.width as f32 / 2.0;
        self.center_y = self.height as f32 / 2.0;
        self.age = 0;

        self.color_direction = if rng.random::<bool>() { 1 } else { -1 };

        let min_length = if self.center_x > self.center_y {
            self.center_y
        } else {
            self.center_x
        };

        self.min_radius = min_length * 0.30;
        self.max_radius = min_length * 0.90;

        let mi_count = if self.count == 0 { 15 } else { self.count };
        if mi_count < MINSTRANDS as i32 {
            self.nstrands = MINSTRANDS;
        } else {
            let max_val = std::cmp::max(
                std::cmp::min(
                    std::cmp::min(MAXSTRANDS as i32, mi_count),
                    ((self.max_radius - self.min_radius) / 5.0) as i32,
                ),
                MINSTRANDS as i32,
            );
            self.nstrands = rng.random_range((MINSTRANDS as i32)..=max_val) as usize;
        }

        self.braidlength = rng.random_range(
            MINLENGTH..=std::cmp::min(MAXLENGTH, self.nstrands * 6)
        );

        for i in 0..self.braidlength {
            self.braidword[i] = rng.random_range(1..self.nstrands as i32) * (rng.random_range(1..=2) * 2 - 3);
            if i > 0 {
                while self.braidword[i] == -self.braidword[i - 1] {
                    self.braidword[i] = rng.random_range(1..self.nstrands as i32) * (rng.random_range(1..=2) * 2 - 3);
                }
            }
        }

        while self.braidword[0] == -self.braidword[self.braidlength - 1] {
            self.braidword[self.braidlength - 1] = rng.random_range(1..self.nstrands as i32) * (rng.random_range(1..=2) * 2 - 3);
        }

        loop {
            let mut used = [0; MAXSTRANDS];
            let mut count = 0;
            for i in 0..self.braidlength {
                used[self.braidword[i].unsigned_abs() as usize] += 1;
            }
            for i in 0..self.nstrands {
                if used[i] > 0 {
                    count += 1;
                }
            }
            if count < self.nstrands - 1 {
                self.braidword[self.braidlength] = rng.random_range(1..self.nstrands as i32) * (rng.random_range(1..=2) * 2 - 3);
                while self.braidword[self.braidlength] == -self.braidword[self.braidlength - 1] &&
                      self.braidword[0] == -self.braidword[self.braidlength] {
                    self.braidword[self.braidlength] = rng.random_range(1..self.nstrands as i32) * (rng.random_range(1..=2) * 2 - 3);
                }
                self.braidlength += 1;
            }
            if count >= self.nstrands - 1 || self.braidlength >= MAXLENGTH {
                break;
            }
        }

        self.startcolor = rng.random_range(0..self.ncolors as i32) as f32;

        for i in 0..self.nstrands {
            self.components[i] = 0;
        }

        let mut c = 1;
        let mut comp = 0;
        self.components[0] = 1;
        loop {
            let mut i = comp;
            loop {
                i = self.applyword(i as i32, 0) as usize;
                self.components[i] = self.components[comp];
                if i == comp {
                    break;
                }
            }
            let mut count = 0;
            for i in 0..self.nstrands {
                if self.components[i] == 0 {
                    count += 1;
                }
            }
            if count > 0 {
                comp = 0;
                while self.components[comp] != 0 {
                    comp += 1;
                }
                c += 1;
                self.components[comp] = c;
            } else {
                break;
            }
        }

        self.linewidth = self.size;
        if self.linewidth < 0 {
            self.linewidth = rng.random_range(0..-self.linewidth) + 1;
        }
        let min_dim = std::cmp::min(self.width, self.height) as i32;
        if self.linewidth * self.linewidth * 8 > min_dim {
            self.linewidth = std::cmp::min(1, ((min_dim as f64 / 8.0).sqrt()) as i32);
        }

        for i in 0..self.nstrands {
            if self.components[i] & 1 == 0 {
                self.components[i] *= -1;
            }
        }
    }
}


impl Animation for Braid {
    fn new(config: &AnimConfig) -> Self {
        let mut b = Braid {
            linewidth: 1,
            braidword: [0; MAXLENGTH],
            components: [0; MAXSTRANDS],
            nstrands: 0,
            braidlength: 0,
            startcolor: 0.0,
            center_x: 0.0,
            center_y: 0.0,
            min_radius: 0.0,
            max_radius: 0.0,
            age: 0,
            color_direction: 1,

            width: config.width,
            height: config.height,
            delay_us: config.delay_us,
            cycles: if config.cycles <= 0 { 100 } else { config.cycles },
            ncolors: if config.ncolors <= 2 { 64 } else { config.ncolors as usize },
            count: if config.count == 0 { 15 } else { config.count },
            size: if config.size == 0 { -7 } else { config.size },
        };
        b.init_braid();
        b
    }

    fn tick(&mut self) {
        let num_points = 500;
        let color_inc = self.ncolors as f32 * self.color_direction as f32 / num_points as f32;
        self.startcolor += SPINRATE * color_inc;
        
        while self.startcolor >= self.ncolors as f32 {
            self.startcolor -= self.ncolors as f32;
        }
        while self.startcolor < 0.0 {
            self.startcolor += self.ncolors as f32;
        }

        self.age += 1;
        if self.age > self.cycles {
            self.init_braid();
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {

        let num_points = 500;
        let theta = (2.0 * PI) / self.braidlength as f32;
        let t_inc = (2.0 * PI) / num_points as f32;
        
        let r_diff = (self.max_radius - self.min_radius) / self.nstrands as f32;
        let color = self.startcolor;
        
        let mut psi = 0.0;
        for i in 0..self.braidlength {
            psi += theta;
            
            // Replicate floating point loop correctly
            let mut t = 0.0;
            // applywordbackto(k, i) depends only on (k, i), not t; precompute per-strand
            // color components once per i instead of every t sub-step. Only 0..nstrands
            // is ever queried below (braidword values are drawn from 1..nstrands, so the
            // crossing branch's `s + 1` index tops out at nstrands - 1).
            let mut comp_back = [0.0f32; MAXSTRANDS];
            for k in 0..self.nstrands {
                comp_back[k] = self.components[self.applywordbackto(k as i32, i) as usize] as f32;
            }
            while t < theta {
                for s in 0..self.nstrands {
                    if self.braidword[i].unsigned_abs() as usize == s {
                        continue;
                    }

                    if (self.braidword[i].abs() - 1) as usize == s {
                        // Crossing
                        let mut color_use = color + SPINRATE * comp_back[s]
                            + (psi + t) / (2.0 * PI) * self.ncolors as f32;
                        
                        while color_use >= self.ncolors as f32 {
                            color_use -= self.ncolors as f32;
                        }
                        while color_use < 0.0 {
                            color_use += self.ncolors as f32;
                        }

                        let c_rgb = Color::from_hsl(color_use / self.ncolors as f32, 1.0, 0.5);

                        let r1 = self.min_radius + r_diff * s as f32;
                        let r2 = self.min_radius + r_diff * (s + 1) as f32;

                        if self.braidword[i] > 0 || ((t - theta / 2.0).abs() > theta / 7.0) {
                            let w1 = 0.5 * (1.0 + (t / theta * PI - PI / 2.0).sin());
                            let w2 = 0.5 * (1.0 + ((theta - t) / theta * PI - PI / 2.0).sin());
                            let x_1 = (w1 * r2 + w2 * r1) * (t + psi).cos() + self.center_x;
                            let y_1 = (w1 * r2 + w2 * r1) * (t + psi).sin() + self.center_y;

                            let wt1 = 0.5 * (1.0 + ((t + t_inc) / theta * PI - PI / 2.0).sin());
                            let wt2 = 0.5 * (1.0 + ((theta - t - t_inc) / theta * PI - PI / 2.0).sin());
                            let x_2 = (wt1 * r2 + wt2 * r1) * (t + t_inc + psi).cos() + self.center_x;
                            let y_2 = (wt1 * r2 + wt2 * r1) * (t + t_inc + psi).sin() + self.center_y;
                            
                            draw_line(buffer, width, height, x_1 as i32, y_1 as i32, x_2 as i32, y_2 as i32, c_rgb);
                        }
                        
                        let mut color_use2 = color + SPINRATE * comp_back[s + 1]
                            + (psi + t) / (2.0 * PI) * self.ncolors as f32;
                            
                        while color_use2 >= self.ncolors as f32 {
                            color_use2 -= self.ncolors as f32;
                        }
                        while color_use2 < 0.0 {
                            color_use2 += self.ncolors as f32;
                        }

                        let c_rgb2 = Color::from_hsl(color_use2 / self.ncolors as f32, 1.0, 0.5);

                        if self.braidword[i] < 0 || ((t - theta / 2.0).abs() > theta / 7.0) {
                            let w1 = 0.5 * (1.0 + (t / theta * PI - PI / 2.0).sin());
                            let w2 = 0.5 * (1.0 + ((theta - t) / theta * PI - PI / 2.0).sin());
                            let x_1 = (w1 * r1 + w2 * r2) * (t + psi).cos() + self.center_x;
                            let y_1 = (w1 * r1 + w2 * r2) * (t + psi).sin() + self.center_y;

                            let wt1 = 0.5 * (1.0 + ((t + t_inc) / theta * PI - PI / 2.0).sin());
                            let wt2 = 0.5 * (1.0 + ((theta - t - t_inc) / theta * PI - PI / 2.0).sin());
                            let x_2 = (wt1 * r1 + wt2 * r2) * (t + t_inc + psi).cos() + self.center_x;
                            let y_2 = (wt1 * r1 + wt2 * r2) * (t + t_inc + psi).sin() + self.center_y;

                            draw_line(buffer, width, height, x_1 as i32, y_1 as i32, x_2 as i32, y_2 as i32, c_rgb2);
                        }

                    } else {
                        // No crossing
                        let mut color_use = color + SPINRATE * comp_back[s]
                            + (psi + t) / (2.0 * PI) * self.ncolors as f32;
                        while color_use >= self.ncolors as f32 {
                            color_use -= self.ncolors as f32;
                        }
                        while color_use < 0.0 {
                            color_use += self.ncolors as f32;
                        }
                        let c_rgb = Color::from_hsl(color_use / self.ncolors as f32, 1.0, 0.5);
                        
                        let r1 = self.min_radius + r_diff * s as f32;
                        let x_1 = r1 * (t + psi).cos() + self.center_x;
                        let y_1 = r1 * (t + psi).sin() + self.center_y;
                        let x_2 = r1 * (t + t_inc + psi).cos() + self.center_x;
                        let y_2 = r1 * (t + t_inc + psi).sin() + self.center_y;

                        draw_line(buffer, width, height, x_1 as i32, y_1 as i32, x_2 as i32, y_2 as i32, c_rgb);
                    }
                }
                t += t_inc;
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.delay_us = config.delay_us;
        self.cycles = if config.cycles <= 0 { 100 } else { config.cycles };
        self.ncolors = if config.ncolors <= 2 { 64 } else { config.ncolors as usize };
        self.count = if config.count == 0 { 15 } else { config.count };
        self.size = if config.size == 0 { -7 } else { config.size };
        self.init_braid();
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
