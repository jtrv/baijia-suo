// xscreensaver, Copyright (c) 2005-2014 Jamie Zawinski <jwz@jwz.org>
//
// Permission to use, copy, modify, distribute, and sell this software and its
// documentation for any purpose is hereby granted without fee, provided that
// the above copyright notice appear in all copies and that both that
// copyright notice and this permission notice appear in supporting
// documentation.  No representations are made about the suitability of this
// software for any purpose.  It is provided "as is" without express or
// implied warranty.
//
// Boxfit -- fills space with a gradient of growing boxes or circles.
//
// Written by jwz, 21-Feb-2005.
//
// Inspired by http://www.levitated.net/daily/levBoxFitting.html
//
// Rust port of xscreensaver's boxfit.c. The image-grabbing mode (*grab) is
// not ported; boxes are always colored from a smooth random colormap, which
// is the hack's default behavior (*grab: False). The border color is taken
// from the palette entry half the colormap away from the fill color (the
// hack's PseudoColor intent), rather than reproducing the C's pixel-as-index
// arithmetic on TrueColor visuals.

use crate::animation::primitives::{
    clear_buffer, draw_circle, draw_line, hsv_to_rgb, put_pixel, rgb16, Color,
};
use crate::animation::{AnimConfig, Animation, RenderPolicy};
use rand::Rng;

const ALIVE: u8 = 1;
const CHANGED: u8 = 2;
const UNDEAD: u8 = 4;

const BG: Color = Color { a: 255, r: 0, g: 0, b: 0 };

#[derive(Clone, Copy)]
struct BoxRec {
    fill: usize, // palette index
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    flags: u8,
}

// port of utils/colors.c make_color_ramp() (closed_p = true)
fn make_color_ramp(h1: i32, s1: f64, v1: f64, h2: i32, s2: f64, v2: f64, total: usize) -> Vec<Color> {
    let mut colors = vec![BG; total];
    let ncolors = total / 2 + 1;
    let dh = (h2 - h1) as f64 / ncolors as f64;
    let ds = (s2 - s1) / ncolors as f64;
    let dv = (v2 - v1) / ncolors as f64;
    for i in 0..ncolors.min(total) {
        let (r, g, b) = hsv_to_rgb(
            (h1 as f64 + i as f64 * dh) as i32,
            s1 + i as f64 * ds,
            v1 + i as f64 * dv,
        );
        colors[i] = rgb16(r, g, b);
    }
    for i in ncolors..total {
        colors[i] = colors[total - i];
    }
    colors
}

// port of utils/colors.c make_color_path()
fn make_color_path(npoints: usize, h: &[i32], s: &[f64], v: &[f64], total: usize) -> Vec<Color> {
    if npoints == 2 {
        return make_color_ramp(h[0], s[0], v[0], h[1], s[1], v[1], total);
    }

    let mut dh = [0.0f64; 5]; // hue step per edge, in degrees
    let mut ds = [0.0f64; 5];
    let mut dv = [0.0f64; 5];
    let mut dist_h = [0.0f64; 5]; // shortest distance around the hue circle, 0-0.5
    let mut edge = [0.0f64; 5];
    let mut ncolors = [0usize; 5];

    for i in 0..npoints {
        let j = (i + 1) % npoints;
        let mut d = (h[i] - h[j]) as f64 / 360.0;
        if d < 0.0 {
            d = -d;
        }
        if d > 0.5 {
            d = 0.5 - (d - 0.5);
        }
        dist_h[i] = d;
    }

    let mut circum = 0.0;
    for i in 0..npoints {
        let j = (i + 1) % npoints;
        edge[i] = (dist_h[i] * dist_h[j] + (s[j] - s[i]).powi(2) + (v[j] - v[i]).powi(2)).sqrt();
        circum += edge[i];
    }
    if circum < 0.0001 {
        // degenerate path; can't happen with the repick constraints
        return vec![Color::new(255, 127, 127, 127); total];
    }

    // space the colors evenly along the circumference
    for i in 0..npoints {
        ncolors[i] = (total as f64 * (edge[i] / circum)) as usize;
    }

    for i in 0..npoints {
        let j = (i + 1) % npoints;
        if ncolors[i] > 0 {
            dh[i] = 360.0 * (dist_h[i] / ncolors[i] as f64);
            ds[i] = (s[j] - s[i]) / ncolors[i] as f64;
            dv[i] = (v[j] - v[i]) / ncolors[i] as f64;
        }
    }

    let mut colors = Vec::with_capacity(total);
    for i in 0..npoints {
        let distance = h[(i + 1) % npoints] - h[i];
        let mut direction = if distance >= 0 { -1.0 } else { 1.0 };
        if (-180..=180).contains(&distance) {
            direction = -direction;
        }
        for j in 0..ncolors[i] {
            if colors.len() >= total {
                break;
            }
            let mut hh = h[i] as f64 + j as f64 * dh[i] * direction;
            if hh < 0.0 {
                hh += 360.0;
            }
            let (r, g, b) = hsv_to_rgb(hh as i32, s[i] + j as f64 * ds[i], v[i] + j as f64 * dv[i]);
            colors.push(rgb16(r, g, b));
        }
    }

    // floating-point round-off can leave us short; pad with the last color
    if colors.is_empty() {
        colors.push(Color::new(255, 127, 127, 127));
    }
    if let Some(&fill) = colors.last() {
        while colors.len() < total {
            colors.push(fill);
        }
    }
    colors
}

