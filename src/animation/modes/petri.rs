//! Mold colonies growing in a petri dish.
//
// Copyright (c) 1992-1999 Dan Bornstein.
//
// Permission to use, copy, modify, distribute, and sell this software and its
// documentation for any purpose is hereby granted without fee, provided that
// the above copyright notice appear in all copies and that both that
// copyright notice and this permission notice appear in supporting
// documentation.  No representations are made about the suitability of this
// software for any purpose.  It is provided "as is" without express or
// implied warranty.
//
// Rust port of xlockmore/modes/petri.c.

use crate::rng::RngExt;
use crate::animation::primitives::{put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

#[derive(Clone, Default)]
struct Cell {
    col: u8,
    isnext: bool,
    nextcol: u8,
    next: usize,
    prev: usize,
    speed: f32,
    growth: f32,
    nextspeed: f32,
}

pub struct Petri {
    arr: Vec<Cell>,
    head: usize,
    tail: usize,
    count: u8,
    blastcount: i32,

    width: u32,
    height: u32,
    arr_width: usize,
    arr_height: usize,
    x_size: usize,
    y_size: usize,
    x_offset: usize,
    y_offset: usize,

    diaglim: f32,
    orthlim: f32,
    anychan: f32,
    minorchan: f32,
    instantdeathchan: f32,
    minlifespan: i32,
    maxlifespan: i32,
    minlifespeed: f32,
    maxlifespeed: f32,
    mindeathspeed: f32,
    maxdeathspeed: f32,

    delay_us: u64,
    colors: Vec<Color>,
    bright_colors: Vec<Color>,

    /// Cells whose displayed state changed this tick; render() repaints
    /// only these. Cleared at the top of tick() — the player renders once
    /// per tick for non-clearing modes, so each list is consumed exactly
    /// once.
    dirty: Vec<usize>,
    /// One full-grid repaint is due (fresh buffer after reset/resize, or a
    /// colony wipe from setup_arr). Consumed by render(&self), hence Cell.
    full_repaint: std::cell::Cell<bool>,
}

impl Petri {
    fn newcell(&mut self, idx: usize, col: u8, spf: f32) {
        if self.arr[idx].col == col {
            return;
        }
        self.arr[idx].nextcol = col;
        self.arr[idx].nextspeed = spf;
        self.arr[idx].isnext = true;

        if self.arr[idx].prev == usize::MAX {
            let next_idx = self.arr[self.head].next;
            self.arr[idx].next = next_idx;
            self.arr[idx].prev = self.head;
            self.arr[self.head].next = idx;
            self.arr[next_idx].prev = idx;
        }
    }

    fn killcell(&mut self, idx: usize) {
        let prev = self.arr[idx].prev;
        let next = self.arr[idx].next;
        self.arr[prev].next = next;
        self.arr[next].prev = prev;
        self.arr[idx].prev = usize::MAX;
        self.arr[idx].speed = 0.0;
        // speed 0 changes the displayed shade (bright -> dim, white -> black)
        self.dirty.push(idx);
    }

    fn randblip(&mut self, doit: bool) -> bool {
        let mut rng = crate::rng::rng();
        let mut b = false;

        if !doit {
            let before = self.blastcount;
            self.blastcount -= 1;
            if before >= 0 && rng.random::<f32>() > self.anychan {
                return true;
            }
        }

        let n;
        if self.blastcount < 0 {
            b = true;
            n = 2;
            self.blastcount = rng.random_range(self.minlifespan..=self.maxlifespan);
            if rng.random::<f32>() < self.instantdeathchan {
                self.setup_arr();
                b = false;
            }
        } else if rng.random::<f32>() <= self.minorchan {
            n = 2;
        } else {
            n = rng.random_range(3..=5);
        }

        for _ in 0..n {
            let x = rng.random_range(0..self.arr_width);
            let y = rng.random_range(0..self.arr_height);
            let idx = y * self.arr_width + x;

            let c;
            let s;
            if b {
                c = 0;
                s = rng.random::<f32>() * (self.maxdeathspeed - self.mindeathspeed) + self.mindeathspeed;
            } else {
                if self.count <= 1 {
                    c = 1;
                } else {
                    c = rng.random_range(1..self.count);
                }
                s = rng.random::<f32>() * (self.maxlifespeed - self.minlifespeed) + self.minlifespeed;
            }
            self.newcell(idx, c, s);
        }
        true
    }

    fn setup_arr(&mut self) {
        for c in &mut self.arr {
            c.speed = 0.0;
            c.growth = 0.0;
            c.col = 0;
            c.isnext = false;
            c.next = 0;
            c.prev = usize::MAX; // means not in list
        }
        
        // head and tail are at the end of the array
        self.head = self.arr_width * self.arr_height;
        self.tail = self.head + 1;
        
        self.arr[self.head].next = self.tail;
        self.arr[self.head].prev = self.head;
        self.arr[self.tail].next = self.tail;
        self.arr[self.tail].prev = self.head;

        // The whole dish just went blank.
        self.full_repaint.set(true);

        let mut rng = crate::rng::rng();
        self.blastcount = rng.random_range(self.minlifespan..=self.maxlifespan);
    }

    fn draw_cell(&self, buffer: &mut [u8], width: u32, height: u32, idx: usize) {
        let black = Color::new(255, 0, 0, 0);
        let white = Color::new(255, 255, 255, 255);
        let cell = &self.arr[idx];
        let is_active = cell.speed > 0.0;
        let color = if cell.col == 0 {
            if is_active { white } else { black }
        } else {
            let ci = (cell.col % self.count) as usize;
            if is_active { self.bright_colors[ci] } else { self.colors[ci] }
        };

        let x = idx % self.arr_width;
        let y = idx / self.arr_width;
        let px = (x * self.x_size + self.x_offset) as i32;
        let py = (y * self.y_size + self.y_offset) as i32;
        for i in 0..self.x_size as i32 {
            for j in 0..self.y_size as i32 {
                put_pixel(buffer, width, height, px + i, py + j, color);
            }
        }
    }
}

impl Animation for Petri {
    fn new(config: &AnimConfig) -> Self {
        let mut p = Petri {
            arr: Vec::new(),
            head: 0,
            tail: 0,
            count: 0,
            blastcount: 0,
            width: config.width,
            height: config.height,
            arr_width: 0,
            arr_height: 0,
            x_size: 0,
            y_size: 0,
            x_offset: 0,
            y_offset: 0,
            diaglim: 0.0,
            orthlim: 1.0,
            anychan: 0.0,
            minorchan: 0.0,
            instantdeathchan: 0.0,
            minlifespan: 0,
            maxlifespan: 0,
            minlifespeed: 0.0,
            maxlifespeed: 0.0,
            mindeathspeed: 0.0,
            maxdeathspeed: 0.0,
            delay_us: config.delay_us,
            colors: Vec::new(),
            bright_colors: Vec::new(),
            dirty: Vec::new(),
            full_repaint: std::cell::Cell::new(true),
        };
        p.reset(config);
        p
    }

    fn tick(&mut self) {
        self.dirty.clear();
        let mut current = self.arr[self.head].next;
        let mut to_kill = Vec::new();

        let coords = [
            (-1, -1), (-1, 1), (1, -1), (1, 1),
            (-1, 0), (1, 0), (0, -1), (0, 1)
        ];

        while current != self.tail {
            if self.arr[current].speed == 0.0 {
                current = self.arr[current].next;
                continue;
            }

            self.arr[current].growth += self.arr[current].speed;
            
            let growth = self.arr[current].growth;
            let range = if growth >= self.diaglim {
                0..8
            } else if growth >= self.orthlim {
                4..8
            } else {
                current = self.arr[current].next;
                continue;
            };

            let cx = (current % self.arr_width) as i32;
            let cy = (current / self.arr_width) as i32;
            let col = self.arr[current].col;
            let speed = self.arr[current].speed;

            for i in range {
                let mut nx = cx + coords[i].0;
                let mut ny = cy + coords[i].1;

                if nx < 0 { nx = self.arr_width as i32 - 1; }
                else if nx >= self.arr_width as i32 { nx = 0; }
                if ny < 0 { ny = self.arr_height as i32 - 1; }
                else if ny >= self.arr_height as i32 { ny = 0; }

                let nidx = (ny * self.arr_width as i32 + nx) as usize;
                self.newcell(nidx, col, speed);
            }

            if growth >= self.diaglim {
                to_kill.push(current);
            }

            current = self.arr[current].next;
        }

        for k in to_kill {
            self.killcell(k);
        }

        let is_empty = self.arr[self.head].next == self.tail;
        if !self.randblip(is_empty) {
            return;
        }

        let mut current = self.arr[self.head].next;
        while current != self.tail {
            if self.arr[current].isnext {
                self.arr[current].isnext = false;
                self.arr[current].speed = self.arr[current].nextspeed;
                self.arr[current].growth = 0.0;
                self.arr[current].col = self.arr[current].nextcol;
                self.dirty.push(current);
            }
            current = self.arr[current].next;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        // Note: unlike the old full-rescan version, dead cells are painted
        // black rather than left to the player's background fill —
        // upstream's dish is black, and incremental repaints need to erase.
        if self.full_repaint.replace(false) {
            for idx in 0..self.arr_width * self.arr_height {
                self.draw_cell(buffer, width, height, idx);
            }
        } else {
            for &idx in &self.dirty {
                self.draw_cell(buffer, width, height, idx);
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = crate::rng::rng();

        self.width = config.width;
        self.height = config.height;
        self.delay_us = config.delay_us;
        
        let mut count = config.ncolors.max(2) as u8;
        if count > 128 {
            count = 128;
        }
        self.count = count;

        self.colors.clear();
        self.bright_colors.clear();
        for _ in 0..self.count {
            let h = rng.random::<f32>();
            let color = Color::from_hsl(h, 1.0, 0.5);
            self.bright_colors.push(color);
            self.colors.push(Color::new(
                255,
                color.r / 2,
                color.g / 2,
                color.b / 2,
            ));
        }

        // DEVIATION from xlockmore: upstream defaults to fullrandom=True
        // (petri.c:79), rerolling these per dish (anychan = random^15,
        // instantdeathchan = random^8, random lifespans/speeds) — which lets
        // the whole-dish instant-death reset dominate some runs. We use the
        // fixed fullrandom=False constants (petri.c DEF_*) instead, trading
        // upstream's run-to-run variety for consistently watchable dishes.
        self.diaglim = 1.414 * self.orthlim;
        self.anychan = 0.0015;
        self.minorchan = 0.5;
        self.instantdeathchan = 0.2;
        self.minlifespan = 500;
        self.maxlifespan = 1500;
        self.minlifespeed = 0.04 * self.diaglim;
        self.maxlifespeed = 0.13 * self.diaglim;
        self.mindeathspeed = 0.42 * self.diaglim;
        self.maxdeathspeed = 0.46 * self.diaglim;

        let mut cell_size = config.size.max(1) as usize;
        let mut arr_width = self.width as usize / cell_size;
        let mut arr_height = self.height as usize / cell_size;

        let mem_throttle = 22 * (1 << 20); // 22M
        let mut alloc_size = std::mem::size_of::<Cell>() * arr_width * arr_height;
        while cell_size < self.width as usize / 10 && cell_size < self.height as usize / 10 && alloc_size > mem_throttle {
            cell_size += 1;
            arr_width = self.width as usize / cell_size;
            arr_height = self.height as usize / cell_size;
            alloc_size = std::mem::size_of::<Cell>() * arr_width * arr_height;
        }

        self.arr_width = arr_width;
        self.arr_height = arr_height;
        self.x_size = self.width as usize / self.arr_width;
        self.y_size = self.height as usize / self.arr_height;
        if self.x_size > self.y_size {
            self.x_size = self.y_size;
        } else {
            self.y_size = self.x_size;
        }

        self.x_offset = (self.width as usize - (self.arr_width * self.x_size)) / 2;
        self.y_offset = (self.height as usize - (self.arr_height * self.y_size)) / 2;

        self.arr = vec![Cell::default(); self.arr_width * self.arr_height + 2];
        self.setup_arr();
        self.randblip(true);
    }


    fn frame_delay_us(&self) -> u64 {
        10_000 // match xlockmore default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_upstream_defaults_and_forced_seed_preserves_lifespan() {
        let config = AnimConfig {
            width: 80,
            height: 60,
            size: 4,
            ..AnimConfig::default()
        };
        let mut petri = Petri::new(&config);

        assert_eq!(petri.anychan, 0.0015);
        assert_eq!(petri.minorchan, 0.5);
        assert_eq!(petri.instantdeathchan, 0.2);
        assert_eq!((petri.minlifespan, petri.maxlifespan), (500, 1500));

        let lifespan = petri.blastcount;
        petri.randblip(true);
        assert_eq!(petri.blastcount, lifespan);
    }
}
