//! Simple exploding bubbles.
//
// Copyright (c) 1998 by Charles Vidal <cvidal@ivsweb.com>
//         http://www.chez.com/vidalc
//         and David Bagley
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
// Rust port of xlockmore/modes/bubble.c.

use rand::Rng;
use std::f32::consts::PI;
use crate::animation::primitives::{put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

const MINSIZE: i32 = 20;
const MINBUBBLES: i32 = 1;

#[derive(Clone)]
struct BubbleType {
    x: i32,
    y: i32,
    life: i32,
}

pub struct Bubble {
    bubbles: Vec<BubbleType>,
    width: u32,
    height: u32,
    direction: i32,
    boil: bool,
    d: i32,
    colors: f32,
    ncolors: usize,
    delay_us: u64,
}

fn draw_hline(buffer: &mut [u8], width: u32, height: u32, x1: i32, x2: i32, y: i32, color: Color) {
    if y < 0 || y >= height as i32 {
        return;
    }
    let start_x = x1.max(0);
    let end_x = x2.min((width as i32) - 1);
    for x in start_x..=end_x {
        put_pixel(buffer, width, height, x, y, color);
    }
}

fn fill_rect(buffer: &mut [u8], width: u32, height: u32, x: i32, y: i32, w: i32, h: i32, color: Color) {
    for iy in y..(y + h) {
        draw_hline(buffer, width, height, x, x + w - 1, iy, color);
    }
}

fn draw_circle(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    cx: i32,
    cy: i32,
    radius: i32,
    color: Color,
) {
    if radius <= 0 {
        return;
    }
    let mut x = radius;
    let mut y = 0;
    let mut err = 0;

    while x >= y {
        put_pixel(buffer, width, height, cx + x, cy + y, color);
        put_pixel(buffer, width, height, cx + y, cy + x, color);
        put_pixel(buffer, width, height, cx - y, cy + x, color);
        put_pixel(buffer, width, height, cx - x, cy + y, color);
        put_pixel(buffer, width, height, cx - x, cy - y, color);
        put_pixel(buffer, width, height, cx - y, cy - x, color);
        put_pixel(buffer, width, height, cx + y, cy - x, color);
        put_pixel(buffer, width, height, cx + x, cy - y, color);

        if err <= 0 {
            y += 1;
            err += 2 * y + 1;
        }
        if err > 0 {
            x -= 1;
            err -= 2 * x + 1;
        }
    }
}

fn fill_circle(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    cx: i32,
    cy: i32,
    radius: i32,
    color: Color,
) {
    if radius <= 0 {
        return;
    }
    let mut x = radius;
    let mut y = 0;
    let mut err = 0;

    while x >= y {
        draw_hline(buffer, width, height, cx - x, cx + x, cy + y, color);
        draw_hline(buffer, width, height, cx - x, cx + x, cy - y, color);
        draw_hline(buffer, width, height, cx - y, cx + y, cy + x, color);
        draw_hline(buffer, width, height, cx - y, cx + y, cy - x, color);

        if err <= 0 {
            y += 1;
            err += 2 * y + 1;
        }
        if err > 0 {
            x -= 1;
            err -= 2 * x + 1;
        }
    }
}

impl Animation for Bubble {
    fn new(config: &AnimConfig) -> Self {
        let mut b = Bubble {
            bubbles: Vec::new(),
            width: config.width,
            height: config.height,
            direction: 0,
            boil: false,
            d: 0,
            colors: 0.0,
            ncolors: 64,
            delay_us: config.delay_us,
        };
        b.reset(config);
        b
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        self.colors += 1.0;
        if self.colors >= self.ncolors as f32 {
            self.colors = 0.0;
        }

        let n = self.bubbles.len();
        for i in 0..n {
            if self.bubbles[i].life != 0 {
                if self.bubbles[i].life + 1 > self.d - rng.random_range(0..16) || self.bubbles[i].y < 0 {
                    self.bubbles[i].life = 0;
                } else {
                    self.bubbles[i].life += 1;
                    if self.boil {
                        self.bubbles[i].y -= self.bubbles[i].life / 2;
                    }
                }
            }
        }

        if n > 0 {
            let i = rng.random_range(0..n);
            if self.bubbles[i].life == 0 {
                self.bubbles[i].x = rng.random_range(0..self.width.max(1) as i32);
                if self.boil {
                    let offset = if self.height >= 16 { rng.random_range(0..=(self.height as i32 / 16)) } else { 0 };
                    self.bubbles[i].y = self.height as i32 - offset;
                } else {
                    self.bubbles[i].y = rng.random_range(0..self.height.max(1) as i32);
                }
                
                self.bubbles[i].life += 1;
                if self.boil {
                    self.bubbles[i].y -= self.bubbles[i].life / 2;
                }
            }
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let h = self.colors / self.ncolors.max(1) as f32;
        let color = if self.ncolors > 2 {
            Color::from_hsl(h, 1.0, 0.5)
        } else {
            Color::new(255, 255, 255, 255)
        };

        for b in &self.bubbles {
            if b.life == 0 {
                continue;
            }

            let diameter = b.life;
            let mut x = b.x;
            if self.boil {
                x += (((diameter + b.x) as f64 / (PI as f64 / 5.0)).cos() * diameter as f64) as i32;
            }

            if diameter < 4 {
                fill_rect(buffer, width, height, x - diameter / 2, b.y - diameter / 2, diameter, diameter, color);
            } else {
                draw_circle(buffer, width, height, x, b.y, diameter / 2, color);
            }

            let diameter4 = diameter / 4;
            if diameter4 > 0 {
                let dx = if self.direction / 2 != 0 { -1 } else { 1 };
                let dy = if self.direction % 2 != 0 { 1 } else { -1 };
                let x4 = x - diameter4 / 2 + dx * diameter / 6;
                let y4 = b.y - diameter4 / 2 + dy * diameter / 6;

                if diameter4 < 4 {
                    fill_rect(buffer, width, height, x4, y4, diameter4, diameter4, color);
                } else {
                    fill_circle(buffer, width, height, x4 + diameter4 / 2, y4 + diameter4 / 2, diameter4 / 2, color);
                }
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;
        self.ncolors = config.ncolors.max(2) as usize;
        // xlockmore default was 100000us (10fps). AnimConfig default is 20000us (50fps).
        // The original bubble animation expects 100ms delay.
        self.delay_us = if config.delay_us > 0 { config.delay_us } else { 100_000 };

        self.direction = rng.random_range(0..4);
        self.boil = rng.random_bool(0.5);

        let size = if config.size == 0 { 100 } else { config.size };
        let min_dim = (self.width.min(self.height) / 2) as i32;
        let max_dim = min_dim.max(MINSIZE);
        
        self.d = if size < -MINSIZE {
            let upper = (-size).min(max_dim);
            rng.random_range(MINSIZE..=(upper.max(MINSIZE)))
        } else if size < MINSIZE {
            MINSIZE
        } else {
            size.min(max_dim)
        };

        let count = if config.count == 0 { 25 } else { config.count };
        let nbubbles = if count < -MINBUBBLES {
            rng.random_range(MINBUBBLES..=(-count).max(MINBUBBLES)) as usize
        } else {
            count.max(MINBUBBLES) as usize
        };

        self.bubbles = vec![BubbleType { x: 0, y: 0, life: 0 }; nbubbles];
        self.colors = rng.random_range(0..self.ncolors.max(1) as u32) as f32;
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
