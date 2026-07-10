//! Fractal lightning bolds.
//
// Copyright (c) 1996 by Keith Romberg <kromberg@saxe.com>
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
// Rust port of xlockmore/modes/lightning.c.

use rand::Rng;
use crate::animation::{AnimConfig, Animation};
use crate::animation::primitives::{draw_line, Color};

const BOLT_NUMBER: usize = 4;
const BOLT_ITERATION: u32 = 4;
const LONG_FORK_ITERATION: u32 = 3;
const MEDIUM_FORK_ITERATION: u32 = 2;
const SMALL_FORK_ITERATION: u32 = 1;

const WIDTH_VARIATION: i32 = 30;
const HEIGHT_VARIATION: i32 = 15;

const DELAY_TIME_AMOUNT: i32 = 15;
const MULTI_DELAY_TIME_BASE: i32 = 5;

const MAX_WIGGLES: i32 = 16;
const WIGGLE_BASE: i32 = 8;
const WIGGLE_AMOUNT: i32 = 14;

const RANDOM_FORK_PROBILITY: u32 = 4;

const FIRST_LEVEL_STRIKE: i32 = 0;
const LEVEL_ONE_STRIKE: i32 = 1;
const LEVEL_TWO_STRIKE: i32 = 2;

const BOLT_VERTICES: usize = 15; // 2^BOLT_ITERATION - 1
const NUMBER_FORK_VERTICES: usize = 9;

const FLASH_PROBILITY: u32 = 20;
const MAX_FLASH_AMOUNT: i32 = 2;

#[derive(Clone, Copy, Default, Debug)]
struct XPoint {
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, Default)]
struct Fork {
    fork_vertices: [XPoint; NUMBER_FORK_VERTICES],
    num_used: usize,
}

#[derive(Clone, Copy, Default)]
struct LightningBolt {
    end1: XPoint,
    end2: XPoint,
    middle: [XPoint; BOLT_VERTICES],
    fork_number: usize,
    forks_start: [usize; 2],
    branch: [Fork; 2],
    wiggle_number: i32,
    wiggle_amount: i32,
    delay_time: i32,
    flash: bool,
    flash_begin: i32,
    flash_stop: i32,
    visible: bool,
    strike_level: i32,
}

pub struct Lightning {
    bolts: [LightningBolt; BOLT_NUMBER],
    scr_width: u32,
    scr_height: u32,
    multi_strike: usize,
    draw_time: i32,
    stage: i32,
    busy_loop: i32,
    color: Color,
    ncolors: usize,
    delay_us: u64,
}

fn distance(a: XPoint, b: XPoint) -> f32 {
    let dx = (a.x - b.x) as f32;
    let dy = (a.y - b.y) as f32;
    (dx * dx + dy * dy).sqrt()
}

fn setup_multi_strike(rng: &mut impl Rng) -> usize {
    let multi_prob = rng.random_range(0..100);
    if multi_prob < 50 {
        1
    } else if multi_prob < 75 {
        2
    } else if multi_prob < 92 {
        3
    } else {
        BOLT_NUMBER
    }
}

fn flash_duration(total_duration: i32, rng: &mut impl Rng) -> (i32, i32) {
    let mid = total_duration / MAX_FLASH_AMOUNT;
    let d = if total_duration / MAX_FLASH_AMOUNT > 0 {
        rng.random_range(0..(total_duration / MAX_FLASH_AMOUNT)) / 2
    } else {
        0
    };
    (mid - d, mid + d)
}

fn generate(a: XPoint, b: XPoint, iter: u32, verts: &mut [XPoint], vert_index: &mut usize, rng: &mut impl Rng) {
    let mid_x = (a.x + b.x) / 2 + rng.random_range(0..WIDTH_VARIATION.max(1)) - WIDTH_VARIATION / 2;
    let mid_y = (a.y + b.y) / 2 + rng.random_range(0..HEIGHT_VARIATION.max(1)) - HEIGHT_VARIATION / 2;

    if iter == 0 {
        if *vert_index < verts.len() {
            verts[*vert_index] = XPoint { x: mid_x, y: mid_y };
            *vert_index += 1;
        }
        return;
    }
    
    let mid = XPoint { x: mid_x, y: mid_y };
    generate(a, mid, iter - 1, verts, vert_index, rng);
    generate(mid, b, iter - 1, verts, vert_index, rng);
}

