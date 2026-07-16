//! Raindrops falling down the screen.
//
// Copyright (c) 1999-2007 by Frank Fesevur
//
// Permission to use, copy, modify, and distribute this software and its
// documentation for any purpose and without fee is hereby granted,
// provided that the above copywidth notice appear in all copies and that
// both that copywidth notice and this permission notice appear in
// supporting documentation.
//
// This file is provided AS IS with no warranties of any kind. The author
// shall have no liability with respect to the infringement of copywidths,
// trade secrets or any patents by this file or any part thereof. In no
// event will the author be liable for any lost revenue or profits or
// other special, indirect and consequential damages.
//
// ("copywidth" sic — the upstream notice really says that.)
//
// Rust port of xlockmore/modes/rain.c.

// 1-to-1 port of xlockmore's rain.c
//
// xlockmore defaults (from ModStruct rain_description):
//     delay=35000, count=1, cycles=1, size=1, ncolors=64
//
// rain.c has a quirky use of XRectangle to store line endpoints:
//   drop.drop.x      = x0  (trailing/upper endpoint, "start" of the line segment)
//   drop.drop.height = y0
//   drop.drop.width  = x1  (leading/lower endpoint, "head" of the falling drop)
//   drop.drop.y      = y1
// The DrawLine draws from (x, height) to (width, y). We use proper x0/y0/x1/y1
// fields and document the mapping where it matters.
//
// xlockmore's DrawEllipse uses drop.drop.x (= x0) and drop.drop.y (= y1) as the
// ellipse center — note the asymmetry: trailing x with leading y. The `pool`
// XPoint is set but never read; the falling check uses `pool.y` only. We
// reproduce that behaviour exactly: ellipse centered at (x0, y1), and we keep
// only `pool_y` since that's the threshold the falling check reads.

use rand::Rng;

