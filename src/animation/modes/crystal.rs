//! Polygons moving according to plane group rules.
//
// Copyright (c) 1997 by Jouk Jansen <joukj AT hrem.nano.tudelft.nl>
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
// Rust port of xlockmore/modes/crystal.c.

use std::f32::consts::PI;
use rand::Rng;

use crate::animation::primitives::{clear_buffer, draw_line, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

const M_PI: f32 = PI;
const PI_RAD: f32 = M_PI / 180.0;

const DEF_NUM_ATOM: i32 = 10;
const DEF_SIZ_ATOM: i32 = 10;

static CENTRO: [bool; 17] = [
    false, true, false, false, false, true, true, true, true, true, true, true, false, false, false, true, true
];

static PRIMITIVE: [bool; 17] = [
    true, true, true, true, false, true, true, true, false, true, true, true, true, true, true, true, true
];

static NUMOPS: [i16; 34] = [
    1, 0, 1, 0, 9, 7, 2, 0, 9, 7, 9, 7, 4, 2, 5, 3, 9, 7, 8, 6, 10, 6, 8, 4, 16, 13, 19, 13, 16, 10, 19, 13, 19, 13
];

static OPERATION: [i16; 114] = [
    1, 0, 0, 1, 0, 0,
    -1, 0, 0, 1, 0, 1,
    -1, 0, 0, 1, 1, 0,
    1, 0, 0, 1, 0, 0,
    -1, 0, 0, 1, 1, 1,
    1, 0, 0, 1, 1, 1,
    0, -1, 1, 0, 0, 0,
    1, 0, 0, 1, 0, 0,
    -1, 0, 0, 1, 0, 0,
    0, 1, 1, 0, 0, 0,
    -1, 0, -1, 1, 0, 0,
    1, -1, 0, -1, 0, 0,
    0, 1, 1, 0, 0, 0,
    0, -1, 1, -1, 0, 0,
    -1, 1, -1, 0, 0, 0,
    1, 0, 0, 1, 0, 0,
    0, -1, -1, 0, 0, 0,
    -1, 1, 0, 1, 0, 0,
    1, 0, 1, -1, 0, 0
];

#[derive(Clone, Copy, Default)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Clone)]
struct CrystalAtom {
    colour: usize,
    x0: i32,
    y0: i32,
    velocity: [i32; 2],
    angle: f32,
    velocity_a: f32,
    num_point: usize,
    at_type: i32,
    size_at: i32,
    xy: [Point; 5],
}

pub struct Crystal {
    width: u32,
    height: u32,
    vertical: bool,
    win_width: i32,
    win_height: i32,
    num_atom: i32,
    planegroup: usize,
    a: i32,
    b: i32,
    offset_w: i32,
    offset_h: i32,
    nx: i32,
    ny: i32,
    gamma: f32,
    cos_g: f32,
    sin_g: f32,
    atoms: Vec<CrystalAtom>,
    unit_cell: bool,
    grid_cell: bool,
    cycle_p: bool,
    direction: i32,
    invert: i32,
    colors: Vec<Color>,
    color_offset: usize,
    delay_us: u64,
    time: i32,
    cycles: i32,
    target_cell_x: i32,
    target_cell_y: i32,
}

impl Crystal {
    fn trans_coor(xyp: &[Point], new_xyp: &mut [Point], num_points: usize, gamma: f32) {
        for i in 0..=num_points {
            new_xyp[i].x = xyp[i].x + (xyp[i].y as f32 * ((gamma - 90.0) * PI_RAD).sin()) as i32;
            new_xyp[i].y = (xyp[i].y as f32 / ((gamma - 90.0) * PI_RAD).cos()) as i32;
        }
    }

    fn trans_coor_back(xyp: &[Point], new_xyp: &mut [Point], num_points: usize, cos_g: f32, sin_g: f32, offset_w: i32, offset_h: i32, winheight: i32, invert: i32, new_vertical: bool) {
        for i in 0..=num_points {
            if new_vertical {
                new_xyp[i].x = (xyp[i].y as f32 * cos_g) as i32 + offset_h;
                new_xyp[i].y = xyp[i].x - (xyp[i].y as f32 * sin_g) as i32 + offset_w;
                if invert != 0 {
                    new_xyp[i].x = winheight - new_xyp[i].x;
                }
            } else {
                new_xyp[i].y = (xyp[i].y as f32 * cos_g) as i32 + offset_h;
                new_xyp[i].x = xyp[i].x - (xyp[i].y as f32 * sin_g) as i32 + offset_w;
                if invert != 0 {
                    new_xyp[i].y = winheight - new_xyp[i].y;
                }
            }
        }
    }

    fn crystal_setupatom(atom0: &mut CrystalAtom, gamma: f32) {
        let mut xy = [Point::default(); 5];
        let y0 = (atom0.y0 as f32 * ((gamma - 90.0) * PI_RAD).cos()) as i32;
        let x0 = atom0.x0 - (atom0.y0 as f32 * ((gamma - 90.0) * PI_RAD).sin()) as i32;
        match atom0.at_type {
            0 => { // rectangles
                let sz = atom0.size_at as f32;
                xy[0].x = x0 + (2.0 * sz * atom0.angle.cos()) as i32 + (sz * atom0.angle.sin()) as i32;
                xy[0].y = y0 + (sz * atom0.angle.cos()) as i32 - (2.0 * sz * atom0.angle.sin()) as i32;
                xy[1].x = x0 + (2.0 * sz * atom0.angle.cos()) as i32 - (sz * atom0.angle.sin()) as i32;
                xy[1].y = y0 - (sz * atom0.angle.cos()) as i32 - (2.0 * sz * atom0.angle.sin()) as i32;
                xy[2].x = x0 - (2.0 * sz * atom0.angle.cos()) as i32 - (sz * atom0.angle.sin()) as i32;
                xy[2].y = y0 - (sz * atom0.angle.cos()) as i32 + (2.0 * sz * atom0.angle.sin()) as i32;
                xy[3].x = x0 - (2.0 * sz * atom0.angle.cos()) as i32 + (sz * atom0.angle.sin()) as i32;
                xy[3].y = y0 + (sz * atom0.angle.cos()) as i32 + (2.0 * sz * atom0.angle.sin()) as i32;
                xy[4] = xy[0];
                Self::trans_coor(&xy, &mut atom0.xy, 4, gamma);
            }
            1 => { // squares
                let sz = atom0.size_at as f32;
                xy[0].x = x0 + (1.5 * sz * atom0.angle.cos()) as i32 + (1.5 * sz * atom0.angle.sin()) as i32;
                xy[0].y = y0 + (1.5 * sz * atom0.angle.cos()) as i32 - (1.5 * sz * atom0.angle.sin()) as i32;
                xy[1].x = x0 + (1.5 * sz * atom0.angle.cos()) as i32 - (1.5 * sz * atom0.angle.sin()) as i32;
                xy[1].y = y0 - (1.5 * sz * atom0.angle.cos()) as i32 - (1.5 * sz * atom0.angle.sin()) as i32;
                xy[2].x = x0 - (1.5 * sz * atom0.angle.cos()) as i32 - (1.5 * sz * atom0.angle.sin()) as i32;
                xy[2].y = y0 - (1.5 * sz * atom0.angle.cos()) as i32 + (1.5 * sz * atom0.angle.sin()) as i32;
                xy[3].x = x0 - (1.5 * sz * atom0.angle.cos()) as i32 + (1.5 * sz * atom0.angle.sin()) as i32;
                xy[3].y = y0 + (1.5 * sz * atom0.angle.cos()) as i32 + (1.5 * sz * atom0.angle.sin()) as i32;
                xy[4] = xy[0];
                Self::trans_coor(&xy, &mut atom0.xy, 4, gamma);
            }
            2 => { // triangles
                let sz = atom0.size_at as f32;
                xy[0].x = x0 + (1.5 * sz * atom0.angle.sin()) as i32;
                xy[0].y = y0 + (1.5 * sz * atom0.angle.cos()) as i32;
                xy[1].x = x0 + (1.5 * sz * atom0.angle.cos()) as i32 - (1.5 * sz * atom0.angle.sin()) as i32;
                xy[1].y = y0 - (1.5 * sz * atom0.angle.cos()) as i32 - (1.5 * sz * atom0.angle.sin()) as i32;
                xy[2].x = x0 - (1.5 * sz * atom0.angle.cos()) as i32 - (1.5 * sz * atom0.angle.sin()) as i32;
                xy[2].y = y0 - (1.5 * sz * atom0.angle.cos()) as i32 + (1.5 * sz * atom0.angle.sin()) as i32;
                xy[3] = xy[0];
                Self::trans_coor(&xy, &mut atom0.xy, 3, gamma);
            }
            _ => {}
        }
    }

    fn fill_polygon(&self, buffer: &mut [u8], width: u32, height: u32, pts: &[Point], color: Color, xor: bool) {
        if pts.is_empty() { return; }
        let mut min_y = pts[0].y;
        let mut max_y = pts[0].y;
        for p in pts {
            if p.y < min_y { min_y = p.y; }
            if p.y > max_y { max_y = p.y; }
        }
        
        let start_y = min_y.max(0);
        let end_y = max_y.min(height as i32 - 1);

        let mut intersections = Vec::with_capacity(8);
        for y in start_y..=end_y {
            intersections.clear();
            for i in 0..pts.len() {
                let p1 = pts[i];
                let p2 = pts[(i + 1) % pts.len()];
                
                if (p1.y <= y && p2.y > y) || (p2.y <= y && p1.y > y) {
                    let dy = p2.y - p1.y;
                    let dx = p2.x - p1.x;
                    if dy != 0 {
                        let x = p1.x + (y - p1.y) * dx / dy;
                        intersections.push(x);
                    }
                }
            }
            intersections.sort_unstable();
            if intersections.len() >= 2 {
                let x1 = intersections[0].max(0);
                let x2 = intersections[intersections.len() - 1].min(width as i32 - 1);
                for x in x1..=x2 {
                    if xor {
                        self.xor_pixel(buffer, width, height, x, y, color);
                    } else {
                        put_pixel(buffer, width, height, x, y, color);
                    }
                }
            }
        }
    }

    #[inline(always)]
    fn xor_pixel(&self, buffer: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            return;
        }
        let stride = (width * 4) as usize;
        let idx = (y as usize) * stride + (x as usize) * 4;
        if idx + 3 < buffer.len() {
            buffer[idx] ^= color.b;
            buffer[idx + 1] ^= color.g;
            buffer[idx + 2] ^= color.r;
            buffer[idx + 3] = 255;
        }
    }

    fn draw_line_vertical_aware(&self, buffer: &mut [u8], width: u32, height: u32, x_1: i32, y_1: i32, x_2: i32, y_2: i32, color: Color) {
        if self.vertical {
            draw_line(buffer, width, height, y_1, x_1, y_2, x_2, color);
        } else {
            draw_line(buffer, width, height, x_1, y_1, x_2, y_2, color);
        }
    }

    fn crystal_drawatom(&self, buffer: &mut [u8], width: u32, height: u32, atom0: &CrystalAtom) {
        let color = self.colors[(atom0.colour + self.color_offset) % self.colors.len()];
        
        for j in NUMOPS[2 * self.planegroup + 1] as usize .. NUMOPS[2 * self.planegroup] as usize {
            let mut xy = [Point::default(); 5];
            let mut xy_1 = [Point::default(); 5];
            let mut new_xy = [Point::default(); 5];
            
            let op0 = OPERATION[j * 6] as i32;
            let op1 = OPERATION[j * 6 + 1] as i32;
            let op2 = OPERATION[j * 6 + 2] as i32;
            let op3 = OPERATION[j * 6 + 3] as i32;
            let op4 = OPERATION[j * 6 + 4] as i32;
            let op5 = OPERATION[j * 6 + 5] as i32;
            
            let mut xtrans = op0 * atom0.x0 + op1 * atom0.y0 + (op4 as f32 * self.a as f32 / 2.0) as i32;
            let mut ytrans = op2 * atom0.x0 + op3 * atom0.y0 + (op5 as f32 * self.b as f32 / 2.0) as i32;
            
            if xtrans < 0 {
                if xtrans < -self.a { xtrans = 2 * self.a; } else { xtrans = self.a; }
            } else if xtrans >= self.a {
                xtrans = -self.a;
            } else {
                xtrans = 0;
            }
            
            if ytrans < 0 {
                ytrans = self.b;
            } else if ytrans >= self.b {
                ytrans = -self.b;
            } else {
                ytrans = 0;
            }
            
            for k in 0..atom0.num_point {
                xy[k].x = op0 * atom0.xy[k].x + op1 * atom0.xy[k].y + (op4 as f32 * self.a as f32 / 2.0) as i32 + xtrans;
                xy[k].y = op2 * atom0.xy[k].x + op3 * atom0.xy[k].y + (op5 as f32 * self.b as f32 / 2.0) as i32 + ytrans;
            }
            xy[atom0.num_point] = xy[0];
            
            for l in 0..self.nx {
                for m in 0..self.ny {
                    for k in 0..=atom0.num_point {
                        xy_1[k].x = xy[k].x + l * self.a;
                        xy_1[k].y = xy[k].y + m * self.b;
                    }
                    Self::trans_coor_back(&xy_1, &mut new_xy, atom0.num_point, self.cos_g, self.sin_g, self.offset_w, self.offset_h, self.win_height, self.invert, self.vertical);
                    self.fill_polygon(buffer, width, height, &new_xy[..atom0.num_point], color, true);
                }
            }
            
            if CENTRO[self.planegroup] {
                for k in 0..=atom0.num_point {
                    xy[k].x = self.a - xy[k].x;
                    xy[k].y = self.b - xy[k].y;
                }
                for l in 0..self.nx {
                    for m in 0..self.ny {
                        for k in 0..=atom0.num_point {
                            xy_1[k].x = xy[k].x + l * self.a;
                            xy_1[k].y = xy[k].y + m * self.b;
                        }
                        Self::trans_coor_back(&xy_1, &mut new_xy, atom0.num_point, self.cos_g, self.sin_g, self.offset_w, self.offset_h, self.win_height, self.invert, self.vertical);
                        self.fill_polygon(buffer, width, height, &new_xy[..atom0.num_point], color, true);
                    }
                }
            }
            
            if !PRIMITIVE[self.planegroup] {
                if xy[atom0.num_point].x >= (self.a as f32 / 2.0) as i32 {
                    xtrans = (-self.a as f32 / 2.0) as i32;
                } else {
                    xtrans = (self.a as f32 / 2.0) as i32;
                }
                if xy[atom0.num_point].y >= (self.b as f32 / 2.0) as i32 {
                    ytrans = (-self.b as f32 / 2.0) as i32;
                } else {
                    ytrans = (self.b as f32 / 2.0) as i32;
                }
                for k in 0..=atom0.num_point {
                    xy[k].x += xtrans;
                    xy[k].y += ytrans;
                }
                for l in 0..self.nx {
                    for m in 0..self.ny {
                        for k in 0..=atom0.num_point {
                            xy_1[k].x = xy[k].x + l * self.a;
                            xy_1[k].y = xy[k].y + m * self.b;
                        }
                        Self::trans_coor_back(&xy_1, &mut new_xy, atom0.num_point, self.cos_g, self.sin_g, self.offset_w, self.offset_h, self.win_height, self.invert, self.vertical);
                        self.fill_polygon(buffer, width, height, &new_xy[..atom0.num_point], color, true);
                    }
                }
                
                if CENTRO[self.planegroup] {
                    let mut xy1 = [Point::default(); 5];
                    for k in 0..=atom0.num_point {
                        xy1[k].x = self.a - xy[k].x;
                        xy1[k].y = self.b - xy[k].y;
                    }
                    for l in 0..self.nx {
                        for m in 0..self.ny {
                            for k in 0..=atom0.num_point {
                                xy_1[k].x = xy1[k].x + l * self.a;
                                xy_1[k].y = xy1[k].y + m * self.b;
                            }
                            Self::trans_coor_back(&xy_1, &mut new_xy, atom0.num_point, self.cos_g, self.sin_g, self.offset_w, self.offset_h, self.win_height, self.invert, self.vertical);
                            self.fill_polygon(buffer, width, height, &new_xy[..atom0.num_point], color, true);
                        }
                    }
                }
            }
        }
    }

    fn draw_grid(&self, buffer: &mut [u8], width: u32, height: u32) {
        if !self.unit_cell { return; }

        let color = Color::new(255, 255, 255, 255); // White
        let cos_g = self.cos_g;
        let sin_g = self.sin_g;

        if self.grid_cell {
            let mut y_coor1: i32;
            let mut y_coor2: i32;
            if self.invert != 0 {
                y_coor1 = self.win_height - self.offset_h;
                y_coor2 = self.win_height - self.offset_h;
            } else {
                y_coor1 = self.offset_h;
                y_coor2 = self.offset_h;
            }
            self.draw_line_vertical_aware(buffer, width, height, self.offset_w, y_coor1, self.offset_w + self.nx * self.a, y_coor2, color);

            if self.invert != 0 {
                y_coor1 = self.win_height - self.offset_h;
                y_coor2 = self.win_height - (self.ny as f32 * self.b as f32 * cos_g) as i32 - self.offset_h;
            } else {
                y_coor1 = self.offset_h;
                y_coor2 = (self.ny as f32 * self.b as f32 * cos_g) as i32 + self.offset_h;
            }
            self.draw_line_vertical_aware(buffer, width, height, self.offset_w, y_coor1, (self.offset_w as f32 - self.ny as f32 * self.b as f32 * sin_g) as i32, y_coor2, color);

            let inx = self.nx;
            for iny in 1..=self.ny {
                let yc1: i32;
                let yc2: i32;
                if self.invert != 0 {
                    yc1 = self.win_height - (iny as f32 * self.b as f32 * cos_g) as i32 - self.offset_h;
                    yc2 = self.win_height - (iny as f32 * self.b as f32 * cos_g) as i32 - self.offset_h;
                } else {
                    yc1 = (iny as f32 * self.b as f32 * cos_g) as i32 + self.offset_h;
                    yc2 = (iny as f32 * self.b as f32 * cos_g) as i32 + self.offset_h;
                }
                self.draw_line_vertical_aware(buffer, width, height,
                    (self.offset_w as f32 + inx as f32 * self.a as f32 - (iny as f32 * self.b as f32 * sin_g)) as i32, yc1,
                    (self.offset_w as f32 - iny as f32 * self.b as f32 * sin_g) as i32, yc2, color);
            }

            let iny = self.ny;
            for inx in 1..=self.nx {
                let yc1: i32;
                let yc2: i32;
                if self.invert != 0 {
                    yc1 = self.win_height - (iny as f32 * self.b as f32 * cos_g) as i32 - self.offset_h;
                    yc2 = self.win_height - self.offset_h;
                } else {
                    yc1 = (iny as f32 * self.b as f32 * cos_g) as i32 + self.offset_h;
                    yc2 = self.offset_h;
                }
                self.draw_line_vertical_aware(buffer, width, height,
                    (self.offset_w as f32 + inx as f32 * self.a as f32 - (iny as f32 * self.b as f32 * sin_g)) as i32, yc1,
                    self.offset_w + inx * self.a, yc2, color);
            }
        } else {
            let inx = self.target_cell_x;
            let iny = self.target_cell_y;

            let yc1: i32;
            let yc2: i32;
            if self.invert != 0 {
                yc1 = self.win_height - (iny as f32 * self.b as f32 * cos_g) as i32 - self.offset_h;
                yc2 = self.win_height - ((iny + 1) as f32 * self.b as f32 * cos_g) as i32 - self.offset_h;
            } else {
                yc1 = (iny as f32 * self.b as f32 * cos_g) as i32 + self.offset_h;
                yc2 = ((iny + 1) as f32 * self.b as f32 * cos_g) as i32 + self.offset_h;
            }
            self.draw_line_vertical_aware(buffer, width, height,
                self.offset_w + inx * self.a - (iny as f32 * self.b as f32 * sin_g) as i32, yc1,
                self.offset_w + (inx + 1) * self.a - (iny as f32 * self.b as f32 * sin_g) as i32, yc1, color);
            self.draw_line_vertical_aware(buffer, width, height,
                self.offset_w + inx * self.a - (iny as f32 * self.b as f32 * sin_g) as i32, yc1,
                self.offset_w + inx * self.a - ((iny + 1) as f32 * self.b as f32 * sin_g) as i32, yc2, color);
            self.draw_line_vertical_aware(buffer, width, height,
                self.offset_w + (inx + 1) * self.a - (iny as f32 * self.b as f32 * sin_g) as i32, yc1,
                self.offset_w + (inx + 1) * self.a - ((iny + 1) as f32 * self.b as f32 * sin_g) as i32, yc2, color);
            self.draw_line_vertical_aware(buffer, width, height,
                self.offset_w + inx * self.a - ((iny + 1) as f32 * self.b as f32 * sin_g) as i32, yc2,
                self.offset_w + (inx + 1) * self.a - ((iny + 1) as f32 * self.b as f32 * sin_g) as i32, yc2, color);
        }
    }
}

