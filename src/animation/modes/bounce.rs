//! Bouncing footballs.
//
// Copyright (c) 1988 by Sun Microsystems
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
// Rust port of xlockmore/modes/bounce.c.

use crate::rng::RngExt;
use std::f64::consts::PI;
use crate::animation::primitives::{put_pixel, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

include!("bounce_bitmaps.rs");

const MAX_STRENGTH: i32 = 24;
const FRICTION: i32 = 24;
const PENETRATION: f64 = 0.3;
const SLIPAGE: f64 = 4.0;
const TIME: i32 = 32;
const MINBALLS: i32 = 1;
const MINSIZE: i32 = 1;
const MINGRIDSIZE: i32 = 5;

const ORIENTS: i32 = 4;
const ORIENTCYCLE: i32 = 16;

#[derive(Clone)]
struct BallStruct {
    x: i32,
    y: i32,
    xlast: i32,
    ylast: i32,
    orientlast: i32,
    spincount: i32,
    spindelay: i32,
    spindir: i32,
    orient: i32,
    vx: i32,
    vy: i32,
    vang: i32,
    color: Color,
}

pub struct Bounce {
    width: i32,
    height: i32,
    restartnum: i32,
    nballs: usize,
    xs: i32,
    ys: i32,
    avgsize: i32,
    balls: Vec<BallStruct>,
    delay_us: u64,
    config: AnimConfig,
}

fn dir(x: i32) -> i32 {
    if x >= 0 { 1 } else { 3 }
}

fn spinball(ball: &mut BallStruct, dir_val: i32, vel: i32, avgsize: i32) -> i32 {
    let sign = if (vel * dir_val) >= 0 { 1.0 } else { -1.0 };
    let term = (vel as f64 + sign * ball.spindelay as f64 * ORIENTCYCLE as f64 / (PI * avgsize as f64)) / SLIPAGE;
    let new_vel = vel - term as i32;
    if new_vel != 0 {
        ball.spindir = dir(new_vel * dir_val);
        ball.vang = new_vel * ORIENTCYCLE;
        ball.spindelay = ((PI * avgsize as f64 / (ball.vang.abs() as f64)) as i32) + 1;
    } else {
        ball.spindir = 0;
    }
    new_vel
}

fn hit_left_wall(ball: &mut BallStruct, ytop: i32, height: i32, x_w: i32, side: bool, _xs: i32, ys: i32, avgsize: i32) {
    if ball.x <= x_w && (ball.xlast >= x_w || side) && ball.y >= ytop - ys && ball.y <= ytop + height {
        ball.x = 2 * x_w - ball.x;
        ball.vx = (ball.vx - (ball.vx * FRICTION)) / FRICTION;
        ball.vy = spinball(ball, -1, ball.vy, avgsize);
    }
}

fn hit_right_wall(ball: &mut BallStruct, ytop: i32, height: i32, x_w: i32, side: bool, xs: i32, ys: i32, avgsize: i32) {
    let x_w = x_w - xs;
    if ball.x >= x_w && (ball.xlast <= x_w || side) && ball.y >= ytop - ys && ball.y <= ytop + height {
        ball.x = 2 * x_w - ball.x;
        ball.vx = (ball.vx - (ball.vx * FRICTION)) / FRICTION;
        ball.vy = spinball(ball, 1, ball.vy, avgsize);
    }
}

fn hit_top_wall(ball: &mut BallStruct, xleft: i32, width: i32, y_w: i32, side: bool, xs: i32, _ys: i32, avgsize: i32) {
    if ball.y <= y_w && (ball.ylast >= y_w || side) && ball.x >= xleft - xs && ball.x <= xleft + width {
        ball.y = 2 * y_w - ball.y;
        if y_w == 0 {
            ball.vy = 0;
        } else {
            ball.vy = (ball.vy - (FRICTION * ball.vy)) / FRICTION;
        }
        ball.vx = spinball(ball, 1, ball.vx, avgsize);
    }
}

fn hit_bottom_wall(ball: &mut BallStruct, xleft: i32, width: i32, y_w: i32, side: bool, xs: i32, ys: i32, avgsize: i32) {
    let y_w = y_w - ys;
    if ball.y >= y_w && (ball.ylast <= y_w || side) && ball.x >= xleft - xs && ball.x <= xleft + width {
        ball.y = y_w;
        ball.vy = (ball.vy - (FRICTION * ball.vy)) / FRICTION;
        ball.vx = spinball(ball, -1, ball.vx, avgsize);
    }
}

fn moveball(width: i32, height: i32, xs: i32, ys: i32, avgsize: i32, ball: &mut BallStruct) {
    ball.xlast = ball.x;
    ball.ylast = ball.y;
    ball.orientlast = ball.orient;
    ball.x += ball.vx;

    hit_right_wall(ball, 0, height, width, true, xs, ys, avgsize);
    hit_left_wall(ball, 0, height, 0, true, xs, ys, avgsize);

    ball.vy += 1;
    ball.y += ball.vy;

    hit_top_wall(ball, 0, width, 0, true, xs, ys, avgsize);
    hit_bottom_wall(ball, 0, width, height, true, xs, ys, avgsize);

    if ball.spindir != 0 {
        ball.spincount -= 1;
        if ball.spincount <= 0 {
            ball.orient = (ball.spindir + ball.orient) % ORIENTS;
            ball.spincount = ball.spindelay;
        }
    }
}

fn check_collision(balls: &mut [BallStruct], aball: usize, avgsize: i32) {
    let mut collision_i = None;
    for i in 0..balls.len() {
        if i != aball {
            let dx = (balls[i].x - balls[aball].x) as f64;
            let dy = (balls[i].y - balls[aball].y) as f64;
            let d = (dx * dx + dy * dy).sqrt() as i32;
            if d > 0 && d < avgsize {
                collision_i = Some((i, dx, dy, d));
                break;
            }
        }
    }
    
    if let Some((i, dx, dy, d)) = collision_i {
        let mut amount = avgsize - d;
        let max_pen = (PENETRATION * avgsize as f64) as i32;
        if amount > max_pen {
            amount = max_pen;
        }
        
        let dvx_i = ((amount as f64 * dx) / d as f64) as i32;
        let dvy_i = ((amount as f64 * dy) / d as f64) as i32;
        
        balls[i].vx += dvx_i;
        balls[i].vy += dvy_i;
        balls[i].vx -= balls[i].vx / FRICTION;
        balls[i].vy -= balls[i].vy / FRICTION;
        
        balls[aball].vx -= dvx_i;
        balls[aball].vy -= dvy_i;
        balls[aball].vx -= balls[aball].vx / FRICTION;
        balls[aball].vy -= balls[aball].vy / FRICTION;
        
        let spin = (balls[i].vang - balls[aball].vang) / (2 * avgsize * SLIPAGE as i32);
        balls[i].vang -= spin;
        balls[aball].vang += spin;
        
        balls[i].spindir = dir(balls[i].vang);
        balls[aball].spindir = dir(balls[aball].vang);
        
        if balls[i].vang == 0 {
            balls[i].spindelay = 1;
            balls[i].spindir = 0;
        } else {
            balls[i].spindelay = ((PI * avgsize as f64 / balls[i].vang.abs() as f64) as i32) + 1;
        }
        
        if balls[aball].vang == 0 {
            balls[aball].spindelay = 1;
            balls[aball].spindir = 0;
        } else {
            balls[aball].spindelay = ((PI * avgsize as f64 / balls[aball].vang.abs() as f64) as i32) + 1;
        }
    }
}

fn initial_collide(balls: &[BallStruct], aball: usize, avgsize: i32) -> usize {
    for i in 0..aball {
        let dx = balls[i].x - balls[aball].x;
        let dy = balls[i].y - balls[aball].y;
        let d = ((dx * dx + dy * dy) as f64).sqrt() as i32;
        if d < avgsize {
            return i;
        }
    }
    aball
}

fn draw_football(buffer: &mut [u8], width: u32, height: u32, x: i32, y: i32, xs: i32, ys: i32, color: Color, orient: i32) {
    let bitmap = &BOUNCE_BITMAPS[(orient % ORIENTS) as usize];
    
    for dy in 0..ys {
        let py = y + dy;
        let by = (dy * 64) / ys; 
        if py < 0 || py >= height as i32 { continue; }
        
        for dx in 0..xs {
            let px = x + dx;
            let bx = (dx * 64) / xs; 
            if px < 0 || px >= width as i32 { continue; }
            
            let byte_idx = (by * 64 + bx) as usize / 8;
            let bit_idx = bx % 8;
            let mask = 1 << bit_idx;
            
            if (BOUNCE_MASK[byte_idx] & mask) != 0 {
                if (bitmap[byte_idx] & mask) != 0 {
                    put_pixel(buffer, width, height, px, py, color);
                } else {
                    put_pixel(buffer, width, height, px, py, Color::new(255, 0, 0, 0));
                }
            }
        }
    }
}

impl Animation for Bounce {
    fn new(config: &AnimConfig) -> Self {
        let mut b = Bounce {
            width: config.width as i32,
            height: config.height as i32,
            restartnum: TIME,
            nballs: 0,
            xs: 0,
            ys: 0,
            avgsize: 0,
            balls: Vec::new(),
            delay_us: if config.delay_us == 0 { 5000 } else { config.delay_us },
            config: config.clone(),
        };
        b.reset(config);
        b
    }

    fn tick(&mut self) {
        let mut rng = crate::rng::rng();
        
        for i in 0..self.balls.len() {
            let mut ball = self.balls[i].clone();
            moveball(self.width, self.height, self.xs, self.ys, self.avgsize, &mut ball);
            self.balls[i] = ball;
        }
        
        for i in 0..self.balls.len() {
            check_collision(&mut self.balls, i, self.avgsize);
        }
        
        if rng.random_range(0..TIME.max(1)) == 0 {
            self.restartnum -= 1;
        }
        if self.restartnum <= 0 {
            let cfg = self.config.clone();
            self.reset(&cfg);
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for ball in &self.balls {
            draw_football(buffer, width, height, ball.x, ball.y, self.xs, self.ys, ball.color, ball.orient);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = crate::rng::rng();
        self.width = config.width as i32;
        self.height = config.height as i32;
        self.restartnum = TIME;
        self.config = config.clone();
        
        let c = if config.count == 0 { -10 } else { config.count };
        self.nballs = if c < -MINBALLS {
            rng.random_range(0..(-c - MINBALLS + 1)) as usize + MINBALLS as usize
        } else {
            c.max(MINBALLS) as usize
        };
        
        // AnimConfig's default size is 1 ("unset") — map it to the C's 0
        // ("use the 64x64 default"); a literal 1px football is invisible.
        let size = if config.size == 1 { 0 } else { config.size };
        let min_dim = self.width.min(self.height);

        if size == 0 || MINGRIDSIZE * size > self.width || MINGRIDSIZE * size > self.height {
            self.ys = 64.max(MINSIZE).min(min_dim / MINGRIDSIZE);
            self.xs = self.ys;
            if self.ys < 64 {
                // Not enough room for 64x64, fall back to max possible size
                self.xs = self.ys;
            } else {
                self.xs = 64;
                self.ys = 64;
            }
        } else {
            if size < -MINSIZE {
                let max_val = (-size).min(min_dim / MINGRIDSIZE).max(MINSIZE);
                self.ys = rng.random_range(0..(max_val - MINSIZE + 1)) + MINSIZE;
            } else if size < MINSIZE {
                self.ys = MINSIZE;
            } else {
                self.ys = size.min(min_dim / MINGRIDSIZE).max(MINSIZE);
            }
            self.xs = self.ys;
        }
        
        self.avgsize = (self.xs + self.ys) / 2;
        
        self.balls.clear();
        let mut i = 0;
        let mut tryagain = 0;
        while i < self.nballs {
            let vx = (if rng.random::<bool>() { -1 } else { 1 }) * (rng.random_range(0..MAX_STRENGTH) + 1);
            let x = if vx >= 0 { 0 } else { self.width - self.xs };
            let y = rng.random_range(0..(self.height / 2).max(1));
            
            let color = if config.ncolors > 2 {
                Color::from_hsl(rng.random::<f32>(), 1.0, 0.5)
            } else {
                Color::new(255, 255, 255, 255)
            };
            
            let ball = BallStruct {
                x, y,
                xlast: -1, ylast: 0,
                orientlast: 0,
                spincount: 1, spindelay: 1,
                spindir: 0,
                orient: rng.random_range(0..ORIENTS),
                vx,
                vy: (if rng.random::<bool>() { -1 } else { 1 }) * rng.random_range(0..MAX_STRENGTH),
                vang: 0,
                color,
            };
            
            self.balls.push(ball);
            
            if initial_collide(&self.balls, i, self.avgsize) == i || tryagain >= 8 {
                i += 1;
                tryagain = 0;
            } else {
                self.balls.pop();
                tryagain += 1;
            }
        }
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
