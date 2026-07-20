/* xscreensaver, Copyright (c) 2000 Paul "Joey" Clark <pclark@bris.ac.uk>
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * 19971004: Johannes Keukelaar <johannes@nada.kth.se>: Use helix screen
 *           eraser.
 *
 * WhirlwindWarp: moving stars.  Ported from QBasic by Joey.
 * Version 1.3.  Smooth with pretty colours.
 *
 * Rust port of xscreensaver's whirlwindwarp.c for baijia-suo.
 */

use crate::animation::primitives::{hsv_to_rgb, put_pixel, rgb16, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};
use rand::Rng;

/* Maximum number of points, maximum tail length, and the number of
 * forcefields/effects (hard-coded) */
const MAXPS: usize = 1000;
const MAXTS: usize = 50;
const FS: usize = 16;

/* defaults table */
const POINTS: usize = 400;
const TAILS: usize = 8;
/* maxfps 200 cap in the C draw loop */
const FRAME_DELAY_US: u64 = 1_000_000 / 200;

const BG: Color = Color { a: 255, r: 0, g: 0, b: 0 };

/// between -1.0 (inclusive) and +1.0 (exclusive)
fn myrnd(rng: &mut impl Rng) -> f32 {
    2.0 * (rng.random_range(0..10_000_000) as f32 / 10_000_000.0 - 0.5)
}

/// Adjust a variable var about optimum op, with damp = dampening about op,
/// force = force of random perturbation
fn stars_perturb(var: f32, op: f32, damp: f32, force: f32, rng: &mut impl Rng) -> f32 {
    op + damp * (var - op) + force * myrnd(rng) / 4.0
}

fn fill_rect(buffer: &mut [u8], w: u32, h: u32, x: i32, y: i32, size: i32, color: Color) {
    for yy in y..y + size {
        for xx in x..x + size {
            put_pixel(buffer, w, h, xx, yy, color);
        }
    }
}

pub struct WhirlwindWarp {
    scrwid: i32,
    scrhei: i32,
    starsize: i32,

    cx: Vec<f32>, // Current x,y of stars in realspace
    cy: Vec<f32>,
    tx: Vec<i32>, // Previous x,y plots in pixelspace for removal later
    ty: Vec<i32>,

    fon: [bool; FS], // Is field on or off?
    var: [f32; FS],  // Current parameter
    op: [f32; FS],   // Optimum (central/mean) value
    acc: [f32; FS],
    vel: [f32; FS],

    ps: usize, // Number of points and tail length
    ts: usize,

    color: Vec<Color>, // The colour assigned to each star
    colsavailable: usize,
    nt: usize,
    hue: i32,

    buffer: Vec<u8>,
}

impl WhirlwindWarp {
    fn stars_newp(&mut self, rng: &mut impl Rng, pp: usize) {
        self.cx[pp] = myrnd(rng);
        self.cy[pp] = myrnd(rng);
    }

    /// Get pixel coordinates of a star
    fn stars_scrpos_x(&self, pp: usize) -> i32 {
        (self.scrwid as f32 * (self.cx[pp] + 1.0) / 2.0) as i32
    }

    fn stars_scrpos_y(&self, pp: usize) -> i32 {
        (self.scrhei as f32 * (self.cy[pp] + 1.0) / 2.0) as i32
    }

    /// Move a star according to acting forcefields
    fn stars_move(&mut self, pp: usize, rot_cos: f32, rot_sin: f32) {
        let mut x = self.cx[pp];
        let mut y = self.cy[pp];

        // Squirge towards edges (makes a leaf shape).
        // These ones must go first, to avoid x+1.0 < 0
        if self.fon[6] {
            x = -1.0 + 2.0 * ((x + 1.0) / 2.0).powf(self.var[6]);
        }
        if self.fon[7] {
            y = -1.0 + 2.0 * ((y + 1.0) / 2.0).powf(self.var[7]);
        }

        // Warping in/out
        if self.fon[1] {
            x *= self.var[1];
            y *= self.var[1];
        }

        // Rotation
        if self.fon[2] {
            let nx = x * rot_cos + y * rot_sin;
            let ny = -x * rot_sin + y * rot_cos;
            x = nx;
            y = ny;
        }

        // Asymptotes (looks like a plane with a horizon; equivalent to 1D warp)
        if self.fon[3] {
            // Horizontal asymptote
            y *= self.var[3];
        }
        if self.fon[4] {
            // Vertical asymptote
            x += self.var[4] * x;
        }
        if self.fon[5] {
            // Vertical asymptote at right of screen
            x = (x - 1.0) * self.var[5] + 1.0;
        }

        // Splitting (whirlwind effect)
        let num_splits = 2 + (self.var[0].abs() * 1000.0) as i32;
        let thru = ((num_splits as f32 * pp as f32 / self.ps as f32) as i32) as f32
            / (num_splits - 1) as f32;
        if self.fon[8] {
            x += 0.5 * self.var[8] * (-1.0 + 2.0 * thru);
        }
        if self.fon[9] {
            y += 0.5 * self.var[9] * (-1.0 + 2.0 * thru);
        }

        // Waves
        if self.fon[10] {
            y += 0.4 * self.var[10] * (300.0 * self.var[12] * x + 600.0 * self.var[11]).sin();
        }
        if self.fon[13] {
            x += 0.4 * self.var[13] * (300.0 * self.var[15] * y + 600.0 * self.var[14]).sin();
        }

        self.cx[pp] = x;
        self.cy[pp] = y;
    }