fn create_fork(f: &mut Fork, start: XPoint, end: XPoint, level: usize, rng: &mut impl Rng) {
    let mut tmp = 1;
    f.fork_vertices[0] = start;

    if level <= 6 {
        generate(start, end, LONG_FORK_ITERATION, &mut f.fork_vertices, &mut tmp, rng);
        f.num_used = 9;
    } else if level > 6 && level <= 11 {
        generate(start, end, MEDIUM_FORK_ITERATION, &mut f.fork_vertices, &mut tmp, rng);
        f.num_used = 5;
    } else {
        if distance(start, end) > 100.0 {
            generate(start, end, MEDIUM_FORK_ITERATION, &mut f.fork_vertices, &mut tmp, rng);
            f.num_used = 5;
        } else {
            generate(start, end, SMALL_FORK_ITERATION, &mut f.fork_vertices, &mut tmp, rng);
            f.num_used = 3;
        }
    }

    if f.num_used > 0 {
        f.fork_vertices[f.num_used - 1] = end;
    }
}

fn random_storm(bolts: &mut [LightningBolt], scr_width: u32, scr_height: u32, multi_strike: usize, rng: &mut impl Rng) {
    for i in 0..multi_strike {
        let bolt = &mut bolts[i];
        bolt.end1.x = rng.random_range(0..scr_width.max(1) as i32);
        bolt.end1.y = 0;
        bolt.end2.x = rng.random_range(0..scr_width.max(1) as i32);
        bolt.end2.y = scr_height as i32;
        bolt.wiggle_number = WIGGLE_BASE + rng.random_range(0..MAX_WIGGLES.max(1));
        
        if rng.random_range(0..FLASH_PROBILITY.max(1)) <= FLASH_PROBILITY {
            bolt.flash = true;
            let (start, end) = flash_duration(bolt.wiggle_number, rng);
            bolt.flash_begin = start;
            bolt.flash_stop = end;
        } else {
            bolt.flash = false;
            bolt.flash_begin = 0;
            bolt.flash_stop = 0;
        }
        
        bolt.wiggle_amount = WIGGLE_AMOUNT;
        
        if i == 0 {
            bolt.delay_time = rng.random_range(0..DELAY_TIME_AMOUNT.max(1));
        } else {
            bolt.delay_time = rng.random_range(0..DELAY_TIME_AMOUNT.max(1)) + (MULTI_DELAY_TIME_BASE * i as i32);
        }
        
        bolt.strike_level = FIRST_LEVEL_STRIKE;
        
        let mut vert_index = 0;
        generate(bolt.end1, bolt.end2, BOLT_ITERATION, &mut bolt.middle, &mut vert_index, rng);
        
        bolt.fork_number = 0;
        bolt.visible = false;
        
        for j in 0..BOLT_VERTICES {
            if bolt.fork_number >= 2 {
                break;
            }
            if rng.random_range(0..100) < RANDOM_FORK_PROBILITY {
                let p = XPoint {
                    x: rng.random_range(0..scr_width.max(1) as i32),
                    y: scr_height as i32,
                };
                bolt.forks_start[bolt.fork_number] = j;
                create_fork(&mut bolt.branch[bolt.fork_number], bolt.middle[j], p, j, rng);
                bolt.fork_number += 1;
            }
        }
    }
}

fn storm_active(bolts: &[LightningBolt], multi_strike: usize) -> bool {
    bolts.iter().take(multi_strike).any(|b| b.wiggle_number > 0)
}

fn wiggle_line(points: &mut [XPoint], amount: i32, rng: &mut impl Rng) {
    if amount <= 0 {
        return;
    }
    for p in points.iter_mut() {
        p.x += rng.random_range(0..amount) - amount / 2;
        p.y += rng.random_range(0..amount) - amount / 2;
    }
}

fn wiggle_bolt(bolt: &mut LightningBolt, rng: &mut impl Rng) {
    let wiggle_amount = bolt.wiggle_amount;
    wiggle_line(&mut bolt.middle, wiggle_amount, rng);
    
    if wiggle_amount > 0 {
        bolt.end2.x += rng.random_range(0..wiggle_amount) - wiggle_amount / 2;
        bolt.end2.y += rng.random_range(0..wiggle_amount) - wiggle_amount / 2;
    }

    for i in 0..bolt.fork_number {
        let num_used = bolt.branch[i].num_used;
        if num_used > 0 {
            wiggle_line(&mut bolt.branch[i].fork_vertices[..num_used], wiggle_amount, rng);
            let start_idx = bolt.forks_start[i];
            bolt.branch[i].fork_vertices[0] = bolt.middle[start_idx];
        }
    }

    if bolt.wiggle_amount > 1 {
        bolt.wiggle_amount -= 1;
    } else {
        bolt.wiggle_amount = 0;
    }
}

