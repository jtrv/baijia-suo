/* cynosure --- draw some rectangles
 *
 * 01-aug-96: written in Java by ozymandias G desiderata <ogd@organic.com>
 * 25-dec-97: ported to C and XScreenSaver by Jamie Zawinski <jwz@jwz.org>
 *
 * Original version:
 *   http://www.organic.com/staff/ogd/java/cynosure.html
 *   http://www.organic.com/staff/ogd/java/source/cynosure/Cynosure-java.txt
 *
 * Original comments and copyright:
 *
 *   Cynosure.java
 *   A Java implementation of Stephen Linhart's Cynosure screen-saver as a
 *   drop-in class.
 *
 *   ozymandias G desiderata <ogd@organic.com>
 *   Thu Aug  1 1996
 *
 *   COPYRIGHT NOTICE
 *
 *   Copyright 1996 ozymandias G desiderata. Title, ownership rights, and
 *   intellectual property rights in and to this software remain with
 *   ozymandias G desiderata. This software may be copied, modified,
 *   or used as long as this copyright is retained. Use this code at your
 *   own risk.
 *
 * Rust port of xscreensaver's cynosure.c (with the needed parts of
 * utils/hsv.c and utils/colors.c ported inline).
 */

use rand::Rng;

use crate::animation::primitives::{clear_buffer, hsv_to_rgb, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

/// The smallest size for an individual cell.
const MINCELLSIZE: i32 = 16;

/// The narrowest a rectangle can be.
const MINRECTSIZE: i32 = 6;

/// How frequently genNewColor() generates a completely random color (1%).
const THRESHOLD: i32 = 100;

/* ---- utils/hsv.c (hsv_to_rgb now in animation::primitives) ---- */

fn rgb_to_hsv(r: u16, g: u16, b: u16) -> (i32, f64, f64) {
    let rf = r as f64 / 65535.0;
    let gf = g as f64 / 65535.0;
    let bf = b as f64 / 65535.0;
    let mut cmax = rf;
    let mut cmin = gf;
    let mut imax = 1;
    if cmax < gf {
        cmax = gf;
        cmin = rf;
        imax = 2;
    }
    if cmax < bf {
        cmax = bf;
        imax = 3;
    }
    if cmin > bf {
        cmin = bf;
    }
    let cmm = cmax - cmin;
    let v = cmax;
    let (h, s);
    if cmm == 0.0 {
        s = 0.0;
        h = 0.0;
    } else {
        s = cmm / cmax;
        let mut hh = match imax {
            1 => (gf - bf) / cmm,
            2 => 2.0 + (bf - rf) / cmm,
            _ => 4.0 + (rf - gf) / cmm,
        };
        if hh < 0.0 {
            hh += 6.0;
        }
        h = hh;
    }
    ((h * 60.0) as i32, s, v)
}

/* ---- utils/colors.c (make_smooth_colormap and helpers, sans X allocation) ---- */

fn make_color_ramp(
    h1: i32,
    s1: f64,
    v1: f64,
    h2: i32,
    s2: f64,
    v2: f64,
    total: usize,
    closed: bool,
) -> Vec<(u16, u16, u16)> {
    let mut colors = vec![(0u16, 0u16, 0u16); total];
    let ncolors = if closed { total / 2 + 1 } else { total };
    if ncolors == 0 {
        return colors;
    }
    let dh = (h2 as f64 - h1 as f64) / ncolors as f64;
    let ds = (s2 - s1) / ncolors as f64;
    let dv = (v2 - v1) / ncolors as f64;
    for (i, c) in colors.iter_mut().enumerate().take(ncolors.min(total)) {
        *c = hsv_to_rgb(
            (h1 as f64 + i as f64 * dh) as i32,
            s1 + i as f64 * ds,
            v1 + i as f64 * dv,
        );
    }
    if closed {
        for i in ncolors..total {
            colors[i] = colors[total - i];
        }
    }
    colors
}

fn make_color_path(h: &[i32], s: &[f64], v: &[f64], total: usize) -> Vec<(u16, u16, u16)> {
    let npoints = h.len();
    if npoints == 0 || total == 0 {
        return Vec::new();
    }
    if npoints == 2 {
        return make_color_ramp(h[0], s[0], v[0], h[1], s[1], v[1], total, true);
    }

    // Distance between hue values in the shortest direction around the circle.
    let mut dh_short = vec![0.0f64; npoints];
    for i in 0..npoints {
        let j = (i + 1) % npoints;
        let mut d = ((h[i] - h[j]) as f64) / 360.0;
        if d < 0.0 {
            d = -d;
        }
        if d > 0.5 {
            d = 0.5 - (d - 0.5);
        }
        dh_short[i] = d;
    }

    let mut edge = vec![0.0f64; npoints];
    let mut circum = 0.0;
    for i in 0..npoints {
        let j = (i + 1) % npoints;
        // C computes DH[i] * DH[j] here (not DH[i] squared); kept as-is.
        edge[i] = ((dh_short[i] * dh_short[j])
            + ((s[j] - s[i]) * (s[j] - s[i]))
            + ((v[j] - v[i]) * (v[j] - v[i])))
            .sqrt();
        circum += edge[i];
    }
    if circum < 0.0001 {
        return vec![(0, 0, 0); total];
    }

    let mut ncolors_per = vec![0usize; npoints];
    let mut dh = vec![0.0f64; npoints];
    let mut ds = vec![0.0f64; npoints];
    let mut dv = vec![0.0f64; npoints];
    for i in 0..npoints {
        ncolors_per[i] = (total as f64 * (edge[i] / circum)) as usize;
    }
    for i in 0..npoints {
        let j = (i + 1) % npoints;
        if ncolors_per[i] > 0 {
            dh[i] = 360.0 * (dh_short[i] / ncolors_per[i] as f64);
            ds[i] = (s[j] - s[i]) / ncolors_per[i] as f64;
            dv[i] = (v[j] - v[i]) / ncolors_per[i] as f64;
        }
    }

    let mut colors: Vec<(u16, u16, u16)> = Vec::with_capacity(total);
    for i in 0..npoints {
        let distance = h[(i + 1) % npoints] - h[i];
        let mut direction = if distance >= 0 { -1.0 } else { 1.0 };
        if (-180..=180).contains(&distance) {
            direction = -direction;
        }
        for j in 0..ncolors_per[i] {
            if colors.len() >= total {
                break;
            }
            let mut hh = h[i] as f64 + (j as f64 * dh[i] * direction);
            if hh < 0.0 {
                hh += 360.0;
            }
            colors.push(hsv_to_rgb(
                hh as i32,
                s[i] + j as f64 * ds[i],
                v[i] + j as f64 * dv[i],
            ));
        }
    }
    // Floating-point round-off can leave us short; pad with the last color.
    if colors.is_empty() {
        colors.push((0, 0, 0));
    }
    if let Some(&fill) = colors.last() {
        while colors.len() < total {
            colors.push(fill);
        }
    }
    colors
}

fn make_smooth_colormap(ncolors: usize, rng: &mut impl Rng) -> Vec<(u16, u16, u16)> {
    let npoints = match rng.random_range(0..20_u32) {
        0..=5 => 2,   /* 30% of the time */
        6..=15 => 3,  /* 50% of the time */
        16..=18 => 4, /* 15% of the time */
        _ => 5,       /*  5% of the time */
    };

    let mut h = vec![0i32; npoints];
    let mut s = vec![0.0f64; npoints];
    let mut v = vec![0.0f64; npoints];
    // Note: like the C, the running totals accumulate across full repicks.
    let mut total_s = 0.0;
    let mut total_v = 0.0;
    let mut guard = 0;
    'repick_all: loop {
        let mut i = 0;
        while i < npoints {
            guard += 1;
            if guard > 10000 {
                break 'repick_all;
            }
            h[i] = rng.random_range(0..360);
            s[i] = rng.random::<f64>();
            v[i] = rng.random::<f64>() * 0.8 + 0.2;

            // Make sure that no two adjacent colors are *too* close together.
            if i > 0 {
                let j = if i + 1 == npoints { 0 } else { i - 1 };
                let hi = h[i] as f64 / 360.0;
                let hj = h[j] as f64 / 360.0;
                let mut dh = hj - hi;
                if dh < 0.0 {
                    dh = -dh;
                }
                if dh > 0.5 {
                    dh = 0.5 - (dh - 0.5);
                }
                let distance = ((dh * dh)
                    + ((s[j] - s[i]) * (s[j] - s[i]))
                    + ((v[j] - v[i]) * (v[j] - v[i])))
                    .sqrt();
                if distance < 0.2 {
                    continue; // repick this color
                }
            }
            total_s += s[i];
            total_v += v[i];
            i += 1;
        }
        // Repick if the average saturation or intensity is too low.
        if (total_s / npoints as f64) < 0.2 {
            continue 'repick_all;
        }
        if (total_v / npoints as f64) < 0.3 {
            continue 'repick_all;
        }
        break;
    }
    make_color_path(&h, &s, &v, ncolors)
}