    /// Turns a forcefield on, and ensures its vars are suitable.
    fn turn_on_field(&mut self, rng: &mut impl Rng, ff: usize) {
        if !self.fon[ff] {
            self.acc[ff] = 0.02 * myrnd(rng);
            self.vel[ff] = 0.0;
            self.var[ff] = self.op[ff];
        }
        self.fon[ff] = true;
        if ff == 10 {
            self.turn_on_field(rng, 11);
            self.turn_on_field(rng, 12);
        }
        if ff == 13 {
            self.turn_on_field(rng, 14);
            self.turn_on_field(rng, 15);
        }
    }

    fn init(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.scrwid = config.width as i32;
        self.scrhei = config.height as i32;
        self.buffer = vec![0u8; config.width as usize * config.height as usize * 4];
        for px in self.buffer.chunks_exact_mut(4) {
            px[3] = 255;
        }

        self.ps = POINTS.min(MAXPS);
        self.ts = TAILS.min(MAXTS);

        self.starsize = (self.scrhei / 480).max(1);

        // Setup colours
        self.color = (0..self.ps)
            .map(|_| {
                let (r, g, b) = hsv_to_rgb(
                    rng.random_range(0..360),
                    (0.6 + 0.4 * myrnd(&mut rng)) as f64,
                    (0.6 + 0.4 * myrnd(&mut rng)) as f64,
                );
                rgb16(r, g, b)
            })
            .collect();
        // every allocation succeeds here, so colsavailable ends at ps-1 as in C
        self.colsavailable = self.ps - 1;

        // Set up central (optimal) points for each different forcefield
        self.op[1] = 1.0; // Warp
        self.op[2] = 0.0; // Rotation
        self.op[3] = 1.0; // Horizontal asymptote
        self.op[4] = 0.0; // Vertical asymptote
        self.op[5] = 1.0; // Vertical asymptote right
        self.op[6] = 1.0; // Squirge x
        self.op[7] = 1.0; // Squirge y
        self.op[0] = 0.0; // Split number (inactive)
        self.op[8] = 0.0; // Split velocity x
        self.op[9] = 0.0; // Split velocity y
        self.op[10] = 0.0; // Horizontal wave amplitude
        self.op[11] = myrnd(&mut rng) * std::f32::consts::PI; // Horizontal wave phase (inactive)
        self.op[12] = 0.01; // Horizontal wave frequency (inactive)
        self.op[13] = 0.0; // Vertical wave amplitude
        self.op[14] = myrnd(&mut rng) * std::f32::consts::PI; // Vertical wave phase (inactive)
        self.op[15] = 0.01; // Vertical wave frequency (inactive)

        // Initialise parameters to optimum, all off
        for f in 0..FS {
            self.var[f] = self.op[f];
            self.fon[f] = myrnd(&mut rng) > 0.5;
            self.acc[f] = 0.02 * myrnd(&mut rng);
            self.vel[f] = 0.0;
        }

        // Initialise stars
        self.cx = vec![0.0; self.ps];
        self.cy = vec![0.0; self.ps];
        for p in 0..self.ps {
            self.stars_newp(&mut rng, p);
        }

        // tx[nt],ty[nt] remember earlier screen plots (tails of stars)
        // which are deleted when nt comes round again
        self.tx = vec![0; self.ps * self.ts];
        self.ty = vec![0; self.ps * self.ts];
        self.nt = 0;

        self.hue = (180.0 + 180.0 * myrnd(&mut rng)) as i32;
    }
}

