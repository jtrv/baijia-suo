/* ccurve, Copyright (c) 1998, 1999
 *  Rick Campbell <rick@campbellcentral.org>
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Rust port of xscreensaver's ccurve.c (draws self-similar linear fractals
 * including the classic "C Curve").
 */

use rand::RngExt;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, SQRT_2};

use crate::animation::primitives::{clear_buffer, draw_line, Color};
use crate::animation::{AnimConfig, Animation};

const MAXIMUM_COLOR_COUNT: usize = 256;
const EPSILON: f64 = 1e-5;

// Hack defaults: .delay: 3 (seconds), .pause: 0.4, .limit: 200000
const DELAY: f64 = 3.0;
const PAUSE: f64 = 0.4;
const MAXIMUM_LINES: i32 = 200_000;

// The player arms its next wakeup from frame_delay_us() *before* ticking, so
// a delay that shrinks (DELAY -> PAUSE when a new fractal starts) makes it
// oversleep 3s on a near-blank frame and then burst all the catch-up
// refinements into one frame. Instead tick at the gcd of the two intervals
// and count ticks, so the reported delay never changes.
const TICK_US: u64 = 200_000;
const PAUSE_TICKS: u32 = ((1_000_000.0 * PAUSE) as u64 / TICK_US) as u32 - 1;
const DELAY_TICKS: u32 = ((1_000_000.0 * DELAY) as u64 / TICK_US) as u32 - 1;

const BLACK: Color = Color { a: 255, r: 0, g: 0, b: 0 };

#[derive(Clone, Copy)]
struct Segment {
    angle: f64,
    length: f64,
}

type Position = (f64, f64);

