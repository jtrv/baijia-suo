use crate::rng::RngExt;

pub fn rgb_to_hsv(r: u16, g: u16, b: u16) -> (i32, f64, f64) {
    let rr = r as f64 / 65535.0;
    let gg = g as f64 / 65535.0;
    let bb = b as f64 / 65535.0;
    let (mut cmax, mut cmin, mut imax) = (rr, gg, 1);
    if cmax < gg {
        cmax = gg;
        cmin = rr;
        imax = 2;
    }
    if cmax < bb {
        cmax = bb;
        imax = 3;
    }
    if cmin > bb {
        cmin = bb;
    }
    let cmm = cmax - cmin;
    let v = cmax;
    let (h, s) = if cmm == 0.0 {
        (0.0, 0.0)
    } else {
        let s = cmm / cmax;
        let mut h = match imax {
            1 => (gg - bb) / cmm,
            2 => 2.0 + (bb - rr) / cmm,
            _ => 4.0 + (rr - gg) / cmm,
        };
        if h < 0.0 {
            h += 6.0;
        }
        (h, s)
    };
    ((h * 60.0) as i32, s, v)
}

/* port of utils/colors.c make_color_ramp (color computation only) */
#[allow(clippy::needless_range_loop, clippy::too_many_arguments)]
/// A simple ARGB color representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub a: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub fn new(a: u8, r: u8, g: u8, b: u8) -> Self {
        Self { a, r, g, b }
    }

    pub fn to_argb32(self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    /// Creates a color from HSL values.
    /// h, s, l are in the range 0.0 - 1.0
    pub fn from_hsl(h: f32, s: f32, l: f32) -> Self {
        if s == 0.0 {
            let v = (l * 255.0) as u8;
            return Self::new(255, v, v, v);
        }

        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;

        let r = Self::hue_to_rgb(p, q, h + 1.0 / 3.0);
        let g = Self::hue_to_rgb(p, q, h);
        let b = Self::hue_to_rgb(p, q, h - 1.0 / 3.0);

        Self::new(255, (r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
    }

    fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            return p + (q - p) * 6.0 * t;
        }
        if t < 1.0 / 2.0 {
            return q;
        }
        if t < 2.0 / 3.0 {
            return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
        }
        p
    }
}

/// Set a pixel in the buffer.
#[inline(always)]
pub fn put_pixel(buffer: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: Color) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }

    let stride = (width * 4) as usize;
    let idx = (y as usize) * stride + (x as usize) * 4;

    if idx + 3 < buffer.len() {
        buffer[idx] = color.b;
        buffer[idx + 1] = color.g;
        buffer[idx + 2] = color.r;
        buffer[idx + 3] = color.a;
    }
}

/// Clear the entire buffer with a color.
pub fn clear_buffer(buffer: &mut [u8], color: Color) {
    let px = color.to_argb32().to_ne_bytes();
    for chunk in buffer.as_chunks_mut::<4>().0 {
        *chunk = px;
    }
}