impl Animation for WhirlwindWarp {
    fn new(config: &AnimConfig) -> Self {
        let mut st = WhirlwindWarp {
            scrwid: 0,
            scrhei: 0,
            starsize: 1,
            cx: Vec::new(),
            cy: Vec::new(),
            tx: Vec::new(),
            ty: Vec::new(),
            fon: [false; FS],
            var: [0.0; FS],
            op: [0.0; FS],
            acc: [0.0; FS],
            vel: [0.0; FS],
            ps: POINTS,
            ts: TAILS,
            color: Vec::new(),
            colsavailable: 0,
            nt: 0,
            hue: 0,
            buffer: Vec::new(),
        };
        st.init(config);
        st
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        if myrnd(&mut rng) > 0.75 {
            // Change one of the allocated colours to something near the current hue.
            // By changing a random colour, we sometimes get a tight colour spread,
            // sometimes a diverse one.
            let pp = (self.colsavailable as f32 * (0.5 + myrnd(&mut rng) / 2.0)) as usize;
            let (r, g, b) = hsv_to_rgb(
                self.hue,
                (0.6 + 0.4 * myrnd(&mut rng)) as f64,
                (0.6 + 0.4 * myrnd(&mut rng)) as f64,
            );
            self.color[pp] = rgb16(r, g, b);
            // C stores hue in an int, so the fraction truncates each step
            self.hue = (self.hue as f32 + 0.5 + myrnd(&mut rng) * 9.0) as i32;
            if self.hue < 0 {
                self.hue += 360;
            }
            if self.hue >= 360 {
                self.hue -= 360;
            }
        }

        // Move current points
        let (w, h) = (self.scrwid as u32, self.scrhei as u32);
        // Rotation angle is constant across this loop (only adjusted afterwards),
        // so compute cos/sin once instead of twice per star.
        let rot_cos = (1.1 * self.var[2]).cos();
        let rot_sin = (1.1 * self.var[2]).sin();
        for p in 0..self.ps {
            // Erase old
            let (ox, oy) = (self.tx[self.nt], self.ty[self.nt]);
            fill_rect(&mut self.buffer, w, h, ox, oy, self.starsize, BG);

            // Move
            self.stars_move(p, rot_cos, rot_sin);
            // If moved off screen, create a new one
            if self.cx[p] <= -0.9999
                || self.cx[p] >= 0.9999
                || self.cy[p] <= -0.9999
                || self.cy[p] >= 0.9999
                || self.cx[p].abs() < 0.0001
                || self.cy[p].abs() < 0.0001
            {
                self.stars_newp(&mut rng, p);
            } else if myrnd(&mut rng) > 0.99 {
                // Reset at random
                self.stars_newp(&mut rng, p);
            }

            // Draw point
            let sx = self.stars_scrpos_x(p);
            let sy = self.stars_scrpos_y(p);
            fill_rect(&mut self.buffer, w, h, sx, sy, self.starsize, self.color[p]);

            // Remember it for removal later
            self.tx[self.nt] = sx;
            self.ty[self.nt] = sy;
            self.nt = (self.nt + 1) % (self.ps * self.ts);
        }

        // Adjust force fields
        let mut cnt = 0;
        for f in 0..FS {
            // Adjust forcefield's parameter
            if self.fon[f] {
                // This configuration produces var[f]s usually below 0.01
                self.acc[f] = stars_perturb(self.acc[f], 0.0, 0.98, 0.005, &mut rng);
                self.vel[f] =
                    stars_perturb(self.vel[f] + 0.03 * self.acc[f], 0.0, 0.995, 0.0, &mut rng);
                self.var[f] =
                    self.op[f] + (self.var[f] - self.op[f]) * 0.9995 + 0.001 * self.vel[f];
            }

            // Decide whether to turn this forcefield on or off.
            // prob_on makes the "splitting" effects less likely than the rest
            let prob_on = if f == 8 || f == 9 { 0.999975 } else { 0.9999 };
            if !self.fon[f] && myrnd(&mut rng) > prob_on {
                self.turn_on_field(&mut rng, f);
            } else if self.fon[f]
                && myrnd(&mut rng) > 0.99
                && (self.var[f] - self.op[f]).abs() < 0.0005
                && self.vel[f].abs() < 0.005
            {
                // We only turn it off if it has gently returned to its optimal
                // (as opposed to rapidly passing through it).
                self.fon[f] = false;
            }

            if self.fon[f] {
                cnt += 1;
            }
        }

        // Ensure at least three forcefields are on.
        if cnt < 3 {
            let f = rng.random_range(0..FS);
            self.turn_on_field(&mut rng, f);
        }
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        let n = buffer.len().min(self.buffer.len());
        buffer[..n].copy_from_slice(&self.buffer[..n]);
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.init(config);
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::CompleteFrame
    }

    fn frame_delay_us(&self) -> u64 {
        FRAME_DELAY_US
    }
}