// port of utils/colors.c make_smooth_colormap()
fn make_smooth_colormap(rng: &mut impl Rng, ncolors: usize) -> Vec<Color> {
    let npoints = match rng.random_range(0..20) {
        0..=5 => 2,  // 30% of the time
        6..=15 => 3, // 50% of the time
        16..=18 => 4, // 15% of the time
        _ => 5,      //  5% of the time
    };

    let mut h = [0i32; 5];
    let mut s = [0.0f64; 5];
    let mut v = [0.0f64; 5];
    // sic: the C never resets these accumulators between full repicks
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

            // make sure no two adjacent colors are *too* close together
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
                let distance = (dh * dh + (s[j] - s[i]).powi(2) + (v[j] - v[i]).powi(2)).sqrt();
                if distance < 0.2 {
                    continue; // repick this color
                }
            }
            total_s += s[i];
            total_v += v[i];
            i += 1;
        }

        // if the average saturation or intensity are too low, repick the colors
        if total_s / (npoints as f64) < 0.2 {
            continue 'repick_all;
        }
        if total_v / (npoints as f64) < 0.3 {
            continue 'repick_all;
        }
        break;
    }

    make_color_path(npoints, &h, &s, &v, ncolors.max(1))
}

fn fill_rect(buf: &mut [u8], width: u32, height: u32, x: i32, y: i32, w: i32, h: i32, color: Color) {
    for py in y..y + h {
        for px in x..x + w {
            put_pixel(buf, width, height, px, py, color);
        }
    }
}

// XDrawRectangle outlines the (w+1) x (h+1) pixel boundary
fn draw_rect(buf: &mut [u8], width: u32, height: u32, x: i32, y: i32, w: i32, h: i32, color: Color) {
    draw_line(buf, width, height, x, y, x + w, y, color);
    draw_line(buf, width, height, x + w, y, x + w, y + h, color);
    draw_line(buf, width, height, x + w, y + h, x, y + h, color);
    draw_line(buf, width, height, x, y + h, x, y, color);
}

// XFillArc equivalent for a full circle in a d x d bounding box
fn fill_circle(buf: &mut [u8], width: u32, height: u32, x: i32, y: i32, d: i32, color: Color) {
    if d <= 0 {
        return;
    }
    let r = d as f32 / 2.0;
    let cx = x as f32 + r;
    let cy = y as f32 + r;
    for py in y..y + d {
        for px in x..x + d {
            let dx = px as f32 + 0.5 - cx;
            let dy = py as f32 + 0.5 - cy;
            if dx * dx + dy * dy <= r * r {
                put_pixel(buf, width, height, px, py, color);
            }
        }
    }
}

pub struct BoxFit {
    canvas: Vec<u8>,
    width: i32,
    height: i32,

    border_size: i32,
    spacing: i32,
    inc: i32,

    circles_p: bool,
    growing_p: bool,
    color_horiz_p: bool,

    box_count: usize,
    boxes: Vec<BoxRec>,

    ncolors: usize,
    requested_colors: usize,
    colors: Vec<Color>,

    delay: u64,
    this_delay: u64,
}

