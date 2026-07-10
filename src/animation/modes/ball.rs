//! Bouncing balls with random drawing functions that leave a trail.
//
// Copyright (c) 1995 by Heath Rice <rice@asl.dl.nec.com>.
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
// Rust port of xlockmore/modes/ball.c.

use rand::Rng;
use crate::animation::primitives::{put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

const MINBALLS: i32 = 1;
const MINSIZE: i32 = 2;
const MINGRIDSIZE: i32 = 4;
const SPEED: i32 = 156;
const SQLIMIT: i32 = (SPEED * SPEED) / (30 * 30);
const RATE: i32 = 600;
const TD: i32 = 10;

#[derive(Clone, Copy)]
struct BallType {
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
    rad: i32,
    bounce: i32,
    dyold: i32,
    def: bool,
    color_bg: Color,
    color_fg: Color,
}

struct CircleOp {
    x: i32,
    y: i32,
    rad: i32,
    color: Color,
}

pub struct Ball {
    bt: Vec<BallType>,
    draw_ops: Vec<CircleOp>,
    rad: i32,
    width: u32,
    height: u32,
    bounce: i32,
    nballs: usize,
    delay_us: u64,
}

impl Animation for Ball {
    fn new(config: &AnimConfig) -> Self {
        let mut s = Ball {
            bt: Vec::new(),
            draw_ops: Vec::new(),
            rad: 0,
            width: config.width,
            height: config.height,
            bounce: 85,
            nballs: 0,
            delay_us: config.delay_us,
        };
        s.reset(config);
        s
    }

    fn tick(&mut self) {
        self.draw_ops.clear();
        let mut rng = rand::rng();

        for i in 0..self.nballs {
            if !self.bt[i].def {
                randomball(&mut self.bt, i, self.width, self.height, self.bounce, self.rad, &mut rng);
            }
        }

        for i in 0..self.nballs {
            if !self.bt[i].def {
                continue;
            }

            self.draw_ops.push(CircleOp {
                x: self.bt[i].x,
                y: self.bt[i].y,
                rad: self.bt[i].rad,
                color: self.bt[i].color_bg,
            });

            let mut redo = 0;
            let speed_sq = self.bt[i].dx * self.bt[i].dx + self.bt[i].dy * self.bt[i].dy;
            if speed_sq < SQLIMIT && self.bt[i].y >= (self.height as i32 - 3) {
                redo = 25;
            }

            let mut dx;
            let mut dy;

            loop {
                dx = (TD * self.bt[i].dx) / RATE;
                dy = (TD * self.bt[i].dy) / RATE;

                if redo > 5 {
                    redo = 0;
                    randomball(&mut self.bt, i, self.width, self.height, self.bounce, self.rad, &mut rng);
                    if !self.bt[i].def {
                        break;
                    }
                    self.draw_ops.push(CircleOp {
                        x: self.bt[i].x,
                        y: self.bt[i].y,
                        rad: self.bt[i].rad,
                        color: self.bt[i].color_fg,
                    });
                }

                let mut n = i;
                let status = inwin(&self.bt, self.width, self.height, dx + self.bt[i].x, dy + self.bt[i].y, &mut n, self.bt[i].rad);
                match status {
                    0 => { // NONE
                        self.bt[i].x += dx;
                        self.bt[i].y += dy;
                        redo = 0;
                    }
                    1 => { // V
                        self.bt[i].dx = ((self.bt[i].bounce as f32 * self.bt[i].dx as f32) / 100.0) as i32;
                        redo += 1;
                    }
                    2 => { // H
                        self.bt[i].dy = ((self.bt[i].bounce as f32 * self.bt[i].dy as f32) / 100.0) as i32;
                        if self.bt[i].bounce != 100 {
                            if self.bt[i].y >= (self.height as i32 - 3) && self.bt[i].dy > -250 && self.bt[i].dy < 0 {
                                redo = 15;
                            }
                            if self.bt[i].y >= (self.height as i32 - 3) && self.bt[i].dy == self.bt[i].dyold {
                                redo = 10;
                            }
                            self.bt[i].dyold = self.bt[i].dy;
                        }
                        redo += 1;
                    }
                    3 => { // B
                        if redo > 5 {
                            if self.bt[i].y >= (self.height as i32 - 3) {
                                randomball(&mut self.bt, i, self.width, self.height, self.bounce, self.rad, &mut rng);
                                redo = 0;
                            } else if self.bt[n].y >= (self.height as i32 - 3) {
                                randomball(&mut self.bt, n, self.width, self.height, self.bounce, self.rad, &mut rng);
                                redo = 0;
                            } else {
                                redo = 0;
                            }
                        } else {
                            collided(&mut self.bt, i, n, &mut dx, &mut dy, self.width, self.height, &mut self.draw_ops);
                            redo = 0;
                        }
                    }
                    _ => {}
                }

                if redo == 0 {
                    break;
                }
            }

            self.bt[i].dy += TD;

            if self.bt[i].def {
                self.draw_ops.push(CircleOp {
                    x: self.bt[i].x,
                    y: self.bt[i].y,
                    rad: self.bt[i].rad,
                    color: self.bt[i].color_fg,
                });
            }
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for op in &self.draw_ops {
            fill_circle(buffer, width, height, op.x, op.y, op.rad, op.color);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();
        self.width = config.width;
        self.height = config.height;
        self.delay_us = config.delay_us;
        self.bounce = 85;

        let count = config.count;
        let mut nballs = if count == 0 { 10 } else { count };
        if nballs < -MINBALLS {
            nballs = rng.random_range(MINBALLS..=(-nballs - MINBALLS + 1) + MINBALLS);
        } else if nballs < MINBALLS {
            nballs = MINBALLS;
        }
        self.nballs = nballs as usize;

        let size = if config.size == 0 { -100 } else { config.size };
        let min_dim = std::cmp::min(self.width, self.height) as i32;
        let max_rad = std::cmp::max(MINSIZE, min_dim / MINGRIDSIZE);

        if size == 0 || MINGRIDSIZE * size > self.width as i32 || MINGRIDSIZE * size > self.height as i32 {
            self.rad = max_rad;
        } else {
            if size < -MINSIZE {
                let limit = std::cmp::min(-size, max_rad);
                self.rad = rng.random_range(MINSIZE..=limit);
            } else if size < MINSIZE {
                self.rad = MINSIZE;
            } else {
                self.rad = std::cmp::min(size, max_rad);
            }
        }

        self.bt.clear();
        self.bt.resize(self.nballs, BallType {
            x: 0, y: 0, dx: 0, dy: 0, rad: 0, bounce: 0, dyold: 0, def: false,
            color_bg: Color::new(255, 0, 0, 0),
            color_fg: Color::new(255, 0, 0, 0),
        });

        for i in 0..self.nballs {
            randomball(&mut self.bt, i, self.width, self.height, self.bounce, self.rad, &mut rng);
        }
    }

    fn clears_each_frame(&self) -> bool {
        false
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}

fn fill_circle(buffer: &mut [u8], width: u32, height: u32, cx: i32, cy: i32, diameter: i32, color: Color) {
    let radius = diameter as f32 / 2.0;
    let r2 = radius * radius;
    let bound = radius.ceil() as i32;
    for y in -bound..=bound {
        let dy = y as f32;
        for x in -bound..=bound {
            let dx = x as f32;
            if dx * dx + dy * dy <= r2 {
                put_pixel(buffer, width, height, cx + x, cy + y, color);
            }
        }
    }
}

fn inwin(balls: &[BallType], width: u32, height: u32, x: i32, y: i32, n: &mut usize, rad: i32) -> u32 {
    if x < 0 || x > width as i32 {
        return 1; // V
    }
    if y < 0 || y > height as i32 {
        return 2; // H
    }

    for i in 0..balls.len() {
        if i == *n || !balls[i].def {
            continue;
        }
        let diffx = balls[i].x - x;
        let diffy = balls[i].y - y;
        let r1 = rad / 2;
        let r2 = balls[i].rad / 2;

        if diffx * diffx + diffy * diffy < 2 * (r1 * r1 + r2 * r2) {
            *n = i;
            return 3; // B
        }
    }
    0 // NONE
}

fn randomball(balls: &mut [BallType], i: usize, width: u32, height: u32, bounce_cfg: i32, rad_cfg: i32, rng: &mut impl rand::Rng) {
    let mut attempts = 0;

    let mut bn = if bounce_cfg == -2 {
        30 + rng.random_range(0..69)
    } else {
        bounce_cfg
    };
    bn = bn.clamp(0, 100);
    bn = -bn;

    let mut dx = rng.random_range(0..(2 * SPEED)) + SPEED;
    let dy = rng.random_range(0..(2 * SPEED)) + (SPEED / 2);

    if rng.random_range(0..9) % 2 == 1 {
        dx = -dx;
    }

    let mut x;
    let mut y;

    loop {
        x = rng.random_range(0..width as i32);
        y = 0;
        attempts += 1;
        if attempts > 5 {
            balls[i].def = false;
            return;
        }

        let mut dummy = i;
        if inwin(balls, width, height, x, y, &mut dummy, rad_cfg) != 0 { continue; }
        dummy = i;
        if inwin(balls, width, height, x + dx, y + dy, &mut dummy, rad_cfg) != 0 { continue; }

        break;
    }

    balls[i].def = true;
    balls[i].x = x;
    balls[i].y = y;
    balls[i].dx = dx;
    balls[i].dy = dy;
    balls[i].bounce = bn;
    balls[i].dyold = 0;
    balls[i].rad = rad_cfg;

    let h1 = rng.random_range(0.0..1.0);
    let h2 = rng.random_range(0.0..1.0);
    balls[i].color_bg = Color::from_hsl(h1, 1.0, 0.5);
    balls[i].color_fg = Color::from_hsl(h2, 1.0, 0.5);
}

fn get_two_mut<T>(slice: &mut [T], i: usize, j: usize) -> (&mut T, &mut T) {
    assert!(i != j);
    if i < j {
        let (left, right) = slice.split_at_mut(j);
        (&mut left[i], &mut right[0])
    } else {
        let (left, right) = slice.split_at_mut(i);
        (&mut right[0], &mut left[j])
    }
}

fn collided(balls: &mut [BallType], i: usize, n: usize, dx: &mut i32, dy: &mut i32, _width: u32, _height: u32, draw_ops: &mut Vec<CircleOp>) {
    let (bti, btn) = get_two_mut(balls, i, n);
    let rx1 = bti.x as f32;
    let ry1 = bti.y as f32;
    let vx1 = bti.dx as f32;
    let vy1 = bti.dy as f32;

    let rx2 = btn.x as f32;
    let ry2 = btn.y as f32;
    let vx2 = btn.dx as f32;
    let vy2 = btn.dy as f32;

    let mut ux1 = rx1 - rx2;
    let mut uy1 = ry1 - ry2;
    let mag1 = (ux1 * ux1 + uy1 * uy1).sqrt();
    if mag1 > 0.0 {
        ux1 /= mag1;
        uy1 /= mag1;
    }

    let mut ux2 = rx2 - rx1;
    let mut uy2 = ry2 - ry1;
    let mag2 = (ux2 * ux2 + uy2 * uy2).sqrt();
    if mag2 > 0.0 {
        ux2 /= mag2;
        uy2 /= mag2;
    }

    let imp = (vx1 * ux2 + vy1 * uy2) + (vx2 * ux1 + vy2 * uy1);

    let nvx1 = vx1 + imp * ux1;
    let nvy1 = vy1 + imp * uy1;

    let nvx2 = vx2 + imp * ux2;
    let nvy2 = vy2 + imp * uy2;

    bti.dx = nvx1 as i32;
    bti.dy = nvy1 as i32;
    btn.dx = nvx2 as i32;
    btn.dy = nvy2 as i32;

    draw_ops.push(CircleOp {
        x: btn.x,
        y: btn.y,
        rad: btn.rad,
        color: btn.color_bg,
    });

    let dx_btn = (TD * btn.dx) / RATE;
    let dy_btn = (TD * btn.dy) / RATE;
    btn.x += dx_btn / 2;
    btn.y += dy_btn / 2;

    draw_ops.push(CircleOp {
        x: btn.x,
        y: btn.y,
        rad: btn.rad,
        color: btn.color_fg,
    });

    *dx = (TD * bti.dx) / RATE;
    *dy = (TD * bti.dy) / RATE;
    bti.x += *dx / 2;
    bti.y += *dy / 2;
}
