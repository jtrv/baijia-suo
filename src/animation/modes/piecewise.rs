/* piecewise, 21jan2003
 * Geoffrey Irving <irving@caltech.edu>
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Rust port of xscreensaver's piecewise.c.  The pointer-based splay trees
 * are arena-backed (indices instead of pointers); XArc drawing is replaced
 * by a polyline arc rasterizer.
 */

use rand::RngExt;

use crate::animation::primitives::{draw_line, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const X_PI: i32 = 180 * 64;

const START: u8 = 0;
const CROSS: u8 = 1;
const FINISH: u8 = 2;

/* C's frand(f): a random double in [0, f) (negative f gives negative
   values, which init_circles relies on for tiny windows) */
fn frand(rng: &mut impl RngExt, f: f64) -> f64 {
    rng.random::<f64>() * f
}

struct Circle {
    r: i32,   /* radius */
    x: f64,   /* position */
    y: f64,
    dx: f64,  /* velocity */
    dy: f64,
    visible: bool, /* default visibility */
    ints: Vec<i32>, /* sorted intersection list (c->i / c->ni) */
}

struct Fringe {
    l: Option<usize>, /* left and right children for splay trees */
    r: Option<usize>,
    c: usize,      /* associated circle */
    side: usize,   /* 0 for lo, 1 for hi */
    ints: Vec<i32>, /* intersection list (f->i / f->ni) */
}

struct Event {
    l: Option<usize>, /* left and right children for splay tree */
    r: Option<usize>,
    kind: u8,
    x: f64,
    y: f64,
    lo: usize, /* fringe indices */
    hi: usize,
}

/******** splaying code
 *
 * Top-down splay routine.  Reference:
 *   "Self-adjusting Binary Search Trees", Sleator and Tarjan,
 *   JACM Volume 32, No 3, July 1985, pp 652-686.
 *
 * Transcribed from the C with `tree **lr` / `tree **rl` slot pointers
 * replaced by Option<usize>: None is the local l/r root variable, Some(n)
 * is node n's right (for lr) or left (for rl) child slot. */

trait SplayNode {
    fn l(&self) -> Option<usize>;
    fn r(&self) -> Option<usize>;
    fn set_l(&mut self, v: Option<usize>);
    fn set_r(&mut self, v: Option<usize>);
}

impl SplayNode for Fringe {
    fn l(&self) -> Option<usize> {
        self.l
    }
    fn r(&self) -> Option<usize> {
        self.r
    }
    fn set_l(&mut self, v: Option<usize>) {
        self.l = v;
    }
    fn set_r(&mut self, v: Option<usize>) {
        self.r = v;
    }
}

impl SplayNode for Event {
    fn l(&self) -> Option<usize> {
        self.l
    }
    fn r(&self) -> Option<usize> {
        self.r
    }
    fn set_l(&mut self, v: Option<usize>) {
        self.l = v;
    }
    fn set_r(&mut self, v: Option<usize>) {
        self.r = v;
    }
}

fn splay<T: SplayNode>(
    a: &mut [T],
    c: &mut dyn FnMut(&[T], usize) -> i32,
    t: Option<usize>,
) -> Option<usize> {
    let mut x = t?;
    let mut l: Option<usize> = None;
    let mut r: Option<usize> = None;
    let mut lr: Option<usize> = None;
    let mut rl: Option<usize> = None;

    macro_rules! set_lr {
        ($v:expr) => {
            match lr {
                Some(n) => a[n].set_r($v),
                None => l = $v,
            }
        };
    }
    macro_rules! set_rl {
        ($v:expr) => {
            match rl {
                Some(n) => a[n].set_l($v),
                None => r = $v,
            }
        };
    }

    loop {
        let v = c(a, x);
        if v == 0 {
            break; /*** success ***/
        } else if v < 0 {
            let y = match a[x].l() {
                None => break, /*** trivial ***/
                Some(y) => y,
            };
            let vv = c(a, y);
            if vv == 0 {
                set_rl!(Some(x)); /*** zig ***/
                rl = Some(x);
                x = y;
                break;
            } else if vv < 0 {
                match a[y].l() {
                    None => {
                        set_rl!(Some(x)); /*** zig ***/
                        rl = Some(x);
                        x = y;
                        break;
                    }
                    Some(z) => {
                        let yr = a[y].r(); /*** zig-zig ***/
                        a[x].set_l(yr);
                        a[y].set_r(Some(x));
                        set_rl!(Some(y));
                        rl = Some(y);
                        x = z;
                    }
                }
            } else {
                match a[y].r() {
                    None => {
                        set_rl!(Some(x)); /*** zig ***/
                        rl = Some(x);
                        x = y;
                        break;
                    }
                    Some(z) => {
                        set_lr!(Some(y)); /*** zig-zag ***/
                        lr = Some(y);
                        set_rl!(Some(x));
                        rl = Some(x);
                        x = z;
                    }
                }
            }
        } else {
            let y = match a[x].r() {
                None => break, /*** trivial ***/
                Some(y) => y,
            };
            let vv = c(a, y);
            if vv == 0 {
                set_lr!(Some(x)); /*** zig ***/
                lr = Some(x);
                x = y;
                break;
            } else if vv > 0 {
                match a[y].r() {
                    None => {
                        set_lr!(Some(x)); /*** zig ***/
                        lr = Some(x);
                        x = y;
                        break;
                    }
                    Some(z) => {
                        let yl = a[y].l(); /*** zig-zig ***/
                        a[x].set_r(yl);
                        a[y].set_l(Some(x));
                        set_lr!(Some(y));
                        lr = Some(y);
                        x = z;
                    }
                }
            } else {
                match a[y].l() {
                    None => {
                        set_lr!(Some(x)); /*** zig ***/
                        lr = Some(x);
                        x = y;
                        break;
                    }
                    Some(z) => {
                        set_rl!(Some(y)); /*** zig-zag ***/
                        rl = Some(y);
                        set_lr!(Some(x));
                        lr = Some(x);
                        x = z;
                    }
                }
            }
        }
    }

    /* completion */
    let xl = a[x].l();
    set_lr!(xl);
    a[x].set_l(l);
    let xr = a[x].r();
    set_rl!(xr);
    a[x].set_r(r);
    Some(x)
}

fn splay_min<T: SplayNode>(a: &mut [T], t: Option<usize>) -> Option<usize> {
    let mut x = t?;
    let mut r: Option<usize> = None;
    let mut rl: Option<usize> = None;

    macro_rules! set_rl {
        ($v:expr) => {
            match rl {
                Some(n) => a[n].set_l($v),
                None => r = $v,
            }
        };
    }

    loop {
        let y = match a[x].l() {
            None => break, /*** trivial ***/
            Some(y) => y,
        };
        match a[y].l() {
            None => {
                set_rl!(Some(x)); /*** zig ***/
                rl = Some(x);
                x = y;
                break;
            }
            Some(z) => {
                let yr = a[y].r(); /*** zig-zig ***/
                a[x].set_l(yr);
                a[y].set_r(Some(x));
                set_rl!(Some(y));
                rl = Some(y);
                x = z;
            }
        }
    }

    a[x].set_l(None);
    let xr = a[x].r();
    set_rl!(xr);
    a[x].set_r(r);
    Some(x)
}

fn splay_max<T: SplayNode>(a: &mut [T], t: Option<usize>) -> Option<usize> {
    let mut x = t?;
    let mut l: Option<usize> = None;
    let mut lr: Option<usize> = None;

    macro_rules! set_lr {
        ($v:expr) => {
            match lr {
                Some(n) => a[n].set_r($v),
                None => l = $v,
            }
        };
    }

    loop {
        let y = match a[x].r() {
            None => break, /*** trivial ***/
            Some(y) => y,
        };
        match a[y].r() {
            None => {
                set_lr!(Some(x)); /*** zig ***/
                lr = Some(x);
                x = y;
                break;
            }
            Some(z) => {
                let yl = a[y].l(); /*** zig-zig ***/
                a[x].set_r(yl);
                a[y].set_l(Some(x));
                set_lr!(Some(y));
                lr = Some(y);
                x = z;
            }
        }
    }

    let xl = a[x].l();
    set_lr!(xl);
    a[x].set_l(l);
    a[x].set_r(None);
    Some(x)
}

/******** circles and fringe */

fn fringe_x(f: &Fringe, circles: &[Circle], y: f64) -> f64 {
    let c = &circles[f.c];
    let dy = c.y - y;
    let d = ((c.r * c.r) as f64 - dy * dy).sqrt();
    if f.side != 0 {
        c.x + d
    } else {
        c.x - d
    }
}

fn fringe_add_intersection(fringes: &mut [Fringe], circles: &[Circle], fi: usize, x: f64, y: f64) {
    let c = &circles[fringes[fi].c];
    let a = ((y - c.y).atan2(x - c.x) * X_PI as f64 / std::f64::consts::PI).round() as i32;
    fringes[fi].ints.push(a);
}

/* this is a hack, but I guess that's what I writing anyways */
fn tweak_circle(c: &mut Circle, rng: &mut impl RngExt) {
    c.x += frand(rng, 2.0) - 1.0;
    c.y += frand(rng, 1.0) + 0.1;
}

fn move_circle(c: &mut Circle, w: i32, h: i32) {
    c.x += c.dx;
    if c.x < c.r as f64 {
        c.x = c.r as f64;
        c.dx = -c.dx;
    } else if c.x >= (w - c.r) as f64 {
        c.x = (w - 1 - c.r) as f64;
        c.dx = -c.dx;
    }
    c.y += c.dy;
    if c.y < c.r as f64 {
        c.y = c.r as f64;
        c.dy = -c.dy;
    } else if c.y >= (h - c.r) as f64 {
        c.y = (h - 1 - c.r) as f64;
        c.dy = -c.dy;
    }
}

/******** event queue */

fn event_insert(events: &mut [Event], eq: &mut Option<usize>, e: usize) {
    if eq.is_none() {
        events[e].l = None;
        events[e].r = None;
        *eq = Some(e);
        return;
    }

    let cut_y = events[e].y;
    *eq = splay(
        events,
        &mut |a: &[Event], i: usize| {
            if cut_y == a[i].y {
                0
            } else if cut_y < a[i].y {
                -1
            } else {
                1
            }
        },
        *eq,
    );
    // splay() of a non-empty tree always returns a root (guarded above); if
    // that invariant ever breaks, insert e as the root instead of panicking.
    let Some(root) = *eq else {
        events[e].l = None;
        events[e].r = None;
        *eq = Some(e);
        return;
    };

    if events[e].y == events[root].y {
        let (elo, ehi) = (events[e].lo, events[e].hi);
        let (rlo, rhi) = (events[root].lo, events[root].hi);
        if !((elo == rlo && ehi == rhi) || (elo == rhi && ehi == rlo)) {
            events[e].l = events[root].l;
            events[e].r = None; /* doing this instead of dying might be dangerous */
            events[root].l = Some(e);
        }
        /* else: duplicate event, dropped (arena is cleared per sweep) */
    } else if events[e].y < events[root].y {
        events[e].l = events[root].l;
        events[e].r = Some(root);
        events[root].l = None;
        *eq = Some(e);
    } else {
        events[e].l = Some(root);
        events[e].r = events[root].r;
        events[root].r = None;
        *eq = Some(e);
    }
}

/* circle ci's lo fringe is 2*ci, hi fringe is 2*ci+1 */
fn circle_start_event(events: &mut Vec<Event>, eq: &mut Option<usize>, circles: &[Circle], ci: usize) {
    let c = &circles[ci];
    events.push(Event {
        l: None,
        r: None,
        kind: START,
        x: c.x,
        y: c.y - c.r as f64,
        lo: 2 * ci,
        hi: 2 * ci + 1,
    });
    let e = events.len() - 1;
    event_insert(events, eq, e);
}

fn circle_finish_event(events: &mut Vec<Event>, eq: &mut Option<usize>, circles: &[Circle], ci: usize) {
    let c = &circles[ci];
    events.push(Event {
        l: None,
        r: None,
        kind: FINISH,
        x: c.x,
        y: c.y + c.r as f64,
        lo: 2 * ci,
        hi: 2 * ci + 1,
    });
    let e = events.len() - 1;
    event_insert(events, eq, e);
}

fn event_next(events: &mut [Event], eq: &mut Option<usize>) -> Option<usize> {
    let e = splay_min(events, *eq)?;
    *eq = events[e].r;
    Some(e)
}

/******** fringe intersection */

fn fringe_intersect(
    events: &mut Vec<Event>,
    eq: &mut Option<usize>,
    circles: &[Circle],
    fringes: &[Fringe],
    y: f64,
    lo: usize,
    hi: usize,
) {
    let (lc, hc) = (fringes[lo].c, fringes[hi].c);
    if lc == hc {
        return;
    }
    let (lcc, hcc) = (&circles[lc], &circles[hc]);

    let dx = hcc.x - lcc.x;
    let dy = hcc.y - lcc.y;
    let mut sd = dx * dx + dy * dy;

    if sd == 0.0 {
        return;
    }

    let rs = (hcc.r + lcc.r) as f64;
    let rd = (hcc.r - lcc.r) as f64;
    let d = (rd * rd - sd) * (sd - rs * rs);

    if d <= 0.0 {
        return;
    }

    sd = 0.5 / sd;
    let rp = rs * rd;
    let sqd = d.sqrt();
    let sx = (lcc.x + hcc.x) / 2.0;
    let sy = (lcc.y + hcc.y) / 2.0;
    let x1 = sx + sd * (dy * sqd - dx * rp);
    let y1 = sy - sd * (dx * sqd + dy * rp);
    let x2 = sx - sd * (dy * sqd + dx * rp);
    let y2 = sy + sd * (dx * sqd - dy * rp);

    let check = |xi: f64, yi: f64| -> bool {
        y <= yi
            && ((xi < lcc.x) ^ (fringes[lo].side != 0))
            && ((xi < hcc.x) ^ (fringes[hi].side != 0))
    };

    let add_cross = |events: &mut Vec<Event>, eq: &mut Option<usize>, xi: f64, yi: f64, ilo: usize, ihi: usize| {
        events.push(Event {
            l: None,
            r: None,
            kind: CROSS,
            x: xi,
            y: yi,
            lo: ilo,
            hi: ihi,
        });
        let e = events.len() - 1;
        event_insert(events, eq, e);
    };

    if check(x1, y1) {
        if check(x2, y2) {
            if y1 < y2 {
                add_cross(events, eq, x1, y1, lo, hi);
                add_cross(events, eq, x2, y2, hi, lo);
            } else {
                add_cross(events, eq, x1, y1, hi, lo);
                add_cross(events, eq, x2, y2, lo, hi);
            }
        } else {
            add_cross(events, eq, x1, y1, lo, hi);
        }
    } else if check(x2, y2) {
        add_cross(events, eq, x2, y2, lo, hi);
    }
}

/******** fringe trees and event handling */

fn check_lo(
    events: &mut Vec<Event>,
    eq: &mut Option<usize>,
    circles: &[Circle],
    fringes: &mut [Fringe],
    y: f64,
    f: Option<usize>,
    hi: usize,
) -> Option<usize> {
    let f = splay_max(fringes, f);
    if let Some(fi) = f {
        fringe_intersect(events, eq, circles, fringes, y, fi, hi);
    }
    f
}

fn check_hi(
    events: &mut Vec<Event>,
    eq: &mut Option<usize>,
    circles: &[Circle],
    fringes: &mut [Fringe],
    y: f64,
    lo: usize,
    f: Option<usize>,
) -> Option<usize> {
    let f = splay_min(fringes, f);
    if let Some(fi) = f {
        fringe_intersect(events, eq, circles, fringes, y, lo, fi);
    }
    f
}

#[allow(clippy::too_many_arguments)]
fn fringe_start(
    events: &mut Vec<Event>,
    eq: &mut Option<usize>,
    circles: &[Circle],
    fringes: &mut [Fringe],
    f: Option<usize>,
    x: f64,
    y: f64,
    lo: usize,
    hi: usize,
) -> (Option<usize>, bool) {
    /* second return value: true when the new circle must be tweaked and
       restarted (degenerate case; caller handles the tweak) */
    let fi = match f {
        None => {
            circle_finish_event(events, eq, circles, fringes[lo].c);
            fringes[lo].l = None;
            fringes[lo].r = Some(hi);
            fringes[hi].l = None;
            fringes[hi].r = None;
            return (Some(lo), false);
        }
        Some(_) => {
            let f = splay(
                fringes,
                &mut |a: &[Fringe], i: usize| {
                    let fx = fringe_x(&a[i], circles, y);
                    if x == fx {
                        0
                    } else if x < fx {
                        -1
                    } else {
                        1
                    }
                },
                f,
            );
            match f {
                Some(fi) => fi,
                // splay() of a non-empty tree always returns a root; degrade
                // to an empty fringe tree rather than panic if it ever fails.
                None => return (None, false),
            }
        }
    };

    let sx = fringe_x(&fringes[fi], circles, y);
    if x == sx {
        /* time to cheat my way out of handling degeneracies */
        (Some(fi), true)
    } else if x < sx {
        circle_finish_event(events, eq, circles, fringes[lo].c);
        let fl = check_lo(events, eq, circles, fringes, y, fringes[fi].l, lo);
        fringes[fi].l = fl;
        fringe_intersect(events, eq, circles, fringes, y, hi, fi);
        fringes[lo].l = fringes[fi].l;
        fringes[lo].r = Some(fi);
        fringes[fi].l = Some(hi);
        fringes[hi].l = None;
        fringes[hi].r = None;
        (Some(lo), false)
    } else {
        circle_finish_event(events, eq, circles, fringes[lo].c);
        fringe_intersect(events, eq, circles, fringes, y, fi, lo);
        let fr = check_hi(events, eq, circles, fringes, y, hi, fringes[fi].r);
        fringes[fi].r = fr;
        fringes[hi].r = fringes[fi].r;
        fringes[hi].l = Some(fi);
        fringes[fi].r = Some(lo);
        fringes[lo].l = None;
        fringes[lo].r = None;
        (Some(hi), false)
    }
}

fn fringe_double_splay(
    circles: &[Circle],
    fringes: &mut [Fringe],
    f: Option<usize>,
    x: f64,
    y: f64,
    lo: usize,
    hi: usize,
) -> bool {
    let f = splay(
        fringes,
        &mut |a: &[Fringe], i: usize| {
            if i == lo || i == hi {
                return 0;
            }
            let fx = fringe_x(&a[i], circles, y);
            if x == fx {
                0
            } else if x < fx {
                -1
            } else {
                1
            }
        },
        f,
    );

    match f {
        Some(root) if root == lo => {
            let nr = splay_min(fringes, fringes[lo].r);
            fringes[lo].r = nr;
            nr == Some(hi)
        }
        Some(root) if root == hi => {
            let nl = splay_max(fringes, fringes[hi].l);
            fringes[hi].l = nl;
            nl == Some(lo)
        }
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn fringe_cross(
    events: &mut Vec<Event>,
    eq: &mut Option<usize>,
    circles: &[Circle],
    fringes: &mut [Fringe],
    f: Option<usize>,
    x: f64,
    y: f64,
    lo: usize,
    hi: usize,
) -> Result<Option<usize>, ()> {
    if !fringe_double_splay(circles, fringes, f, x, y, lo, hi) {
        return Err(()); /* PANIC */
    }
    let l = check_lo(events, eq, circles, fringes, y, fringes[lo].l, hi);
    let r = check_hi(events, eq, circles, fringes, y, lo, fringes[hi].r);
    fringes[lo].l = Some(hi);
    fringes[lo].r = r;
    fringes[hi].l = l;
    fringes[hi].r = None;
    Ok(Some(lo))
}

#[allow(clippy::too_many_arguments)]
fn fringe_finish(
    events: &mut Vec<Event>,
    eq: &mut Option<usize>,
    circles: &[Circle],
    fringes: &mut [Fringe],
    f: Option<usize>,
    x: f64,
    y: f64,
    lo: usize,
    hi: usize,
) -> Result<Option<usize>, ()> {
    if !fringe_double_splay(circles, fringes, f, x, y, lo, hi) {
        return Err(()); /* PANIC */
    }
    if fringes[lo].l.is_none() {
        Ok(fringes[hi].r)
    } else if fringes[hi].r.is_none() {
        Ok(fringes[lo].l)
    } else {
        // Both sides checked non-empty above; treat a miss like the C PANIC
        // path (abort the sweep) instead of actually panicking.
        let Some(ll) = splay_max(fringes, fringes[lo].l) else {
            return Err(());
        };
        fringes[lo].l = Some(ll);
        let Some(hr) = splay_min(fringes, fringes[hi].r) else {
            return Err(());
        };
        fringes[hi].r = Some(hr);
        fringe_intersect(events, eq, circles, fringes, y, ll, hr);
        fringes[ll].r = Some(hr);
        fringes[hr].l = None;
        Ok(Some(ll))
    }
}

/******** hsv (exact port of utils/hsv.c, for make_color_loop hues) */

fn hsv_to_color(h: f64, s: f64, v: f64) -> Color {
    let hh = (h / 60.0) % 6.0;
    let i = hh.floor();
    let f = hh - i;
    let p1 = v * (1.0 - s);
    let p2 = v * (1.0 - s * f);
    let p3 = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i as i32 {
        0 => (v, p3, p1),
        1 => (p2, v, p1),
        2 => (p1, v, p3),
        3 => (p1, p2, v),
        4 => (p3, p1, v),
        _ => (v, p1, p2),
    };
    Color::new(
        255,
        (r * 255.0 + 0.5) as u8,
        (g * 255.0 + 0.5) as u8,
        (b * 255.0 + 0.5) as u8,
    )
}

/******** arc drawing (replaces the XDrawArcs buffer) */

struct Arc {
    cx: f64,
    cy: f64,
    r: f64,
    a1: i32, /* start angle, X_PI units (screen atan2 convention) */
    a2: i32, /* end angle */
}

fn draw_arc(buffer: &mut [u8], width: u32, height: u32, arc: &Arc, color: Color) {
    let ra1 = arc.a1 as f64 * std::f64::consts::PI / X_PI as f64;
    let ra2 = arc.a2 as f64 * std::f64::consts::PI / X_PI as f64;
    let sweep = ra2 - ra1;
    let steps = ((sweep.abs() * arc.r).ceil() as i32).max(1);
    // Advance the point with the 2D rotation recurrence instead of a fresh
    // sin/cos per step (thousands of trig pairs per frame across ~32
    // circles). Drift over a full circle at f64 is far below a pixel.
    let (step_sin, step_cos) = (sweep / steps as f64).sin_cos();
    let (mut uy, mut ux) = ra1.sin_cos();
    let mut px = (arc.cx + arc.r * ux).round() as i32;
    let mut py = (arc.cy + arc.r * uy).round() as i32;
    for _ in 1..=steps {
        let nux = ux * step_cos - uy * step_sin;
        uy = ux * step_sin + uy * step_cos;
        ux = nux;
        let nx = (arc.cx + arc.r * ux).round() as i32;
        let ny = (arc.cy + arc.r * uy).round() as i32;
        draw_line(buffer, width, height, px, py, nx, ny, color);
        px = nx;
        py = ny;
    }
}

/******** toplevel */

pub struct Piecewise {
    width: u32,
    height: u32,
    delay_us: u64,

    count: usize,
    ncolors: usize,
    color_index: usize,
    color_iterations: usize,
    iterations: usize,
    colors: Vec<Color>,

    circles: Vec<Circle>,
    fringes: Vec<Fringe>,
    events: Vec<Event>,

    /* arcs computed in tick(), rasterized in render() */
    arcs: Vec<Arc>,
    frame_color: Color,
}

impl Piecewise {
    fn init_circles(&mut self, n: usize, w: i32, h: i32) {
        let mut rng = rand::rng();
        let speed = 15.0; /* *speed: 15 */
        let minradius = 0.05; /* .minradius: 0.05 */
        let maxradius = 0.2; /* .maxradius: 0.2 */

        let r0 = (minradius * h as f64).ceil() as i32;
        let dr = (maxradius * h as f64).floor() as i32 - r0 + 1;

        self.circles.clear();
        self.fringes.clear();
        for i in 0..n {
            let r = r0 + if dr > 0 { rng.random_range(0..dr) } else { 0 };
            let x = r as f64 + frand(&mut rng, (w - 1 - 2 * r) as f64);
            let y = r as f64 + frand(&mut rng, (h - 1 - 2 * r) as f64);
            let a = frand(&mut rng, 2.0 * std::f64::consts::PI);
            let v = (1.0 + frand(&mut rng, 0.5)) * speed / 10.0;
            self.circles.push(Circle {
                r,
                x,
                y,
                dx: v * a.cos(),
                dy: v * a.sin(),
                visible: rng.random::<bool>(),
                ints: Vec::new(),
            });
            self.fringes.push(Fringe {
                l: None,
                r: None,
                c: i,
                side: 0,
                ints: Vec::new(),
            });
            self.fringes.push(Fringe {
                l: None,
                r: None,
                c: i,
                side: 1,
                ints: Vec::new(),
            });
        }
    }

    /******** plane sweep */
    fn sweep(&mut self) {
        let n = self.count;
        'restart: loop {
            self.events.clear();
            let mut eq: Option<usize> = None;
            for i in 0..n {
                circle_start_event(&mut self.events, &mut eq, &self.circles, i);
            }
            let mut f: Option<usize> = None;

            while let Some(ei) = event_next(&mut self.events, &mut eq) {
                let (kind, ex, ey, elo, ehi) = {
                    let e = &self.events[ei];
                    (e.kind, e.x, e.y, e.lo, e.hi)
                };
                match kind {
                    START => {
                        let (nf, degenerate) = fringe_start(
                            &mut self.events,
                            &mut eq,
                            &self.circles,
                            &mut self.fringes,
                            f,
                            ex,
                            ey,
                            elo,
                            ehi,
                        );
                        f = nf;
                        if degenerate {
                            let mut rng = rand::rng();
                            let ci = self.fringes[elo].c;
                            tweak_circle(&mut self.circles[ci], &mut rng);
                            circle_start_event(&mut self.events, &mut eq, &self.circles, ci);
                        }
                    }
                    CROSS => {
                        match fringe_cross(
                            &mut self.events,
                            &mut eq,
                            &self.circles,
                            &mut self.fringes,
                            f,
                            ex,
                            ey,
                            elo,
                            ehi,
                        ) {
                            Ok(nf) => f = nf,
                            Err(()) => {
                                self.panic_reset();
                                continue 'restart;
                            }
                        }
                        fringe_add_intersection(&mut self.fringes, &self.circles, elo, ex, ey);
                        fringe_add_intersection(&mut self.fringes, &self.circles, ehi, ex, ey);
                    }
                    _ => {
                        match fringe_finish(
                            &mut self.events,
                            &mut eq,
                            &self.circles,
                            &mut self.fringes,
                            f,
                            ex,
                            ey,
                            elo,
                            ehi,
                        ) {
                            Ok(nf) => f = nf,
                            Err(()) => {
                                self.panic_reset();
                                continue 'restart;
                            }
                        }
                    }
                }
            }
            break;
        }
    }

    /* the CHECK_PANIC restart path: perturb everything and try again */
    fn panic_reset(&mut self) {
        let mut rng = rand::rng();
        for i in 0..self.count {
            tweak_circle(&mut self.circles[i], &mut rng);
            self.fringes[2 * i].ints.clear();
            self.fringes[2 * i + 1].ints.clear();
        }
    }

    /******** circle drawing */

    fn adjust_circle_visibility(&mut self, ci: usize) {
        let lo_ints = std::mem::take(&mut self.fringes[2 * ci].ints);
        let hi_ints = std::mem::take(&mut self.fringes[2 * ci + 1].ints);
        let c = &mut self.circles[ci];

        let n = lo_ints.len() + hi_ints.len();
        let mut inv: Vec<i32> = Vec::with_capacity(n);
        inv.extend_from_slice(&hi_ints);
        for &v in lo_ints.iter().rev() {
            inv.push(if v > 0 { v } else { v + 2 * X_PI });
        }

        let mut i = 0usize;
        let mut j = 0usize;
        let mut a: i32 = 0;
        while i < n && j < c.ints.len() {
            /* whee */
            a = (if inv[i] < c.ints[j] {
                let v = inv[i];
                i += 1;
                v
            } else {
                let v = c.ints[j];
                j += 1;
                v
            }) - a;
        }
        while i < n {
            a = inv[i] - a;
            i += 1;
        }
        while j < c.ints.len() {
            a = c.ints[j] - a;
            j += 1;
        }

        if a > X_PI {
            c.visible = !c.visible;
        }
        c.ints = inv;
    }

    fn record_circle_arcs(&mut self, ci: usize) {
        self.adjust_circle_visibility(ci);
        let c = &self.circles[ci];

        let xi = (c.x - c.r as f64).round();
        let yi = (c.y - c.r as f64).round();
        let r = c.r as f64;
        let visible = c.visible;
        let ints = c.ints.clone();

        let mut arc = |p: usize, a1: i32, a2: i32| {
            if ((p & 1) == 1) != visible {
                self.arcs.push(Arc {
                    cx: xi + r,
                    cy: yi + r,
                    r,
                    a1,
                    a2,
                });
            }
        };

        if ints.is_empty() {
            arc(0, 0, 2 * X_PI);
        } else {
            arc(0, ints[ints.len() - 1], ints[0] + 2 * X_PI);
        }
        for i in 1..ints.len() {
            arc(i, ints[i - 1], ints[i]);
        }
    }
}

impl Animation for Piecewise {
    fn new(config: &AnimConfig) -> Self {
        let mut st = Piecewise {
            width: 0,
            height: 0,
            delay_us: 10_000, /* hack default *delay: 10000 */
            count: 0,
            ncolors: 0,
            color_index: 0,
            color_iterations: 10, /* 100 / colorspeed(10) */
            iterations: 0,
            colors: Vec::new(),
            circles: Vec::new(),
            fringes: Vec::new(),
            events: Vec::new(),
            arcs: Vec::new(),
            frame_color: Color::new(255, 255, 255, 255),
        };
        st.reset(config);
        st
    }

    fn tick(&mut self) {
        self.arcs.clear();
        self.frame_color = self.colors[self.color_index];

        self.sweep();
        let (w, h) = (self.width as i32, self.height as i32);
        for i in 0..self.count {
            self.record_circle_arcs(i);
            move_circle(&mut self.circles[i], w, h);
        }

        self.iterations += 1;
        if self.iterations % self.color_iterations == 0 {
            self.color_index = (self.color_index + 1) % self.ncolors;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        for arc in &self.arcs {
            draw_arc(buffer, width, height, arc, self.frame_color);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;
        self.delay_us = 10_000;

        /* hack defaults: count 32, ncolors 256, colorspeed 10 */
        self.count = if config.count > 0 { config.count as usize } else { 32 };
        self.ncolors = if config.ncolors > 1 {
            config.ncolors as usize
        } else {
            256
        };
        let colorspeed = 10usize;
        self.color_iterations = 100usize.checked_div(colorspeed).unwrap_or(100000);
        if self.color_iterations == 0 {
            self.color_iterations = 1;
        }

        /* make_color_loop through hues 0 -> 120 -> 240 -> 0 at s=1 v=1
           is a plain hue wheel */
        self.colors = (0..self.ncolors)
            .map(|i| hsv_to_color(360.0 * i as f64 / self.ncolors as f64, 1.0, 1.0))
            .collect();
        self.color_index = rng.random_range(0..self.ncolors);

        self.init_circles(self.count, config.width as i32, config.height as i32);
        self.events.clear();
        self.arcs.clear();
        self.iterations = 0;
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
