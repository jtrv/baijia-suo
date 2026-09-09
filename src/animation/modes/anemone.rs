/* anemone, Copyright (c) 2001 Gabriel Finch
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Rust port of xscreensaver's anemone.c (with the needed parts of
 * utils/hsv.c / utils/colors.c ported inline).
 */

use crate::rng::RngExt;
use std::f64::consts::PI;

use crate::animation::primitives::{draw_thick_line, make_smooth_colormap, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const TWO_PI: f64 = 2.0 * PI;

/* anemone_defaults[]: *arms: 128, *width: 2, *finpoints: 64, *delay: 40000,
 * *withdraw: 1200, *turnspeed: 50, *colors: 20. */
const DEF_ARMS: usize = 128;
const DEF_WIDTH: i32 = 2;
const DEF_FINPOINTS: usize = 64;
const DEF_DELAY_US: u64 = 40_000;
const DEF_WITHDRAW: i32 = 1200;
const DEF_TURNSPEED: f64 = 50.0;
const DEF_COLORS: usize = 20;

/* hsv.c / colors.c / spline.c helpers and CapRound wide lines now live in
 * animation::primitives (shared with attraction). */

/* 3D representation of appendages: virtual coords (x,y,z) with x and z
 * combining to give the screen x; per-point speeds (sx,sy,sz). */
#[derive(Clone, Copy, Default)]
struct VPend {
    x: f64,
    y: f64,
    z: f64,
    sx: i32,
    sy: i32,
    sz: i32,
}

/* per-appendage: colour, number of points, grow/shrink indicator, and
 * growth rate (smaller == faster growth) */
#[derive(Clone)]
struct AppDef {
    col: Color,
    numpt: usize,
    growth: i32,
    rate: u16,
}

pub struct Anemone {
    arms: usize,      /* number of arms */
    finpoints: usize, /* final number of points in each array */
    line_width: i32,
    withdraw: i32,

    v_pendage: Vec<Vec<VPend>>,
    app_d: Vec<AppDef>,

    turn: f64,
    turndelta: f64,
    /* animateAnemone() computes sin/cos once per frame, before the per-arm
     * turn increments; render draws with these frozen values. */
    sint: f64,
    cost: f64,

    mx: i32, /* max screen coordinates */
    my: i32,
}

impl Anemone {
    fn create_points(&mut self, rng: &mut impl RngExt) {
        let withdrawall = rng.random_range(0..self.withdraw.max(1));

        for i in 0..self.arms {
            if withdrawall == 0 {
                self.app_d[i].growth = -(self.finpoints as i32);
                /* The C flips turndelta once per arm here; with an even
                 * number of arms that is a net no-op.  Kept as-is. */
                self.turndelta = -self.turndelta;
            } else if withdrawall < 11 {
                self.app_d[i].growth = -(self.app_d[i].numpt as i32);
            } else if rng.random_range(0..100_u32) < self.app_d[i].rate as u32 {
                let app = &mut self.app_d[i];
                if app.growth > 0 {
                    app.growth -= 1;
                    if app.growth == 0 {
                        app.growth = -rng.random_range(0..self.finpoints as i32) - 1;
                    }
                    let n = app.numpt;
                    if n < self.finpoints - 1 {
                        /* add a piece: new speed at point n, then the
                         * position one ahead at n+1 (as in the C, positions
                         * are filled one point ahead of the speeds). */
                        app.numpt += 1;
                        let v = &mut self.v_pendage[i];
                        v[n].sx = v[n - 1].sx + rng.random_range(0..3) - 1;
                        v[n].sy = v[n - 1].sy + rng.random_range(0..3) - 1;
                        v[n].sz = v[n - 1].sz + rng.random_range(0..3) - 1;
                        v[n + 1].x = v[n].x + v[n].sx as f64;
                        v[n + 1].y = v[n].y + v[n].sy as f64;
                        v[n + 1].z = v[n].z + v[n].sz as f64;
                    }
                }
            }
        }
    }
}

impl Animation for Anemone {
    fn new(config: &AnimConfig) -> Self {
        let mut s = Anemone {
            arms: DEF_ARMS,
            finpoints: DEF_FINPOINTS,
            line_width: DEF_WIDTH,
            withdraw: DEF_WITHDRAW,
            v_pendage: Vec::new(),
            app_d: Vec::new(),
            turn: 0.0,
            turndelta: DEF_TURNSPEED / 100000.0,
            sint: 0.0,
            cost: 1.0,
            mx: 0,
            my: 0,
        };
        s.reset(config);
        s
    }

    /* animateAnemone() minus the drawing (done in render). */
    fn tick(&mut self) {
        let mut rng = crate::rng::rng();

        self.sint = self.turn.sin();
        self.cost = self.turn.cos();

        for i in 0..self.arms {
            let app = &mut self.app_d[i];
            if rng.random_range(0..25_u32) < app.rate as u32 && app.growth < 0 {
                if app.numpt > 1 {
                    app.numpt -= 1;
                }
                app.growth += 1;
                if app.growth == 0 {
                    app.growth =
                        rng.random_range(0..(self.finpoints - app.numpt).max(1) as i32) + 1;
                }
            }
            self.turn += self.turndelta;
        }
        self.create_points(&mut rng);

        if self.turn >= TWO_PI {
            self.turn -= TWO_PI;
        }
    }

    /* drawImage() for every arm.  The jitter is applied at draw time and
     * never stored back, exactly as in the C. */
    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let mut rng = crate::rng::rng();
        let (sint, cost) = (self.sint, self.cost);
        let mx2 = (self.mx / 2) as f64;

        for i in 0..self.arms {
            let numpt = self.app_d[i].numpt;
            if numpt == 1 {
                continue;
            }
            let col = self.app_d[i].col;
            let v = &self.v_pendage[i];

            let (mut cx, mut cy, mut cz) = (v[0].x, v[0].y, v[0].z);
            let (mut nx, mut ny, mut nz) = (0.0_f64, 0.0_f64, 0.0_f64);

            for q in 0..numpt - 1 {
                nx = v[q + 1].x + 2.0 - rng.random_range(0..5) as f64;
                ny = v[q + 1].y + 2.0 - rng.random_range(0..5) as f64;
                nz = v[q + 1].z + 2.0 - rng.random_range(0..5) as f64;

                draw_thick_line(
                    buffer,
                    width,
                    height,
                    (mx2 + cx * cost - cz * sint) as i32,
                    cy as i32,
                    (mx2 + nx * cost - nz * sint) as i32,
                    ny as i32,
                    self.line_width,
                    col,
                );

                cx = nx;
                cy = ny;
                cz = nz;
            }
            /* The C redraws the final segment at triple width with round
             * caps; both endpoints coincide, so it is a round tip. */
            draw_thick_line(
                buffer,
                width,
                height,
                (mx2 + cx * cost - cz * sint) as i32,
                cy as i32,
                (mx2 + nx * cost - nz * sint) as i32,
                ny as i32,
                self.line_width * 3,
                col,
            );
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = crate::rng::rng();

        let scr_width = config.width as i32;
        let scr_height = config.height as i32;

        self.turn = 0.0;
        self.sint = 0.0;
        self.cost = 1.0;

        self.arms = if config.count <= 0 { DEF_ARMS } else { config.count as usize };
        self.finpoints = if config.cycles <= 0 { DEF_FINPOINTS } else { config.cycles as usize };
        /* AnimConfig's generic size default (1) doesn't encode anemone's
         * *width: 2 resource, so treat <= 1 as "use the default". */
        self.line_width = if config.size <= 1 { DEF_WIDTH } else { config.size };
        self.withdraw = DEF_WITHDRAW;
        self.turndelta = DEF_TURNSPEED / 100000.0;

        if scr_width > 2560 || scr_height > 2560 {
            /* Retina displays */
            self.line_width *= 4;
            self.finpoints *= 2;
            self.withdraw *= 2;
        }

        /* anemone_init: ncolors = *colors resource + 3, smooth colormap. */
        let colors = make_smooth_colormap(DEF_COLORS + 3, &mut rng);

        /* initAppendages() */
        self.mx = scr_width - 1;
        self.my = scr_height - 1;

        self.v_pendage = vec![vec![VPend::default(); self.finpoints + 1]; self.arms];
        self.app_d = Vec::with_capacity(self.arms);

        for i in 0..self.arms {
            let col = colors[rng.random_range(0..colors.len())];
            let growth = (self.finpoints / 2) as i32
                + rng.random_range(0..(self.finpoints / 2).max(1) as i32);
            let rate = (rng.random_range(0..11_u32) * rng.random_range(0..11_u32)) as u16;
            self.app_d.push(AppDef { col, numpt: 1, growth, rate });

            /* The C computes x = (1 - RND(1001) / 500) with *integer*
             * division, so x, y, z are each in {-1, 0, 1} and only
             * (0, 0, 0) passes the dist < 1 test: every arm starts at
             * the screen centre.  Reproduced faithfully. */
            let (x, y, z);
            loop {
                let xi = 1 - rng.random_range(0..1001_i32) / 500;
                let yi = 1 - rng.random_range(0..1001_i32) / 500;
                let zi = 1 - rng.random_range(0..1001_i32) / 500;
                if xi * xi + yi * yi + zi * zi < 1 {
                    x = xi as f64;
                    y = yi as f64;
                    z = zi as f64;
                    break;
                }
            }

            let v = &mut self.v_pendage[i];
            v[0].x = x * 200.0;
            v[0].y = (self.my / 2) as f64 + y * 200.0;
            v[0].z = z * 200.0;

            /* start the arm going outwards */
            v[0].sx = (v[0].x / 5.0) as i32;
            v[0].sy = ((v[0].y - (self.my / 2) as f64) / 5.0) as i32;
            v[0].sz = (v[0].z / 5.0) as i32;

            v[1].x = v[0].x + v[0].sx as f64;
            v[1].y = v[0].y + v[0].sy as f64;
            v[1].z = v[0].z + v[0].sz as f64;
        }
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        DEF_DELAY_US
    }
}
