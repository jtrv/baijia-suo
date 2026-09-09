//! life3d --- Extension to Conway's Game of Life, Carter Bays' B5/S45 3D Life.
//!
//! Ported from xlockmore modes/life3d.c 5.27 (2008/07/28):
//!
//! Copyright (c) 1994 by David Bagley.
//!
//! Permission to use, copy, modify, and distribute this software and its
//! documentation for any purpose and without fee is hereby granted,
//! provided that the above copyright notice appear in all copies and that
//! both that copyright notice and this permission notice appear in
//! supporting documentation.
//!
//! Based on the DOS 3dlife by Anthony Wesley (life.anu.edu.au
//! /pub/complex_systems/alife/3DLIFE.ZIP) and Carter Bays' papers on
//! candidate rules for 3D Life (Complex Systems 1987-2006).
//!
//! Deviation from the C: NewViewpoint derives both projection factors (A and
//! C) from the window *width* alone, so on ultrawide monitors (3440x1440,
//! 5120x1440) the cubes balloon relative to the screen height and the colony
//! crops. We clamp the width used for scale to a 16:9 aspect
//! (min(width, height*16/9)); rendering is bit-identical to the C at 16:9
//! and narrower, and wider windows just gain horizontal breathing room.
//!
//! Deviation from the C: lissajous() sweeps the eye elevation over
//! 30*sin+45 = 15..75 degrees; above ~60 the colony reads as a top-down
//! height map (only cube tops visible). We compress the sweep to 15..55
//! degrees (20*sin+35) so the lattice always shows three cube faces.

use crate::rng::RngExt;
use std::cell::RefCell;
use std::collections::HashSet;
use std::f64::consts::PI;

