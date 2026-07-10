/* fluidballs, Copyright (c) 2000 by Peter Birtles <peter@bqdesign.com.au>
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Ported to X11 and xscreensaver by jwz, 27-Feb-2002.
 * Some physics improvements by Steven Barker <steve@blckknght.org>
 *
 * Rust port of xscreensaver's fluidballs.c (mouse/FPS machinery omitted).
 */

use rand::Rng;
use std::time::Instant;

use crate::animation::primitives::{put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

fn fill_circle(buffer: &mut [u8], width: u32, height: u32, cx: i32, cy: i32, r: i32, color: Color) {
    for dy in -r..=r {
        let dx = ((r * r - dy * dy) as f64).sqrt() as i32;
        for x in (cx - dx)..=(cx + dx) {
            put_pixel(buffer, width, height, x, cy + dy, color);
        }
    }
}

pub struct FluidBalls {
    delay_us: u64,

    count: usize,
    xmin: f32,
    ymin: f32,
    xmax: f32,
    ymax: f32,

    tc: f32,   // time constant (time-warp multiplier)
    accx: f32, // horizontal acceleration (wind)
    accy: f32, // vertical acceleration (gravity)

    vx: Vec<f32>,
    vy: Vec<f32>,
    px: Vec<f32>,
    py: Vec<f32>,
    opx: Vec<f32>,
    opy: Vec<f32>,
    r: Vec<f32>,
    m: Vec<f32>,

    e: f32, // coefficient of elasticity
    max_radius: f32,

    random_sizes_p: bool,
    shake_p: bool,
    shake_threshold: f32,
    last_shake: Instant,

    color: Color,
}

impl FluidBalls {
    /* Re-pick the color of the balls (recolor in the C source; the
       mouse-ball color is unused since there is no mouse). */
    fn recolor(&mut self) {
        let mut rng = rand::rng();
        let mut ch = || ((0x8888u32 + rng.random_range(0..0x8888u32)) >> 8) as u8;
        self.color = Color::new(255, ch(), ch(), ch());
    }

    /* Messes with gravity: permute "down" to be in a random direction. */
    fn shake(&mut self) {
        let a = self.accx;
        let b = self.accy;
        match rand::rng().random_range(0..4u32) {
            0 => {
                self.accx = a;
                self.accy = b;
            }
            1 => {
                self.accx = -a;
                self.accy = -b;
            }
            2 => {
                self.accx = b;
                self.accy = a;
            }
            _ => {
                self.accx = -b;
                self.accy = -a;
            }
        }
        self.last_shake = Instant::now();
        self.recolor();
    }

    fn time_since_shake(&self) -> u64 {
        self.last_shake.elapsed().as_secs()
    }

    /* Implements the laws of physics: move balls to their new positions. */
    fn update_balls(&mut self) {
        let count = self.count;

        /* For each ball, compute the influence of every other ball. */
        for a in 0..count.saturating_sub(1) {
            for b in (a + 1)..count {
                let mut d = (self.px[a] - self.px[b]) * (self.px[a] - self.px[b])
                    + (self.py[a] - self.py[b]) * (self.py[a] - self.py[b]);
                let dee2 = (self.r[a] + self.r[b]) * (self.r[a] + self.r[b]);
                if d < dee2 {
                    d = d.sqrt();
                    let dd = self.r[a] + self.r[b] - d;

                    let cdx = (self.px[b] - self.px[a]) / d;
                    let cdy = (self.py[b] - self.py[a]) / d;

                    /* Move each ball apart from the other by half the
                     * 'collision' distance. */
                    self.px[a] -= 0.5 * dd * cdx;
                    self.py[a] -= 0.5 * dd * cdy;
                    self.px[b] += 0.5 * dd * cdx;
                    self.py[b] += 0.5 * dd * cdy;

                    let ma = self.m[a];
                    let mb = self.m[b];

                    let mut vxa = self.vx[a];
                    let mut vya = self.vy[a];
                    let mut vxb = self.vx[b];
                    let mut vyb = self.vy[b];

                    let vca = vxa * cdx + vya * cdy; /* the component of each velocity */
                    let vcb = vxb * cdx + vyb * cdy; /* along the axis of the collision */

                    /* elastic collision */
                    let mut dva = (vca * (ma - mb) + vcb * 2.0 * mb) / (ma + mb) - vca;
                    let mut dvb = (vcb * (mb - ma) + vca * 2.0 * ma) / (ma + mb) - vcb;

                    dva *= self.e; /* some energy lost to inelasticity */
                    dvb *= self.e;

                    vxa += dva * cdx;
                    vya += dva * cdy;
                    vxb += dvb * cdx;
                    vyb += dvb * cdy;

                    self.vx[a] = vxa;
                    self.vy[a] = vya;
                    self.vx[b] = vxb;
                    self.vy[b] = vyb;
                }
            }
        }

        /* Force all balls to be on screen. */
        for a in 0..count {
            if self.px[a] <= self.xmin + self.r[a] {
                self.px[a] = self.xmin + self.r[a];
                self.vx[a] = -self.vx[a] * self.e;
            }
            if self.px[a] >= self.xmax - self.r[a] {
                self.px[a] = self.xmax - self.r[a];
                self.vx[a] = -self.vx[a] * self.e;
            }
            if self.py[a] <= self.ymin + self.r[a] {
                self.py[a] = self.ymin + self.r[a];
                self.vy[a] = -self.vy[a] * self.e;
            }
            if self.py[a] >= self.ymax - self.r[a] {
                self.py[a] = self.ymax - self.r[a];
                self.vy[a] = -self.vy[a] * self.e;
            }
        }

        /* Apply gravity to all balls. */
        for a in 0..count {
            self.vx[a] += self.accx * self.tc;
            self.vy[a] += self.accy * self.tc;
            self.px[a] += self.vx[a] * self.tc;
            self.py[a] += self.vy[a] * self.tc;
        }
    }
}

impl Animation for FluidBalls {
    fn new(config: &AnimConfig) -> Self {
        let mut state = FluidBalls {
            delay_us: 10_000, /* hack default *delay: 10000 */
            count: 0,
            xmin: 0.0,
            ymin: 0.0,
            xmax: 0.0,
            ymax: 0.0,
            tc: 1.0,
            accx: 0.0,
            accy: 0.01,
            vx: Vec::new(),
            vy: Vec::new(),
            px: Vec::new(),
            py: Vec::new(),
            opx: Vec::new(),
            opy: Vec::new(),
            r: Vec::new(),
            m: Vec::new(),
            e: 0.97,
            max_radius: 12.5,
            random_sizes_p: true,
            shake_p: true,
            shake_threshold: 0.015,
            last_shake: Instant::now(),
            color: Color::new(255, 255, 255, 0),
        };
        state.reset(config);
        state
    }

    fn tick(&mut self) {
        /* Bookkeeping from repaint_balls: track how far each ball moved
           this frame, and shake when things settle (or after 30 seconds). */
        let mut max_d: f32 = 0.0;
        for a in 0..self.count {
            if self.shake_p {
                let d = (self.px[a] - self.opx[a]) * (self.px[a] - self.opx[a])
                    + (self.py[a] - self.opy[a]) * (self.py[a] - self.opy[a]);
                if d > max_d {
                    max_d = d;
                }
            }
            self.opx[a] = self.px[a];
            self.opy[a] = self.py[a];
        }

        if self.shake_p && self.time_since_shake() > 5 {
            max_d /= self.max_radius;
            if max_d < self.shake_threshold ||     /* when it's stable */
                self.time_since_shake() > 30
            /* or when 30 secs has passed */
            {
                self.shake();
            }
        }

        self.update_balls();
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for a in 0..self.count {
            /* XFillArc bounding box, as in repaint_balls */
            let x1 = (self.px[a] - self.r[a] - self.xmin) as i32;
            let y1 = (self.py[a] - self.r[a] - self.ymin) as i32;
            let x2 = (self.px[a] + self.r[a] - self.xmin) as i32;
            let y2 = (self.py[a] + self.r[a] - self.ymin) as i32;
            let r = (x2 - x1) / 2;
            fill_circle(buffer, width, height, x1 + r, y1 + (y2 - y1) / 2, r, self.color);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.delay_us = 10_000;

        self.xmin = 0.0;
        self.ymin = 0.0;
        self.xmax = config.width as f32;
        self.ymax = config.height as f32;

        let extx = self.xmax - self.xmin;
        let exty = self.ymax - self.ymin;

        /* hack defaults: count 300, size 25 (AnimConfig values of 0/1 mean
           "use the hack default") */
        let mut count = if config.count > 0 { config.count as usize } else { 300 };
        if count < 1 {
            count = 20;
        }

        let size = if config.size > 1 { config.size } else { 25 };
        self.max_radius = size as f32 / 2.0;
        if self.max_radius < 1.0 {
            self.max_radius = 1.0;
        }

        if config.width > 2560 || config.height > 2560 {
            self.max_radius *= 3.0; /* Retina displays */
        }

        if config.width < 100 || config.height < 100 {
            /* tiny window */
            if self.max_radius > 5.0 {
                self.max_radius = 5.0;
            }
        }

        self.random_sizes_p = true;

        /* If the initial window size is too small to hold all these balls,
           make fewer of them... */
        {
            let r = if self.random_sizes_p {
                self.max_radius * 0.7
            } else {
                self.max_radius
            };
            let ball_area = std::f32::consts::PI * r * r;
            let balls_area = count as f32 * ball_area;
            let mut window_area = (config.width * config.height) as f32;
            window_area *= 0.75; /* don't pack it completely full */
            if balls_area > window_area {
                count = (window_area / ball_area) as usize;
            }
        }
        self.count = count;

        self.accx = 0.0; /* wind */
        self.accy = 0.01; /* gravity */
        self.e = 0.97; /* elasticity */
        self.tc = 1.0; /* timeScale */
        self.shake_p = true;
        self.shake_threshold = 0.015;
        self.last_shake = Instant::now();

        self.recolor();

        self.m = vec![0.0; count];
        self.r = vec![0.0; count];
        self.vx = vec![0.0; count];
        self.vy = vec![0.0; count];
        self.px = vec![0.0; count];
        self.py = vec![0.0; count];

        for i in 0..count {
            self.px[i] = rng.random::<f32>() * extx + self.xmin;
            self.py[i] = rng.random::<f32>() * exty + self.ymin;
            self.vx[i] = rng.random::<f32>() * 0.2 - 0.1;
            self.vy[i] = rng.random::<f32>() * 0.2 - 0.1;

            self.r[i] = if self.random_sizes_p {
                (0.2 + rng.random::<f32>() * 0.8) * self.max_radius
            } else {
                self.max_radius
            };

            self.m[i] = self.r[i].powi(3) * std::f32::consts::PI * 1.3333;
        }

        self.opx = self.px.clone();
        self.opy = self.py.clone();
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