impl BoxFit {
    fn reset_boxes(&mut self, rng: &mut impl Rng) {
        self.boxes.clear();
        self.growing_p = true;
        self.color_horiz_p = rng.random::<bool>();
        // *mode: random
        self.circles_p = rng.random::<bool>();

        self.ncolors = self.requested_colors.max(1);
        self.colors = make_smooth_colormap(rng, self.ncolors);
        self.ncolors = self.colors.len();

        clear_buffer(&mut self.canvas, BG);
    }

    fn boxes_overlap_p(a: &BoxRec, b: &BoxRec, pad: i32) -> bool {
        // Two rectangles overlap if the max of the tops is less than the
        // min of the bottoms and the max of the lefts is less than the min
        // of the rights.
        let maxleft = (a.x - pad).max(b.x);
        let maxtop = (a.y - pad).max(b.y);
        let minright = (a.x + a.w + pad + pad - 1).min(b.x + b.w);
        let minbot = (a.y + a.h + pad + pad - 1).min(b.y + b.h);
        maxtop < minbot && maxleft < minright
    }

    fn circles_overlap_p(a: &BoxRec, b: &BoxRec, pad: i32) -> bool {
        let ar = a.w / 2; // radius
        let br = b.w / 2;
        let ax = a.x + ar; // center
        let ay = a.y + ar;
        let bx = b.x + br;
        let by = b.y + br;
        let d2 = (bx - ax) * (bx - ax) + (by - ay) * (by - ay);
        let r2 = (ar + br + pad) * (ar + br + pad);
        d2 < r2
    }

    fn box_collides_p(&self, idx: usize, pad: i32) -> bool {
        let a = &self.boxes[idx];

        // collide with wall
        if a.x - pad < 0
            || a.y - pad < 0
            || a.x + a.w + pad + pad >= self.width
            || a.y + a.h + pad + pad >= self.height
        {
            return true;
        }

        // collide with another box
        for (i, b) in self.boxes.iter().enumerate() {
            if i != idx
                && (if self.circles_p {
                    Self::circles_overlap_p(a, b, pad)
                } else {
                    Self::boxes_overlap_p(a, b, pad)
                })
            {
                return true;
            }
        }

        false
    }

    fn grow_boxes(&mut self, rng: &mut impl Rng) -> u64 {
        let inc2 = self.inc + self.spacing + self.border_size;
        let mut live_count = 0;

        // check box collisions, and grow if none
        for i in 0..self.boxes.len() {
            if self.boxes[i].flags & ALIVE == 0 {
                continue;
            }
            if self.box_collides_p(i, inc2) {
                self.boxes[i].flags &= !ALIVE;
                continue;
            }
            live_count += 1;
            let a = &mut self.boxes[i];
            a.x -= self.inc;
            a.y -= self.inc;
            a.w += self.inc + self.inc;
            a.h += self.inc + self.inc;
            a.flags |= CHANGED;
        }

        // add more boxes
        while live_count < self.box_count {
            self.boxes.push(BoxRec { fill: 0, x: 0, y: 0, w: 0, h: 0, flags: CHANGED });
            let idx = self.boxes.len() - 1;

            for _ in 0..100 {
                self.boxes[idx].x = inc2 + rng.random_range(0..(self.width - inc2).max(1));
                self.boxes[idx].y = inc2 + rng.random_range(0..(self.height - inc2).max(1));
                self.boxes[idx].w = 0;
                self.boxes[idx].h = 0;

                if !self.box_collides_p(idx, inc2) {
                    self.boxes[idx].flags |= ALIVE;
                    live_count += 1;
                    break;
                }
            }

            if self.boxes[idx].flags & ALIVE == 0 || // too many retries;
                self.boxes.len() > 65535
            // that's about 1MB of box structs
            {
                self.boxes.pop(); // go into "fade out" mode now
                self.growing_p = false;
                return 2_000_000;
            }

            // pick a color for this box from the gradient
            let n = if self.color_horiz_p {
                self.boxes[idx].x as usize * self.ncolors / self.width.max(1) as usize
            } else {
                self.boxes[idx].y as usize * self.ncolors / self.height.max(1) as usize
            };
            self.boxes[idx].fill = n % self.ncolors;
        }

        self.delay
    }

