/* xscreensaver, Copyright (c) 1997-2008 Jamie Zawinski <jwz@jwz.org>
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Rust port of xscreensaver's goop.c (with the needed parts of
 * utils/spline.c -- Copyright (c) 1987-1989 Stanford University, from the
 * InterViews distribution -- and utils/hsv.c ported inline).
 *
 * This implements goop's default configuration: mode `transparent` with
 * `additive` blending, planes=12.  The X11 color-plane trick
 * (utils/alpha.c) is emulated directly: each layer gets a random dim HSV
 * color (the additive PseudoColor path of allocate_alpha_colors) and the
 * union of a layer's blobs is added, saturating, into the frame -- so
 * overlapping layers mix additively while blobs within one layer do not
 * double-brighten (they share a plane).
 *
 * Deliberate deviations: the X11 transparent path throbs/moves each blob
 * twice per displayed frame (draw_layer_plane draws to a pixmap that is
 * never shown, then draw_layer_blobs advances again); we advance once per
 * frame like the JWXYZ/opaque path.  The mono/xor/opaque/outline modes are
 * not implemented (we always have a color display).
 */

use crate::rng::RngExt;
use std::f64::consts::PI;
use crate::animation::primitives::{Color, Spline};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const SCALE: i64 = 10000; /* fixed-point math, for sub-pixel motion */
const DEF_COUNT: i32 = 12; /* When planes and count are 0, how many blobs. */

/* goop.c defaults table:
 *   *delay: 12000   *additive: true   *mode: transparent
 *   *count: 1 (per layer; <= 0 spreads DEF_COUNT over the layers)
 *   *planes: 12   *torque: 0.0075   *elasticity: 0.9   *maxVelocity: 0.5
 */
const PLANES: usize = 12;
const TORQUE: f64 = 0.0075;
const ELASTICITY: f64 = 0.9;
const MAX_VELOCITY: f64 = 0.5;

fn rand_sign(rng: &mut impl RngExt) -> i64 {
    if rng.random::<bool>() { 1 } else { -1 }
}

fn nrand(rng: &mut impl RngExt, v: i64) -> i64 {
    if v <= 0 { 0 } else { rng.random_range(0..v) }
}

/* ---- utils/hsv.c ---- */

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

/* utils/spline.c ported into animation::primitives (shared with attraction). */

/* ---- goop.c proper ---- */

struct Blob {
    x: i64,
    y: i64,
    dx: i64,
    dy: i64,
    torque: f64,
    th: f64,
    elasticity: i64,
    max_velocity: i64,
    min_r: i64,
    max_r: i64,
    npoints: usize,
    r: Vec<i64>,
    spline: Spline,
}

struct Layer {
    blobs: Vec<Blob>,
    color: Color,
    /// Pixel indices this layer covers in the current frame (deduped union
    /// of its blobs), rasterized in `tick` and composited in `render`.
    covered: Vec<usize>,
}

pub struct Goop {
    layers: Vec<Layer>,
    width: u32,
    height: u32,
    /// Reused coverage scratch, sized width*height; kept all-false between
    /// frames (each layer clears its own touched pixels after rasterizing).
    mask: Vec<bool>,
    /// Reused scanline crossing buffer for `fill_polygon_mask`.
    xs_scratch: Vec<i32>,
}

fn make_blob(maxx: i32, maxy: i32, size: i32, rng: &mut impl RngExt) -> Blob {
    let maxx = (maxx as i64) * SCALE;
    let maxy = (maxy as i64) * SCALE;
    let size = (size as i64) * SCALE;

    let max_r = size / 2;
    let mut min_r = size / 10;
    if min_r < 5 * SCALE {
        min_r = 5 * SCALE;
    }
    let mid = (min_r + max_r) / 2;

    let max_velocity = (SCALE as f64 * MAX_VELOCITY) as i64;
    let npoints = rng.random_range(0..5usize) + 5;

    Blob {
        x: nrand(rng, maxx),
        y: nrand(rng, maxy),
        dx: nrand(rng, max_velocity) * rand_sign(rng),
        dy: nrand(rng, max_velocity) * rand_sign(rng),
        torque: TORQUE,
        th: rng.random::<f64>() * (PI + PI) * rand_sign(rng) as f64,
        elasticity: (SCALE as f64 * ELASTICITY) as i64,
        max_velocity,
        min_r,
        max_r,
        npoints,
        r: (0..npoints)
            .map(|_| (nrand(rng, mid) + mid / 2) * rand_sign(rng))
            .collect(),
        spline: Spline::new(npoints),
    }
}