/// Exact port of hsv_to_rgb from utils/hsv.c (output scaled to 8 bits).
// 8-bit-direct rounding ((x*255.0) as u8) differs from primitives::hsv_to_rgb's
// 16-bit path (e.g. v=0.999 -> 254 here vs 255 there); kept for port fidelity.
fn hsv_to_rgb(h: i32, s: f64, v: f64) -> Color {
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let hh = h.rem_euclid(360) as f64 / 60.0;
    let i = hh as i32;
    let f = hh - i as f64;
    let p1 = v * (1.0 - s);
    let p2 = v * (1.0 - s * f);
    let p3 = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i {
        0 => (v, p3, p1),
        1 => (p2, v, p1),
        2 => (p1, v, p3),
        3 => (p1, p2, v),
        4 => (p3, p1, v),
        _ => (v, p1, p2),
    };
    Color::new(255, (r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn random_double(rng: &mut impl RngExt, base: f64, limit: f64, epsilon: f64) -> f64 {
    let steps = ((limit - base) / epsilon).floor() as u32;
    base + rng.random_range(0..steps.max(1)) as f64 * epsilon
}

/// normalize alters the sequence to go from (0,0) to (1,0)
fn normalized_plot(segments: &[Segment]) -> Vec<Position> {
    let mut points = Vec::with_capacity(segments.len());
    let mut x = 0.0;
    let mut y = 0.0;
    for segment in segments {
        x += segment.length * segment.angle.cos();
        y += segment.length * segment.angle.sin();
        points.push((x, y));
    }
    let angle = -y.atan2(x);
    let cosine = angle.cos();
    let sine = angle.sin();
    let length = (x * x + y * y).sqrt();
    // rotate and scale
    for p in points.iter_mut() {
        let (temp_x, temp_y) = *p;
        *p = (
            (temp_x * cosine + temp_y * (-sine)) / length,
            (temp_x * sine + temp_y * cosine) / length,
        );
    }
    points
}

fn realign(x1: f64, y1: f64, x2: f64, y2: f64, points: &mut [Position]) {
    let delta_x = x2 - x1;
    let delta_y = y2 - y1;
    let angle = delta_y.atan2(delta_x);
    let cosine = angle.cos();
    let sine = angle.sin();
    let length = (delta_x * delta_x + delta_y * delta_y).sqrt();
    // rotate, scale, then shift
    for p in points.iter_mut() {
        let (temp_x, temp_y) = *p;
        *p = (
            length * (temp_x * cosine + temp_y * (-sine)) + x1,
            length * (temp_x * sine + temp_y * cosine) + y1,
        );
    }
}

fn select_2_pattern(rng: &mut impl RngExt, segments: &mut [Segment]) {
    if rng.random_range(0..2u32) == 0 {
        if rng.random_range(0..2u32) == 0 {
            segments[0] = Segment { angle: -FRAC_PI_4, length: SQRT_2 };
            segments[1] = Segment { angle: FRAC_PI_4, length: SQRT_2 };
        } else {
            segments[0] = Segment { angle: FRAC_PI_4, length: SQRT_2 };
            segments[1] = Segment { angle: -FRAC_PI_4, length: SQRT_2 };
        }
    } else {
        segments[0].angle = random_double(rng, PI / 6.0, PI / 3.0, PI / 180.0);
        segments[0].length = random_double(rng, 0.25, 0.67, 0.001);
        if rng.random_range(0..2u32) == 0 {
            segments[1].angle = -segments[0].angle;
            segments[1].length = segments[0].length;
        } else {
            segments[1].angle = random_double(rng, -PI / 3.0, -PI / 6.0, PI / 180.0);
            segments[1].length = random_double(rng, 0.25, 0.67, 0.001);
        }
    }
}

fn select_3_pattern(rng: &mut impl RngExt, segments: &mut [Segment]) {
    match rng.random_range(0..5u32) {
        0 => {
            if rng.random_range(0..2u32) == 0 {
                segments[0] = Segment { angle: FRAC_PI_4, length: SQRT_2 / 4.0 };
                segments[1] = Segment { angle: -FRAC_PI_4, length: SQRT_2 / 2.0 };
                segments[2] = Segment { angle: FRAC_PI_4, length: SQRT_2 / 4.0 };
            } else {
                segments[0] = Segment { angle: -FRAC_PI_4, length: SQRT_2 / 4.0 };
                segments[1] = Segment { angle: FRAC_PI_4, length: SQRT_2 / 2.0 };
                segments[2] = Segment { angle: -FRAC_PI_4, length: SQRT_2 / 4.0 };
            }
        }
        1 => {
            if rng.random_range(0..2u32) == 0 {
                segments[0] = Segment { angle: PI / 6.0, length: 1.0 };
                segments[1] = Segment { angle: -FRAC_PI_2, length: 1.0 };
                segments[2] = Segment { angle: PI / 6.0, length: 1.0 };
            } else {
                segments[0] = Segment { angle: -PI / 6.0, length: 1.0 };
                segments[1] = Segment { angle: FRAC_PI_2, length: 1.0 };
                segments[2] = Segment { angle: -PI / 6.0, length: 1.0 };
            }
        }
        _ => {
            segments[0].angle = random_double(rng, PI / 6.0, PI / 3.0, PI / 180.0);
            segments[0].length = random_double(rng, 0.25, 0.67, 0.001);
            segments[1].angle = random_double(rng, -PI / 3.0, -PI / 6.0, PI / 180.0);
            segments[1].length = random_double(rng, 0.25, 0.67, 0.001);
            if rng.random_range(0..3u32) == 0 {
                if rng.random_range(0..2u32) == 0 {
                    segments[2].angle = segments[0].angle;
                } else {
                    segments[2].angle = -segments[0].angle;
                }
                segments[2].length = segments[0].length;
            } else {
                segments[2].angle = random_double(rng, -PI / 3.0, -PI / 6.0, PI / 180.0);
                segments[2].length = random_double(rng, 0.25, 0.67, 0.001);
            }
        }
    }
}

fn select_4_pattern(rng: &mut impl RngExt, segments: &mut [Segment]) {
    match rng.random_range(0..9u32) {
        0 => {
            let length = random_double(rng, 0.25, 0.50, 0.001);
            if rng.random_range(0..2u32) == 0 {
                segments[0] = Segment { angle: 0.0, length: 0.5 };
                segments[1] = Segment { angle: FRAC_PI_2, length };
                segments[2] = Segment { angle: -FRAC_PI_2, length };
                segments[3] = Segment { angle: 0.0, length: 0.5 };
            } else {
                segments[0] = Segment { angle: 0.0, length: 0.5 };
                segments[1] = Segment { angle: -FRAC_PI_2, length };
                segments[2] = Segment { angle: FRAC_PI_2, length };
                segments[3] = Segment { angle: 0.0, length: 0.5 };
            }
        }
        1 => {
            if rng.random_range(0..2u32) == 0 {
                segments[0] = Segment { angle: 0.0, length: 0.5 };
                segments[1] = Segment { angle: FRAC_PI_2, length: 0.45 };
                segments[2] = Segment { angle: -FRAC_PI_2, length: 0.45 };
                segments[3] = Segment { angle: 0.0, length: 0.5 };
            } else {
                segments[0] = Segment { angle: 0.0, length: 0.5 };
                segments[1] = Segment { angle: -FRAC_PI_2, length: 0.45 };
                segments[2] = Segment { angle: FRAC_PI_2, length: 0.45 };
                segments[3] = Segment { angle: 0.0, length: 0.5 };
            }
        }
        2 => {
            if rng.random_range(0..2u32) == 0 {
                segments[0] = Segment { angle: 0.0, length: 1.0 };
                segments[1] = Segment { angle: 5.0 * PI / 12.0, length: 1.2 };
                segments[2] = Segment { angle: -5.0 * PI / 12.0, length: 1.2 };
                segments[3] = Segment { angle: 0.0, length: 1.0 };
            } else {
                segments[0] = Segment { angle: 0.0, length: 1.0 };
                segments[1] = Segment { angle: -5.0 * PI / 12.0, length: 1.2 };
                segments[2] = Segment { angle: 5.0 * PI / 12.0, length: 1.2 };
                segments[3] = Segment { angle: 0.0, length: 1.0 };
            }
        }
        // cases 3 and 4 are identical in the original
        3 | 4 => {
            let angle = random_double(rng, PI / 4.0, FRAC_PI_2, PI / 180.0);
            if rng.random_range(0..2u32) == 0 {
                segments[0] = Segment { angle: 0.0, length: 1.0 };
                segments[1] = Segment { angle, length: 1.2 };
                segments[2] = Segment { angle: -angle, length: 1.2 };
                segments[3] = Segment { angle: 0.0, length: 1.0 };
            } else {
                segments[0] = Segment { angle: 0.0, length: 1.0 };
                segments[1] = Segment { angle: -angle, length: 1.2 };
                segments[2] = Segment { angle, length: 1.2 };
                segments[3] = Segment { angle: 0.0, length: 1.0 };
            }
        }
        5 => {
            let angle = random_double(rng, PI / 4.0, FRAC_PI_2, PI / 180.0);
            let length = random_double(rng, 0.25, 0.50, 0.001);
            if rng.random_range(0..2u32) == 0 {
                segments[0] = Segment { angle: 0.0, length: 1.0 };
                segments[1] = Segment { angle, length };
                segments[2] = Segment { angle: -angle, length };
                segments[3] = Segment { angle: 0.0, length: 1.0 };
            } else {
                segments[0] = Segment { angle: 0.0, length: 1.0 };
                segments[1] = Segment { angle: -angle, length };
                segments[2] = Segment { angle, length };
                segments[3] = Segment { angle: 0.0, length: 1.0 };
            }
        }
        _ => {
            segments[0].angle = random_double(rng, PI / 12.0, 11.0 * PI / 12.0, 0.001);
            segments[0].length = random_double(rng, 0.25, 0.50, 0.001);
            segments[1].angle = random_double(rng, PI / 12.0, 11.0 * PI / 12.0, 0.001);
            segments[1].length = random_double(rng, 0.25, 0.50, 0.001);
            if rng.random_range(0..3u32) == 0 {
                segments[2].angle = random_double(rng, PI / 12.0, 11.0 * PI / 12.0, 0.001);
                segments[2].length = random_double(rng, 0.25, 0.50, 0.001);
                segments[3].angle = random_double(rng, PI / 12.0, 11.0 * PI / 12.0, 0.001);
                segments[3].length = random_double(rng, 0.25, 0.50, 0.001);
            } else if rng.random_range(0..2u32) == 0 {
                segments[2].angle = -segments[1].angle;
                segments[2].length = segments[1].length;
                segments[3].angle = -segments[0].angle;
                segments[3].length = segments[0].length;
            } else {
                segments[2].angle = segments[1].angle;
                segments[2].length = segments[1].length;
                segments[3].angle = segments[0].angle;
                segments[3].length = segments[0].length;
            }
        }
    }
}

fn select_pattern(rng: &mut impl RngExt, segment_count: usize) -> Vec<Segment> {
    let mut segments = vec![Segment { angle: 0.0, length: 0.0 }; segment_count];
    match segment_count {
        2 => select_2_pattern(rng, &mut segments),
        3 => select_3_pattern(rng, &mut segments),
        _ => select_4_pattern(rng, &mut segments),
    }
    segments
}

// X11 silently clips huge coordinates; Bresenham would walk them all.
// Clamped endpoints stay far off screen so nothing visible changes.
fn coord(v: f64) -> i32 {
    v.clamp(-32768.0, 32768.0) as i32
}

pub struct CCurve {
    width: u32,
    height: u32,
    pixels: Vec<u8>,

    colors: Vec<Color>,
    color_count: usize,
    line_count: i32,
    total_lines: i32,
    plot_maximum_x: f64,
    plot_maximum_y: f64,
    plot_minimum_x: f64,
    plot_minimum_y: f64,

    wait_ticks: u32,

    draw_index: i32,
    draw_iterations: i32,
    draw_maximum_x: f64,
    draw_maximum_y: f64,
    draw_minimum_x: f64,
    draw_minimum_y: f64,
    draw_segments: Vec<Segment>,
    draw_x1: f64,
    draw_y1: f64,
    draw_x2: f64,
    draw_y2: f64,
}

impl CCurve {
    fn build(config: &AnimConfig) -> CCurve {
        // make_color_loop(0,1,1, 120,1,1, 240,1,1) with 256 colors: with three
        // equally spaced fully-saturated hue anchors this is a uniform hue wheel.
        let colors: Vec<Color> = (0..MAXIMUM_COLOR_COUNT)
            .map(|i| hsv_to_rgb((i * 360 / MAXIMUM_COLOR_COUNT) as i32, 1.0, 1.0))
            .collect();

        CCurve {
            width: config.width,
            height: config.height,
            pixels: vec![0u8; (config.width * config.height * 4) as usize],
            color_count: colors.len(),
            colors,
            line_count: 0,
            total_lines: 0,
            plot_maximum_x: -1000.0,
            plot_maximum_y: -1000.0,
            plot_minimum_x: 1000.0,
            plot_minimum_y: 1000.0,
            wait_ticks: 0,
            draw_index: 0,
            draw_iterations: 0,
            draw_maximum_x: 1.20,
            draw_maximum_y: 0.525,
            draw_minimum_x: -0.20,
            draw_minimum_y: -0.525,
            draw_segments: Vec::new(),
            draw_x1: 0.0,
            draw_y1: 0.0,
            draw_x2: 1.0,
            draw_y2: 0.0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn self_similar_normalized(
        &mut self,
        pixels: &mut [u8],
        iterations: i32,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        maximum_x: f64,
        maximum_y: f64,
        minimum_x: f64,
        minimum_y: f64,
        points: &[Position],
    ) -> bool {
        if iterations == 0 {
            let delta_x = maximum_x - minimum_x;
            let delta_y = maximum_y - minimum_y;
            let color_index = ((self.line_count as f64 * self.color_count as f64)
                / self.total_lines as f64) as usize;
            self.line_count += 1;
            if self.plot_maximum_x < x1 {
                self.plot_maximum_x = x1;
            }
            if self.plot_maximum_x < x2 {
                self.plot_maximum_x = x2;
            }
            if self.plot_maximum_y < y1 {
                self.plot_maximum_y = y1;
            }
            if self.plot_maximum_y < y2 {
                self.plot_maximum_y = y2;
            }
            if self.plot_minimum_x > x1 {
                self.plot_minimum_x = x1;
            }
            if self.plot_minimum_x > x2 {
                self.plot_minimum_x = x2;
            }
            if self.plot_minimum_y > y1 {
                self.plot_minimum_y = y1;
            }
            if self.plot_minimum_y > y2 {
                self.plot_minimum_y = y2;
            }
            draw_line(
                pixels,
                self.width,
                self.height,
                coord(((x1 - minimum_x) / delta_x) * self.width as f64),
                coord(((maximum_y - y1) / delta_y) * self.height as f64),
                coord(((x2 - minimum_x) / delta_x) * self.width as f64),
                coord(((maximum_y - y2) / delta_y) * self.height as f64),
                self.colors[color_index.min(self.color_count - 1)],
            );
        } else {
            let mut replacement = points.to_vec();
            realign(x1, y1, x2, y2, &mut replacement);
            // jwz: bail on the fractal instead of crashing
            let last = replacement[replacement.len() - 1];
            if (x2 - last.0).abs() >= EPSILON || (y2 - last.1).abs() >= EPSILON {
                return false;
            }
            let mut x = x1;
            let mut y = y1;
            for &(next_x, next_y) in replacement.iter() {
                if !self.self_similar_normalized(
                    pixels,
                    iterations - 1,
                    x,
                    y,
                    next_x,
                    next_y,
                    maximum_x,
                    maximum_y,
                    minimum_x,
                    minimum_y,
                    points,
                ) {
                    return false;
                }
                x = next_x;
                y = next_y;
            }
        }
        true
    }
}

impl Animation for CCurve {
    fn new(config: &AnimConfig) -> Self {
        Self::build(config)
    }

    fn tick(&mut self) {
        if self.wait_ticks > 0 {
            self.wait_ticks -= 1;
            return;
        }

        let mut rng = rand::rng();
        const LENGTHS: [usize; 9] = [4, 4, 4, 4, 4, 3, 3, 3, 2];

        if self.draw_index == 0 {
            let segment_count = LENGTHS[rng.random_range(0..LENGTHS.len())];
            self.draw_segments = select_pattern(&mut rng, segment_count);
            self.draw_iterations =
                ((MAXIMUM_LINES as f64).ln() / (segment_count as f64).ln()).floor() as i32;
            if rng.random_range(0..3u32) != 0 {
                let factor = 0.45;
                self.draw_x1 += random_double(&mut rng, -factor, factor, 0.001);
                self.draw_y1 += random_double(&mut rng, -factor, factor, 0.001);
                self.draw_x2 += random_double(&mut rng, -factor, factor, 0.001);
                self.draw_y2 += random_double(&mut rng, -factor, factor, 0.001);
            }
        }

        let mut pixels = std::mem::take(&mut self.pixels);
        clear_buffer(&mut pixels, BLACK);
        self.line_count = 0;
        self.total_lines = (self.draw_segments.len() as f64).powi(self.draw_index) as i32;
        self.plot_maximum_x = -1000.0;
        self.plot_maximum_y = -1000.0;
        self.plot_minimum_x = 1000.0;
        self.plot_minimum_y = 1000.0;

        let points = normalized_plot(&self.draw_segments);
        let (x1, y1, x2, y2) = (self.draw_x1, self.draw_y1, self.draw_x2, self.draw_y2);
        let (max_x, max_y, min_x, min_y) = (
            self.draw_maximum_x,
            self.draw_maximum_y,
            self.draw_minimum_x,
            self.draw_minimum_y,
        );
        self.self_similar_normalized(
            &mut pixels,
            self.draw_index,
            x1,
            y1,
            x2,
            y2,
            max_x,
            max_y,
            min_x,
            min_y,
            &points,
        );
        self.pixels = pixels;

        // zoom the view to the plotted extent, with a 20% margin,
        // then pad to the window's aspect ratio
        let delta_x = self.plot_maximum_x - self.plot_minimum_x;
        let delta_y = self.plot_maximum_y - self.plot_minimum_y;
        self.draw_maximum_x = self.plot_maximum_x + delta_x * 0.2;
        self.draw_maximum_y = self.plot_maximum_y + delta_y * 0.2;
        self.draw_minimum_x = self.plot_minimum_x - delta_x * 0.2;
        self.draw_minimum_y = self.plot_minimum_y - delta_y * 0.2;
        let delta_x = self.draw_maximum_x - self.draw_minimum_x;
        let delta_y = self.draw_maximum_y - self.draw_minimum_y;
        if delta_y / delta_x > self.height as f64 / self.width as f64 {
            let new_delta_x = delta_y * self.width as f64 / self.height as f64;
            self.draw_minimum_x -= (new_delta_x - delta_x) / 2.0;
            self.draw_maximum_x += (new_delta_x - delta_x) / 2.0;
        } else {
            let new_delta_y = delta_x * self.height as f64 / self.width as f64;
            self.draw_minimum_y -= (new_delta_y - delta_y) / 2.0;
            self.draw_maximum_y += (new_delta_y - delta_y) / 2.0;
        }

        self.draw_index += 1;
        if self.draw_index >= self.draw_iterations {
            self.draw_index = 0;
            self.wait_ticks = DELAY_TICKS;
        } else {
            self.wait_ticks = PAUSE_TICKS;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if width == self.width && height == self.height {
            let n = buffer.len().min(self.pixels.len());
            buffer[..n].copy_from_slice(&self.pixels[..n]);
        } else {
            let w = (width as usize).min(self.width as usize);
            let h = (height as usize).min(self.height as usize);
            for row in 0..h {
                let src = row * self.width as usize * 4;
                let dst = row * width as usize * 4;
                buffer[dst..dst + w * 4].copy_from_slice(&self.pixels[src..src + w * 4]);
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        *self = Self::build(config);
    }

    fn frame_delay_us(&self) -> u64 {
        TICK_US
    }
}