/// Draw a line using Bresenham's algorithm.
// The natural line signature: buffer + dims + two endpoints + color. Bundling
// the points into structs would churn 100+ C-port call sites for no depth.
#[allow(clippy::too_many_arguments)]
pub fn draw_line(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: Color,
) {
    // X11 rasterizes a thin line identically regardless of drawing
    // direction, and modes rely on that: several erase by redrawing a
    // segment with swapped endpoints (e.g. attraction's tail mode).
    // Bresenham tie-breaks are direction-dependent, so canonicalize.
    let (mut x0, mut y0, x1, y1) = if (x1, y1) < (x0, y0) {
        (x1, y1, x0, y0)
    } else {
        (x0, y0, x1, y1)
    };
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        put_pixel(buffer, width, height, x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/// Draw a circle outline using midpoint circle algorithm.
pub fn draw_circle(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    cx: i32,
    cy: i32,
    radius: i32,
    color: Color,
) {
    let mut x = radius;
    let mut y = 0;
    let mut err = 0;

    while x >= y {
        put_pixel(buffer, width, height, cx + x, cy + y, color);
        put_pixel(buffer, width, height, cx + y, cy + x, color);
        put_pixel(buffer, width, height, cx - y, cy + x, color);
        put_pixel(buffer, width, height, cx - x, cy + y, color);
        put_pixel(buffer, width, height, cx - x, cy - y, color);
        put_pixel(buffer, width, height, cx - y, cy - x, color);
        put_pixel(buffer, width, height, cx + y, cy - x, color);
        put_pixel(buffer, width, height, cx + x, cy - y, color);

        if err <= 0 {
            y += 1;
            err += 2 * y + 1;
        }
        if err > 0 {
            x -= 1;
            err -= 2 * x + 1;
        }
    }
}

const BLACK: Color = Color {
    a: 255,
    r: 0,
    g: 0,
    b: 0,
};

/* ---- utils/hsv.c ---- */

/// Convert HSV (h in degrees, s/v in 0.0..=1.0) to 16-bit-per-channel RGB,
/// matching xscreensaver's utils/hsv.c. Ported 1:1 for the modes that build
/// colormaps from 16-bit color (attraction, anemone, cynosure, vermiculate).
pub fn hsv_to_rgb(h: i32, s: f64, v: f64) -> (u16, u16, u16) {
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
    (
        (r * 65535.0) as u16,
        (g * 65535.0) as u16,
        (b * 65535.0) as u16,
    )
}

/// Pack a 16-bit-per-channel RGB triple into an opaque 8-bit `Color`.
pub fn rgb16(r: u16, g: u16, b: u16) -> Color {
    Color::new(255, (r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8)
}

/* ---- utils/colors.c ---- */

/// utils/colors.c make_color_ramp: a linear HSV ramp of `total` colors.
/// The moire and popsquares ports truncate the hue offset to an integer before
/// adding it to `h1`, where [`make_color_ramp`] truncates the sum. On a
/// descending ramp the two differ by one degree of hue, so they are not
/// interchangeable; `ramps_are_not_interchangeable` pins that.
#[allow(clippy::too_many_arguments)]
pub fn make_color_ramp_stepped_hue(
    h1: i32,
    s1: f64,
    v1: f64,
    h2: i32,
    s2: f64,
    v2: f64,
    total: usize,
    closed: bool,
) -> Vec<Color> {
    let n = if closed { total / 2 + 1 } else { total };
    let dh = (h2 - h1) as f64 / n as f64;
    let ds = (s2 - s1) / n as f64;
    let dv = (v2 - v1) / n as f64;
    let mut out = vec![BLACK; total];
    for (i, c) in out.iter_mut().enumerate().take(n.min(total)) {
        let (r, g, b) = hsv_to_rgb(
            h1 + (dh * i as f64) as i32,
            s1 + ds * i as f64,
            v1 + dv * i as f64,
        );
        *c = rgb16(r, g, b);
    }
    if closed {
        for i in n..total {
            out[i] = out[total - i];
        }
    }
    out
}

/// utils/colors.c make_color_ramp: a linear HSV ramp of `total` colors.
#[allow(clippy::too_many_arguments)]
pub fn make_color_ramp(
    h1: i32,
    s1: f64,
    v1: f64,
    h2: i32,
    s2: f64,
    v2: f64,
    total: usize,
    closed: bool,
) -> Vec<Color> {
    let mut colors = vec![BLACK; total];
    let ncolors = if closed { total / 2 + 1 } else { total };
    if ncolors == 0 {
        return colors;
    }
    let dh = (h2 as f64 - h1 as f64) / ncolors as f64;
    let ds = (s2 - s1) / ncolors as f64;
    let dv = (v2 - v1) / ncolors as f64;
    for (i, c) in colors.iter_mut().enumerate().take(ncolors.min(total)) {
        let (r, g, b) = hsv_to_rgb(
            (h1 as f64 + i as f64 * dh) as i32,
            s1 + i as f64 * ds,
            v1 + i as f64 * dv,
        );
        *c = rgb16(r, g, b);
    }
    if closed {
        for i in ncolors..total {
            colors[i] = colors[total - i];
        }
    }
    colors
}

/// utils/colors.c make_color_path: a smooth closed path through the given
/// HSV control points, producing `total` colors.
pub fn make_color_path(h: &[i32], s: &[f64], v: &[f64], total: usize) -> Vec<Color> {
    let npoints = h.len();
    if npoints == 0 || total == 0 {
        return Vec::new();
    }
    if npoints == 2 {
        return make_color_ramp(h[0], s[0], v[0], h[1], s[1], v[1], total, true);
    }

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
        return vec![BLACK; total];
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

    let mut colors: Vec<Color> = Vec::with_capacity(total);
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
            let (r, g, b) = hsv_to_rgb(hh as i32, s[i] + j as f64 * ds[i], v[i] + j as f64 * dv[i]);
            colors.push(rgb16(r, g, b));
        }
    }
    // Floating-point round-off can leave us short; pad with the last color.
    if colors.is_empty() {
        colors.push(BLACK);
    }
    while colors.len() < total {
        colors.push(*colors.last().unwrap());
    }
    colors
}

/// utils/colors.c make_smooth_colormap: pick random HSV control points that
/// are perceptually spread out, then trace a smooth color path through them.
pub fn make_smooth_colormap(ncolors: usize, rng: &mut impl RngExt) -> Vec<Color> {
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
                let distance =
                    ((dh * dh) + ((s[j] - s[i]) * (s[j] - s[i])) + ((v[j] - v[i]) * (v[j] - v[i])))
                        .sqrt();
                if distance < 0.2 {
                    continue; // repick this color
                }
            }
            total_s += s[i];
            total_v += v[i];
            i += 1;
        }
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

