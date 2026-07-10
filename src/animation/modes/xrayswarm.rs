// Copyright (c) 2000 by Chris Leger (xrayjones@users.sourceforge.net)
//
// xrayswarm - a shameless ripoff of the 'swarm' screensaver on SGI boxes.
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE X CONSORTIUM BE LIABLE FOR ANY CLAIM, DAMAGES OR
// OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
// ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
// OTHER DEALINGS IN THE SOFTWARE.
//
// Except as contained in this notice, the name of the X Consortium shall
// not be used in advertising or otherwise to promote the sale, use or
// other dealings in this Software without prior written authorization
// from the X Consortium.
//
// Rust port of xscreensaver's xrayswarm.c.

use crate::animation::primitives::{clear_buffer, draw_line, Color};
use crate::animation::{AnimConfig, Animation};
use rand::Rng;
use std::time::Instant;

const MAX_TRAIL_LEN: usize = 60;
const MAX_BUGS: usize = 100;
const MAX_TARGETS: usize = 10;

const MAX_FPS: f32 = 150.0;
const MIN_FPS: f32 = 16.0;
const DESIRED_DT: f32 = 0.2;

const GRAY_TRAILS: i32 = 0;
const GRAY_SCHIZO: i32 = 1;
const COLOR_TRAILS: i32 = 2;
const RANDOM_TRAILS: i32 = 3;
const RANDOM_SCHIZO: i32 = 4;
const COLOR_SCHIZO: i32 = 5;
const NUM_SCHEMES: i32 = 6; // too many schizos; don't use last 2

#[derive(Clone, Copy)]
struct Bug {
    pos: [f32; 2],
    hist: [[i32; 2]; MAX_TRAIL_LEN],
    vel: [f32; 2],
    closest: usize, // index into targets
}

const ZERO_BUG: Bug = Bug {
    pos: [0.0; 2],
    hist: [[0; 2]; MAX_TRAIL_LEN],
    vel: [0.0; 2],
    closest: 0,
};

fn frand(rng: &mut impl Rng, x: f32) -> f32 {
    rng.random::<f32>() * x
}

pub struct XRaySwarm {
    canvas: Vec<u8>,
    width: u32,
    height: u32,

    colors: [u8; 768],
    xsize: i32,
    ysize: i32,
    delay: u64,
    maxx: f32,
    maxy: f32,

    dt: f32,
    target_vel: f32,
    target_acc: f32,
    max_vel: f32,
    max_acc: f32,
    noise: f32,
    min_vel_multiplier: f32,

    nbugs: usize,
    ntargets: usize,
    trail_len: usize,

    dt_inv: f32,
    half_dt_sq: f32,
    target_vel_sq: f32,
    max_vel_sq: f32,
    min_vel_sq: f32,
    min_vel: f32,

    bugs: Vec<Bug>,
    targets: Vec<Bug>,
    head: usize,
    tail: usize,
    color_scheme: i32,
    change_prob: f32,

    gray_index: [i32; MAX_TRAIL_LEN],
    red_index: [i32; MAX_TRAIL_LEN],
    blue_index: [i32; MAX_TRAIL_LEN],
    gray_s_index: [i32; MAX_TRAIL_LEN],
    red_s_index: [i32; MAX_TRAIL_LEN],
    blue_s_index: [i32; MAX_TRAIL_LEN],
    random_index: [i32; MAX_TRAIL_LEN],
    num_random_colors: usize,

    check_index: usize,
    rsc_call_depth: i32,
    rbc_call_depth: i32,

    start: Instant,
    draw_nframes: u32,
    draw_delay_accum: u64,
    draw_sleep_count: u32,
    this_delay: u64,
}