impl Animation for Crystal {
    fn new(config: &AnimConfig) -> Self {
        let mut c = Crystal {
            width: config.width,
            height: config.height,
            vertical: false,
            win_width: 0,
            win_height: 0,
            num_atom: 0,
            planegroup: 0,
            a: 0,
            b: 0,
            offset_w: 0,
            offset_h: 0,
            nx: 0,
            ny: 0,
            gamma: 0.0,
            cos_g: 0.0,
            sin_g: 0.0,
            atoms: Vec::new(),
            unit_cell: false,
            grid_cell: false,
            cycle_p: true,
            direction: 1,
            invert: 0,
            colors: Vec::new(),
            color_offset: 0,
            delay_us: 60_000,
            time: 0,
            cycles: 200,
            target_cell_x: 0,
            target_cell_y: 0,
        };
        c.reset(config);
        c
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        if self.cycle_p {
            if self.direction > 0 {
                self.color_offset = (self.color_offset + 1) % self.colors.len().max(1);
            } else {
                if self.color_offset == 0 {
                    self.color_offset = self.colors.len().saturating_sub(1);
                } else {
                    self.color_offset -= 1;
                }
            }
            if rng.random_range(0..1000) == 0 {
                self.direction = -self.direction;
            }
        }

        for i in 0..self.num_atom as usize {
            let mut atom0 = self.atoms[i].clone();
            
            atom0.velocity[0] += rng.random_range(0..3) - 1;
            atom0.velocity[0] = atom0.velocity[0].clamp(-20, 20);
            atom0.velocity[1] += rng.random_range(0..3) - 1;
            atom0.velocity[1] = atom0.velocity[1].clamp(-20, 20);
            
            atom0.x0 += atom0.velocity[0];
            if atom0.x0 < 0 {
                atom0.x0 += self.a;
            } else if atom0.x0 >= self.a {
                atom0.x0 -= self.a;
            }
            
            atom0.y0 += atom0.velocity[1];
            if atom0.y0 < 0 {
                atom0.y0 += self.b;
            } else if atom0.y0 >= self.b {
                atom0.y0 -= self.b;
            }
            
            atom0.velocity_a += (rng.random_range(0..1001) as f32 - 500.0) / 2000.0;
            atom0.angle += atom0.velocity_a;
            
            Self::crystal_setupatom(&mut atom0, self.gamma);
            self.atoms[i] = atom0;
        }
        
        self.time += 1;
        if self.time > self.cycles {
            let cfg = AnimConfig {
                width: self.width,
                height: self.height,
                count: 0,
                cycles: self.cycles,
                size: 0,
                ncolors: self.colors.len() as i32,
                delay_us: self.delay_us,
                max_fps: 0, // internal re-init config; only the player reads max_fps
            };
            self.reset(&cfg);
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        clear_buffer(buffer, Color::new(255, 0, 0, 0));
        self.draw_grid(buffer, width, height);
        for atom in &self.atoms {
            self.crystal_drawatom(buffer, width, height, atom);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();
        
        self.width = config.width;
        self.height = config.height;
        self.delay_us = if config.delay_us == 0 { 60_000 } else { config.delay_us };
        self.cycles = if config.cycles <= 0 { 200 } else { config.cycles };
        self.time = 0;
        
        self.direction = if rng.random::<bool>() { 1 } else { -1 };
        
        let nx_opt = -3;
        let ny_opt = -3;
        let centre_opt = false;
        let maxsize_opt = false;

        self.unit_cell = rng.random::<bool>();
        self.vertical = rng.random::<bool>();
        if self.unit_cell {
            self.grid_cell = rng.random::<bool>();
        } else {
            self.grid_cell = false;
        }
        
        const MIN_CELL: i32 = 200;
        
        if self.vertical {
            self.win_height = (self.width as i32 + 1).max(MIN_CELL);
            self.win_width = (self.height as i32 + 1).max(MIN_CELL);
        } else {
            self.win_width = (self.width as i32 + 1).max(MIN_CELL);
            self.win_height = (self.height as i32 + 1).max(MIN_CELL);
        }
        
        let mut cell_min = (self.win_width / 2 + 1).min(MIN_CELL);
        cell_min = cell_min.min(self.win_height / 2 + 1);
        
        self.planegroup = rng.random_range(0..17);
        self.invert = rng.random_range(0..2);
        
        if self.planegroup > 11 {
            self.gamma = 120.0;
        } else if self.planegroup < 2 {
            self.gamma = 60.0 + rng.random_range(0..60) as f32;
        } else {
            self.gamma = 90.0;
        }
        
        let mut neqv = (NUMOPS[2 * self.planegroup] - NUMOPS[2 * self.planegroup + 1]) as i32;
        if CENTRO[self.planegroup] { neqv *= 2; }
        if !PRIMITIVE[self.planegroup] { neqv *= 2; }
        
        if nx_opt > 0 {
            self.nx = nx_opt;
        } else if nx_opt < 0 {
            self.nx = rng.random_range(0..(-nx_opt)) + 1;
        } else {
            self.nx = 1;
        }
        
        if self.planegroup > 8 {
            self.ny = self.nx;
        } else if ny_opt > 0 {
            self.ny = ny_opt;
        } else if ny_opt < 0 {
            self.ny = rng.random_range(0..(-ny_opt)) + 1;
        } else {
            self.ny = 1;
        }
        
        neqv = neqv * self.nx * self.ny;
        
        // crystal.c ModStruct count default is -40 (random 1..=40 atoms).
        // An earlier -500 here overpopulated the lattice ~12x.
        let count = if config.count == 0 { -40 } else { config.count };
        
        if count == 0 {
            self.num_atom = DEF_NUM_ATOM;
        } else if count < 0 {
            self.num_atom = rng.random_range(0..(-count)) + 1;
        } else {
            self.num_atom = count;
        }
        
        if neqv > 1 {
            self.num_atom = self.num_atom / neqv + 1;
        }
        
        if maxsize_opt || self.width < 92 || self.height < 92 {
            if self.planegroup < 13 {
                self.gamma = 90.0;
                self.offset_w = 0;
                self.offset_h = 0;
                if self.planegroup < 10 {
                    self.b = self.win_height;
                    self.a = self.win_width;
                } else {
                    self.b = self.win_height.min(self.win_width);
                    self.a = self.b;
                }
            } else {
                self.gamma = 120.0;
                self.a = (self.win_width as f32 * 2.0 / 3.0) as i32;
                self.b = self.a;
                self.offset_h = (self.b as f32 * 0.25 * ((self.gamma - 90.0) * PI_RAD).cos()) as i32;
                self.offset_w = (self.b as f32 * 0.5) as i32;
            }
        } else {
            self.offset_w = -1;
            while self.offset_w < 4 || ((self.offset_w as f32 - self.b as f32 * ((self.gamma - 90.0) * PI_RAD).sin()) as i32) < 4 {
                let div = ((self.gamma - 90.0) * PI_RAD).cos();
                let max_b = (self.win_height as f32 / div) as i32;
                if max_b > cell_min {
                    self.b = rng.random_range(0..(max_b - cell_min)) + cell_min;
                } else {
                    self.b = cell_min;
                }
                
                if self.planegroup > 8 {
                    self.a = self.b;
                } else {
                    if self.win_width > cell_min {
                        self.a = rng.random_range(0..(self.win_width - cell_min)) + cell_min;
                    } else {
                        self.a = cell_min;
                    }
                }
                self.offset_w = ((self.win_width as f32 - (self.a as f32 - self.b as f32 * ((self.gamma - 90.0) * PI_RAD).sin())) / 2.0) as i32;
            }
            self.offset_h = ((self.win_height as f32 - self.b as f32 * ((self.gamma - 90.0) * PI_RAD).cos()) / 2.0) as i32;
            if !centre_opt {
                if self.offset_h > 0 {
                    self.offset_h = rng.random_range(0..(2 * self.offset_h).max(1));
                }
                self.offset_w = (self.win_width as f32 - self.a as f32 - self.b as f32 * ((self.gamma - 90.0) * PI_RAD).sin().abs()) as i32;
                if self.gamma > 90.0 {
                    if self.offset_w > 0 {
                        self.offset_w = rng.random_range(0..self.offset_w.max(1)) + (self.b as f32 * ((self.gamma - 90.0) * PI_RAD).sin()) as i32;
                    } else {
                        self.offset_w = (self.b as f32 * ((self.gamma - 90.0) * PI_RAD).sin()) as i32;
                    }
                } else if self.offset_w > 0 {
                    self.offset_w = rng.random_range(0..self.offset_w.max(1));
                } else {
                    self.offset_w = 0;
                }
            }
        }

        self.cos_g = ((self.gamma - 90.0) * PI_RAD).cos();
        self.sin_g = ((self.gamma - 90.0) * PI_RAD).sin();

        let mut size_atom = ((self.a as f32 / 40.0) as i32 + 1).min((self.b as f32 / 40.0) as i32 + 1);
        let mut m_size = config.size;
        if m_size == 0 { m_size = -15; }
        
        if m_size < size_atom {
            if m_size < -size_atom {
                size_atom = -size_atom;
            } else {
                size_atom = m_size;
            }
        }
        
        self.a /= self.nx.max(1);
        self.b /= self.ny.max(1);
        
        let ncolors = if config.ncolors <= 0 { 100 } else { config.ncolors.max(2) };
        self.colors.clear();
        // crystal.c picks a colormap style per run: 1 in 10 make_random_colormap
        // (independent random hues), else half make_uniform_colormap (evenly
        // spaced hue wheel), else make_smooth_colormap (ramp between random
        // anchors). A fixed rainbow ramp made every run look alike.
        if rng.random_range(0..10) == 0 {
            for _ in 0..ncolors {
                self.colors.push(Color::from_hsl(rng.random::<f32>(), 1.0, 0.5));
            }
        } else if rng.random::<bool>() {
            for i in 0..ncolors {
                self.colors.push(Color::from_hsl(i as f32 / ncolors as f32, 1.0, 0.5));
            }
        } else {
            let h1 = rng.random::<f32>();
            let h2 = rng.random::<f32>();
            for i in 0..ncolors {
                let t = i as f32 / ncolors as f32;
                let h = (h1 + (h2 - h1) * t).rem_euclid(1.0);
                self.colors.push(Color::from_hsl(h, 1.0, 0.5));
            }
        }
        self.color_offset = 0;
        self.cycle_p = rng.random_range(0..8) != 0;
        
        self.atoms.clear();
        for _ in 0..self.num_atom {
            let mut atom0 = CrystalAtom {
                colour: 0,
                x0: 0,
                y0: 0,
                velocity: [0, 0],
                angle: 0.0,
                velocity_a: 0.0,
                num_point: 0,
                at_type: 0,
                size_at: 0,
                xy: [Point::default(); 5],
            };
            atom0.colour = rng.random_range(0..ncolors as usize);
            atom0.x0 = rng.random_range(0..self.a.max(1));
            atom0.y0 = rng.random_range(0..self.b.max(1));
            atom0.velocity[0] = rng.random_range(0..7) - 3;
            atom0.velocity[1] = rng.random_range(0..7) - 3;
            atom0.velocity_a = (rng.random_range(0..7) - 3) as f32 * PI_RAD;
            atom0.angle = rng.random_range(0..90) as f32 * PI_RAD;
            atom0.at_type = rng.random_range(0..3);
            if size_atom == 0 {
                atom0.size_at = DEF_SIZ_ATOM;
            } else if size_atom > 0 {
                atom0.size_at = size_atom;
            } else {
                atom0.size_at = rng.random_range(0..(-size_atom).max(1)) + 1;
            }
            atom0.size_at += 1;
            if atom0.at_type == 2 {
                atom0.num_point = 3;
            } else {
                atom0.num_point = 4;
            }
            Self::crystal_setupatom(&mut atom0, self.gamma);
            self.atoms.push(atom0);
        }
        
        self.target_cell_x = rng.random_range(0..self.nx.max(1));
        self.target_cell_y = rng.random_range(0..self.ny.max(1));
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        60_000 // Match xlockmore default
    }
}
