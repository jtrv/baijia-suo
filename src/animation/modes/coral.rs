/* coral, by "Frederick G.M. Roeber" <roeber@netscape.com>, 15-jul-97.
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Rust port of xscreensaver's coral.c for baijia-suo.
 */

use crate::animation::primitives::{clear_buffer, hsv_to_rgb, put_pixel, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};
use rand::RngExt;

// defaults from coral.c: *density: 25, *seeds: 20, *delay: 5 (seconds),
// *delay2: 20000 (us), .background: black, .foreground: white
const NCOLORSMAX: usize = 200;
const DENSITY: i32 = 25;
const SEEDS: i32 = 20;
const DELAY_SEC: u64 = 5;
const DELAY2_US: u64 = 20_000;

/* port of utils/colors.c make_uniform_colormap (color computation only):
a full-hue ramp at random saturation/value in the 66%-100% range */
fn make_uniform_colormap(ncolors: usize, rng: &mut impl RngExt) -> Vec<Color> {
    let s = (rng.random_range(0..34) + 66) as f64 / 100.0;
    let v = (rng.random_range(0..34) + 66) as f64 / 100.0;
    let dh = 359.0 / ncolors as f64;
    (0..ncolors)
        .map(|i| {
            let (r, g, b) = hsv_to_rgb((i as f64 * dh) as i32, s, v);
            Color::new(255, (r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8)
        })
        .collect()
}

pub struct Coral {
    width: i32,
    height: i32,
    widthb: i32,
    scale: i32,
    board: Vec<u32>,
    walkers: Vec<(i32, i32)>,
    nwalkers: usize,
    ncolors: usize,
    colorindex: usize,
    colorsloth: usize,
    colors: Vec<Color>,
    pixels: Vec<u8>,
    done: bool,
    needs_init: bool,
    next_delay_us: u64,
}

impl Coral {
    fn getdot(&self, x: i32, y: i32) -> bool {
        self.board[(y * self.widthb + (x >> 5)) as usize] & (1 << (x & 31)) != 0
    }

    fn setdot(&mut self, x: i32, y: i32) {
        self.board[(y * self.widthb + (x >> 5)) as usize] |= 1 << (x & 31);
    }

    /* port of init_coral */
    fn init(&mut self) {
        let mut rng = rand::rng();
        clear_buffer(&mut self.pixels, Color::new(255, 0, 0, 0));

        self.scale = 1;
        if self.width > 2560 || self.height > 2560 {
            self.scale *= 2; /* Retina displays */
        }

        self.board = vec![0u32; (self.widthb * self.height) as usize];
        self.ncolors = NCOLORSMAX;
        self.colors = make_uniform_colormap(self.ncolors, &mut rng);
        self.colorindex = rng.random_range(0..self.ncolors);

        let density = DENSITY; // clamped to 1..=100 in C; constant 25 here
        self.nwalkers = ((self.width * self.height * density) / 100) as usize;
        self.walkers = vec![(0, 0); self.nwalkers];

        let seeds = SEEDS; // clamped to 1..=1000 in C; constant 20 here

        self.colorsloth = self.nwalkers * 2 / self.ncolors;

        if self.width <= 2 || self.height <= 2 {
            return;
        }

        for _ in 0..seeds {
            let mut x;
            let mut y;
            let mut max_repeat = 10;
            loop {
                x = 1 + rng.random_range(0..self.width - 2);
                y = 1 + rng.random_range(0..self.height - 2);
                if !self.getdot(x, y) || max_repeat == 0 {
                    break;
                }
                max_repeat -= 1;
            }

            self.setdot(x - 1, y - 1);
            self.setdot(x, y - 1);
            self.setdot(x + 1, y - 1);
            self.setdot(x - 1, y);
            self.setdot(x, y);
            self.setdot(x + 1, y);
            self.setdot(x - 1, y + 1);
            self.setdot(x, y + 1);
            self.setdot(x + 1, y + 1);
            let c = self.colors[self.colorindex];
            put_pixel(&mut self.pixels, self.width as u32, self.height as u32, x, y, c);
        }

        /* like C's `random() % (width-2)`: one RNG word per coordinate pair,
        cheap modulo instead of rejection sampling (half a million walkers) */
        let (mw, mh) = ((self.width - 2) as u32, (self.height - 2) as u32);
        for w in self.walkers.iter_mut() {
            let r: u64 = rng.random();
            w.0 = (r as u32 % mw) as i32 + 1;
            w.1 = ((r >> 32) as u32 % mh) as i32 + 1;
        }
    }

    /* port of coral(): one pass over all walkers; returns true when done */
    fn step(&mut self) -> bool {
        let mut rng = rand::rng();
        let bw = self.width as u32;
        let bh = self.height as u32;

        /* C's rand_2(): peel 2-bit direction samples off one RNG word
        instead of paying a full gen_range per walker per frame */
        let mut bits: u64 = 0;
        let mut nbits = 0u32;

        let mut i = 0;
        while i < self.nwalkers {
            let (x, y) = self.walkers[i];

            if self.getdot(x, y) {
                // the walker sticks: paint a scale x scale dot
                let c = self.colors[self.colorindex];
                for dy in 0..self.scale {
                    for dx in 0..self.scale {
                        put_pixel(&mut self.pixels, bw, bh, x + dx, y + dy, c);
                    }
                }

                /* mark the surrounding area as "sticky" */
                self.setdot(x - 1, y - 1);
                self.setdot(x, y - 1);
                self.setdot(x + 1, y - 1);
                self.setdot(x - 1, y);
                self.setdot(x + 1, y);
                self.setdot(x - 1, y + 1);
                self.setdot(x, y + 1);
                self.setdot(x + 1, y + 1);

                self.nwalkers -= 1;
                self.walkers[i] = self.walkers[self.nwalkers];
                let color = if self.colorsloth != 0 {
                    self.nwalkers.is_multiple_of(self.colorsloth)
                } else {
                    true
                };
                if color {
                    self.colorindex += 1;
                    if self.colorindex == self.ncolors {
                        self.colorindex = 0;
                    }
                }
            } else {
                /* move it a notch (C's rand_2 is just a batched random()%4) */
                if nbits == 0 {
                    bits = rng.random();
                    nbits = 32;
                }
                let dir = (bits & 3) as u32;
                bits >>= 2;
                nbits -= 1;
                match dir {
                    0 => {
                        if x > self.scale {
                            self.walkers[i].0 -= self.scale;
                        }
                    }
                    1 => {
                        if x < self.width - 2 * self.scale {
                            self.walkers[i].0 += self.scale;
                        }
                    }
                    2 => {
                        if y > self.scale {
                            self.walkers[i].1 -= self.scale;
                        }
                    }
                    _ => {
                        if y < self.height - 2 * self.scale {
                            self.walkers[i].1 += self.scale;
                        }
                    }
                }
            }
            i += 1;
        }

        self.nwalkers == 0
    }
}

impl Animation for Coral {
    fn new(config: &AnimConfig) -> Self {
        let width = config.width as i32;
        let height = config.height as i32;
        let mut coral = Coral {
            width,
            height,
            widthb: (width + 31) >> 5,
            scale: 1,
            board: Vec::new(),
            walkers: Vec::new(),
            nwalkers: 0,
            ncolors: 0,
            colorindex: 0,
            colorsloth: 0,
            colors: Vec::new(),
            pixels: vec![0u8; (config.width * config.height * 4) as usize],
            done: false,
            needs_init: true,
            next_delay_us: DELAY2_US,
        };
        clear_buffer(&mut coral.pixels, Color::new(255, 0, 0, 0));
        coral
    }

    fn tick(&mut self) {
        if self.done {
            // ponytail: C's erase_window() plays a random wipe; we just clear
            self.done = false;
            self.needs_init = true;
            clear_buffer(&mut self.pixels, Color::new(255, 0, 0, 0));
            self.next_delay_us = DELAY2_US;
            return;
        }

        if self.needs_init {
            self.init();
            self.needs_init = false;
        }

        if self.step() {
            self.done = true;
            self.next_delay_us = DELAY_SEC * 1_000_000; // linger on the finished coral
        } else {
            self.next_delay_us = DELAY2_US;
        }
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        if buffer.len() == self.pixels.len() {
            buffer.copy_from_slice(&self.pixels);
        } else {
            clear_buffer(buffer, Color::new(255, 0, 0, 0));
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        *self = Self::new(config);
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::CompleteFrame
    }

    fn frame_delay_us(&self) -> u64 {
        self.next_delay_us
    }
}