impl XRaySwarm {
    fn get_time(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn init_cmap(&mut self, rng: &mut impl Rng) {
        let mut n = 0usize;

        // color 0 is black
        self.colors[n] = 0;
        self.colors[n + 1] = 0;
        self.colors[n + 2] = 0;
        n += 3;

        // colors 1, 2, 3 (sic: the C sets all three to red)
        for _ in 0..3 {
            self.colors[n] = 255;
            self.colors[n + 1] = 0;
            self.colors[n + 2] = 0;
            n += 3;
        }

        // grayscale colors start at 4; 16 levels
        for i in 0..16 {
            let temp = (i * 16).min(255) as u8;
            self.colors[n] = 255 - temp;
            self.colors[n + 1] = 255 - temp;
            self.colors[n + 2] = 255 - temp;
            n += 3;
        }

        // red fade starts at 20; 16 levels
        for i in 0..16i32 {
            let temp = (i * 16).min(255);
            self.colors[n] = (255 - temp) as u8;
            self.colors[n + 1] = (255.0 - (i as f64 / 16.0 + 0.001).powf(0.3) * 255.0) as u8;
            self.colors[n + 2] = (65 - temp / 4) as u8;
            n += 3;
        }

        // blue fade starts at 36; 16 levels
        for i in 0..16i32 {
            let temp = (i * 16).min(255);
            self.colors[n] = (32 - temp / 8) as u8;
            self.colors[n + 1] = (180.0 - (i as f64 / 16.0 + 0.001).powf(0.3) * 180.0) as u8;
            self.colors[n + 2] = (255 - temp) as u8;
            n += 3;
        }

        // random colors start at 52
        self.num_random_colors = MAX_TRAIL_LEN;

        self.colors[n] = (rng.random::<u32>() & 255) as u8;
        n += 1;
        self.colors[n] = (rng.random::<u32>() & 255) as u8;
        n += 1;
        self.colors[n] = self.colors[n - 2] / 2 + self.colors[n - 3] / 2;
        n += 1;

        for i in 0..self.num_random_colors {
            self.colors[n] = ((self.colors[n - 3] as i32 + (rng.random::<u32>() & 31) as i32 - 16) & 255) as u8;
            n += 1;
            self.colors[n] = ((self.colors[n - 3] as i32 + (rng.random::<u32>() & 31) as i32 - 16) & 255) as u8;
            n += 1;
            self.colors[n] = (self.colors[n - 2] as f32 / (i + 2) as f32
                + self.colors[n - 3] as f32 / (i + 2) as f32) as u8;
            n += 1;
        }
    }

    fn palette(&self, idx: i32) -> Color {
        let i = idx as usize * 3;
        Color::new(255, self.colors[i], self.colors[i + 1], self.colors[i + 2])
    }

    fn line(&mut self, p0: [i32; 2], p1: [i32; 2], color: Color) {
        draw_line(&mut self.canvas, self.width, self.height, p0[0], p0[1], p1[0], p1[1], color);
    }

    fn init_bugs(&mut self, rng: &mut impl Rng) {
        self.head = 0;
        self.tail = 0;

        for b in self.bugs.iter_mut() {
            *b = ZERO_BUG;
        }
        for t in self.targets.iter_mut() {
            *t = ZERO_BUG;
        }

        if self.ntargets < 1 {
            self.ntargets = 1;
        }
        if self.nbugs <= self.ntargets {
            self.nbugs = self.ntargets + 1;
        }
        if self.nbugs > MAX_BUGS {
            self.nbugs = MAX_BUGS;
        }
        if self.ntargets > MAX_TARGETS {
            self.ntargets = MAX_TARGETS;
        }
        if self.trail_len > MAX_TRAIL_LEN {
            self.trail_len = MAX_TRAIL_LEN;
        }

        for i in 0..self.nbugs {
            let closest = rng.random_range(0..self.ntargets);
            let (px, py) = (frand(rng, self.maxx), frand(rng, self.maxy));
            let (vx, vy) = (frand(rng, self.max_vel / 2.0), frand(rng, self.max_vel / 2.0));
            let b = &mut self.bugs[i];
            b.pos[0] = px;
            b.pos[1] = py;
            b.vel[0] = vx;
            b.vel[1] = vy;
            b.hist[0][0] = (b.pos[0] * self.xsize as f32) as i32;
            b.hist[0][1] = (b.pos[1] * self.xsize as f32) as i32;
            b.closest = closest;
        }

        for i in 0..self.ntargets {
            let (px, py) = (frand(rng, self.maxx), frand(rng, self.maxy));
            let (vx, vy) = (frand(rng, self.target_vel / 2.0), frand(rng, self.target_vel / 2.0));
            let b = &mut self.targets[i];
            b.pos[0] = px;
            b.pos[1] = py;
            b.vel[0] = vx;
            b.vel[1] = vy;
            b.hist[0][0] = (b.pos[0] * self.xsize as f32) as i32;
            b.hist[0][1] = (b.pos[1] * self.xsize as f32) as i32;
        }
    }

    fn pick_new_targets(&mut self, rng: &mut impl Rng) {
        for i in 0..self.nbugs {
            self.bugs[i].closest = rng.random_range(0..self.ntargets);
        }
    }

    fn compute_constants(&mut self) {
        self.half_dt_sq = self.dt * self.dt * 0.5;
        self.dt_inv = 1.0 / self.dt;
        self.target_vel_sq = self.target_vel * self.target_vel;
        self.max_vel_sq = self.max_vel * self.max_vel;
        self.min_vel = self.max_vel * self.min_vel_multiplier;
        self.min_vel_sq = self.min_vel * self.min_vel;
    }

    fn compute_color_indices(&mut self, rng: &mut impl Rng) {
        let tl = self.trail_len;
        // note: colors are used in *reverse* order!

        // grayscale
        for i in 0..tl {
            self.gray_index[tl - 1 - i] = ((4.0 + i as f32 * 16.0 / tl as f32 + 0.5) as i32).min(19);
        }
        // red
        for i in 0..tl {
            self.red_index[tl - 1 - i] = ((20.0 + i as f32 * 16.0 / tl as f32 + 0.5) as i32).min(35);
        }
        // blue
        for i in 0..tl {
            self.blue_index[tl - 1 - i] = ((36.0 + i as f32 * 16.0 / tl as f32 + 0.5) as i32).min(51);
        }
        // gray schizo - same as gray
        for i in 0..tl {
            self.gray_s_index[tl - 1 - i] = ((4.0 + i as f32 * 16.0 / tl as f32 + 0.5) as i32).min(19);
        }
        // red schizo - same as red
        for i in 0..tl {
            self.red_s_index[tl - 1 - i] = ((20.0 + i as f32 * 16.0 / tl as f32 + 0.5) as i32).min(35);
        }
        // blue schizo
        let schizo_length = (tl / 2).max(3);
        for i in 0..tl {
            self.blue_s_index[tl - 1 - i] =
                ((36.0 + 16.0 * (i % schizo_length) as f32 / (schizo_length as f32 - 1.0) + 0.5) as i32).min(51);
        }
        // random
        for i in 0..tl {
            self.random_index[i] = 52 + rng.random_range(0..self.num_random_colors) as i32;
        }
    }

    // (target index array, tci0, bug index array, ci0); tnc == nc == trail_len
    fn color_indices(&self) -> ([i32; MAX_TRAIL_LEN], usize, [i32; MAX_TRAIL_LEN], usize) {
        match self.color_scheme {
            COLOR_TRAILS => (self.red_index, 0, self.blue_index, 0),
            GRAY_SCHIZO => (self.gray_s_index, self.head, self.gray_s_index, self.head),
            COLOR_SCHIZO => (self.red_s_index, self.head, self.blue_s_index, self.head),
            GRAY_TRAILS => (self.gray_index, 0, self.gray_index, 0),
            RANDOM_TRAILS => (self.red_index, 0, self.random_index, 0),
            RANDOM_SCHIZO => (self.red_index, self.head, self.random_index, self.head),
            _ => (self.gray_index, 0, self.gray_index, 0),
        }
    }

    fn draw_bugs(&mut self) {
        let (t_idx, mut tci0, b_idx, mut ci0) = self.color_indices();
        let nc = self.trail_len;
        let tnc = self.trail_len;
        let bg = self.palette(0);

        if (self.head + 1) % self.trail_len == self.tail {
            // first, erase last segment of bugs if necessary
            let temp = (self.tail + 1) % self.trail_len;
            for i in 0..self.nbugs {
                let (p0, p1) = (self.bugs[i].hist[self.tail], self.bugs[i].hist[temp]);
                self.line(p0, p1, bg);
            }
            for i in 0..self.ntargets {
                let (p0, p1) = (self.targets[i].hist[self.tail], self.targets[i].hist[temp]);
                self.line(p0, p1, bg);
            }
            self.tail = (self.tail + 1) % self.trail_len;
        }

        let mut j = self.tail;
        while j != self.head {
            let temp = (j + 1) % self.trail_len;
            let bc = self.palette(b_idx[ci0]);
            let tc = self.palette(t_idx[tci0]);
            for i in 0..self.nbugs {
                let (p0, p1) = (self.bugs[i].hist[j], self.bugs[i].hist[temp]);
                self.line(p0, p1, bc);
            }
            for i in 0..self.ntargets {
                let (p0, p1) = (self.targets[i].hist[j], self.targets[i].hist[temp]);
                self.line(p0, p1, tc);
            }
            ci0 = (ci0 + 1) % nc;
            tci0 = (tci0 + 1) % tnc;
            j = temp;
        }
    }

    fn clear_bugs(&mut self) {
        let bg = self.palette(0);

        self.tail = if self.tail == 0 { self.trail_len - 1 } else { self.tail - 1 };

        if (self.head + 1) % self.trail_len == self.tail {
            let temp = (self.tail + 1) % self.trail_len;
            for i in 0..self.nbugs {
                let (p0, p1) = (self.bugs[i].hist[self.tail], self.bugs[i].hist[temp]);
                self.line(p0, p1, bg);
            }
            for i in 0..self.ntargets {
                let (p0, p1) = (self.targets[i].hist[self.tail], self.targets[i].hist[temp]);
                self.line(p0, p1, bg);
            }
            self.tail = (self.tail + 1) % self.trail_len;
        }

        let mut j = self.tail;
        while j != self.head {
            let temp = (j + 1) % self.trail_len;
            for i in 0..self.nbugs {
                let (p0, p1) = (self.bugs[i].hist[j], self.bugs[i].hist[temp]);
                self.line(p0, p1, bg);
            }
            for i in 0..self.ntargets {
                let (p0, p1) = (self.targets[i].hist[j], self.targets[i].hist[temp]);
                self.line(p0, p1, bg);
            }
            j = temp;
        }
    }

    fn update_state(&mut self, rng: &mut impl Rng) {
        self.head = (self.head + 1) % self.trail_len;

        for _ in 0..5 {
            // update closest target for the bug indicated by check_index
            self.check_index = (self.check_index + 1) % self.nbugs;
            let bp = self.bugs[self.check_index].pos;
            let mut closest = self.bugs[self.check_index].closest;
            let ax = self.targets[closest].pos[0] - bp[0];
            let ay = self.targets[closest].pos[1] - bp[1];
            let mut temp = ax * ax + ay * ay;
            for i in 0..self.ntargets {
                if i == closest {
                    continue;
                }
                let ax = self.targets[i].pos[0] - bp[0];
                let ay = self.targets[i].pos[1] - bp[1];
                let theta = ax * ax + ay * ay;
                if theta < temp * 2.0 {
                    closest = i;
                    temp = theta;
                }
            }
            self.bugs[self.check_index].closest = closest;
        }

        let (dt, dt_inv, half_dt_sq) = (self.dt, self.dt_inv, self.half_dt_sq);
        let (maxx, maxy, xsize) = (self.maxx, self.maxy, self.xsize as f32);
        let head = self.head;

        // update target state
        let (target_acc, target_vel, target_vel_sq) = (self.target_acc, self.target_vel, self.target_vel_sq);
        for i in 0..self.ntargets {
            let theta = frand(rng, std::f32::consts::TAU);
            let b = &mut self.targets[i];
            let mut ax = target_acc * theta.cos();
            let mut ay = target_acc * theta.sin();

            b.vel[0] += ax * dt;
            b.vel[1] += ay * dt;

            // check velocity
            let temp = b.vel[0] * b.vel[0] + b.vel[1] * b.vel[1];
            if temp > target_vel_sq {
                let temp = target_vel / temp.sqrt();
                // save old vel for acc computation
                ax = b.vel[0];
                ay = b.vel[1];
                // compute new velocity
                b.vel[0] *= temp;
                b.vel[1] *= temp;
                // update acceleration
                ax = (b.vel[0] - ax) * dt_inv;
                ay = (b.vel[1] - ay) * dt_inv;
            }

            // update position
            b.pos[0] += b.vel[0] * dt + ax * half_dt_sq;
            b.pos[1] += b.vel[1] * dt + ay * half_dt_sq;

            // bounce off the walls
            if b.pos[0] < 0.0 {
                b.pos[0] = -b.pos[0];
                b.vel[0] = -b.vel[0];
            } else if b.pos[0] >= maxx {
                b.pos[0] = 2.0 * maxx - b.pos[0];
                b.vel[0] = -b.vel[0];
            }
            if b.pos[1] < 0.0 {
                b.pos[1] = -b.pos[1];
                b.vel[1] = -b.vel[1];
            } else if b.pos[1] >= maxy {
                b.pos[1] = 2.0 * maxy - b.pos[1];
                b.vel[1] = -b.vel[1];
            }

            b.hist[head][0] = (b.pos[0] * xsize) as i32;
            b.hist[head][1] = (b.pos[1] * xsize) as i32;
        }

        // update bug state
        let (max_acc, max_vel, max_vel_sq) = (self.max_acc, self.max_vel, self.max_vel_sq);
        let (min_vel, min_vel_sq, noise) = (self.min_vel, self.min_vel_sq, self.noise);
        for i in 0..self.nbugs {
            let cp = self.targets[self.bugs[i].closest].pos;
            let noise_y = frand(rng, noise);
            let noise_x = frand(rng, noise);
            let b = &mut self.bugs[i];
            let theta = (cp[1] - b.pos[1] + noise_y).atan2(cp[0] - b.pos[0] + noise_x);
            let mut ax = max_acc * theta.cos();
            let mut ay = max_acc * theta.sin();

            b.vel[0] += ax * dt;
            b.vel[1] += ay * dt;

            // check velocity
            let temp = b.vel[0] * b.vel[0] + b.vel[1] * b.vel[1];
            if temp > max_vel_sq {
                let temp = max_vel / temp.sqrt();
                ax = b.vel[0];
                ay = b.vel[1];
                b.vel[0] *= temp;
                b.vel[1] *= temp;
                ax = (b.vel[0] - ax) * dt_inv;
                ay = (b.vel[1] - ay) * dt_inv;
            } else if temp < min_vel_sq {
                let temp = min_vel / temp.sqrt();
                ax = b.vel[0];
                ay = b.vel[1];
                b.vel[0] *= temp;
                b.vel[1] *= temp;
                ax = (b.vel[0] - ax) * dt_inv;
                ay = (b.vel[1] - ay) * dt_inv;
            }

            // update position
            b.pos[0] += b.vel[0] * dt + ax * half_dt_sq;
            b.pos[1] += b.vel[1] * dt + ay * half_dt_sq;

            if b.pos[0] < 0.0 {
                b.pos[0] = -b.pos[0];
                b.vel[0] = -b.vel[0];
            } else if b.pos[0] >= maxx {
                b.pos[0] = 2.0 * maxx - b.pos[0];
                b.vel[0] = -b.vel[0];
            }
            if b.pos[1] < 0.0 {
                b.pos[1] = -b.pos[1];
                b.vel[1] = -b.vel[1];
            } else if b.pos[1] >= maxy {
                b.pos[1] = 2.0 * maxy - b.pos[1];
                b.vel[1] = -b.vel[1];
            }

            b.hist[head][0] = (b.pos[0] * xsize) as i32;
            b.hist[head][1] = (b.pos[1] * xsize) as i32;
        }
    }

    fn mutate_bug(&mut self, rng: &mut impl Rng, which: i32) {
        if which == 0 {
            // turn bug into target
            if self.ntargets < MAX_TARGETS - 1 && self.nbugs > 1 {
                let i = rng.random_range(0..self.nbugs);
                self.targets[self.ntargets] = self.bugs[i];
                self.bugs[i] = self.bugs[self.nbugs - 1];
                self.targets[self.ntargets].pos[0] = frand(rng, self.maxx);
                self.targets[self.ntargets].pos[1] = frand(rng, self.maxy);
                self.nbugs -= 1;
                self.ntargets += 1;

                let mut i = 0;
                while i < self.nbugs {
                    self.bugs[i].closest = self.ntargets - 1;
                    i += self.ntargets;
                }
            }
        } else {
            // turn target into bug
            if self.ntargets > 1 && self.nbugs < MAX_BUGS - 1 {
                // pick a target
                let i = rng.random_range(0..self.ntargets);

                // copy state into a new bug
                self.bugs[self.nbugs] = self.targets[i];
                self.ntargets -= 1;

                // pick a target for the new bug
                self.bugs[self.nbugs].closest = rng.random_range(0..self.ntargets);

                for j in 0..self.nbugs {
                    if self.bugs[j].closest == self.ntargets {
                        self.bugs[j].closest = i;
                    } else if self.bugs[j].closest == i {
                        self.bugs[j].closest = rng.random_range(0..self.ntargets);
                    }
                }
                self.nbugs += 1;

                // copy the last target into the one we just deleted
                self.targets[i] = self.targets[self.ntargets];
            }
        }
    }

    fn random_small_change(&mut self, rng: &mut impl Rng) {
        let which_case = rng.random_range(0..11);

        self.rsc_call_depth += 1;
        if self.rsc_call_depth > 10 {
            self.rsc_call_depth -= 1;
            return;
        }

        match which_case {
            0 => self.max_acc *= 0.75 + frand(rng, 0.5),
            1 => self.target_acc *= 0.75 + frand(rng, 0.5),
            2 => self.max_vel *= 0.75 + frand(rng, 0.5),
            3 => self.target_vel *= 0.75 + frand(rng, 0.5),
            4 => self.noise *= 0.75 + frand(rng, 0.5),
            5 => self.min_vel_multiplier *= 0.75 + frand(rng, 0.5),
            6 | 7 => {
                // target to bug
                if self.ntargets >= 2 {
                    self.mutate_bug(rng, 1);
                }
            }
            8 => {
                // bug to target
                if self.nbugs >= 2 {
                    self.mutate_bug(rng, 0);
                    if self.nbugs >= 2 {
                        self.mutate_bug(rng, 0);
                    }
                }
            }
            9 => {
                // color scheme
                self.color_scheme = rng.random_range(0..NUM_SCHEMES);
                if self.color_scheme == RANDOM_SCHIZO || self.color_scheme == COLOR_SCHIZO {
                    // don't use these quite as much
                    self.color_scheme = rng.random_range(0..NUM_SCHEMES);
                }
            }
            _ => {
                self.random_small_change(rng);
                self.random_small_change(rng);
                self.random_small_change(rng);
                self.random_small_change(rng);
            }
        }

        self.min_vel_multiplier = self.min_vel_multiplier.clamp(0.3, 0.9);
        if self.noise < 0.01 {
            self.noise = 0.01;
        }
        if self.max_vel < 0.02 {
            self.max_vel = 0.02;
        }
        if self.target_vel < 0.02 {
            self.target_vel = 0.02;
        }
        if self.target_acc > self.target_vel * 0.7 {
            self.target_acc = self.target_vel * 0.7;
        }
        if self.max_acc > self.max_vel * 0.7 {
            self.max_acc = self.max_vel * 0.7;
        }
        if self.max_acc < 0.01 {
            self.max_acc = 0.01;
        }
        if self.target_acc < 0.005 {
            self.target_acc = 0.005;
        }

        self.compute_constants();
        self.rsc_call_depth -= 1;
    }

    fn random_big_change(&mut self, rng: &mut impl Rng) {
        let which_case = rng.random_range(0..4);

        self.rbc_call_depth += 1;
        if self.rbc_call_depth > 3 {
            self.rbc_call_depth -= 1;
            return;
        }

        match which_case {
            0 => {
                // trail length
                let temp = rng.random_range(0..(MAX_TRAIL_LEN - 25)) + 25;
                self.clear_bugs();
                self.trail_len = temp;
                self.compute_color_indices(rng);
                self.init_bugs(rng);
            }
            1 => {
                // Whee!
                for _ in 0..8 {
                    self.random_small_change(rng);
                }
            }
            2 => {
                self.clear_bugs();
                self.init_bugs(rng);
            }
            _ => self.pick_new_targets(rng),
        }

        self.rbc_call_depth -= 1;
    }
}

impl Animation for XRaySwarm {
    fn new(config: &AnimConfig) -> Self {
        let mut s = XRaySwarm {
            canvas: Vec::new(),
            width: config.width,
            height: config.height,
            colors: [0; 768],
            xsize: config.width as i32,
            ysize: config.height as i32,
            delay: 20_000, // *delay: 20000
            maxx: 1.0,
            maxy: 1.0,
            dt: 0.3,
            target_vel: 0.03,
            target_acc: 0.02,
            max_vel: 0.05,
            max_acc: 0.03,
            noise: 0.01,
            min_vel_multiplier: 0.5,
            nbugs: 0,
            ntargets: 0,
            trail_len: MAX_TRAIL_LEN,
            dt_inv: 0.0,
            half_dt_sq: 0.0,
            target_vel_sq: 0.0,
            max_vel_sq: 0.0,
            min_vel_sq: 0.0,
            min_vel: 0.0,
            bugs: vec![ZERO_BUG; MAX_BUGS],
            targets: vec![ZERO_BUG; MAX_TARGETS],
            head: 0,
            tail: 0,
            color_scheme: COLOR_TRAILS,
            change_prob: 0.08,
            gray_index: [0; MAX_TRAIL_LEN],
            red_index: [0; MAX_TRAIL_LEN],
            blue_index: [0; MAX_TRAIL_LEN],
            gray_s_index: [0; MAX_TRAIL_LEN],
            red_s_index: [0; MAX_TRAIL_LEN],
            blue_s_index: [0; MAX_TRAIL_LEN],
            random_index: [0; MAX_TRAIL_LEN],
            num_random_colors: MAX_TRAIL_LEN,
            check_index: 0,
            rsc_call_depth: 0,
            rbc_call_depth: 0,
            start: Instant::now(),
            draw_nframes: 0,
            draw_delay_accum: 0,
            draw_sleep_count: 0,
            this_delay: 20_000,
        };
        s.reset(config);
        s
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();
        let mut this_delay = self.delay;

        let draw_start = self.get_time();

        let draw_cnt;
        if self.delay > 0 {
            draw_cnt = 2;
            self.dt = DESIRED_DT / 2.0;
        } else {
            draw_cnt = 1;
            self.dt = DESIRED_DT;
        }

        for _ in 0..draw_cnt {
            self.update_state(&mut rng);
            self.draw_bugs();
        }

        let draw_end = self.get_time();
        self.draw_nframes += 1;

        // automagical level-of-detail and parameter mutation, driven by wall time
        if draw_end > draw_start + 0.5 {
            if frand(&mut rng, 1.0) < self.change_prob {
                self.random_small_change(&mut rng);
            }
            if frand(&mut rng, 1.0) < self.change_prob * 0.3 {
                self.random_big_change(&mut rng);
            }
            let elapsed = (draw_end - draw_start) as f32;

            let time_per_frame = elapsed / self.draw_nframes as f32 - self.delay as f32 * 1e-6;
            let fps = self.draw_nframes as f32 / elapsed;

            if fps > MAX_FPS {
                // ponytail: the C stuffs a possibly negative float into an
                // unsigned long here; clamp to 0 instead
                let d = (1.0 / MAX_FPS - (time_per_frame + self.delay as f32 * 1e-6)) * 1e6;
                self.delay = if d < 0.0 { 0 } else { d as u64 };
            } else if self.dt * fps < MIN_FPS * DESIRED_DT {
                // need to speed things up somehow: reduce trail length
                if self.trail_len > 10 {
                    self.clear_bugs();
                    self.trail_len = (self.trail_len as f32 * (fps / MIN_FPS)) as usize;
                    if self.trail_len < 10 {
                        self.trail_len = 10;
                    }
                    self.compute_color_indices(&mut rng);
                    self.init_bugs(&mut rng);
                }
            }

            self.draw_nframes = 0;
        }

        if self.delay <= 10000 {
            self.draw_delay_accum += self.delay;
            if self.draw_delay_accum > 10000 {
                this_delay = self.draw_delay_accum;
                self.draw_delay_accum = 0;
                self.draw_sleep_count = 0;
            }
            self.draw_sleep_count += 1;
            if self.draw_sleep_count > 2 {
                self.draw_sleep_count = 0;
                this_delay = 10000;
            }
        }

        self.this_delay = this_delay;
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        let len = self.canvas.len().min(buffer.len());
        buffer[..len].copy_from_slice(&self.canvas[..len]);
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;
        self.canvas = vec![0u8; (config.width * config.height * 4) as usize];
        clear_buffer(&mut self.canvas, Color::new(255, 0, 0, 0));

        self.dt = 0.3;
        self.target_vel = 0.03;
        self.target_acc = 0.02;
        self.max_vel = 0.05;
        self.max_acc = 0.03;
        self.noise = 0.01;
        self.min_vel_multiplier = 0.5;
        self.color_scheme = COLOR_TRAILS; // the C hardcodes 2 rather than -1 (random)
        self.change_prob = 0.08;
        self.delay = 20_000;
        self.this_delay = 20_000;
        self.draw_nframes = 0;
        self.draw_delay_accum = 0;
        self.draw_sleep_count = 0;
        self.check_index = 0;
        self.rsc_call_depth = 0;
        self.rbc_call_depth = 0;

        self.init_cmap(&mut rng);

        self.xsize = config.width as i32;
        self.ysize = config.height as i32;
        self.maxx = 1.0;
        self.maxy = self.ysize as f32 / self.xsize as f32;
        if self.color_scheme < 0 {
            self.color_scheme = rng.random_range(0..NUM_SCHEMES);
        }

        self.compute_constants();

        // nbugs/ntargets/trailLen default to -1 in the C: randomize them here
        // (initBugs' negative-value branch)
        let mut ntargets =
            ((0.25 + frand(&mut rng, 0.75) * frand(&mut rng, 1.0)) * MAX_TARGETS as f32) as i32;
        if ntargets < 1 {
            ntargets = 1;
        }
        self.ntargets = ntargets as usize;
        let nbugs = ((0.25 + frand(&mut rng, 0.75) * frand(&mut rng, 1.0)) * MAX_BUGS as f32) as i32;
        self.nbugs = nbugs.max(0) as usize;
        self.trail_len =
            ((1.0 - frand(&mut rng, 0.6) * frand(&mut rng, 1.0)) * MAX_TRAIL_LEN as f32) as usize;

        self.init_bugs(&mut rng);
        self.start = Instant::now();
        self.compute_color_indices(&mut rng);

        if self.change_prob > 0.0 {
            // for (i = random()%5+5; i >= 0; i--)
            for _ in 0..(rng.random_range(0..5) + 6) {
                self.random_small_change(&mut rng);
            }
        }
    }

    fn clears_each_frame(&self) -> bool {
        false
    }

    fn frame_delay_us(&self) -> u64 {
        self.this_delay
    }
}