fn update_bolt(bolt: &mut LightningBolt, time_now: i32, rng: &mut impl Rng) {
    wiggle_bolt(bolt, rng);
    
    if bolt.wiggle_amount == 0 && bolt.wiggle_number > 2 {
        bolt.wiggle_number = 0;
    }
    if time_now % 3 == 0 {
        bolt.wiggle_amount += 1;
    }

    bolt.visible = (time_now >= bolt.delay_time && time_now < bolt.flash_begin) || time_now > bolt.flash_stop;

    if time_now == bolt.delay_time {
        bolt.strike_level = FIRST_LEVEL_STRIKE;
    } else if time_now == bolt.delay_time + 1 {
        bolt.strike_level = LEVEL_ONE_STRIKE;
    } else if time_now > bolt.delay_time + 1 && time_now <= bolt.delay_time + bolt.flash_begin - 2 {
        bolt.strike_level = LEVEL_TWO_STRIKE;
    } else if time_now == bolt.delay_time + bolt.flash_begin - 1
        || time_now == bolt.delay_time + bolt.flash_stop + 1
    {
        bolt.strike_level = LEVEL_ONE_STRIKE;
    } else {
        bolt.strike_level = LEVEL_TWO_STRIKE;
    }
}

fn draw_line_with_offset(buffer: &mut [u8], width: u32, height: u32, points: &[XPoint], color: Color, offset: i32) {
    if points.is_empty() {
        return;
    }
    for i in 0..points.len() - 1 {
        if points[i].y <= points[i + 1].y {
            draw_line(
                buffer, width, height,
                points[i].x + offset, points[i].y,
                points[i + 1].x + offset, points[i + 1].y,
                color
            );
        } else {
            if points[i].x < points[i + 1].x {
                draw_line(
                    buffer, width, height,
                    points[i].x + offset, points[i].y + offset,
                    points[i + 1].x + offset, points[i + 1].y + offset,
                    color
                );
            } else {
                draw_line(
                    buffer, width, height,
                    points[i].x - offset, points[i].y + offset,
                    points[i + 1].x - offset, points[i + 1].y + offset,
                    color
                );
            }
        }
    }
}

fn first_strike(bolt: &LightningBolt, buffer: &mut [u8], width: u32, height: u32) {
    let color = Color::new(255, 255, 255, 255);

    draw_line(
        buffer, width, height,
        bolt.end1.x, bolt.end1.y,
        bolt.middle[0].x, bolt.middle[0].y,
        color
    );

    draw_line_with_offset(buffer, width, height, &bolt.middle, color, 0);

    draw_line(
        buffer, width, height,
        bolt.middle[BOLT_VERTICES - 1].x, bolt.middle[BOLT_VERTICES - 1].y,
        bolt.end2.x, bolt.end2.y,
        color
    );

    for i in 0..bolt.fork_number {
        let num_used = bolt.branch[i].num_used;
        if num_used > 0 {
            draw_line_with_offset(buffer, width, height, &bolt.branch[i].fork_vertices[..num_used], color, 0);
        }
    }
}

fn level1_strike(bolt: &LightningBolt, buffer: &mut [u8], width: u32, height: u32, strike_color: Color) {
    let color = strike_color;

    draw_line(
        buffer, width, height,
        bolt.end1.x - 1, bolt.end1.y,
        bolt.middle[0].x - 1, bolt.middle[0].y,
        color
    );
    draw_line_with_offset(buffer, width, height, &bolt.middle, color, -1);
    draw_line(
        buffer, width, height,
        bolt.middle[BOLT_VERTICES - 1].x - 1, bolt.middle[BOLT_VERTICES - 1].y,
        bolt.end2.x - 1, bolt.end2.y,
        color
    );

    draw_line(
        buffer, width, height,
        bolt.end1.x + 1, bolt.end1.y,
        bolt.middle[0].x + 1, bolt.middle[0].y,
        color
    );
    draw_line_with_offset(buffer, width, height, &bolt.middle, color, 1);
    draw_line(
        buffer, width, height,
        bolt.middle[BOLT_VERTICES - 1].x + 1, bolt.middle[BOLT_VERTICES - 1].y,
        bolt.end2.x + 1, bolt.end2.y,
        color
    );

    for i in 0..bolt.fork_number {
        let num_used = bolt.branch[i].num_used;
        if num_used > 0 {
            draw_line_with_offset(buffer, width, height, &bolt.branch[i].fork_vertices[..num_used], color, -1);
            draw_line_with_offset(buffer, width, height, &bolt.branch[i].fork_vertices[..num_used], color, 1);
        }
    }

    first_strike(bolt, buffer, width, height);
}