use crate::animation::primitives::{draw_line, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

const MAX_RADIUS: i32 = 25;

#[derive(Clone)]
struct Drop {
    // Line endpoints. The line is drawn from (x0, y0) to (x1, y1) each frame
    // and the head (x1, y1) advances by (direction * offset_x, offset_y); the
    // trailing point (x0, y0) takes on the previous head's position.
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    offset_x: i32,
    offset_y: i32,
    // Pool depth — y coordinate at which the drop transitions from falling to
    // splashing. (xlockmore's pool.x is set but never read; we omit it.)
    pool_y: i32,
    color: Color,
    radius: i32,
    radius_step: i32,
    max_radius: i32,
}

impl Drop {
    fn blank() -> Self {
        Drop {
            x0: 0,
            y0: 0,
            x1: 0,
            y1: 0,
            offset_x: 0,
            offset_y: 0,
            pool_y: 0,
            color: Color::new(0, 0, 0, 0),
            radius: 0,
            radius_step: 0,
            max_radius: 0,
        }
    }
}

pub struct Rain {
    drops: Vec<Drop>,
    direction: i32,
    colored_drops: bool,
    // xlockmore stores `base_color` as a raw pixel value with `0` as a sentinel
    // meaning "no base — use a fully random color per drop." Using `Option`
    // avoids the sentinel collision that xlockmore can hit when pixel 0 of the
    // palette happens to be a valid color.
    base_color: Option<f32>,
    ncolors: i32,
    width: u32,
    height: u32,
    delay_us: u64,
}

fn init_drop_colors(rng: &mut impl Rng, colored_drops: &mut bool, base_color: &mut Option<f32>) {
    // xlockmore: 1-in-10 chance of monochrome (white-only) drops.
    *colored_drops = rng.random_range(0..10) != 1;
    if *colored_drops {
        // 1-in-2 chance of a base color (drops cluster around it); otherwise
        // every drop picks its own fully random color.
        if rng.random_range(0..2) == 1 {
            *base_color = Some(rng.random::<f32>());
        } else {
            *base_color = None;
        }
    }
    // xlockmore leaves base_color untouched when colored_drops is false — the
    // value carries over from the previous call. We do the same.
}

fn init_drop(
    drop: &mut Drop,
    width: u32,
    height: u32,
    colored_drops: bool,
    base_color: Option<f32>,
    ncolors: i32,
    rng: &mut impl Rng,
) {
    // Where in the lower 4/5 of the screen does the splash land?
    let y_min = (height / 5) as i32;
    let y_max = (height as i32) - ((MAX_RADIUS * 3) / 2);
    let y_range = (y_max - y_min).max(1);
    drop.pool_y = y_min + rng.random_range(0..y_range);

    // Deviation from xlockmore: the original's 5-9/20-39 px-per-tick offsets
    // are absolute speeds tuned for ~480px-tall screens; unscaled, drops
    // cross a 1080p panel at half the visual rate. Scale both axes by screen
    // height so the fall angle and screen-relative speed match the era look.
    let speed_scale = height as i32;
    drop.offset_x = ((5 + rng.random_range(0..5)) * speed_scale / 480).max(1);
    drop.offset_y = ((20 + rng.random_range(0..20)) * speed_scale / 480).max(1);

    // xlockmore initial endpoints — note that the initial `x1 = x0 + offset_x`
    // is HARDCODED to the positive direction. Only subsequent advances multiply
    // by `direction`, so the very first segment of a leftward drop goes right
    // for one frame before veering left. Faithful to the C bug.
    drop.x0 = rng.random_range(0..(width as i32).max(1));
    drop.y0 = 0;
    drop.x1 = drop.x0 + drop.offset_x;
    drop.y1 = drop.y0 + drop.offset_y;

    drop.radius = 0;
    drop.radius_step = 1 + rng.random_range(0..2);
    drop.max_radius = (MAX_RADIUS / 2) + rng.random_range(0..(MAX_RADIUS / 2));

    if colored_drops {
        if let Some(hue) = base_color {
            // xlockmore: `base_color + (NRAND(2) == 1 ? -1 : 1) * NRAND(12)`
            // (raw pixel arithmetic). For a 64-entry SMOOTH_COLORS palette
            // this is roughly `±[0,12)/ncolors` in hue space.
            let n = ncolors.max(1) as f32;
            let sign: f32 = if rng.random_range(0..2) == 1 { -1.0 } else { 1.0 };
            let shift = sign * (rng.random_range(0..12) as f32 / n);
            let mut final_hue = (hue + shift).fract();
            if final_hue < 0.0 {
                final_hue += 1.0;
            }
            drop.color = Color::from_hsl(final_hue, 1.0, 0.5);
        } else {
            drop.color = Color::from_hsl(rng.random::<f32>(), 1.0, 0.5);
        }
    } else {
        drop.color = Color::new(255, 255, 255, 255);
    }
}

// Midpoint ellipse algorithm — draws the outline of an axis-aligned ellipse
// with semi-axes `rx` (= 2*radius) and `ry` (= 2*radius/3) centered at
// `(xc, yc)`. Matches xlockmore's XDrawArc(0, 360*64) — outline only, no fill.
fn draw_ellipse(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    xc: i32,
    yc: i32,
    radius: i32,
    color: Color,
) {
    let rx = 2 * radius;
    let ry = (radius * 2) / 3;

    if rx == 0 || ry == 0 {
        put_pixel(buffer, width, height, xc, yc, color);
        return;
    }

    let rx2 = (rx * rx) as i64;
    let ry2 = (ry * ry) as i64;
    let tworx2 = 2 * rx2;
    let twory2 = 2 * ry2;

    let mut x = 0i64;
    let mut y = ry as i64;
    let mut px = 0i64;
    let mut py = tworx2 * y;

    let mut p = (ry2 as f64 - (rx2 as f64) * (ry as f64) + 0.25 * (rx2 as f64)).round() as i64;

    while px < py {
        put_pixel(buffer, width, height, xc + x as i32, yc + y as i32, color);
        put_pixel(buffer, width, height, xc - x as i32, yc + y as i32, color);
        put_pixel(buffer, width, height, xc + x as i32, yc - y as i32, color);
        put_pixel(buffer, width, height, xc - x as i32, yc - y as i32, color);
        x += 1;
        px += twory2;
        if p < 0 {
            p += ry2 + px;
        } else {
            y -= 1;
            py -= tworx2;
            p += ry2 + px - py;
        }
    }

    let mut p2 = (ry2 as f64 * (x as f64 + 0.5).powi(2)
        + rx2 as f64 * (y as f64 - 1.0).powi(2)
        - (rx2 * ry2) as f64)
        .round() as i64;

    while y >= 0 {
        put_pixel(buffer, width, height, xc + x as i32, yc + y as i32, color);
        put_pixel(buffer, width, height, xc - x as i32, yc + y as i32, color);
        put_pixel(buffer, width, height, xc + x as i32, yc - y as i32, color);
        put_pixel(buffer, width, height, xc - x as i32, yc - y as i32, color);
        y -= 1;
        py -= tworx2;
        if p2 > 0 {
            p2 += rx2 - py;
        } else {
            x += 1;
            px += twory2;
            p2 += rx2 - py + px;
        }
    }
}

// True if a drop is still falling (vs splashing). xlockmore: the head x must
// stay clear of both screen edges by `max_radius` so the eventual splash
// ellipse fits on screen, AND the head y must not yet have reached pool_y.
fn is_falling(drop: &Drop, width: i32) -> bool {
    drop.x1 < width - drop.max_radius && drop.x1 > drop.max_radius && drop.y1 < drop.pool_y
}

impl Animation for Rain {
    fn new(config: &AnimConfig) -> Self {
        let mut r = Rain {
            drops: Vec::new(),
            direction: 1,
            colored_drops: false,
            base_color: None,
            ncolors: 64,
            width: config.width,
            height: config.height,
            delay_us: 35_000,
        };
        r.reset(config);
        r
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        // xlockmore: per-frame 1-in-(width/4) chance to reroll the color
        // scheme. `.max(2)` is a safety guard for tiny windows.
        let chance = (self.width / 4).max(2);
        if rng.random_range(0..chance) == 1 {
            init_drop_colors(&mut rng, &mut self.colored_drops, &mut self.base_color);
        }

        let width_i32 = self.width as i32;
        let direction = self.direction;
        let colored_drops = self.colored_drops;
        let base_color = self.base_color;
        let ncolors = self.ncolors;
        let width = self.width;
        let height = self.height;

        for drop in &mut self.drops {
            if is_falling(drop, width_i32) {
                // Trail follows head; head advances by (direction*ox, oy).
                drop.x0 = drop.x1;
                drop.y0 = drop.y1;
                drop.x1 += direction * drop.offset_x;
                drop.y1 += drop.offset_y;
            } else {
                // Splash. xlockmore updates `pool.x = drop.width; pool.y =
                // drop.y;` on the first splash tick (radius == 0), but only
                // pool.y has any effect (it's used by the falling check next
                // tick, locking us into the splash branch).
                if drop.radius == 0 {
                    drop.pool_y = drop.y1;
                }

                if drop.radius > drop.max_radius {
                    init_drop(drop, width, height, colored_drops, base_color, ncolors, &mut rng);
                } else {
                    drop.radius += drop.radius_step;
                }
            }
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let width_i32 = width as i32;
        for drop in &self.drops {
            if is_falling(drop, width_i32) {
                draw_line(
                    buffer,
                    width,
                    height,
                    drop.x0,
                    drop.y0,
                    drop.x1,
                    drop.y1,
                    drop.color,
                );
            } else {
                // xlockmore quirk: ellipse center is (drop.drop.x, drop.drop.y)
                // = (x0, y1) — the trailing endpoint's x with the head
                // endpoint's y. The splash visually trails the drop's path by
                // one `offset_x` rather than landing exactly under the head.
                draw_ellipse(buffer, width, height, drop.x0, drop.y1, drop.radius, drop.color);
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;
        self.ncolors = if config.ncolors <= 0 { 64 } else { config.ncolors };
        self.delay_us = if config.delay_us == 0 { 35_000 } else { config.delay_us };

        // xlockmore: 50/50 left-to-right vs right-to-left.
        self.direction = if rng.random_range(0..2) == 1 { -1 } else { 1 };
        init_drop_colors(&mut rng, &mut self.colored_drops, &mut self.base_color);

        // nr_drops in [i, 2*i) where i = max(width/100, 2). Then clamped to
        // [1, max_drops]. With width=1920: max_drops=38, i=19, nr_drops in [19, 38).
        let max_drops = (config.width / 50).max(1) as usize;
        let i = (max_drops / 2).max(2);
        let mut nr_drops = rng.random_range(0..i) + i;
        nr_drops = nr_drops.clamp(1, max_drops);

        self.drops.clear();
        self.drops.reserve(nr_drops);
        for _ in 0..nr_drops {
            let mut drop = Drop::blank();
            init_drop(
                &mut drop,
                config.width,
                config.height,
                self.colored_drops,
                self.base_color,
                self.ncolors,
                &mut rng,
            );
            self.drops.push(drop);
        }
    }

    fn clears_each_frame(&self) -> bool {
        // xlockmore is incremental (DrawLine in black to erase, then DrawLine
        // in color), but a full clear produces the same image since only the
        // current frame's lines and ellipses are ever visible.
        true
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