fn throb_blob(b: &mut Blob, rng: &mut impl RngExt) {
    let frac = (PI + PI) / (b.npoints as f64);

    for i in 0..b.npoints {
        let mut r = b.r[i];
        let mut ra = r.abs();
        let th = b.th.abs();

        /* place control points evenly around perimeter, shifted by theta */
        let x = b.x + (ra as f64 * (i as f64 * frac + th).cos()) as i64;
        let y = b.y + (ra as f64 * (i as f64 * frac + th).sin()) as i64;

        b.spline.control_x[i] = (x / SCALE) as f64;
        b.spline.control_y[i] = (y / SCALE) as f64;

        /* alter the radius by a random amount, in the direction in which
           it had been going (the sign of the radius indicates direction.) */
        ra += nrand(rng, b.elasticity) * if r > 0 { 1 } else { -1 };
        r = ra * if r >= 0 { 1 } else { -1 };

        /* If we've reached the end (too long or too short) reverse direction. */
        if (ra > b.max_r && r >= 0) || (ra < b.min_r && r < 0) {
            r = -r;
        /* And reverse direction in mid-course once every 50 times. */
        } else if nrand(rng, 50) == 0 {
            r = -r;
        }

        b.r[i] = r;
    }
}

fn move_blob(b: &mut Blob, maxx: i32, maxy: i32, rng: &mut impl RngExt) {
    let maxx = (maxx as i64) * SCALE;
    let maxy = (maxy as i64) * SCALE;

    b.x += b.dx;
    b.y += b.dy;

    /* If we've reached the edge of the box, reverse direction. */
    if (b.x > maxx && b.dx >= 0) || (b.x < 0 && b.dx < 0) {
        b.dx = -b.dx;
    }
    if (b.y > maxy && b.dy >= 0) || (b.y < 0 && b.dy < 0) {
        b.dy = -b.dy;
    }

    /* Alter velocity randomly. */
    if rng.random_range(0..10) == 0 {
        b.dx += nrand(rng, b.max_velocity / 2) * rand_sign(rng);
        b.dy += nrand(rng, b.max_velocity / 2) * rand_sign(rng);

        /* Throttle velocity */
        if b.dx > b.max_velocity || b.dx < -b.max_velocity {
            b.dx /= 2;
        }
        if b.dy > b.max_velocity || b.dy < -b.max_velocity {
            b.dy /= 2;
        }
    }

    let mut th = b.th;
    let d = if b.torque == 0.0 { 0.0 } else { b.torque * rng.random::<f64>() };

    if th < 0.0 {
        th = -(th + d);
    } else {
        th += d;
    }

    if th > PI + PI {
        th -= PI + PI;
    } else if th < 0.0 {
        th += PI + PI;
    }

    b.th = if b.th > 0.0 { th } else { -th };

    /* Alter direction of rotation randomly. */
    if rng.random_range(0..100) == 0 {
        b.th *= -1.0;
    }
}

/* XFillPolygon (Nonconvex, even-odd scanline fill) into a per-layer
 * coverage mask -- the plane-mask analog: blobs in one layer OR together.
 * Newly-set pixels are recorded in `covered` (deduped via the mask) so the
 * caller can both composite and clear them without scanning the whole grid.
 * `xs` is a reusable scratch buffer for the scanline crossings. */
fn fill_polygon_mask(
    mask: &mut [bool],
    covered: &mut Vec<usize>,
    width: u32,
    height: u32,
    verts: &[(i32, i32)],
    xs: &mut Vec<i32>,
) {
    let n = verts.len();
    if n < 3 {
        return;
    }
    let (Some(min_y), Some(max_y)) = (
        verts.iter().map(|&(_, y)| y).min(),
        verts.iter().map(|&(_, y)| y).max(),
    ) else {
        return;
    };
    let min_y = min_y.max(0) as u32;
    let max_y = match max_y.min(height as i32 - 1) {
        v if v < 0 => return,
        v => v as u32,
    };
    for scan_y in min_y..=max_y {
        let sy = scan_y as i32;
        xs.clear();
        for i in 0..n {
            let (x0, y0) = verts[i];
            let (x1, y1) = verts[(i + 1) % n];
            if (y0 <= sy && sy < y1) || (y1 <= sy && sy < y0) {
                let x = x0 + (x1 - x0) * (sy - y0) / (y1 - y0);
                xs.push(x);
            }
        }
        xs.sort_unstable();
        let mut i = 0;
        let row = scan_y as usize * width as usize;
        while i + 1 < xs.len() {
            let x0 = xs[i].max(0);
            let x1 = xs[i + 1].min(width as i32 - 1);
            for x in x0..=x1 {
                let p = row + x as usize;
                if !mask[p] {
                    mask[p] = true;
                    covered.push(p);
                }
            }
            i += 2;
        }
    }
}