fn level2_strike(bolt: &LightningBolt, buffer: &mut [u8], width: u32, height: u32, strike_color: Color) {
    let color = strike_color;

    draw_line(
        buffer, width, height,
        bolt.end1.x - 2, bolt.end1.y,
        bolt.middle[0].x - 2, bolt.middle[0].y,
        color
    );
    draw_line_with_offset(buffer, width, height, &bolt.middle, color, -2);
    draw_line(
        buffer, width, height,
        bolt.middle[BOLT_VERTICES - 1].x - 2, bolt.middle[BOLT_VERTICES - 1].y,
        bolt.end2.x - 2, bolt.end2.y,
        color
    );

    draw_line(
        buffer, width, height,
        bolt.end1.x + 2, bolt.end1.y,
        bolt.middle[0].x + 2, bolt.middle[0].y,
        color
    );
    draw_line_with_offset(buffer, width, height, &bolt.middle, color, 2);
    draw_line(
        buffer, width, height,
        bolt.middle[BOLT_VERTICES - 1].x + 2, bolt.middle[BOLT_VERTICES - 1].y,
        bolt.end2.x + 2, bolt.end2.y,
        color
    );

    for i in 0..bolt.fork_number {
        let num_used = bolt.branch[i].num_used;
        if num_used > 0 {
            draw_line_with_offset(buffer, width, height, &bolt.branch[i].fork_vertices[..num_used], color, -2);
            draw_line_with_offset(buffer, width, height, &bolt.branch[i].fork_vertices[..num_used], color, 2);
        }
    }

    level1_strike(bolt, buffer, width, height, strike_color);
}

fn draw_bolt(bolt: &LightningBolt, buffer: &mut [u8], width: u32, height: u32, color: Color) {
    if bolt.strike_level == FIRST_LEVEL_STRIKE {
        first_strike(bolt, buffer, width, height);
    } else if bolt.strike_level == LEVEL_ONE_STRIKE {
        level1_strike(bolt, buffer, width, height, color);
    } else {
        level2_strike(bolt, buffer, width, height, color);
    }
}

impl Animation for Lightning {
    fn new(config: &AnimConfig) -> Self {
        let mut lightning = Lightning {
            bolts: [LightningBolt::default(); BOLT_NUMBER],
            scr_width: config.width,
            scr_height: config.height,
            multi_strike: 1,
            draw_time: 0,
            stage: 0,
            busy_loop: 0,
            color: Color::new(255, 255, 255, 255),
            ncolors: config.ncolors.max(2) as usize,
            delay_us: config.delay_us,
        };
        lightning.reset(config);
        lightning
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();
        self.scr_width = config.width;
        self.scr_height = config.height;
        self.delay_us = config.delay_us;
        self.ncolors = config.ncolors.max(2) as usize;
        
        self.multi_strike = setup_multi_strike(&mut rng);
        random_storm(&mut self.bolts, self.scr_width, self.scr_height, self.multi_strike, &mut rng);
        self.stage = 0;
        self.busy_loop = 0;
        self.draw_time = 0;
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        match self.stage {
            0 => {
                let color_idx = rng.random_range(0..self.ncolors);
                let h = color_idx as f32 / self.ncolors as f32;
                self.color = Color::from_hsl(h, 1.0, 0.5);
                self.draw_time = 0;
                if storm_active(&self.bolts, self.multi_strike) {
                    self.stage = 1;
                } else {
                    self.stage = 4;
                }
            }
            1 => {
                self.stage = 2;
                self.busy_loop = 0;
            }
            2 => {
                self.busy_loop += 1;
                if self.busy_loop > 6 {
                    self.stage = 3;
                    self.busy_loop = 0;
                }
            }
            3 => {
                for i in 0..self.multi_strike {
                    update_bolt(&mut self.bolts[i], self.draw_time, &mut rng);
                }
                self.draw_time += 1;

                if storm_active(&self.bolts, self.multi_strike) {
                    self.stage = 1;
                } else {
                    self.stage = 4;
                }
            }
            4 => {
                self.busy_loop += 1;
                if self.busy_loop > 100 {
                    self.busy_loop = 0;
                    self.multi_strike = setup_multi_strike(&mut rng);
                    random_storm(&mut self.bolts, self.scr_width, self.scr_height, self.multi_strike, &mut rng);
                    self.stage = 0;
                }
            }
            _ => {}
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if self.stage == 1 || self.stage == 2 {
            for i in 0..self.multi_strike {
                let bolt = &self.bolts[i];
                if bolt.visible {
                    draw_bolt(bolt, buffer, width, height, self.color);
                }
            }
        }
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