/* ---- drawing helpers (XFillRectangle / XDrawRectangle) ---- */

fn fill_rect(buffer: &mut [u8], width: u32, height: u32, x: i32, y: i32, w: i32, h: i32, color: Color) {
    for yy in y..y + h {
        for xx in x..x + w {
            put_pixel(buffer, width, height, xx, yy, color);
        }
    }
}

fn draw_rect(buffer: &mut [u8], width: u32, height: u32, x: i32, y: i32, w: i32, h: i32, color: Color) {
    for xx in x..=x + w {
        put_pixel(buffer, width, height, xx, y, color);
        put_pixel(buffer, width, height, xx, y + h, color);
    }
    for yy in y..=y + h {
        put_pixel(buffer, width, height, x, yy, color);
        put_pixel(buffer, width, height, x + w, yy, color);
    }
}

pub struct Cynosure {
    colors: Vec<Color>,
    colors2: Vec<Color>, // dropshadow colors (same hue, value * 0.4)
    ncolors: usize,

    cur_color: i32,
    cur_base: i32, // color progression
    shadow_width: i32,
    elevation: i32, // offset of dropshadow
    sway: i32,      // time until base color changed
    time_left: i32, // until base color used
    tweak: i32,     // amount of color variance
    grid_size: i32,
    iterations: i32,
    i: i32,
    delay_us: u64,