impl Animation for Goop {
    fn new(config: &AnimConfig) -> Self {
        let mut g = Goop {
            layers: Vec::new(),
            width: config.width,
            height: config.height,
            mask: Vec::new(),
            xs_scratch: Vec::new(),
        };
        g.reset(config);
        g
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = crate::rng::rng();
        self.width = config.width;
        self.height = config.height;
        self.mask = vec![false; self.width as usize * self.height as usize];
        self.xs_scratch = Vec::new();

        /* nlayers is the "planes" resource; goop.c defaults it to 12 and only
         * randomizes (LRAND() % (depth-2) + 2) when planes <= 0. */
        let nlayers = PLANES;
        let nblobs = config.count;

        /* count <= 0: spread DEF_COUNT blobs over the layers, round-robin. */
        let mut lblobs = [0i32; PLANES];
        if nblobs <= 0 {
            let mut total = DEF_COUNT;
            while total > 0 {
                for l in lblobs.iter_mut() {
                    if total > 0 {
                        *l += 1;
                        total -= 1;
                    }
                }
            }
        }

        self.layers.clear();
        for lcount in lblobs.iter().take(nlayers) {
            let count = if nblobs > 0 { nblobs } else { *lcount };

            /* make_layer() */
            let mut blob_max = (config.width.min(config.height) as i32) / 2;
            if config.width < 100 || config.height < 100 {
                blob_max *= 10; /* tiny window */
            }
            let blob_min = (blob_max * 2) / 3;

            let mut blobs = Vec::with_capacity(count.max(0) as usize);
            for _ in 0..count {
                let j = (blob_max - blob_min) as i64;
                let size = nrand(&mut rng, j) as i32 + blob_min;
                blobs.push(make_blob(
                    config.width as i32,
                    config.height as i32,
                    size,
                    &mut rng,
                ));
            }

            /* allocate_alpha_colors(), additive: pick dim base colors so the
             * saturating sums stay in range.  H 0-360, S 0-100%, V 20-70%. */
            let color = hsv_to_rgb(
                nrand(&mut rng, 360) as i32,
                rng.random::<f64>(),
                rng.random::<f64>() * 0.5 + 0.2,
            );
            self.layers.push(Layer { blobs, color, covered: Vec::new() });
        }
    }

    fn tick(&mut self) {
        let mut rng = crate::rng::rng();
        let (w, h) = (self.width as i32, self.height as i32);
        for layer in &mut self.layers {
            for blob in &mut layer.blobs {
                throb_blob(blob, &mut rng);
                move_blob(blob, w, h, &mut rng);
                blob.spline.compute_closed_spline();
            }
        }

        // Rasterize each layer's blob union into the reused mask, recording
        // the covered pixels for render(), then clear the mask for the next
        // layer using that same list (no full-grid scan, no per-frame alloc).
        let (mw, mh) = (self.width, self.height);
        let Goop { layers, mask, xs_scratch, .. } = self;
        for layer in layers.iter_mut() {
            layer.covered.clear();
            for blob in &layer.blobs {
                fill_polygon_mask(mask, &mut layer.covered, mw, mh, &blob.spline.points, xs_scratch);
            }
            for &p in &layer.covered {
                mask[p] = false;
            }
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let npix = self.width as usize * self.height as usize;
        if width != self.width || height != self.height || buffer.len() < npix * 4 {
            return;
        }
        for layer in &self.layers {
            let c = layer.color;
            for &p in &layer.covered {
                let idx = p * 4;
                // buffer layout matches put_pixel: b, g, r, a
                buffer[idx] = buffer[idx].saturating_add(c.b);
                buffer[idx + 1] = buffer[idx + 1].saturating_add(c.g);
                buffer[idx + 2] = buffer[idx + 2].saturating_add(c.r);
                buffer[idx + 3] = 255;
            }
        }
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        12_000 // goop.c default: *delay: 12000
    }
}
