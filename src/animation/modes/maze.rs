//! A varying maze.
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
// Rust port of xlockmore/modes/maze.c.

use crate::rng::RngExt;

use crate::animation::primitives::{clear_buffer, draw_line, Color};
use crate::animation::{AnimConfig, Animation};

const MINGRIDSIZE: i32 = 2;

const WALL_TOP: u32    = 0x8000;
const WALL_RIGHT: u32  = 0x4000;
const WALL_BOTTOM: u32 = 0x2000;
const WALL_LEFT: u32   = 0x1000;

const DOOR_IN_TOP: u32    = 0x800;
const DOOR_IN_RIGHT: u32  = 0x400;
const DOOR_IN_BOTTOM: u32 = 0x200;
const DOOR_IN_LEFT: u32   = 0x100;
const DOOR_IN_ANY: u32    = 0xF00;

const DOOR_OUT_TOP: u32    = 0x80;
const DOOR_OUT_RIGHT: u32  = 0x40;
const DOOR_OUT_BOTTOM: u32 = 0x20;
const DOOR_OUT_LEFT: u32   = 0x10;


const START_SQUARE: u32 = 0x2;
const END_SQUARE: u32   = 0x1;

#[derive(Clone, Copy, Default)]
struct PathNode {
    x: i32,
    y: i32,
    dir: i8,
}

#[derive(Clone)]
enum DrawOp {
    Clear(Color),
    Line(i32, i32, i32, i32, Color),
    FillRect(i32, i32, i32, i32, Color),
}

fn fill_rectangle(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: Color,
) {
    if w <= 0 || h <= 0 { return; }
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(width as i32);
    let y1 = (y + h).min(height as i32);
    if x0 >= x1 || y0 >= y1 { return; }
    let c_bytes = color.to_argb32().to_ne_bytes();
    for yi in y0..y1 {
        let start = ((yi as usize) * (width as usize) + (x0 as usize)) * 4;
        let end = ((yi as usize) * (width as usize) + (x1 as usize)) * 4;
        let row = &mut buffer[start..end];
        for chunk in row.as_chunks_mut::<4>().0 {
            *chunk = c_bytes;
        }
    }
}

pub struct Maze {
    width: u32,
    height: u32,
    cycles: i32,
    config_size: i32,
    config_ncolors: i32,

    stage: i32,
    time: i32,
    solving: bool,
    
    ncols: i32,
    nrows: i32,
    maze_size: usize,
    xb: i32,
    yb: i32,
    xs: i32,
    ys: i32,
    space: i32,
    threed: i32,
    
    sqnum: i32,
    cur_sq_x: i32,
    cur_sq_y: i32,
    path_length: i32,
    current_path: usize,
    
    start_x: i32,
    start_y: i32,
    start_dir: i32,
    end_x: i32,
    end_y: i32,
    end_dir: i32,
    
    color: Color,
    gray_color: Color,
    
    maze: Vec<u32>,
    move_list: Vec<PathNode>,
    save_path: Vec<PathNode>,
    path: Vec<PathNode>,
    
    ops: Vec<DrawOp>,
    delay_us: u64,
}

impl Animation for Maze {
    fn new(config: &AnimConfig) -> Self {
        let mut m = Maze {
            width: config.width,
            height: config.height,
            cycles: if config.cycles <= 0 { 3000 } else { config.cycles },
            config_size: if config.size == 0 { -40 } else { config.size },
            config_ncolors: if config.ncolors <= 0 { 64 } else { config.ncolors },
            
            stage: 0,
            time: 0,
            solving: false,
            
            ncols: 0,
            nrows: 0,
            maze_size: 0,
            xb: 0,
            yb: 0,
            xs: 0,
            ys: 0,
            space: 0,
            threed: 0,
            
            sqnum: 0,
            cur_sq_x: 0,
            cur_sq_y: 0,
            path_length: 0,
            current_path: 0,
            
            start_x: 0,
            start_y: 0,
            start_dir: 0,
            end_x: 0,
            end_y: 0,
            end_dir: 0,
            
            color: Color::new(255, 255, 255, 255),
            gray_color: Color::new(255, 128, 128, 128),
            
            maze: Vec::new(),
            move_list: Vec::new(),
            save_path: Vec::new(),
            path: Vec::new(),
            
            ops: Vec::new(),
            delay_us: if config.delay_us == 0 { 1000 } else { config.delay_us },
        };
        m.init_maze_state();
        m
    }