    width: u32,
    height: u32,
    buf: Vec<u8>,
}

impl Cynosure {
    /// Utility function to generate a tweaked color value.
    fn c_tweak(&self, rng: &mut impl Rng, base: i32, tweak: i32) -> i32 {
        let ran_tweak = rng.random_range(0..(2 * tweak).max(1));
        let n = (base + (ran_tweak - tweak)).abs();
        n.min(255)
    }

    /// Creates a random new color within a certain range of an existing color.
    fn gen_constrained_color(&self, rng: &mut impl Rng, base: i32) -> i32 {
        let mut i = 1 + rng.random_range(0..self.tweak.max(1));
        if rng.random::<bool>() {
            i = -i;
        }
        (base + i).rem_euclid(self.ncolors as i32)
    }

    /// Returns a new color, gradually mutating the colors and occasionally
    /// returning a totally random color, just for variety.
    fn gen_new_color(&mut self, rng: &mut impl Rng) -> usize {
        if self.time_left == 0 {
            self.time_left = self.c_tweak(rng, self.sway, self.sway / 3);
            self.cur_color = self.cur_base;
        } else {
            self.time_left -= 1;
        }

        if rng.random_range(0..THRESHOLD) == 0 {
            rng.random_range(0..self.ncolors)
        } else {
            self.cur_base = self.gen_constrained_color(rng, self.cur_color);
            self.cur_base as usize
        }
    }