    fn shrink_boxes(&mut self, rng: &mut impl Rng) -> u64 {
        let mut remaining = 0;

        for a in self.boxes.iter_mut() {
            if a.w <= 0 || a.h <= 0 {
                continue;
            }
            a.x += self.inc;
            a.y += self.inc;
            a.w -= self.inc + self.inc;
            a.h -= self.inc + self.inc;
            a.flags |= CHANGED;
            if a.w < 0 {
                a.w = 0;
            }
            if a.h < 0 {
                a.h = 0;
            }
            if a.w > 0 && a.h > 0 {
                remaining += 1;
            }
        }

        if remaining == 0 {
            self.reset_boxes(rng);
            1_000_000
        } else {
            self.delay
        }
    }

    fn draw_boxes(&mut self) {
        let (w, h) = (self.width as u32, self.height as u32);
        for i in 0..self.boxes.len() {
            let b = self.boxes[i];
            if b.flags & UNDEAD != 0 {
                continue;
            }
            if b.flags & CHANGED == 0 {
                continue;
            }
            self.boxes[i].flags &= !CHANGED;

            if !self.growing_p {
                // When shrinking, black out an area outside of the border
                // before re-drawing the box.
                let margin = self.inc + self.border_size;

                if self.circles_p {
                    fill_circle(&mut self.canvas, w, h, b.x - margin, b.y - margin, b.w + margin * 2, BG);
                } else {
                    fill_rect(
                        &mut self.canvas,
                        w,
                        h,
                        b.x - margin,
                        b.y - margin,
                        b.w + margin * 2,
                        b.h + margin * 2,
                        BG,
                    );
                }

                if b.w <= 0 || b.h <= 0 {
                    self.boxes[i].flags |= UNDEAD; // really very dead now
                }
            }

            if b.w <= 0 || b.h <= 0 {
                continue;
            }

            let fill = self.colors[b.fill % self.ncolors];
            if self.circles_p {
                fill_circle(&mut self.canvas, w, h, b.x, b.y, b.w, fill);
            } else {
                fill_rect(&mut self.canvas, w, h, b.x, b.y, b.w, b.h, fill);
            }

            if self.border_size > 0 {
                let bd = self.colors[(b.fill + self.ncolors / 2) % self.ncolors];
                if self.circles_p {
                    draw_circle(&mut self.canvas, w, h, b.x + b.w / 2, b.y + b.w / 2, b.w / 2, bd);
                } else {
                    draw_rect(&mut self.canvas, w, h, b.x, b.y, b.w, b.h, bd);
                }
            }
        }
    }
}

impl Animation for BoxFit {
    fn new(config: &AnimConfig) -> Self {
        let mut b = BoxFit {
            canvas: Vec::new(),
            width: config.width as i32,
            height: config.height as i32,
            border_size: 1, // *borderSize: 1
            spacing: 1,     // *spacing: 1
            inc: 1,         // *growBy: 1
            circles_p: false,
            growing_p: true,
            color_horiz_p: false,
            box_count: 50, // *boxCount: 50
            boxes: Vec::new(),
            ncolors: 64, // *colors: 64
            requested_colors: 64,
            colors: Vec::new(),
            delay: 20_000, // *delay: 20000
            this_delay: 20_000,
        };
        b.reset(config);
        b
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();
        if self.growing_p {
            self.draw_boxes();
            self.this_delay = self.grow_boxes(&mut rng);
        } else {
            self.this_delay = self.shrink_boxes(&mut rng);
            self.draw_boxes();
        }
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        let len = self.canvas.len().min(buffer.len());
        buffer[..len].copy_from_slice(&self.canvas[..len]);
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width as i32;
        self.height = config.height as i32;
        self.canvas = vec![0u8; (config.width * config.height * 4) as usize];

        self.box_count = if config.count > 0 { config.count as usize } else { 50 };
        self.requested_colors = config.ncolors.max(1) as usize;
        self.inc = 1;
        self.spacing = 1;
        self.border_size = 1;
        if config.width > 2560 || config.height > 2560 {
            // Retina displays
            self.border_size *= 3;
            self.spacing *= 3;
        }

        self.delay = 20_000;
        self.this_delay = self.delay;

        self.reset_boxes(&mut rng);
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::CompleteFrame
    }

    fn frame_delay_us(&self) -> u64 {
        self.this_delay
    }
}