use crate::animation::primitives::{clear_buffer, draw_line, put_pixel, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const MAXCOLUMNS: i32 = 128;
const MAXROWS: i32 = 128;
const MAXSTACKS: i32 = 64;

const SOUPPERCENT: i32 = 30;
const SOUPSIZE: i32 = 10;
const EYE_TO_SCREEN: f64 = 72.0;
const HALF_SCREEN_D: f64 = 14.0;
const LEN: f64 = 0.45;
const IP: f64 = PI / 180.0;
const RT_ANGLE: i32 = 90;
const HALFRT_ANGLE: i32 = 45;

const ON: u8 = 0x40;
const OFF: u8 = 0;

// RandomSoup symmetry kinds (life3d.c).
const NOSYMRAND: i32 = 0;
const ODDSYMRAND: i32 = 1;
const EVENSYMRAND: i32 = 2;
const ODDANTISYMRAND: i32 = 3;
const EVENANTISYMRAND: i32 = 4;
const DIAGSYMRAND: i32 = 5;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CellPos {
    x: i32,
    y: i32,
    z: i32,
}

pub struct Life3D {
    width: u32,
    height: u32,
    cycles: i32,
    ncolors: i32,

    generation: i32,
    no_change_count: i32,
    /// Any live cell currently projects onto the screen (lp->visible).
    visible: bool,

    /// Cell state: ON bit 0x40, low 5 bits = live-neighbor count.
    base: Vec<u8>,
    cells: Vec<CellPos>,
    /// Scratch buffers for run_life_3d, persistent to avoid a per-tick
    /// alloc: new_cells accumulates next generation, checked_cells is the
    /// visited-set (and doubles as the set of base indices touched this
    /// generation, see the ClearMem comment below).
    new_cells: Vec<CellPos>,
    checked_cells: HashSet<CellPos>,
    /// Painter's-algorithm scratch for render(&self); see mandelbrot.rs's
    /// needs_clear for the interior-mutability precedent.
    render_order: RefCell<Vec<(f64, CellPos)>>,

    birth_rule: u32,
    survival_rule: u32,

    ox: i32,
    oy: i32,
    oz: i32,
    vx: f64,
    vy: f64,
    vz: f64,
    a: f64,
    b: f64,
    c: f64,
    f: f64,
    azm: f64,

    meta_alt: f64,
    meta_azm: f64,
    meta_dist: f64,

    /// [black, red-slot, green-slot, blue-slot, white] like lp->colors.
    colors: [Color; 5],
}

impl Life3D {
    /// Width the projection scale is derived from: the real width, clamped
    /// to a 16:9 aspect so ultrawide windows don't balloon the cubes (see
    /// header note).
    fn scale_width(&self) -> f64 {
        (self.width as f64).min(self.height as f64 * 16.0 / 9.0)
    }

    #[inline]
    fn get_index(x: i32, y: i32, z: i32) -> usize {
        (z as usize * (MAXCOLUMNS as usize * MAXROWS as usize))
            + (y as usize * MAXCOLUMNS as usize)
            + x as usize
    }

    /// The 26 toroidally wrapped neighbors of a cell (order irrelevant for
    /// counting; C's positionOfNeighbor visits the same set).
    fn neighbor(p: CellPos, n: usize) -> CellPos {
        // n in 0..26 -> the n-th nonzero (dx,dy,dz) in {-1,0,1}^3
        let mut k = n;
        if k >= 13 {
            k += 1; // skip (0,0,0)
        }
        let dx = (k % 3) as i32 - 1;
        let dy = ((k / 3) % 3) as i32 - 1;
        let dz = (k / 9) as i32 - 1;
        CellPos {
            x: (p.x + dx).rem_euclid(MAXCOLUMNS),
            y: (p.y + dy).rem_euclid(MAXROWS),
            z: (p.z + dz).rem_euclid(MAXSTACKS),
        }
    }

    fn cell_state_3d(c: u8) -> u8 {
        c & ON
    }

    fn cell_nbrs_3d(c: u8) -> u8 {
        c & 0x1f
    }

    /// Set a cell ON and add it to the live list if it wasn't already.
    fn set_list(&mut self, x: i32, y: i32, z: i32) {
        if x < 0 || y < 0 || z < 0 || x >= MAXCOLUMNS || y >= MAXROWS || z >= MAXSTACKS {
            return;
        }
        let idx = Self::get_index(x, y, z);
        if Self::cell_state_3d(self.base[idx]) == OFF {
            self.base[idx] |= ON;
            self.cells.push(CellPos { x, y, z });
        }
    }

    fn new_viewpoint(&mut self, x: f64, y: f64, z: f64) {
        let mut k = x * x + y * y;
        let l = (k + z * z).sqrt();
        k = k.sqrt();
        if k == 0.0 {
            k = 1e-5;
        }
        let sw = self.scale_width();
        let d1 = EYE_TO_SCREEN / HALF_SCREEN_D;
        let d2 = EYE_TO_SCREEN / (HALF_SCREEN_D * self.height as f64 / sw);

        self.a = d1 * l * (sw / 2.0) / k;
        self.b = l * l;
        self.c = d2 * (self.height as f64 / 2.0) / k;
        self.f = k * k;
    }

    fn new_point(&self, x: f64, y: f64, z: f64) -> (i32, i32) {
        let p1 = x * self.vx + y * self.vy;
        let e = self.b - p1 - z * self.vz;
        if e.abs() < 1e-5 {
            return (-10000, -10000);
        }
        let px = (self.width as f64 / 2.0 - self.a * (self.vx * y - self.vy * x) / e) as i32;
        let py = (self.height as f64 / 2.0 - self.c * (z * self.f - self.vz * p1) / e) as i32;
        (px, py)
    }

    fn lissajous(&mut self) {
        // Deviation from the C (see header): elevation sweeps 15..=55 degrees
        // instead of the C's 30*sin+45 = 15..=75.
        let alt = 20.0 * (self.meta_alt * IP).sin() + 35.0;
        self.meta_alt += 1.123;
        if self.meta_alt >= 360.0 {
            self.meta_alt -= 360.0;
        }
        if self.meta_alt < 0.0 {
            self.meta_alt += 360.0;
        }

        let azm = 30.0 * (self.meta_azm * IP).sin() + 45.0;
        self.meta_azm += 0.987;
        if self.meta_azm >= 360.0 {
            self.meta_azm -= 360.0;
        }
        if self.meta_azm < 0.0 {
            self.meta_azm += 360.0;
        }

        let dist = 10.0 * (self.meta_dist * IP).sin() + 50.0;
        self.meta_dist += 1.0;
        if self.meta_dist >= 360.0 {
            self.meta_dist -= 360.0;
        }
        if self.meta_dist < 0.0 {
            self.meta_dist += 360.0;
        }

        self.azm = azm;
        self.vx = (azm * IP).sin() * (alt * IP).cos() * dist;
        self.vy = (azm * IP).cos() * (alt * IP).cos() * dist;
        self.vz = (alt * IP).sin() * dist;

        self.new_viewpoint(self.vx, self.vy, self.vz);
    }

    /// Distance from viewpoint and whether the cell is in front of the eye
    /// (SortList's per-cell visibility test).
    fn cell_view(&self, p: &CellPos) -> (f64, bool) {
        let x = p.x as f64 - self.ox as f64;
        let y = p.y as f64 - self.oy as f64;
        let z = p.z as f64 - self.oz as f64;
        let d = ((self.vx - x) * (self.vx - x)
            + (self.vy - y) * (self.vy - y)
            + (self.vz - z) * (self.vz - z))
            .sqrt();
        let front = self.vx * (self.vx - x) + self.vy * (self.vy - y) + self.vz * (self.vz - z)
            > 0.0
            && d > 1.5;
        (d, front)
    }

    /// lp->visible from SortList: true when some live cell projects near
    /// the screen. Drives the "colony wandered off / died" reseed.
    fn update_visible(&mut self) {
        let mut rsize = 0.47 * self.scale_width() / (HALF_SCREEN_D * 2.0);
        let mut i = (self.azm as i32).abs() % RT_ANGLE;
        if i > HALFRT_ANGLE {
            i = RT_ANGLE - i;
        }
        rsize /= (i as f64 * IP).cos();

        self.visible = false;
        for p in &self.cells {
            let (d, front) = self.cell_view(p);
            if !front {
                continue;
            }
            let x = p.x as f64 - self.ox as f64;
            let y = p.y as f64 - self.oy as f64;
            let z = p.z as f64 - self.oz as f64;
            let (px, py) = self.new_point(x, y, z);
            let r = (rsize * EYE_TO_SCREEN / (d as i32 as f64)) as i32;
            if px + r >= 0
                && py + r >= 0
                && px - r < self.width as i32
                && py - r < self.height as i32
            {
                self.visible = true;
                return;
            }
        }
    }

    /// One generation of RunLife3D.
    fn run_life_3d(&mut self) {
        let mut visible = false;

        // Step 1 - Add 1 to all neighbors of living cells.
        for i in 0..self.cells.len() {
            let p = self.cells[i];
            for n in 0..26 {
                let q = Self::neighbor(p, n);
                self.base[Self::get_index(q.x, q.y, q.z)] += 1;
            }
        }

        // Step 2 - Apply birth rule to neighbors of live cells and the
        // survival rule to the live cells themselves. C walks the live list
        // normalizing memory as it goes; a visited-set gives the same
        // evaluate-once semantics.
        self.new_cells.clear();
        self.checked_cells.clear();

        for p in &self.cells {
            for n in 0..26 {
                let q = Self::neighbor(*p, n);
                if self.checked_cells.insert(q) {
                    let c = self.base[Self::get_index(q.x, q.y, q.z)];
                    if Self::cell_state_3d(c) == OFF {
                        if (self.birth_rule & (1 << Self::cell_nbrs_3d(c))) != 0 {
                            visible = true;
                            self.new_cells.push(q);
                        }
                    } else if (self.survival_rule & (1 << Self::cell_nbrs_3d(c))) != 0 {
                        self.new_cells.push(q);
                    }
                }
            }
            if self.checked_cells.insert(*p) {
                let c = self.base[Self::get_index(p.x, p.y, p.z)];
                if Self::cell_state_3d(c) == ON
                    && (self.survival_rule & (1 << Self::cell_nbrs_3d(c))) != 0
                {
                    self.new_cells.push(*p);
                }
            }
        }

        // ClearMem + rebuild state. checked_cells is exactly the set of
        // base indices touched this generation: every neighbor visited by
        // step 1/2 above plus the live cells themselves (inserted via the
        // `checked_cells.insert(*p)` above), which is exactly what step 1
        // incremented (already-live cells keep their ON bit from the prior
        // rebuild). So zeroing just those indices is equivalent to zeroing
        // the whole 128x128x64 grid, without walking cells that were never
        // touched.
        for p in &self.checked_cells {
            self.base[Self::get_index(p.x, p.y, p.z)] = 0;
        }
        for p in &self.new_cells {
            self.base[Self::get_index(p.x, p.y, p.z)] = ON;
        }
        std::mem::swap(&mut self.cells, &mut self.new_cells);

        if visible {
            self.no_change_count = 0;
        } else {
            self.no_change_count += 1;
        }
    }

    /// RandomSoup: 30% soup in a small centered box, usually with a mirror
    /// or diagonal symmetry, exactly like the C's active code paths.
    fn random_soup(&mut self, n: i32) {
        let mut rng = crate::rng::rng();
        let hc = MAXCOLUMNS / 2;
        let hr = MAXROWS / 2;
        let hs = MAXSTACKS / 2;
        let (mut vx, mut vy, mut vz) = (SOUPSIZE, SOUPSIZE, SOUPSIZE);

        let mut xrand = rng.random_range(0..5);
        let mut yrand = rng.random_range(0..5);
        let mut zrand = rng.random_range(0..5);
        if rng.random_range(0..4) == 0 {
            xrand = NOSYMRAND;
            yrand = DIAGSYMRAND;
            zrand = rng.random_range(0..3);
        }
        if rng.random_range(0..6) == 0 {
            // Full diagonal
            xrand = NOSYMRAND;
            yrand = NOSYMRAND;
            zrand = DIAGSYMRAND;
        }
        if zrand == DIAGSYMRAND {
            if vx != vy || vy != vz {
                let m = vx.min(vy).min(vz);
                vx = m;
                vy = m;
                vz = m;
            }
        } else if yrand == DIAGSYMRAND && vx != vy {
            let m = vx.min(vy);
            vx = m;
            vy = m;
        }
        vx = (vx / 2).max(1);
        vy = (vy / 2).max(1);
        vz = (vz / 2).max(1);

        let hit = |rng: &mut crate::rng::Rng, p: i32| rng.random_range(0..100) < p;

        if xrand == NOSYMRAND && yrand == NOSYMRAND && zrand == NOSYMRAND {
            for stack in (hs - vz)..(hs + vz) {
                for row in (hr - vy)..(hr + vy) {
                    for col in (hc - vx)..(hc + vx) {
                        if hit(&mut rng, n) {
                            self.set_list(col, row, stack);
                        }
                    }
                }
            }
            return;
        }
        if zrand == DIAGSYMRAND {
            for stack in 0..(2 * vz) {
                for row in stack..(2 * vy) {
                    for col in row..(2 * vx) {
                        if hit(&mut rng, n) {
                            self.set_list(col + hc - vx, row + hr - vy, stack + hs - vz);
                            self.set_list(row + hc - vx, stack + hr - vy, col + hs - vz);
                            self.set_list(stack + hc - vx, col + hr - vy, row + hs - vz);
                        }
                    }
                }
            }
            return;
        }

        match yrand {
            NOSYMRAND => {
                for stack in (hs - vz)..(hs + vz) {
                    for row in (hr - vy)..(hr + vy) {
                        match xrand {
                            NOSYMRAND => {
                                for col in (hc - vx)..(hc + vx) {
                                    if hit(&mut rng, n) {
                                        self.set_list(col, row, stack);
                                    }
                                }
                            }
                            ODDSYMRAND => {
                                for col in hc..(hc + vx) {
                                    if hit(&mut rng, n / 2) {
                                        self.set_list(col, row, stack);
                                        self.set_list(2 * hc - col, row, stack);
                                    }
                                }
                            }
                            EVENSYMRAND => {
                                for col in hc..(hc + vx) {
                                    if hit(&mut rng, n / 2) {
                                        self.set_list(col, row, stack);
                                        self.set_list(2 * hc - col - 1, row, stack);
                                    }
                                }
                            }
                            ODDANTISYMRAND => {
                                for col in hc..(hc + vx) {
                                    if hit(&mut rng, n / 2) {
                                        self.set_list(col, row, stack);
                                        self.set_list(2 * hc - col, 2 * hr - row - 1, stack);
                                    }
                                }
                            }
                            EVENANTISYMRAND => {
                                for col in hc..(hc + vx) {
                                    if hit(&mut rng, n / 2) {
                                        self.set_list(col, row, stack);
                                        self.set_list(2 * hc - col - 1, 2 * hr - row - 1, stack);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            ODDSYMRAND => {
                for stack in (hs - vz)..(hs + vz) {
                    for row in hr..(hr + vy) {
                        match xrand {
                            NOSYMRAND => {
                                for col in (hc - vx)..(hc + vx) {
                                    if hit(&mut rng, n / 2) {
                                        self.set_list(col, row, stack);
                                        self.set_list(col, 2 * hr - row, stack);
                                    }
                                }
                            }
                            ODDSYMRAND => {
                                for col in hc..(hc + vx) {
                                    if hit(&mut rng, n / 4) {
                                        self.set_list(col, row, stack);
                                        self.set_list(col, 2 * hr - row, stack);
                                        self.set_list(2 * hc - col, row, stack);
                                        self.set_list(2 * hc - col, 2 * hr - row, stack);
                                    }
                                }
                            }
                            EVENSYMRAND => {
                                for col in hc..(hc + vx) {
                                    if hit(&mut rng, n / 2) {
                                        self.set_list(col, row, stack);
                                        self.set_list(2 * hc - col - 1, row, stack);
                                        self.set_list(col, 2 * hr - row, stack);
                                        self.set_list(2 * hc - col - 1, 2 * hr - row, stack);
                                    }
                                }
                            }
                            // xrand 3/4 seed nothing here in the C either;
                            // the empty soup triggers an immediate reseed.
                            _ => {}
                        }
                    }
                }
            }
            EVENSYMRAND => {
                for stack in (hs - vz)..(hs + vz) {
                    for row in hr..(hr + vy) {
                        match xrand {
                            NOSYMRAND => {
                                for col in (hc - vx)..(hc + vx) {
                                    if hit(&mut rng, n / 2) {
                                        self.set_list(col, row, stack);
                                        self.set_list(col, 2 * hr - row - 1, stack);
                                    }
                                }
                            }
                            ODDSYMRAND => {
                                for col in hc..(hc + vx) {
                                    if hit(&mut rng, n / 4) {
                                        self.set_list(col, row, stack);
                                        self.set_list(2 * hc - col, row, stack);
                                        self.set_list(col, 2 * hr - row - 1, stack);
                                        self.set_list(2 * hc - col, 2 * hr - row - 1, stack);
                                    }
                                }
                            }
                            EVENSYMRAND => {
                                for col in hc..(hc + vx) {
                                    if hit(&mut rng, n / 4) {
                                        self.set_list(col, row, stack);
                                        self.set_list(2 * hc - col - 1, row, stack);
                                        self.set_list(col, 2 * hr - row - 1, stack);
                                        self.set_list(2 * hc - col - 1, 2 * hr - row - 1, stack);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            ODDANTISYMRAND | EVENANTISYMRAND
                if xrand == NOSYMRAND => {
                    let off = if yrand == ODDANTISYMRAND { 0 } else { 1 };
                    for stack in (hs - vz)..(hs + vz) {
                        for row in hr..(hr + vy) {
                            for col in (hc - vx)..(hc + vx) {
                                if hit(&mut rng, n / 2) {
                                    self.set_list(col, row, stack);
                                    self.set_list(2 * hc - col - 1, 2 * hr - row - off, stack);
                                }
                            }
                        }
                    }
                }
            DIAGSYMRAND => {
                let xrand = rng.random_range(0..4);
                vx = vx.min(vy);
                for stack in (hs - vz)..(hs + vz) {
                    let (row_lo, p) = match xrand {
                        0 | 1 => (hr - vx, n / 2),
                        2 => (hr - vx, n / 4),
                        _ => (hr, n / 8),
                    };
                    for row in row_lo..(hr + vx) {
                        for col in (row - hr + hc)..(hc + vx) {
                            if !hit(&mut rng, p) {
                                continue;
                            }
                            match xrand {
                                0 => {
                                    self.set_list(col, row, stack);
                                    self.set_list(row + hc - hr, col + hr - hc, stack);
                                }
                                1 => {
                                    self.set_list(2 * hc - col, row, stack);
                                    self.set_list(hc - row + hr, col + hr - hc, stack);
                                }
                                2 => {
                                    self.set_list(2 * hc - col, row, stack);
                                    self.set_list(hc - row + hr, col + hr - hc, stack);
                                    self.set_list(col, 2 * hr - row, stack);
                                    self.set_list(row + hc - hr, hr - col + hc, stack);
                                }
                                _ => {
                                    self.set_list(col, row, stack);
                                    self.set_list(row + hc - hr, col + hr - hc, stack);
                                    self.set_list(2 * hc - col, row, stack);
                                    self.set_list(hc - row + hr, col + hr - hc, stack);
                                    self.set_list(col, 2 * hr - row, stack);
                                    self.set_list(row + hc - hr, hr - col + hc, stack);
                                    self.set_list(2 * hc - col, 2 * hr - row, stack);
                                    self.set_list(hc - row + hr, hr - col + hc, stack);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Three neighboring hues off the ncolors wheel, randomly rotated among
    /// the red/green/blue face slots (init_life3d's color pick).
    fn pick_colors(&mut self) {
        let mut rng = crate::rng::rng();
        let nc = self.ncolors.max(6);
        let j = rng.random_range(0..nc);
        let i = rng.random_range(0..3) as usize;
        let hue = |k: i32| Color::from_hsl((k.rem_euclid(nc)) as f32 / nc as f32, 1.0, 0.5);
        self.colors[i + 1] = hue(j);
        self.colors[(i + 1) % 3 + 1] = hue(j + 2);
        self.colors[(i + 2) % 3 + 1] = hue(j + 4);
    }

    /// The per-cycle part of init_life3d: fresh colors and a fresh soup.
    /// Meta angles persist so the viewpoint keeps moving smoothly.
    fn reinit(&mut self) {
        self.base.fill(0);
        self.cells.clear();
        self.generation = 0;
        self.no_change_count = 0;
        self.pick_colors();
        self.lissajous();
        self.random_soup(SOUPPERCENT);
        self.update_visible();
    }

    /// drawQuad: filled face in the given color plus a white outline.
    fn draw_quad(
        &self,
        buffer: &mut [u8],
        pts: &[(i32, i32); 8],
        a: usize,
        b: usize,
        c: usize,
        d: usize,
        color: Color,
    ) {
        let quad = [pts[a], pts[b], pts[c], pts[d]];
        fill_quad(buffer, self.width, self.height, &quad, color);
        let white = self.colors[4];
        for k in 0..4 {
            let (x0, y0) = quad[k];
            let (x1, y1) = quad[(k + 1) % 4];
            draw_line(buffer, self.width, self.height, x0, y0, x1, y1, white);
        }
    }

    fn draw_cube(&self, buffer: &mut [u8], cell: &CellPos) {
        let x = cell.x as f64 - self.ox as f64;
        let y = cell.y as f64 - self.oy as f64;
        let z = cell.z as f64 - self.oz as f64;

        // C's corner order: dx varies fastest, then dy, then dz.
        let mut pts = [(0, 0); 8];
        let mut out = 0;
        let mut i = 0;
        for dz in [z - LEN, z + LEN] {
            for dy in [y - LEN, y + LEN] {
                for dx in [x - LEN, x + LEN] {
                    pts[i] = self.new_point(dx, dy, dz);
                    if pts[i].0 < 0
                        || pts[i].0 >= self.width as i32
                        || pts[i].1 < 0
                        || pts[i].1 >= self.height as i32
                    {
                        out += 1;
                    }
                    i += 1;
                }
            }
        }
        if out == 8 {
            return;
        }

        // Only draw the faces that are toward the viewpoint.
        let dx = self.vx - x;
        let dy = self.vy - y;
        let dz = self.vz - z;

        let red = self.colors[1];
        let green = self.colors[2];
        let blue = self.colors[3];

        if dz > LEN {
            self.draw_quad(buffer, &pts, 4, 5, 7, 6, blue);
        } else if dz < -LEN {
            self.draw_quad(buffer, &pts, 0, 1, 3, 2, blue);
        }
        if dx > LEN {
            self.draw_quad(buffer, &pts, 1, 3, 7, 5, green);
        } else if dx < -LEN {
            self.draw_quad(buffer, &pts, 0, 2, 6, 4, green);
        }
        if dy > LEN {
            self.draw_quad(buffer, &pts, 2, 3, 7, 6, red);
        } else if dy < -LEN {
            self.draw_quad(buffer, &pts, 0, 1, 5, 4, red);
        }
    }
}

/// Scanline-fill a convex quad, clipped to the buffer.
fn fill_quad(buffer: &mut [u8], width: u32, height: u32, q: &[(i32, i32); 4], color: Color) {
    let ymin = q[0].1.min(q[1].1).min(q[2].1).min(q[3].1).max(0);
    let ymax = q[0].1.max(q[1].1).max(q[2].1).max(q[3].1).min(height as i32 - 1);
    for y in ymin..=ymax {
        let mut xmin = i32::MAX;
        let mut xmax = i32::MIN;
        for k in 0..4 {
            let (x0, y0) = q[k];
            let (x1, y1) = q[(k + 1) % 4];
            if y0 == y1 {
                if y == y0 {
                    xmin = xmin.min(x0.min(x1));
                    xmax = xmax.max(x0.max(x1));
                }
            } else if y >= y0.min(y1) && y <= y0.max(y1) {
                let xi = x0 as f64 + (y - y0) as f64 * (x1 - x0) as f64 / (y1 - y0) as f64;
                let xi = xi.round() as i32;
                xmin = xmin.min(xi);
                xmax = xmax.max(xi);
            }
        }
        if xmin <= xmax {
            for x in xmin.max(0)..=xmax.min(width as i32 - 1) {
                put_pixel(buffer, width, height, x, y, color);
            }
        }
    }
}

impl Animation for Life3D {
    fn new(config: &AnimConfig) -> Self {
        let mut life = Life3D {
            width: config.width.max(1),
            height: config.height.max(1),
            cycles: if config.cycles <= 0 { 85 } else { config.cycles },
            ncolors: config.ncolors,
            generation: 0,
            no_change_count: 0,
            visible: false,
            base: vec![0; (MAXCOLUMNS * MAXROWS * MAXSTACKS) as usize],
            cells: Vec::new(),
            new_cells: Vec::new(),
            checked_cells: HashSet::new(),
            render_order: RefCell::new(Vec::new()),
            birth_rule: 1 << 5,                     // B5
            survival_rule: (1 << 4) | (1 << 5),     // S45 (Carter Bays' 3D Life)
            ox: MAXCOLUMNS / 2,
            oy: MAXROWS / 2,
            oz: MAXSTACKS / 2,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            a: 0.0,
            b: 0.0,
            c: 0.0,
            f: 0.0,
            azm: 0.0,
            meta_alt: 0.0,
            meta_azm: 0.0,
            meta_dist: 0.0,
            colors: [
                Color::new(255, 0, 0, 0),           // black
                Color::new(255, 255, 50, 50),       // red slot (repicked)
                Color::new(255, 50, 255, 50),       // green slot (repicked)
                Color::new(255, 50, 100, 255),      // blue slot (repicked)
                Color::new(255, 255, 255, 255),     // white outlines
            ],
        };
        life.reset(config);
        life
    }

    fn tick(&mut self) {
        // draw_life3d: run a generation, move the viewpoint, then decide
        // whether this colony is done (dead, static, off screen, or old).
        self.run_life_3d();
        self.lissajous();
        self.update_visible();

        if !self.visible || self.no_change_count >= 8 {
            self.reinit();
        } else {
            self.generation += 1;
            if self.generation > self.cycles {
                self.reinit();
            }
        }
        // ponytail: C also fires shooter() gliders at the colony every
        // MI_COUNT generations; needs the life3d.h glider tables to port.
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        clear_buffer(buffer, self.colors[0]);

        // Painter's algorithm: draw the furthest cubes first. Cells behind
        // the eye are skipped (C fills them black, same result).
        let mut order = self.render_order.borrow_mut();
        order.clear();
        order.extend(self.cells.iter().filter_map(|p| {
            let (d, front) = self.cell_view(p);
            front.then_some((d, *p))
        }));
        order.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));

        for (_, cell) in order.iter() {
            self.draw_cube(buffer, cell);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width.max(1);
        self.height = config.height.max(1);
        let mut rng = crate::rng::rng();
        self.meta_alt = rng.random_range(0.0..360.0);
        self.meta_azm = rng.random_range(0.0..360.0);
        self.meta_dist = rng.random_range(0.0..360.0);
        self.reinit();
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        1_000_000 // xlockmore life3d default delay
    }
}