/* ---- utils/spline.c ---- */

const SMOOTHNESS: f64 = 1.0;

fn mid_point(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
    ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
}

fn third_point(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
    ((2.0 * x0 + x1) / 3.0, (2.0 * y0 + y1) / 3.0)
}

fn can_approx_with_line(x0: f64, y0: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> bool {
    // actually 4 times the area
    let mut triangle_area = x0 * y2 - x2 * y0 + x2 * y3 - x3 * y2 + x3 * y0 - x0 * y3;
    triangle_area *= triangle_area;
    let dx = x3 - x0;
    let dy = y3 - y0;
    let side_squared = dx * dx + dy * dy;
    triangle_area <= SMOOTHNESS * side_squared
}

/// utils/spline.c: a closed Catmull-Rom-style spline through control points,
/// flattened into an integer point list. Shared by attraction and goop.
pub struct Spline {
    pub control_x: Vec<f64>,
    pub control_y: Vec<f64>,
    pub points: Vec<(i32, i32)>,
}

impl Spline {
    pub fn new(n_controls: usize) -> Self {
        Spline {
            control_x: vec![0.0; n_controls],
            control_y: vec![0.0; n_controls],
            points: Vec::new(),
        }
    }

    fn add_line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        if self.points.is_empty() {
            self.points.push((x0 as i32, y0 as i32));
        }
        self.points.push((x1 as i32, y1 as i32));
    }

    #[allow(clippy::too_many_arguments)]
    fn add_bezier_arc(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x3: f64,
        y3: f64,
    ) {
        let (midx01, midy01) = mid_point(x0, y0, x1, y1);
        let (midx12, midy12) = mid_point(x1, y1, x2, y2);
        let (midx23, midy23) = mid_point(x2, y2, x3, y3);
        let (midlsegx, midlsegy) = mid_point(midx01, midy01, midx12, midy12);
        let (midrsegx, midrsegy) = mid_point(midx12, midy12, midx23, midy23);
        let (cx, cy) = mid_point(midlsegx, midlsegy, midrsegx, midrsegy);

        if can_approx_with_line(x0, y0, midlsegx, midlsegy, cx, cy) {
            self.add_line(x0, y0, cx, cy);
        } else if (midx01 != x1)
            || (midy01 != y1)
            || (midlsegx != x2)
            || (midlsegy != y2)
            || (cx != x3)
            || (cy != y3)
        {
            self.add_bezier_arc(x0, y0, midx01, midy01, midlsegx, midlsegy, cx, cy);
        }

        if can_approx_with_line(cx, cy, midx23, midy23, x3, y3) {
            self.add_line(cx, cy, x3, y3);
        } else if (cx != x0)
            || (cy != y0)
            || (midrsegx != x1)
            || (midrsegy != y1)
            || (midx23 != x2)
            || (midy23 != y2)
        {
            self.add_bezier_arc(cx, cy, midrsegx, midrsegy, midx23, midy23, x3, y3);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn calc_section(
        &mut self,
        cminus1x: f64,
        cminus1y: f64,
        cx: f64,
        cy: f64,
        cplus1x: f64,
        cplus1y: f64,
        cplus2x: f64,
        cplus2y: f64,
    ) {
        let (p1x, p1y) = third_point(cx, cy, cplus1x, cplus1y);
        let (p2x, p2y) = third_point(cplus1x, cplus1y, cx, cy);
        let (tempx, tempy) = third_point(cx, cy, cminus1x, cminus1y);
        let (p0x, p0y) = mid_point(tempx, tempy, p1x, p1y);
        let (tempx, tempy) = third_point(cplus1x, cplus1y, cplus2x, cplus2y);
        let (p3x, p3y) = mid_point(tempx, tempy, p2x, p2y);
        self.add_bezier_arc(p0x, p0y, p1x, p1y, p2x, p2y, p3x, p3y);
    }

    pub fn compute_closed_spline(&mut self) {
        self.points.clear();
        let n = self.control_x.len();
        if n < 3 {
            return;
        }
        let cx = |i: usize| self.control_x[i];
        let cy = |i: usize| self.control_y[i];

        let args = (
            cx(n - 1),
            cy(n - 1),
            cx(0),
            cy(0),
            cx(1),
            cy(1),
            cx(2),
            cy(2),
        );
        self.calc_section(
            args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7,
        );

        for i in 1..n - 2 {
            let args = (
                self.control_x[i - 1],
                self.control_y[i - 1],
                self.control_x[i],
                self.control_y[i],
                self.control_x[i + 1],
                self.control_y[i + 1],
                self.control_x[i + 2],
                self.control_y[i + 2],
            );
            self.calc_section(
                args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7,
            );
        }

        let i = n - 2;
        let args = (
            self.control_x[i - 1],
            self.control_y[i - 1],
            self.control_x[i],
            self.control_y[i],
            self.control_x[i + 1],
            self.control_y[i + 1],
            self.control_x[0],
            self.control_y[0],
        );
        self.calc_section(
            args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7,
        );
        let args = (
            self.control_x[i],
            self.control_y[i],
            self.control_x[i + 1],
            self.control_y[i + 1],
            self.control_x[0],
            self.control_y[0],
            self.control_x[1],
            self.control_y[1],
        );
        self.calc_section(
            args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7,
        );
    }
}

/* ---- CapRound wide lines, like XSetLineAttributes(width, CapRound) ---- */

fn fill_disc(buffer: &mut [u8], width: u32, height: u32, cx: i32, cy: i32, r: i32, color: Color) {
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                put_pixel(buffer, width, height, cx + dx, cy + dy, color);
            }
        }
    }
}

/// A line of width `lw` drawn with round caps (Bresenham path stamped with
/// filled discs), like XSetLineAttributes(lw, CapRound).
#[allow(clippy::too_many_arguments)]
pub fn draw_thick_line(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    lw: i32,
    color: Color,
) {
    if lw <= 1 {
        draw_line(buffer, width, height, x0, y0, x1, y1, color);
        return;
    }
    // Same direction-canonicalization as draw_line (see comment there).
    let (mut x0, mut y0, x1, y1) = if (x1, y1) < (x0, y0) {
        (x1, y1, x0, y0)
    } else {
        (x0, y0, x1, y1)
    };
    let r = lw / 2;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        fill_disc(buffer, width, height, x0, y0, r, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

#[cfg(test)]
mod ramp_tests {
    use super::*;

    /// The two ramps look interchangeable and are not. Merging them silently
    /// shifts moire and popsquares hues by a degree on descending ramps.
    #[test]
    fn ramps_are_not_interchangeable() {
        let stepped = make_color_ramp_stepped_hue(180, 0.3, 0.7, 10, 0.9, 0.2, 3, false);
        let summed = make_color_ramp(180, 0.3, 0.7, 10, 0.9, 0.2, 3, false);
        assert_ne!(stepped, summed);
    }
}