    fn tick(&mut self) {
        self.ops.clear();
        
        if self.solving {
            self.solve_maze();
            return;
        }
        
        match self.stage {
            0 => {
                self.ops.push(DrawOp::Clear(Color::new(255, 0, 0, 0)));
                self.set_maze_sizes();
                self.initialize_maze();
                self.create_maze_walls();
                self.stage += 1;
            }
            1 => {
                self.draw_maze_border();
                self.stage += 1;
            }
            2 => {
                self.draw_maze_walls();
                self.stage += 1;
            }
            3 => {
                self.time += 1;
                if self.time > self.cycles / 30 {
                    self.stage += 1;
                }
            }
            4 => {
                self.solve_maze();
                self.stage += 1;
            }
            5 => {
                self.time += 1;
                if self.time > self.cycles / 10 {
                    self.init_maze_state();
                }
            }
            _ => {}
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for op in &self.ops {
            match op {
                DrawOp::Clear(color) => clear_buffer(buffer, *color),
                DrawOp::Line(x0, y0, x1, y1, color) => draw_line(buffer, width, height, *x0, *y0, *x1, *y1, *color),
                DrawOp::FillRect(x, y, w, h, color) => fill_rectangle(buffer, width, height, *x, *y, *w, *h, *color),
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.cycles = if config.cycles <= 0 { 3000 } else { config.cycles };
        self.config_size = if config.size == 0 { -40 } else { config.size };
        self.config_ncolors = if config.ncolors <= 0 { 64 } else { config.ncolors };
        self.delay_us = if config.delay_us == 0 { 1000 } else { config.delay_us };
        self.init_maze_state();
        self.ops.clear();
        self.ops.push(DrawOp::Clear(Color::new(255, 0, 0, 0)));
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}

impl Maze {
    fn init_maze_state(&mut self) {
        self.stage = 0;
        self.time = 0;
        self.solving = false;
        if self.cycles < 4 {
            self.cycles = 4;
        }
    }

    fn set_maze_sizes(&mut self) {
        let mut rng = crate::rng::rng();
        let rand_num = rng.random_range(0..4);
        self.threed = if rand_num == 1 { 1 } else { 0 };
        // Deviation from xlockmore: upstream picks space = 0 for 1 in 4
        // mazes, which makes the solver fill corridors completely — you
        // can't see the path it took. Always keep the 1px inset.
        self.space = 1;
        
        let minsize = self.space * 4 + 3 + self.threed;
        let max_grid_size = ((self.width.min(self.height) as i32) - 1) / MINGRIDSIZE;
        let size = self.config_size;
        
        let ys = if size < -minsize {
            let limit = (-size).min(minsize.max(max_grid_size));
            let range = limit - minsize + 1;
            if range > 0 {
                rng.random_range(0..range) + minsize
            } else {
                minsize
            }
        } else if size < minsize {
            if size == 0 {
                minsize.max(max_grid_size)
            } else {
                minsize
            }
        } else {
            size.min(minsize.max(max_grid_size))
        };
        self.ys = ys;
        self.xs = ys;
        
        self.ncols = ((self.width as i32 - 1) / self.xs).max(MINGRIDSIZE);
        self.nrows = ((self.height as i32 - 1) / self.ys).max(MINGRIDSIZE);
        self.xb = (self.width as i32 - self.ncols * self.xs) / 2;
        self.yb = (self.height as i32 - self.nrows * self.ys) / 2;
        self.maze_size = (self.ncols * self.nrows) as usize;
        
        self.maze.resize(self.maze_size, 0);
        self.move_list.resize(self.maze_size + 2, PathNode::default());
        self.save_path.resize(self.maze_size + 2, PathNode::default());
        self.path.resize(self.maze_size + 2, PathNode::default());
    }

    fn initialize_maze(&mut self) {
        let mut rng = crate::rng::rng();
        if self.config_ncolors > 2 {
            let h = rng.random_range(0.0..1.0);
            self.color = Color::from_hsl(h, 1.0, 0.5);
            self.gray_color = Color::from_hsl(h, 1.0, 0.25);
        } else {
            self.color = Color::new(255, 255, 255, 255);
            self.gray_color = Color::new(255, 128, 128, 128);
        }
        
        for i in 0..self.maze_size {
            self.maze[i] = 0;
        }
        
        let nrows = self.nrows;
        let ncols = self.ncols;
        for i in 0..ncols {
            self.maze[(i * nrows) as usize] |= WALL_TOP;
            self.maze[(i * nrows + nrows - 1) as usize] |= WALL_BOTTOM;
        }
        for j in 0..nrows {
            self.maze[((ncols - 1) * nrows + j) as usize] |= WALL_RIGHT;
            self.maze[j as usize] |= WALL_LEFT;
        }
        
        let mut wall = rng.random_range(0..4);
        let mut i = 0;
        let mut j = 0;
        match wall {
            0 => { i = rng.random_range(0..ncols); j = 0; },
            1 => { i = ncols - 1; j = rng.random_range(0..nrows); },
            2 => { i = rng.random_range(0..ncols); j = nrows - 1; },
            3 => { i = 0; j = rng.random_range(0..nrows); },
            _ => {}
        }
        
        self.maze[(i * nrows + j) as usize] |= START_SQUARE;
        self.maze[(i * nrows + j) as usize] |= DOOR_IN_TOP >> wall;
        self.maze[(i * nrows + j) as usize] &= !(WALL_TOP >> wall);
        self.cur_sq_x = i;
        self.cur_sq_y = j;
        self.start_x = i;
        self.start_y = j;
        self.start_dir = wall;
        self.sqnum = 0;
        
        wall = (wall + 2) % 4;
        match wall {
            0 => { i = rng.random_range(0..ncols); j = 0; },
            1 => { i = ncols - 1; j = rng.random_range(0..nrows); },
            2 => { i = rng.random_range(0..ncols); j = nrows - 1; },
            3 => { i = 0; j = rng.random_range(0..nrows); },
            _ => {}
        }
        
        self.maze[(i * nrows + j) as usize] |= END_SQUARE;
        self.maze[(i * nrows + j) as usize] |= DOOR_OUT_TOP >> wall;
        self.maze[(i * nrows + j) as usize] &= !(WALL_TOP >> wall);
        self.end_x = i;
        self.end_y = j;
        self.end_dir = wall;
    }

    fn backup(&mut self) -> i32 {
        self.sqnum -= 1;
        if self.sqnum >= 0 {
            self.cur_sq_x = self.move_list[self.sqnum as usize].x;
            self.cur_sq_y = self.move_list[self.sqnum as usize].y;
        }
        self.sqnum
    }

    fn choose_door(&mut self) -> i32 {
        let mut candidates = [0, 0, 0, 0];
        let mut num_candidates = 0;
        let mut rng = crate::rng::rng();
        
        let cx = self.cur_sq_x;
        let cy = self.cur_sq_y;
        let nr = self.nrows;
        
        let sq = self.maze[(cx * nr + cy) as usize];
        
        if (sq & DOOR_IN_TOP) == 0 && (sq & DOOR_OUT_TOP) == 0 && (sq & WALL_TOP) == 0 {
            if (self.maze[(cx * nr + cy - 1) as usize] & DOOR_IN_ANY) != 0 {
                self.maze[(cx * nr + cy) as usize] |= WALL_TOP;
                self.maze[(cx * nr + cy - 1) as usize] |= WALL_BOTTOM;
            } else {
                candidates[num_candidates] = 0;
                num_candidates += 1;
            }
        }
        if (sq & DOOR_IN_RIGHT) == 0 && (sq & DOOR_OUT_RIGHT) == 0 && (sq & WALL_RIGHT) == 0 {
            if (self.maze[((cx + 1) * nr + cy) as usize] & DOOR_IN_ANY) != 0 {
                self.maze[(cx * nr + cy) as usize] |= WALL_RIGHT;
                self.maze[((cx + 1) * nr + cy) as usize] |= WALL_LEFT;
            } else {
                candidates[num_candidates] = 1;
                num_candidates += 1;
            }
        }
        if (sq & DOOR_IN_BOTTOM) == 0 && (sq & DOOR_OUT_BOTTOM) == 0 && (sq & WALL_BOTTOM) == 0 {
            if (self.maze[(cx * nr + cy + 1) as usize] & DOOR_IN_ANY) != 0 {
                self.maze[(cx * nr + cy) as usize] |= WALL_BOTTOM;
                self.maze[(cx * nr + cy + 1) as usize] |= WALL_TOP;
            } else {
                candidates[num_candidates] = 2;
                num_candidates += 1;
            }
        }
        if (sq & DOOR_IN_LEFT) == 0 && (sq & DOOR_OUT_LEFT) == 0 && (sq & WALL_LEFT) == 0 {
            if (self.maze[((cx - 1) * nr + cy) as usize] & DOOR_IN_ANY) != 0 {
                self.maze[(cx * nr + cy) as usize] |= WALL_LEFT;
                self.maze[((cx - 1) * nr + cy) as usize] |= WALL_RIGHT;
            } else {
                candidates[num_candidates] = 3;
                num_candidates += 1;
            }
        }
        
        if num_candidates == 0 { return -1; }
        if num_candidates == 1 { return candidates[0]; }
        candidates[rng.random_range(0..num_candidates)]
    }

    fn create_maze_walls(&mut self) {
        loop {
            let sq = self.sqnum as usize;
            self.move_list[sq].x = self.cur_sq_x;
            self.move_list[sq].y = self.cur_sq_y;
            self.move_list[sq].dir = -1;
            
            let mut newdoor;
            loop {
                newdoor = self.choose_door();
                if newdoor == -1 {
                    if self.backup() == -1 {
                        return;
                    }
                } else {
                    break;
                }
            }
            
            let cx = self.cur_sq_x;
            let cy = self.cur_sq_y;
            let nr = self.nrows;
            
            self.maze[(cx * nr + cy) as usize] |= DOOR_OUT_TOP >> newdoor;
            
            match newdoor {
                0 => self.cur_sq_y -= 1,
                1 => self.cur_sq_x += 1,
                2 => self.cur_sq_y += 1,
                3 => self.cur_sq_x -= 1,
                _ => {}
            }
            self.sqnum += 1;
            
            let cx2 = self.cur_sq_x;
            let cy2 = self.cur_sq_y;
            self.maze[(cx2 * nr + cy2) as usize] |= DOOR_IN_TOP >> ((newdoor + 2) % 4);
            
            if (self.maze[(cx2 * nr + cy2) as usize] & END_SQUARE) != 0 {
                self.path_length = self.sqnum;
                for i in 0..self.path_length as usize {
                    self.save_path[i] = self.move_list[i];
                }
            }
        }
    }

    fn draw_solid_square(&mut self, color: Color, i: i32, j: i32, dir: i32) {
        match dir {
            0 => {
                self.ops.push(DrawOp::FillRect(
                    self.xb + 3 * self.space + self.xs * i,
                    self.yb - 3 * self.space + self.ys * j,
                    self.xs + 1 - 6 * self.space - self.threed,
                    self.ys + 1 - self.threed,
                    color
                ));
            }
            1 => {
                self.ops.push(DrawOp::FillRect(
                    self.xb + 3 * self.space + self.xs * i,
                    self.yb + 3 * self.space + self.ys * j,
                    self.xs + 1 - self.threed,
                    self.ys + 1 - 6 * self.space - self.threed,
                    color
                ));
            }
            2 => {
                self.ops.push(DrawOp::FillRect(
                    self.xb + 3 * self.space + self.xs * i,
                    self.yb + 3 * self.space + self.ys * j,
                    self.xs + 1 - 6 * self.space - self.threed,
                    self.ys + 1 - self.threed,
                    color
                ));
            }
            3 => {
                self.ops.push(DrawOp::FillRect(
                    self.xb - 3 * self.space + self.xs * i,
                    self.yb + 3 * self.space + self.ys * j,
                    self.xs + 1 - self.threed,
                    self.ys + 1 - 6 * self.space - self.threed,
                    color
                ));
            }
            _ => {}
        }
    }

    fn draw_wall(&mut self, i: i32, j: i32, dir: i32) {
        let color = self.color;
        match dir {
            0 => {
                self.ops.push(DrawOp::Line(
                    self.xb + self.xs * i, self.yb + self.ys * j,
                    self.xb + self.xs * (i + 1), self.yb + self.ys * j,
                    color
                ));
            }
            1 => {
                self.ops.push(DrawOp::Line(
                    self.xb + self.xs * (i + 1), self.yb + self.ys * j,
                    self.xb + self.xs * (i + 1), self.yb + self.ys * (j + 1),
                    color
                ));
            }
            2 => {
                self.ops.push(DrawOp::Line(
                    self.xb + self.xs * i, self.yb + self.ys * (j + 1),
                    self.xb + self.xs * (i + 1), self.yb + self.ys * (j + 1),
                    color
                ));
            }
            3 => {
                self.ops.push(DrawOp::Line(
                    self.xb + self.xs * i, self.yb + self.ys * j,
                    self.xb + self.xs * i, self.yb + self.ys * (j + 1),
                    color
                ));
            }
            _ => {}
        }
    }

    fn draw_maze_border(&mut self) {
        for i in 0..self.ncols {
            if (self.maze[(i * self.nrows) as usize] & WALL_TOP) != 0 {
                let color = self.color;
                self.ops.push(DrawOp::Line(
                    self.xb + self.xs * i, self.yb,
                    self.xb + self.xs * (i + 1), self.yb,
                    color
                ));
            }
            if (self.maze[(i * self.nrows + self.nrows - 1) as usize] & WALL_BOTTOM) != 0 {
                let color = self.color;
                self.ops.push(DrawOp::Line(
                    self.xb + self.xs * i, self.yb + self.ys * self.nrows,
                    self.xb + self.xs * (i + 1), self.yb + self.ys * self.nrows,
                    color
                ));
            }
        }
        for j in 0..self.nrows {
            if (self.maze[((self.ncols - 1) * self.nrows + j) as usize] & WALL_RIGHT) != 0 {
                let color = self.color;
                self.ops.push(DrawOp::Line(
                    self.xb + self.xs * self.ncols, self.yb + self.ys * j,
                    self.xb + self.xs * self.ncols, self.yb + self.ys * (j + 1),
                    color
                ));
            }
            if (self.maze[j as usize] & WALL_LEFT) != 0 {
                let color = self.color;
                self.ops.push(DrawOp::Line(
                    self.xb, self.yb + self.ys * j,
                    self.xb, self.yb + self.ys * (j + 1),
                    color
                ));
            }
        }
        
        let c = self.color;
        self.draw_solid_square(c, self.start_x, self.start_y, self.start_dir);
        self.draw_solid_square(c, self.end_x, self.end_y, self.end_dir);
    }

    fn draw_maze_walls(&mut self) {
        for i in 0..self.ncols {
            let isize = i * self.nrows;
            for j in 0..self.nrows {
                if (self.maze[(isize + j) as usize] & WALL_TOP) != 0 {
                    self.draw_wall(i, j, 0);
                }
                if (self.maze[(isize + j) as usize] & WALL_RIGHT) != 0 {
                    self.draw_wall(i, j, 1);
                }
            }
        }
    }

    fn enter_square(&mut self, n: usize) {
        let p_x = self.path[n].x;
        let p_y = self.path[n].y;
        let p_dir = self.path[n].dir;
        let c = self.color;
        self.draw_solid_square(c, p_x, p_y, p_dir as i32);
        
        self.path[n + 1].dir = -1;
        match p_dir {
            0 => { self.path[n + 1].x = p_x; self.path[n + 1].y = p_y - 1; },
            1 => { self.path[n + 1].x = p_x + 1; self.path[n + 1].y = p_y; },
            2 => { self.path[n + 1].x = p_x; self.path[n + 1].y = p_y + 1; },
            3 => { self.path[n + 1].x = p_x - 1; self.path[n + 1].y = p_y; },
            _ => {}
        }
    }

    fn solve_maze(&mut self) {
        if !self.solving {
            let nr = self.nrows;
            self.maze[(self.start_x * nr + self.start_y) as usize] |= WALL_TOP >> self.start_dir;
            self.maze[(self.end_x * nr + self.end_y) as usize] |= WALL_TOP >> self.end_dir;
            
            self.current_path = 0;
            self.path[self.current_path].x = self.end_x;
            self.path[self.current_path].y = self.end_y;
            self.path[self.current_path].dir = -1;
            
            self.solving = true;
        }
        
        let c_path = self.current_path;
        self.path[c_path].dir += 1;
        if self.path[c_path].dir >= 4 {
            if c_path == 0 {
                self.path[c_path].dir = -1;
                return;
            }
            
            let px = self.path[c_path].x;
            let py = self.path[c_path].y;
            let pdir = (self.path[c_path - 1].dir as i32 + 2) % 4;
            let gray = self.gray_color;
            self.draw_solid_square(gray, px, py, pdir);
            
            self.current_path -= 1;
            let pdir2 = self.path[self.current_path].dir as i32;
            let px2 = self.path[self.current_path].x;
            let py2 = self.path[self.current_path].y;
            self.draw_solid_square(gray, px2, py2, pdir2);
        } else {
            let px = self.path[c_path].x;
            let py = self.path[c_path].y;
            let pdir = self.path[c_path].dir as u32;
            let sq = self.maze[(px * self.nrows + py) as usize];
            
            let is_wall = (sq & (WALL_TOP >> pdir)) != 0;
            let going_back = if c_path == 0 { false } else {
                pdir == ((self.path[c_path - 1].dir as u32 + 2) % 4)
            };
            
            if !is_wall && !going_back {
                self.enter_square(c_path);
                self.current_path += 1;
                
                let px2 = self.path[self.current_path].x;
                let py2 = self.path[self.current_path].y;
                let pdir2 = (self.path[self.current_path - 1].dir as i32 + 2) % 4;
                let c = self.color;
                self.draw_solid_square(c, px2, py2, pdir2);
                
                if (self.maze[(px2 * self.nrows + py2) as usize] & START_SQUARE) != 0 {
                    self.solving = false;
                }
            }
        }
    }
}