    /// paint adds a new layer of multicolored rectangles within a grid of
    /// randomly generated size. Each row of rectangles is the same color,
    /// but colors vary slightly from row to row.
    fn paint(&mut self, rng: &mut impl Rng) {
        let width = self.width as i32;
        let height = self.height as i32;

        // Grid dims equal to gridSize +/- (gridSize / 2).
        let mut cells_wide = self.c_tweak(rng, self.grid_size, self.grid_size / 2);
        let mut cells_high = self.c_tweak(rng, self.grid_size, self.grid_size / 2);
        let mut cell_width = width / cells_wide.max(1);
        let mut cell_height = height / cells_high.max(1);

        // Ensure that each cell is above a certain minimum size.
        if cell_width < MINCELLSIZE {
            cell_width = MINCELLSIZE;
            cells_wide = width / cell_width;
        }
        if cell_height < MINCELLSIZE {
            cell_height = MINCELLSIZE;
            // C bug preserved: recomputes the row count from the width.
            cells_high = width / cell_width;
        }

        let black = Color::new(255, 0, 0, 0);

        // Fill the grid with randomly-generated cells.
        for i in 0..cells_high {
            // Each row is a different color, randomly generated (but constrained).
            let c = self.gen_new_color(rng);
            let fg = self.colors[c];
            let shadow = self.colors2[c];

            for j in 0..cells_wide {
                let mut cur_height = rng.random_range(0..(cell_height - self.shadow_width).max(1));
                if cur_height < MINRECTSIZE {
                    cur_height = MINRECTSIZE;
                }
                let mut cur_width = rng.random_range(0..(cell_width - self.shadow_width).max(1));
                if cur_width < MINRECTSIZE {
                    cur_width = MINRECTSIZE;
                }
                let cur_y = i * cell_height
                    + rng.random_range(0..((cell_height - cur_height) - self.shadow_width).max(1));
                let cur_x = j * cell_width
                    + rng.random_range(0..((cell_width - cur_width) - self.shadow_width).max(1));

                // Draw the shadow.
                if self.elevation > 0 {
                    fill_rect(
                        &mut self.buf,
                        self.width,
                        self.height,
                        cur_x + self.elevation,
                        cur_y + self.elevation,
                        cur_width,
                        cur_height,
                        shadow,
                    );
                }
                // Draw the edge.
                if self.shadow_width > 0 {
                    fill_rect(
                        &mut self.buf,
                        self.width,
                        self.height,
                        cur_x + self.shadow_width,
                        cur_y + self.shadow_width,
                        cur_width,
                        cur_height,
                        black,
                    );
                }
                fill_rect(&mut self.buf, self.width, self.height, cur_x, cur_y, cur_width, cur_height, fg);
                // Draw a 1-pixel black border around the rectangle.
                draw_rect(&mut self.buf, self.width, self.height, cur_x, cur_y, cur_width, cur_height, black);
            }
        }
    }
}

impl Animation for Cynosure {
    fn new(config: &AnimConfig) -> Self {
        let mut c = Cynosure {
            colors: Vec::new(),
            colors2: Vec::new(),
            ncolors: 2,
            cur_color: 0,
            cur_base: 0,
            shadow_width: 2,
            elevation: 5,
            sway: 30,
            time_left: 0,
            tweak: 20,
            grid_size: 12,
            iterations: 100,
            i: 0,
            delay_us: 500_000,
            width: config.width,
            height: config.height,
            buf: Vec::new(),
        };
        c.reset(config);
        c
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();
        if self.iterations > 0 {
            self.i += 1;
            if self.i >= self.iterations {
                self.i = 0;
                let bg = self.colors[rng.random_range(0..self.ncolors)];
                clear_buffer(&mut self.buf, bg);
            }
        }
        self.paint(&mut rng);
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let w = (self.width.min(width) as usize) * 4;
        let h = self.height.min(height) as usize;
        for y in 0..h {
            let s = y * self.width as usize * 4;
            let d = y * width as usize * 4;
            buffer[d..d + w].copy_from_slice(&self.buf[s..s + w]);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;
        self.cur_color = 0;
        self.cur_base = self.cur_color;
        self.shadow_width = 2;
        self.elevation = 5;
        self.sway = 30;
        self.tweak = 20;
        self.grid_size = 12;
        self.time_left = 0;
        self.i = 0;

        if config.width > 2560 || config.height > 2560 {
            // Retina displays
            self.shadow_width *= 2;
            self.elevation *= 2;
        }

        self.ncolors = config.ncolors.max(2) as usize;
        let raw = make_smooth_colormap(self.ncolors, &mut rng);
        self.ncolors = raw.len().max(2);
        self.colors = raw
            .iter()
            .map(|&(r, g, b)| Color::new(255, (r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8))
            .collect();
        // Shadow palette: same hue and saturation, value * 0.4.
        self.colors2 = raw
            .iter()
            .map(|&(r, g, b)| {
                let (h, s, v) = rgb_to_hsv(r, g, b);
                let (r2, g2, b2) = hsv_to_rgb(h, s, v * 0.4);
                Color::new(255, (r2 >> 8) as u8, (g2 >> 8) as u8, (b2 >> 8) as u8)
            })
            .collect();

        self.delay_us = 500_000;
        self.iterations = if config.cycles <= 0 { 100 } else { config.cycles };

        self.buf = vec![0u8; (config.width * config.height * 4) as usize];
        clear_buffer(&mut self.buf, Color::new(255, 0, 0, 0));
    }


    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
