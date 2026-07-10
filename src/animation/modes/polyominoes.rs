/* polyominoes --- Shows attempts to place polyominoes into a rectangle
 *
 * Copyright (c) 2000 by Stephen Montgomery-Smith <stephen@math.missouri.edu>
 *
 * Permission to use, copy, modify, and distribute this software and its
 * documentation for any purpose and without fee is hereby granted,
 * provided that the above copyright notice appear in all copies and that
 * both that copyright notice and this permission notice appear in
 * supporting documentation.
 *
 * This file is provided AS IS with no warranties of any kind.  The author
 * shall have no liability with respect to the infringement of copyrights,
 * trade secrets or any patents by this file or any part thereof.  In no
 * event will the author be liable for any lost revenue or profits or
 * other special, indirect and consequential damages.
 *
 * Rust port of xscreensaver's polyominoes.c (xlockmore).
 */

use rand::Rng;

use crate::animation::primitives::{draw_line, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};

/// One puzzle-piece spec: (points, allowed transforms, max piece width).
type PolyominoSpec<'a> = (&'a [(i32, i32)], &'a [i32], i32);

// Defaults from the DEFAULTS table in polyominoes.c.
const DEF_DELAY_US: u64 = 10_000;
const DEF_CYCLES: i32 = 2000;
const DEF_NCOLORS: i32 = 64;

/* Bitmap index bits: a set bit indicates that an edge or corner is required. */
const LEFT: usize = 1 << 0;
const RIGHT: usize = 1 << 1;
const UP: usize = 1 << 2;
const DOWN: usize = 1 << 3;
const LEFT_UP: usize = 1 << 4;
const LEFT_DOWN: usize = 1 << 5;
const RIGHT_UP: usize = 1 << 6;
const RIGHT_DOWN: usize = 1 << 7;

/* Exact port of hsv_to_rgb from utils/hsv.c (output scaled to 8 bits). */
// 8-bit-direct rounding ((x*255.0) as u8) differs from primitives::hsv_to_rgb's
// 16-bit path (e.g. v=0.999 -> 254 here vs 255 there); kept for port fidelity.
fn hsv_to_rgb(h: i32, s: f64, v: f64) -> Color {
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let hh = (h % 360) as f64 / 60.0;
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
    Color::new(
        255,
        (r * 255.0) as u8,
        (g * 255.0) as u8,
        (b * 255.0) as u8,
    )
}

/* Piece definition tables, ported verbatim from polyominoes.c.
   transforms holds only the live entries of the C transform_list
   (the -1 padding is dropped; transform_len is the slice length). */
struct PolySpec {
    points: &'static [(i32, i32)],
    transforms: &'static [i32],
    max_white: i32,
}

const T_ALL: &[i32] = &[0, 1, 2, 3, 4, 5, 6, 7];
const T_01: &[i32] = &[0, 1];
const T_0123: &[i32] = &[0, 1, 2, 3];
const T_0145: &[i32] = &[0, 1, 4, 5];
const T_0: &[i32] = &[0];

const TETROMINO: [PolySpec; 5] = [
    // xxxx
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (3, 0)], transforms: T_01, max_white: 2 },
    // xxx / ..x
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (2, 1)], transforms: T_ALL, max_white: 2 },
    // xxx / .x.
    PolySpec { points: &[(0, 0), (1, 0), (1, 1), (2, 0)], transforms: T_0123, max_white: 3 },
    // xx. / .xx
    PolySpec { points: &[(0, 0), (1, 0), (1, 1), (2, 1)], transforms: T_0145, max_white: 2 },
    // xx / xx
    PolySpec { points: &[(0, 0), (0, 1), (1, 0), (1, 1)], transforms: T_0, max_white: 2 },
];

const PENTOMINO: [PolySpec; 12] = [
    // xxxxx
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)], transforms: T_01, max_white: 3 },
    // xxxx / ...x
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (3, 0), (3, 1)], transforms: T_ALL, max_white: 3 },
    // xxxx / ..x.
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (2, 1), (3, 0)], transforms: T_ALL, max_white: 3 },
    // ..x / xxx / ..x
    PolySpec { points: &[(0, 0), (1, 0), (2, -1), (2, 0), (2, 1)], transforms: T_0123, max_white: 3 },
    // xxx / ..xx
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (2, 1), (3, 1)], transforms: T_ALL, max_white: 3 },
    // xxx / .xx
    PolySpec { points: &[(0, 0), (1, 0), (1, 1), (2, 0), (2, 1)], transforms: T_ALL, max_white: 3 },
    // xxx / ..x / ..x
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (2, 1), (2, 2)], transforms: T_0123, max_white: 3 },
    // .x / xxx / ..x
    PolySpec { points: &[(0, 0), (1, -1), (1, 0), (2, 0), (2, 1)], transforms: T_ALL, max_white: 3 },
    // xxx / x.x
    PolySpec { points: &[(0, 0), (0, 1), (1, 0), (2, 0), (2, 1)], transforms: T_0123, max_white: 3 },
    // ..x / xxx / x..
    PolySpec { points: &[(0, 0), (0, 1), (1, 0), (2, -1), (2, 0)], transforms: T_0145, max_white: 3 },
    // .x / xxx / .x
    PolySpec { points: &[(0, 0), (1, -1), (1, 0), (1, 1), (2, 0)], transforms: T_0, max_white: 4 },
    // xx / .xx / ..x
    PolySpec { points: &[(0, 0), (1, 0), (1, 1), (2, 1), (2, 2)], transforms: T_0123, max_white: 3 },
];

const HEXOMINO: [PolySpec; 35] = [
    // xxxxxx
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0)], transforms: T_01, max_white: 3 },
    // xxxxx / ....x
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (4, 1)], transforms: T_ALL, max_white: 3 },
    // xxxxx / ...x.
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (3, 0), (3, 1), (4, 0)], transforms: T_ALL, max_white: 4 },
    // xxxxx / ..x..
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (2, 1), (3, 0), (4, 0)], transforms: T_0123, max_white: 3 },
    // ...x / xxxx / ...x
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (3, -1), (3, 0), (3, 1)], transforms: T_0123, max_white: 4 },
    // xxxx / ...xx
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (3, 0), (3, 1), (4, 1)], transforms: T_ALL, max_white: 3 },
    // xxxx / ..xx
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (2, 1), (3, 0), (3, 1)], transforms: T_ALL, max_white: 3 },
    // xxxx / ...x / ...x
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (3, 0), (3, 1), (3, 2)], transforms: T_ALL, max_white: 3 },
    // ..x. / xxxx / ...x
    PolySpec { points: &[(0, 0), (1, 0), (2, -1), (2, 0), (3, 0), (3, 1)], transforms: T_ALL, max_white: 3 },
    // xxxx / .x.x
    PolySpec { points: &[(0, 0), (1, 0), (1, 1), (2, 0), (3, 0), (3, 1)], transforms: T_ALL, max_white: 4 },
    // .x.. / xxxx / ...x
    PolySpec { points: &[(0, 0), (1, -1), (1, 0), (2, 0), (3, 0), (3, 1)], transforms: T_ALL, max_white: 4 },
    // xxxx / x..x
    PolySpec { points: &[(0, 0), (0, 1), (1, 0), (2, 0), (3, 0), (3, 1)], transforms: T_0123, max_white: 3 },
    // ...x / xxxx / x...
    PolySpec { points: &[(0, 0), (0, 1), (1, 0), (2, 0), (3, -1), (3, 0)], transforms: T_0145, max_white: 3 },
    // ..x. / xxxx / ..x.
    PolySpec { points: &[(0, 0), (1, 0), (2, -1), (2, 0), (2, 1), (3, 0)], transforms: T_0123, max_white: 4 },
    // xxxx / .xx.
    PolySpec { points: &[(0, 0), (1, 0), (1, 1), (2, 0), (2, 1), (3, 0)], transforms: T_0123, max_white: 3 },
    // xxxx / ..x. / ..x.
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (2, 1), (2, 2), (3, 0)], transforms: T_ALL, max_white: 3 },
    // .x.. / xxxx / ..x.
    PolySpec { points: &[(0, 0), (1, -1), (1, 0), (2, 0), (2, 1), (3, 0)], transforms: T_0145, max_white: 3 },
    // ..xx / xxx. / ..x.
    PolySpec { points: &[(0, 0), (1, 0), (2, -1), (2, 0), (2, 1), (3, -1)], transforms: T_ALL, max_white: 3 },
    // .xx / xxx / ..x
    PolySpec { points: &[(0, 0), (1, -1), (1, 0), (2, -1), (2, 0), (2, 1)], transforms: T_ALL, max_white: 3 },
    // ..x / xxx / x.x
    PolySpec { points: &[(0, 0), (0, 1), (1, 0), (2, -1), (2, 0), (2, 1)], transforms: T_ALL, max_white: 4 },
    // xxx / ..xxx
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (2, 1), (3, 1), (4, 1)], transforms: T_0145, max_white: 3 },
    // xxx / ..xx / ...x
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (2, 1), (3, 1), (3, 2)], transforms: T_ALL, max_white: 3 },
    // xxx / .xxx
    PolySpec { points: &[(0, 0), (1, 0), (1, 1), (2, 0), (2, 1), (3, 1)], transforms: T_0145, max_white: 4 },
    // xxx / ..xx / ..x.
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (2, 1), (2, 2), (3, 1)], transforms: T_ALL, max_white: 4 },
    // .x / xxx / ..xx
    PolySpec { points: &[(0, 0), (1, -1), (1, 0), (2, 0), (2, 1), (3, 1)], transforms: T_ALL, max_white: 4 },
    // xxx / x.xx
    PolySpec { points: &[(0, 0), (0, 1), (1, 0), (2, 0), (2, 1), (3, 1)], transforms: T_ALL, max_white: 3 },
    // xxx / .xx / ..x
    PolySpec { points: &[(0, 0), (1, 0), (1, 1), (2, 0), (2, 1), (2, 2)], transforms: T_0123, max_white: 4 },
    // .x / xxx / .xx
    PolySpec { points: &[(0, 0), (1, -1), (1, 0), (1, 1), (2, 0), (2, 1)], transforms: T_0123, max_white: 4 },
    // xxx / xxx
    PolySpec { points: &[(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (2, 1)], transforms: T_01, max_white: 3 },
    // xxx / ..x / ..xx
    PolySpec { points: &[(0, 0), (1, 0), (2, 0), (2, 1), (2, 2), (3, 2)], transforms: T_ALL, max_white: 3 },
    // xxx / ..x / .xx
    PolySpec { points: &[(0, 0), (1, 0), (1, 2), (2, 0), (2, 1), (2, 2)], transforms: T_ALL, max_white: 3 },
    // .x / xxx / x.x
    PolySpec { points: &[(0, 0), (0, 1), (1, -1), (1, 0), (2, 0), (2, 1)], transforms: T_0123, max_white: 3 },
    // ..xx / xxx. / x...
    PolySpec { points: &[(0, 0), (0, 1), (1, 0), (2, -1), (2, 0), (3, -1)], transforms: T_ALL, max_white: 3 },
    // .xx / xxx / x..
    PolySpec { points: &[(0, 0), (0, 1), (1, -1), (1, 0), (2, -1), (2, 0)], transforms: T_ALL, max_white: 3 },
    // xx.. / .xx. / ..xx
    PolySpec { points: &[(0, 0), (1, 0), (1, 1), (2, 1), (2, 2), (3, 2)], transforms: T_0145, max_white: 3 },
];

/* .x.. / xxxx fills a 10x5 rectangle */
const PENTOMINO1: PolySpec = PolySpec {
    points: &[(0, 0), (1, 0), (2, 0), (3, 0), (1, 1)],
    transforms: T_ALL,
    max_white: 3,
};

/* .x... / xxxxx fills a 24x23 rectangle */
const HEXOMINO1: PolySpec = PolySpec {
    points: &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (1, 1)],
    transforms: T_ALL,
    max_white: 4,
};

/* .xx.. / xxxxx fills a 21x26 rectangle */
const HEPTOMINO1: PolySpec = PolySpec {
    points: &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (1, 1), (2, 1)],
    transforms: T_ALL,
    max_white: 4,
};

/* xxx. / xxxx / xxxx fills a 25x22 rectangle */
const ELEVENOMINO1: PolySpec = PolySpec {
    points: &[
        (0, 0), (1, 0), (2, 0),
        (0, 1), (1, 1), (2, 1), (3, 1),
        (0, 2), (1, 2), (2, 2), (3, 2),
    ],
    transforms: T_ALL,
    max_white: 6,
};

/* .x.. / .x.. / xxxx / xxxx fills a 32x30 rectangle */
const DEKOMINO1: PolySpec = PolySpec {
    points: &[
        (1, -1),
        (1, 0),
        (0, 1), (1, 1), (2, 1), (3, 1),
        (0, 2), (1, 2), (2, 2), (3, 2),
    ],
    transforms: T_ALL,
    max_white: 5,
};

/* .x.. / xxx. / xxxx fills a 96x26 rectangle */
const OCTOMINO1: PolySpec = PolySpec {
    points: &[
        (1, 0),
        (0, 1), (1, 1), (2, 1),
        (0, 2), (1, 2), (2, 2), (3, 2),
    ],
    transforms: T_ALL,
    max_white: 5,
};

/* An owned spec, used for the generated one-sided piece sets. */
struct OwnedSpec {
    points: Vec<(i32, i32)>,
    transforms: Vec<i32>,
    max_white: i32,
}

/* make_one_sided_pentomino / make_one_sided_hexomino: split each piece
   whose transform list contains a reflection (>=4) into two one-sided
   pieces at that position. */
fn make_one_sided(base: &[PolySpec]) -> Vec<OwnedSpec> {
    let mut out = Vec::new();
    for spec in base {
        if let Some(t) = spec.transforms.iter().position(|&v| v >= 4) {
            out.push(OwnedSpec {
                points: spec.points.to_vec(),
                transforms: spec.transforms[..t].to_vec(),
                max_white: spec.max_white,
            });
            out.push(OwnedSpec {
                points: spec.points.to_vec(),
                transforms: spec.transforms[t..].to_vec(),
                max_white: spec.max_white,
            });
        } else {
            out.push(OwnedSpec {
                points: spec.points.to_vec(),
                transforms: spec.transforms.to_vec(),
                max_white: spec.max_white,
            });
        }
    }
    out
}

fn random_permutation(rng: &mut impl Rng, n: usize) -> Vec<usize> {
    let mut a = vec![usize::MAX; n];
    for i in 0..n {
        let r = rng.random_range(0..n - i);
        let mut k = 0;
        while a[k] != usize::MAX {
            k += 1;
        }
        for _ in 0..r {
            k += 1;
            while a[k] != usize::MAX {
                k += 1;
            }
        }
        a[k] = i;
    }
    a
}

#[derive(Clone)]
struct Polyomino {
    points: Vec<(i32, i32)>,
    transform_list: Vec<i32>,
    max_white: i32,
    color: Color,
    attached: bool,
    attach_point: (i32, i32),
    point_no: usize,
    transform_index: usize,
}

/* copy_polyomino: build a runtime piece with permuted point and
   transform lists. */
fn make_poly(
    points: &[(i32, i32)],
    transforms: &[i32],
    max_white: i32,
    perm_point: &[usize],
    perm_transform: &[usize],
) -> Polyomino {
    Polyomino {
        points: perm_point.iter().map(|&i| points[i]).collect(),
        transform_list: perm_transform.iter().map(|&i| transforms[i]).collect(),
        max_white,
        color: Color::new(255, 0, 0, 0),
        attached: false,
        attach_point: (0, 0),
        point_no: 0,
        transform_index: 0,
    }
}

fn transform(inp: (i32, i32), offset: (i32, i32), transform_no: i32, attach: (i32, i32)) -> (i32, i32) {
    let dx = inp.0 - offset.0;
    let dy = inp.1 - offset.1;
    match transform_no {
        0 => (dx + attach.0, dy + attach.1),
        1 => (-dy + attach.0, dx + attach.1),
        2 => (-dx + attach.0, -dy + attach.1),
        3 => (dy + attach.0, -dx + attach.1),
        4 => (-dx + attach.0, dy + attach.1),
        5 => (dy + attach.0, dx + attach.1),
        6 => (dx + attach.0, -dy + attach.1),
        _ => (-dy + attach.0, -dx + attach.1),
    }
}

#[derive(Clone, Copy)]
enum CheckKind {
    /* check_all_regions_multiple_of(n) && whites_ok */
    MultipleOf(i32),
    /* check_all_regions_positive_combination_of(m,n) && whites_ok */
    Combination(i32, i32),
}

/* Resolve the bitmap-aliasing chain from create_bitmaps: a corner bit is
   redundant when an adjacent edge bit is set. */
fn canonical(n: usize) -> usize {
    let mut n = n;
    if n & LEFT_UP != 0 && (n & LEFT != 0 || n & UP != 0) {
        n &= !LEFT_UP;
    }
    if n & LEFT_DOWN != 0 && (n & LEFT != 0 || n & DOWN != 0) {
        n &= !LEFT_DOWN;
    }
    if n & RIGHT_UP != 0 && (n & RIGHT != 0 || n & UP != 0) {
        n &= !RIGHT_UP;
    }
    if n & RIGHT_DOWN != 0 && (n & RIGHT != 0 || n & DOWN != 0) {
        n &= !RIGHT_DOWN;
    }
    n
}

fn setb(bits: &mut [bool], b: i32, x: i32, y: i32, v: bool) {
    if x >= 0 && x < b && y >= 0 && y < b {
        bits[(y * b + x) as usize] = v;
    }
}

fn twothirdsbit(bits: &mut [bool], b: i32, x: i32, y: i32) {
    setb(bits, b, x, y, (x + y - 1) % 3 != 0);
}

fn halfbit(bits: &mut [bool], b: i32, x: i32, y: i32) {
    setb(bits, b, x, y, (x - y) % 2 != 0);
}

fn thirdbit(bits: &mut [bool], b: i32, x: i32, y: i32) {
    setb(bits, b, x, y, (x - y - 1) % 3 == 0);
}

fn threequartersbit(bits: &mut [bool], b: i32, x: i32, y: i32) {
    setb(bits, b, x, y, (y % 2 != 0) || ((x + 2 + y / 2 + 1) % 2 != 0));
}

/* Port of create_bitmaps' per-index bitmap generation. */
fn make_bitmap(n: usize, b: i32, use3d: bool) -> Vec<bool> {
    /* Parameters for bitmaps. */
    let g = b / 45 + 1; /* 1/2 of gap between polyominoes. */
    let t = if b <= 12 { 1 } else { g * 2 }; /* Thickness of walls. */
    let r = if b <= 12 { 1 } else { g * 6 }; /* Amount of rounding. */
    let rt = if b <= 12 { 1 } else { g * 3 }; /* Thickness of rounded walls. */
    let rr = 0; /* Roof ridge thickness */

    let left = n & LEFT != 0;
    let right = n & RIGHT != 0;
    let up = n & UP != 0;
    let down = n & DOWN != 0;
    let left_up = n & LEFT_UP != 0;
    let left_down = n & LEFT_DOWN != 0;
    let right_up = n & RIGHT_UP != 0;
    let right_down = n & RIGHT_DOWN != 0;

    let mut bits = vec![false; (b * b) as usize];

    for y in 0..b {
        for x in 0..b {
            if !use3d {
                halfbit(&mut bits, b, x, y);
            } else if (x >= y && x < b - y && up)
                || (x <= y && x < b - y && y < b / 2 && !left)
                || (x >= y && x >= b - y - 1 && y < b / 2 && !right)
            {
                setb(&mut bits, b, x, y, true);
            } else if (x <= y && x < b - y && left)
                || (x >= y && x < b - y && x < b / 2 && !up)
                || (x <= y && x >= b - y - 1 && x < b / 2 && !down)
            {
                twothirdsbit(&mut bits, b, x, y);
            } else if (x >= y && x >= b - y - 1 && right)
                || (x >= y && x < b - y && x >= b / 2 && !up)
                || (x <= y && x >= b - y - 1 && x >= b / 2 && !down)
            {
                halfbit(&mut bits, b, x, y);
            } else if (x <= y && x >= b - y - 1 && down)
                || (x <= y && x < b - y && y >= b / 2 && !left)
                || (x >= y && x >= b - y - 1 && y >= b / 2 && !right)
            {
                thirdbit(&mut bits, b, x, y);
            }
        }
    }

    if left {
        for y in 0..b {
            for x in g..g + t {
                setb(&mut bits, b, x, y, true);
            }
        }
    }
    if right {
        for y in 0..b {
            for x in g..g + t {
                setb(&mut bits, b, b - 1 - x, y, true);
            }
        }
    }
    if up {
        for x in 0..b {
            for y in g..g + t {
                setb(&mut bits, b, x, y, true);
            }
        }
    }
    if down {
        for x in 0..b {
            for y in g..g + t {
                setb(&mut bits, b, x, b - 1 - y, true);
            }
        }
    }
    if left {
        for y in 0..b {
            for x in 0..g {
                setb(&mut bits, b, x, y, false);
            }
        }
    }
    if right {
        for y in 0..b {
            for x in 0..g {
                setb(&mut bits, b, b - 1 - x, y, false);
            }
        }
    }
    if up {
        for x in 0..b {
            for y in 0..g {
                setb(&mut bits, b, x, y, false);
            }
        }
    }
    if down {
        for x in 0..b {
            for y in 0..g {
                setb(&mut bits, b, x, b - 1 - y, false);
            }
        }
    }

    /* Rounded corners where two walls meet. */
    if left && up {
        for x in g..=g + r {
            for y in g..=r + 2 * g - x {
                setb(&mut bits, b, x, y, x + y > r + 2 * g - rt);
            }
        }
    }
    if left && down {
        for x in g..=g + r {
            for y in g..=r + 2 * g - x {
                setb(&mut bits, b, x, b - 1 - y, x + y > r + 2 * g - rt);
            }
        }
    }
    if right && up {
        for x in g..=g + r {
            for y in g..=r + 2 * g - x {
                setb(&mut bits, b, b - 1 - x, y, x + y > r + 2 * g - rt);
            }
        }
    }
    if right && down {
        for x in g..=g + r {
            for y in g..=r + 2 * g - x {
                setb(&mut bits, b, b - 1 - x, b - 1 - y, x + y > r + 2 * g - rt);
            }
        }
    }

    /* Inner corner notches. */
    if !left && !up && left_up {
        for x in 0..g {
            for y in 0..g {
                setb(&mut bits, b, x, y, false);
            }
        }
        for x in g..g + t {
            for y in 0..g {
                setb(&mut bits, b, x, y, true);
            }
        }
        for x in 0..g + t {
            for y in g..g + t {
                setb(&mut bits, b, x, y, true);
            }
        }
    }
    if !left && !down && left_down {
        for x in 0..g {
            for y in 0..g {
                setb(&mut bits, b, x, b - 1 - y, false);
            }
        }
        for x in g..g + t {
            for y in 0..g {
                setb(&mut bits, b, x, b - 1 - y, true);
            }
        }
        for x in 0..g + t {
            for y in g..g + t {
                setb(&mut bits, b, x, b - 1 - y, true);
            }
        }
    }
    if !right && !up && right_up {
        for x in 0..g {
            for y in 0..g {
                setb(&mut bits, b, b - 1 - x, y, false);
            }
        }
        for x in g..g + t {
            for y in 0..g {
                setb(&mut bits, b, b - 1 - x, y, true);
            }
        }
        for x in 0..g + t {
            for y in g..g + t {
                setb(&mut bits, b, b - 1 - x, y, true);
            }
        }
    }
    if !right && !down && right_down {
        for x in 0..g {
            for y in 0..g {
                setb(&mut bits, b, b - 1 - x, b - 1 - y, false);
            }
        }
        for x in g..g + t {
            for y in 0..g {
                setb(&mut bits, b, b - 1 - x, b - 1 - y, true);
            }
        }
        for x in 0..g + t {
            for y in g..g + t {
                setb(&mut bits, b, b - 1 - x, b - 1 - y, true);
            }
        }
    }

    /* "Roof" quadrant shading for corners with no adjacent wall (3D only). */
    if use3d {
        if !left && !up && !left_up {
            for x in 0..b / 2 - rr {
                for y in 0..b / 2 - rr {
                    threequartersbit(&mut bits, b, x, y);
                }
            }
        }
        if !left && !down && !left_down {
            for x in 0..b / 2 - rr {
                for y in b / 2 + rr..b {
                    threequartersbit(&mut bits, b, x, y);
                }
            }
        }
        if !right && !up && !right_up {
            for x in b / 2 + rr..b {
                for y in 0..b / 2 - rr {
                    threequartersbit(&mut bits, b, x, y);
                }
            }
        }
        if !right && !down && !right_down {
            for x in b / 2 + rr..b {
                for y in b / 2 + rr..b {
                    threequartersbit(&mut bits, b, x, y);
                }
            }
        }
    }

    bits
}

fn fill_rect(buffer: &mut [u8], w: u32, h: u32, x: i32, y: i32, rw: i32, rh: i32, color: Color) {
    for yy in y..y + rh {
        for xx in x..x + rw {
            put_pixel(buffer, w, h, xx, yy, color);
        }
    }
}

pub struct Polyominoes {
    win_w: u32,
    win_h: u32,
    npixels: i32,
    cycles: i32,
    counter: i32,
    wait: i32,

    bw: i32, // puzzle board width in cells
    bh: i32,
    border_color: Color,
    mono: bool,

    polyomino: Vec<Polyomino>,
    identical: bool,
    use3d: bool,
    attach_list: Vec<usize>,
    nr_attached: usize,

    /* The array that tells where the polyominoes are attached. */
    array: Vec<i32>,

    box_size: i32,
    x_margin: i32,
    y_margin: i32,

    left_right: bool,
    top_bottom: bool,

    use_bitmaps: bool,
    bitmaps: Vec<Vec<bool>>, // 256 entries; empty for aliased indices

    check_kind: CheckKind,
    rot180: bool,
    /* reason_to_not_attach[row*nr + piece] - see the long comment in
       polyominoes.c: which attached pieces blocked an attachment. */
    reason: Vec<i32>,

    delay_us: u64,
}

impl Polyominoes {
    fn nr(&self) -> usize {
        self.polyomino.len()
    }

    #[inline]
    fn ai(&self, x: i32, y: i32) -> usize {
        (x * self.bh + y) as usize
    }

    /* The ARR macro: out-of-bounds cells read as -2. */
    fn arr(&self, x: i32, y: i32) -> i32 {
        if x < 0 || x >= self.bw || y < 0 || y >= self.bh {
            -2
        } else {
            self.array[self.ai(x, y)]
        }
    }

    fn first_poly_no(&self) -> usize {
        let mut poly_no = 0;
        while poly_no < self.nr() && self.polyomino[poly_no].attached {
            poly_no += 1;
        }
        poly_no
    }

    fn next_poly_no(&self, poly_no: &mut usize) {
        if self.identical {
            *poly_no = self.nr();
        } else {
            loop {
                *poly_no += 1;
                if !(*poly_no < self.nr() && self.polyomino[*poly_no].attached) {
                    break;
                }
            }
        }
    }

    fn count_adjacent_blanks(&mut self, x: i32, y: i32, blank_mark: i32) -> i32 {
        // Iterative flood fill; the C version recurses.
        let mut count = 0;
        let mut stack = vec![(x, y)];
        while let Some((x, y)) = stack.pop() {
            let idx = self.ai(x, y);
            if self.array[idx] == -1 {
                count += 1;
                self.array[idx] = blank_mark;
                if x >= 1 {
                    stack.push((x - 1, y));
                }
                if x < self.bw - 1 {
                    stack.push((x + 1, y));
                }
                if y >= 1 {
                    stack.push((x, y - 1));
                }
                if y < self.bh - 1 {
                    stack.push((x, y + 1));
                }
            }
        }
        count
    }

    fn restore_blanks(&mut self, mark: i32) {
        for v in &mut self.array {
            if *v == mark {
                *v = -1;
            }
        }
    }

    fn check_all_regions_multiple_of(&mut self, n: i32) -> bool {
        let mut good = true;
        'outer: for x in 0..self.bw {
            for y in 0..self.bh {
                let count = self.count_adjacent_blanks(x, y, -2);
                if count % n != 0 {
                    good = false;
                    break 'outer;
                }
            }
        }
        self.restore_blanks(-2);
        good
    }

    fn check_all_regions_positive_combination_of(&mut self, m: i32, n: i32) -> bool {
        let mut good = true;
        'outer: for x in 0..self.bw {
            for y in 0..self.bh {
                let mut count = self.count_adjacent_blanks(x, y, -2);
                good = false;
                while count >= 0 && !good {
                    good = count % n == 0;
                    count -= m;
                }
                if !good {
                    break 'outer;
                }
            }
        }
        self.restore_blanks(-2);
        good
    }

    fn find_smallest_blank_component(&mut self) -> i32 {
        let mut blank_mark = -10;
        let mut smallest_mark = -10;
        let mut smallest_size = 1_000_000_000;
        for x in 0..self.bw {
            for y in 0..self.bh {
                if self.array[self.ai(x, y)] == -1 {
                    let size = self.count_adjacent_blanks(x, y, blank_mark);
                    if size < smallest_size {
                        smallest_mark = blank_mark;
                        smallest_size = size;
                    }
                    blank_mark -= 1;
                }
            }
        }
        smallest_mark
    }

    /* "Chess board" check: makes sure the remaining blanks have a valid
       number of white and black squares for the unattached pieces. */
    fn whites_ok(&self) -> bool {
        let mut whites = 0;
        let mut blacks = 0;
        let mut max_white = 0;
        let mut min_white = 0;
        for x in 0..self.bw {
            for y in 0..self.bh {
                if self.array[self.ai(x, y)] == -1 && (x + y) % 2 != 0 {
                    whites += 1;
                }
                if self.array[self.ai(x, y)] == -1 && (x + y + 1) % 2 != 0 {
                    blacks += 1;
                }
            }
        }
        for p in &self.polyomino {
            if !p.attached {
                max_white += p.max_white;
                min_white += p.points.len() as i32 - p.max_white;
            }
        }
        min_white <= blacks && min_white <= whites && blacks <= max_white && whites <= max_white
    }

    fn run_check_ok(&mut self) -> bool {
        let ok = match self.check_kind {
            CheckKind::MultipleOf(n) => self.check_all_regions_multiple_of(n),
            CheckKind::Combination(m, n) => self.check_all_regions_positive_combination_of(m, n),
        };
        ok && self.whites_ok()
    }

    /* How many piece placements can cover point (x,y). */
    fn score_point(&self, x: i32, y: i32, min_score_so_far: i32) -> i32 {
        if x >= 1
            && x < self.bw - 1
            && y >= 1
            && y < self.bh - 1
            && self.arr(x - 1, y - 1) < 0
            && self.arr(x - 1, y) < 0
            && self.arr(x - 1, y + 1) < 0
            && self.arr(x + 1, y - 1) < 0
            && self.arr(x + 1, y) < 0
            && self.arr(x + 1, y + 1) < 0
            && self.arr(x, y - 1) < 0
            && self.arr(x, y + 1) < 0
        {
            return 10000;
        }

        let attach_point = (x, y);
        let mut score = 0;
        let mut poly_no = self.first_poly_no();
        while poly_no < self.nr() {
            if !self.polyomino[poly_no].attached {
                let poly = &self.polyomino[poly_no];
                for point_no in 0..poly.points.len() {
                    for transform_index in 0..poly.transform_list.len() {
                        let mut attachable = true;
                        for i in 0..poly.points.len() {
                            let tp = transform(
                                poly.points[i],
                                poly.points[point_no],
                                poly.transform_list[transform_index],
                                attach_point,
                            );
                            if !(tp.0 >= 0
                                && tp.0 < self.bw
                                && tp.1 >= 0
                                && tp.1 < self.bh
                                && self.array[self.ai(tp.0, tp.1)] < 0)
                            {
                                attachable = false;
                                break;
                            }
                        }
                        if attachable {
                            score += 1;
                            if score >= min_score_so_far {
                                return score;
                            }
                        }
                    }
                }
            }
            self.next_poly_no(&mut poly_no);
        }
        score
    }

    fn find_blank(&mut self) -> (i32, i32) {
        let blank_mark = self.find_smallest_blank_component();

        let mut point = (0, 0);
        let mut worst_score = 1_000_000;
        for x in 0..self.bw {
            for y in 0..self.bh {
                if self.array[self.ai(x, y)] == blank_mark {
                    let mut score = 100 * self.score_point(x, y, worst_score);
                    if score > 0 {
                        score += if self.left_right { 10 * x } else { 10 * (self.bw - 1 - x) };
                        score += if self.top_bottom { y } else { self.bh - 1 - y };
                    }
                    if score < worst_score {
                        point = (x, y);
                        worst_score = score;
                    }
                }
            }
        }

        for v in &mut self.array {
            if *v < 0 {
                *v = -1;
            }
        }
        point
    }

    /* Detaches the most recently attached polyomino. */
    fn detach(&mut self, rot180: i32) -> (usize, usize, usize, (i32, i32)) {
        if self.nr_attached == 0 {
            return (0, 0, 0, (0, 0));
        }
        self.nr_attached -= 1;
        let poly_no = self.attach_list[self.nr_attached];
        let point_no = self.polyomino[poly_no].point_no;
        let transform_index = self.polyomino[poly_no].transform_index;
        let attach_point = self.polyomino[poly_no].attach_point;
        for i in 0..self.polyomino[poly_no].points.len() {
            let tp = transform(
                self.polyomino[poly_no].points[i],
                self.polyomino[poly_no].points[point_no],
                self.polyomino[poly_no].transform_list[transform_index] ^ (rot180 << 1),
                attach_point,
            );
            let idx = self.ai(tp.0, tp.1);
            self.array[idx] = -1;
        }
        self.polyomino[poly_no].attached = false;
        (poly_no, point_no, transform_index, attach_point)
    }

    /* Attempts to attach a polyomino at attach_point.  reason_row indexes
       the reason_to_not_attach table (identical mode only). */
    fn attach(
        &mut self,
        poly_no: usize,
        point_no: usize,
        transform_index: usize,
        mut attach_point: (i32, i32),
        rot180: i32,
        reason_row: usize,
    ) -> bool {
        if rot180 != 0 {
            attach_point.0 = self.bw - 1 - attach_point.0;
            attach_point.1 = self.bh - 1 - attach_point.1;
        }

        if poly_no >= self.nr() || self.polyomino[poly_no].attached {
            return false;
        }

        let mut attachable = true;
        let mut worst_reason_not_to_attach: i32 = 1_000_000_000;
        for i in 0..self.polyomino[poly_no].points.len() {
            let tp = transform(
                self.polyomino[poly_no].points[i],
                self.polyomino[poly_no].points[point_no],
                self.polyomino[poly_no].transform_list[transform_index] ^ (rot180 << 1),
                attach_point,
            );
            let in_bounds = tp.0 >= 0 && tp.0 < self.bw && tp.1 >= 0 && tp.1 < self.bh;
            if !(in_bounds && self.array[self.ai(tp.0, tp.1)] == -1) {
                if self.identical {
                    attachable = false;
                    if in_bounds
                        && self.array[self.ai(tp.0, tp.1)] >= 0
                        && self.array[self.ai(tp.0, tp.1)] < worst_reason_not_to_attach
                    {
                        worst_reason_not_to_attach = self.array[self.ai(tp.0, tp.1)];
                    }
                } else {
                    return false;
                }
            }
        }

        if self.identical && !attachable {
            if worst_reason_not_to_attach < 1_000_000_000 {
                let nr = self.nr();
                self.reason[reason_row * nr + worst_reason_not_to_attach as usize] = 1;
            }
            return false;
        }

        for i in 0..self.polyomino[poly_no].points.len() {
            let tp = transform(
                self.polyomino[poly_no].points[i],
                self.polyomino[poly_no].points[point_no],
                self.polyomino[poly_no].transform_list[transform_index] ^ (rot180 << 1),
                attach_point,
            );
            let idx = self.ai(tp.0, tp.1);
            self.array[idx] = poly_no as i32;
        }

        self.attach_list[self.nr_attached] = poly_no;
        self.nr_attached += 1;

        self.polyomino[poly_no].attached = true;
        self.polyomino[poly_no].point_no = point_no;
        self.polyomino[poly_no].attach_point = attach_point;
        self.polyomino[poly_no].transform_index = transform_index;

        if !self.run_check_ok() {
            self.detach(rot180);
            return false;
        }

        true
    }

    fn next_attach_try(
        &self,
        poly_no: &mut usize,
        point_no: &mut usize,
        transform_index: &mut usize,
    ) -> bool {
        *transform_index += 1;
        if *transform_index >= self.polyomino[*poly_no].transform_list.len() {
            *transform_index = 0;
            *point_no += 1;
            if *point_no >= self.polyomino[*poly_no].points.len() {
                *point_no = 0;
                self.next_poly_no(poly_no);
                if *poly_no >= self.nr() {
                    *poly_no = self.first_poly_no();
                    return false;
                }
            }
        }
        true
    }

    /*************************************************
    Puzzle specific initialization routines.
    *************************************************/

    fn set_uniform_puzzle(
        &mut self,
        rng: &mut impl Rng,
        specs: &[PolyominoSpec],
        nr: usize,
        check: CheckKind,
    ) {
        // Pieces drawn from a permuted spec list, each with fresh
        // point/transform permutations.
        let perm_poly = random_permutation(rng, nr);
        self.polyomino = (0..nr)
            .map(|p| {
                let (points, transforms, mw) = specs[perm_poly[p]];
                let pp = random_permutation(rng, points.len());
                let pt = random_permutation(rng, transforms.len());
                make_poly(points, transforms, mw, &pp, &pt)
            })
            .collect();
        self.check_kind = check;
    }

    fn set_scatter_puzzle(
        &mut self,
        rng: &mut impl Rng,
        specs: &[PolyominoSpec],
        nr: usize,
        check: CheckKind,
    ) {
        // Pieces placed at permuted positions (tetr-pentomino and
        // pent-hexomino puzzles).
        let perm_poly = random_permutation(rng, nr);
        let mut polys: Vec<Option<Polyomino>> = vec![None; nr];
        for (p, &(points, transforms, mw)) in specs.iter().enumerate() {
            let pp = random_permutation(rng, points.len());
            let pt = random_permutation(rng, transforms.len());
            polys[perm_poly[p]] = Some(make_poly(points, transforms, mw, &pp, &pt));
        }
        // perm_poly is a permutation of 0..nr and specs.len() == nr, so every
        // slot is Some; flatten() degrades gracefully instead of panicking.
        self.polyomino = polys.into_iter().flatten().collect();
        self.check_kind = check;
    }

    fn set_identical_puzzle(
        &mut self,
        rng: &mut impl Rng,
        spec: &PolySpec,
        nr: usize,
        w: i32,
        h: i32,
        rot180: bool,
        check: CheckKind,
    ) {
        self.rot180 = rot180;
        self.bw = w;
        self.bh = h;
        self.polyomino = Vec::with_capacity(nr);
        if rot180 {
            /* Pairs share the same point/transform permutations
               (copy_polyomino with new_rand=0 for the second of each pair). */
            for _ in (0..nr).step_by(2) {
                let pp = random_permutation(rng, spec.points.len());
                let pt = random_permutation(rng, spec.transforms.len());
                self.polyomino
                    .push(make_poly(spec.points, spec.transforms, spec.max_white, &pp, &pt));
                self.polyomino
                    .push(make_poly(spec.points, spec.transforms, spec.max_white, &pp, &pt));
            }
        } else {
            for _ in 0..nr {
                let pp = random_permutation(rng, spec.points.len());
                let pt = random_permutation(rng, spec.transforms.len());
                self.polyomino
                    .push(make_poly(spec.points, spec.transforms, spec.max_white, &pp, &pt));
            }
        }
        self.check_kind = check;
    }

    /* init_polyominoes */
    fn init(&mut self) {
        let mut rng = rand::rng();

        self.rot180 = false;
        self.counter = 0;

        /* MI_IS_FULLRANDOM is always true in the xscreensaver build. */
        self.identical = rng.random::<bool>();
        self.use3d = rng.random_range(0..4) != 0;

        if self.identical {
            match rng.random_range(0..9) {
                0 => self.set_identical_puzzle(&mut rng, &PENTOMINO1, 10, 10, 5, false, CheckKind::MultipleOf(5)),
                1 => self.set_identical_puzzle(&mut rng, &HEXOMINO1, 92, 24, 23, false, CheckKind::MultipleOf(6)),
                2 => self.set_identical_puzzle(&mut rng, &HEPTOMINO1, 78, 26, 21, true, CheckKind::MultipleOf(7)),
                3 => self.set_identical_puzzle(&mut rng, &HEPTOMINO1, 76, 28, 19, false, CheckKind::MultipleOf(7)),
                4 => self.set_identical_puzzle(&mut rng, &ELEVENOMINO1, 50, 25, 22, true, CheckKind::MultipleOf(11)),
                5 => self.set_identical_puzzle(&mut rng, &DEKOMINO1, 96, 32, 30, false, CheckKind::MultipleOf(10)),
                6 => self.set_identical_puzzle(&mut rng, &OCTOMINO1, 312, 96, 26, false, CheckKind::MultipleOf(8)),
                7 => self.set_identical_puzzle(&mut rng, &PENTOMINO1, 45, 15, 15, false, CheckKind::MultipleOf(5)),
                _ => self.set_identical_puzzle(&mut rng, &ELEVENOMINO1, 141, 47, 33, false, CheckKind::MultipleOf(11)),
            }
        } else {
            match rng.random_range(0..5) {
                0 => {
                    /* All twelve pentominoes. */
                    let (w, h) = [(20, 3), (15, 4), (12, 5), (10, 6)][rng.random_range(0..4)];
                    self.bw = w;
                    self.bh = h;
                    let specs: Vec<_> = PENTOMINO
                        .iter()
                        .map(|s| (s.points, s.transforms, s.max_white))
                        .collect();
                    self.set_uniform_puzzle(&mut rng, &specs, 12, CheckKind::MultipleOf(5));
                }
                1 => {
                    /* All eighteen one-sided pentominoes. */
                    let (w, h) = [(30, 3), (18, 5), (15, 6), (10, 9)][rng.random_range(0..4)];
                    self.bw = w;
                    self.bh = h;
                    let one_sided = make_one_sided(&PENTOMINO);
                    let specs: Vec<_> = one_sided
                        .iter()
                        .map(|s| (s.points.as_slice(), s.transforms.as_slice(), s.max_white))
                        .collect();
                    self.set_uniform_puzzle(&mut rng, &specs, 18, CheckKind::MultipleOf(5));
                }
                2 => {
                    /* All sixty one-sided hexominoes. */
                    let (w, h) = [
                        (20, 18), (24, 15), (30, 12), (36, 10),
                        (40, 9), (45, 8), (60, 6), (72, 5),
                    ][rng.random_range(0..8)];
                    self.bw = w;
                    self.bh = h;
                    let one_sided = make_one_sided(&HEXOMINO);
                    let specs: Vec<_> = one_sided
                        .iter()
                        .map(|s| (s.points.as_slice(), s.transforms.as_slice(), s.max_white))
                        .collect();
                    self.set_uniform_puzzle(&mut rng, &specs, 60, CheckKind::MultipleOf(6));
                }
                3 => {
                    /* All twelve pentominoes and all thirty five hexominoes. */
                    let (w, h) = [(54, 5), (45, 6), (30, 9), (27, 10), (18, 15)][rng.random_range(0..5)];
                    self.bw = w;
                    self.bh = h;
                    let specs: Vec<_> = PENTOMINO
                        .iter()
                        .chain(HEXOMINO.iter())
                        .map(|s| (s.points, s.transforms, s.max_white))
                        .collect();
                    self.set_scatter_puzzle(&mut rng, &specs, 47, CheckKind::Combination(6, 5));
                }
                _ => {
                    /* All five tetrominoes and all twelve pentominoes. */
                    let (w, h) = [(20, 4), (16, 5), (10, 8)][rng.random_range(0..3)];
                    self.bw = w;
                    self.bh = h;
                    let specs: Vec<_> = TETROMINO
                        .iter()
                        .chain(PENTOMINO.iter())
                        .map(|s| (s.points, s.transforms, s.max_white))
                        .collect();
                    self.set_scatter_puzzle(&mut rng, &specs, 17, CheckKind::Combination(5, 4));
                }
            }
        }

        if self.win_h > self.win_w {
            /* rotate if portrait */
            std::mem::swap(&mut self.bw, &mut self.bh);
        }

        let nr = self.nr();
        self.attach_list = vec![0; nr];
        self.nr_attached = 0;

        self.reason = if self.identical { vec![0; nr * nr] } else { Vec::new() };

        self.array = vec![-1; (self.bw * self.bh) as usize];

        self.left_right = rng.random_range(0..2) != 0;
        self.top_bottom = rng.random_range(0..2) != 0;

        let box1 = self.win_w as i32 / (self.bw + 2);
        let box2 = self.win_h as i32 / (self.bh + 2);
        self.box_size = box1.min(box2);

        if self.win_w > self.win_h * 5 || self.win_h > self.win_w * 5 {
            /* weird window aspect ratio */
            let ratio = if self.win_w > self.win_h {
                self.win_w as f64 / self.win_h as f64
            } else {
                self.win_h as f64 / self.win_w as f64
            };
            self.box_size = (self.box_size as f64 * ratio) as i32;
        }

        if self.box_size >= 12 {
            self.box_size = (self.box_size / 12) * 12;
            self.bitmaps = (0..256)
                .map(|n| {
                    if canonical(n) != n {
                        Vec::new()
                    } else {
                        make_bitmap(n, self.box_size, self.use3d)
                    }
                })
                .collect();
            self.use_bitmaps = true;
        } else {
            self.use_bitmaps = false;
            self.bitmaps = Vec::new();
        }

        let perm = random_permutation(&mut rng, nr);
        self.mono = self.npixels < 12;
        let start = rng.random_range(0..self.npixels);
        let mut i = 0;
        while i < nr {
            if !self.mono {
                let k = (perm[i] as i32 * self.npixels / nr as i32 + start) % self.npixels;
                let color = hsv_to_rgb(k * 360 / self.npixels, 1.0, 1.0);
                self.polyomino[i].color = color;
                if self.rot180 {
                    self.polyomino[i + 1].color = color;
                    i += 1;
                }
            } else if self.use_bitmaps {
                self.polyomino[i].color = Color::new(255, 255, 255, 255);
            } else {
                self.polyomino[i].color = Color::new(255, 0, 0, 0);
            }
            i += 1;
        }

        if self.use_bitmaps {
            self.border_color = if self.mono {
                Color::new(255, 255, 255, 255)
            } else {
                let k = rng.random_range(0..self.npixels);
                hsv_to_rgb(k * 360 / self.npixels, 1.0, 1.0)
            };
        }

        self.x_margin = (self.win_w as i32 - self.box_size * self.bw) / 2;
        self.y_margin = (self.win_h as i32 - self.box_size * self.bh) / 2;

        self.wait = 0;
    }

    /*******************************************************
    Display routines.
    *******************************************************/

    fn draw_with_bitmaps(&self, buffer: &mut [u8], w: u32, h: u32) {
        let b = self.box_size;
        let g = b / 45 + 1;
        let t = if b <= 12 { 1 } else { g * 2 };

        for x in 0..self.bw {
            for y in 0..self.bh {
                let v = self.array[self.ai(x, y)];
                if v < 0 {
                    // Blank cells stay background black.
                    continue;
                }
                let color = self.polyomino[v as usize].color;
                let mut idx = 0;
                if self.arr(x, y) != self.arr(x - 1, y) {
                    idx |= LEFT;
                }
                if self.arr(x, y) != self.arr(x + 1, y) {
                    idx |= RIGHT;
                }
                if self.arr(x, y) != self.arr(x, y - 1) {
                    idx |= UP;
                }
                if self.arr(x, y) != self.arr(x, y + 1) {
                    idx |= DOWN;
                }
                if self.arr(x, y) != self.arr(x - 1, y - 1) {
                    idx |= LEFT_UP;
                }
                if self.arr(x, y) != self.arr(x - 1, y + 1) {
                    idx |= LEFT_DOWN;
                }
                if self.arr(x, y) != self.arr(x + 1, y - 1) {
                    idx |= RIGHT_UP;
                }
                if self.arr(x, y) != self.arr(x + 1, y + 1) {
                    idx |= RIGHT_DOWN;
                }
                let bm = &self.bitmaps[canonical(idx)];
                let ox = self.x_margin + b * x;
                let oy = self.y_margin + b * y;
                for by in 0..b {
                    for bx in 0..b {
                        if bm[(by * b + bx) as usize] {
                            put_pixel(buffer, w, h, ox + bx, oy + by, color);
                        }
                    }
                }
            }
        }

        // Border rectangles.
        for tt in g..g + t {
            let x0 = self.x_margin - tt - 1;
            let y0 = self.y_margin - tt - 1;
            let rw = b * self.bw + 1 + 2 * tt;
            let rh = b * self.bh + 1 + 2 * tt;
            draw_line(buffer, w, h, x0, y0, x0 + rw, y0, self.border_color);
            draw_line(buffer, w, h, x0, y0 + rh, x0 + rw, y0 + rh, self.border_color);
            draw_line(buffer, w, h, x0, y0, x0, y0 + rh, self.border_color);
            draw_line(buffer, w, h, x0 + rw, y0, x0 + rw, y0 + rh, self.border_color);
        }
    }

    fn draw_without_bitmaps(&self, buffer: &mut [u8], w: u32, h: u32) {
        let b = self.box_size;
        let lw = b / 10 + 1;
        let white = Color::new(255, 255, 255, 255);

        for x in 0..self.bw {
            for y in 0..self.bh {
                let v = self.array[self.ai(x, y)];
                if v >= 0 {
                    fill_rect(
                        buffer, w, h,
                        self.x_margin + b * x,
                        self.y_margin + b * y,
                        b, b,
                        self.polyomino[v as usize].color,
                    );
                }
            }
        }

        // White outer rectangle (thick).
        let rw = b * self.bw;
        let rh = b * self.bh;
        fill_rect(buffer, w, h, self.x_margin - lw / 2, self.y_margin - lw / 2, rw + 1 + lw - 1, lw, white);
        fill_rect(buffer, w, h, self.x_margin - lw / 2, self.y_margin + rh - lw / 2, rw + 1 + lw - 1, lw, white);
        fill_rect(buffer, w, h, self.x_margin - lw / 2, self.y_margin - lw / 2, lw, rh + 1 + lw - 1, white);
        fill_rect(buffer, w, h, self.x_margin + rw - lw / 2, self.y_margin - lw / 2, lw, rh + 1 + lw - 1, white);

        // White segments at piece boundaries.
        for x in 0..self.bw - 1 {
            for y in 0..self.bh {
                if self.array[self.ai(x + 1, y)] != self.array[self.ai(x, y)] {
                    fill_rect(
                        buffer, w, h,
                        self.x_margin + b * (x + 1) - lw / 2,
                        self.y_margin + b * y,
                        lw, b + 1,
                        white,
                    );
                }
            }
        }
        for x in 0..self.bw {
            for y in 0..self.bh - 1 {
                if self.array[self.ai(x, y + 1)] != self.array[self.ai(x, y)] {
                    fill_rect(
                        buffer, w, h,
                        self.x_margin + b * x,
                        self.y_margin + b * (y + 1) - lw / 2,
                        b + 1, lw,
                        white,
                    );
                }
            }
        }
    }
}

impl Animation for Polyominoes {
    fn new(config: &AnimConfig) -> Self {
        let mut p = Polyominoes {
            win_w: config.width,
            win_h: config.height,
            npixels: DEF_NCOLORS,
            cycles: DEF_CYCLES,
            counter: 0,
            wait: 0,
            bw: 1,
            bh: 1,
            border_color: Color::new(255, 255, 255, 255),
            mono: false,
            polyomino: Vec::new(),
            identical: false,
            use3d: true,
            attach_list: Vec::new(),
            nr_attached: 0,
            array: Vec::new(),
            box_size: 0,
            x_margin: 0,
            y_margin: 0,
            left_right: false,
            top_bottom: false,
            use_bitmaps: false,
            bitmaps: Vec::new(),
            check_kind: CheckKind::MultipleOf(5),
            rot180: false,
            reason: Vec::new(),
            delay_us: DEF_DELAY_US,
        };
        p.reset(config);
        p
    }

    /* draw_polyominoes: attach one more piece per frame (backtracking as
       needed). */
    fn tick(&mut self) {
        if self.cycles != 0 {
            self.counter += 1;
            if self.counter > self.cycles {
                self.init();
                return;
            }
        }

        if self.box_size == 0 {
            self.init();
            return;
        }

        self.wait -= 1;
        if self.wait > 0 {
            return;
        }

        let nr = self.nr();
        let mut poly_no = self.first_poly_no();
        let mut point_no: usize = 0;
        let mut transform_index: usize = 0;
        let mut done = false;
        let mut another_attachment_try = true;
        let mut attach_point = self.find_blank();
        if self.identical && self.nr_attached < nr {
            let row = self.nr_attached;
            for i in 0..nr {
                self.reason[row * nr + i] = 0;
            }
        }
        while !done {
            if self.nr_attached < nr {
                while !done && another_attachment_try {
                    done = self.attach(
                        poly_no,
                        point_no,
                        transform_index,
                        attach_point,
                        0,
                        self.nr_attached,
                    );
                    if done && self.rot180 {
                        poly_no = self.first_poly_no();
                        done = self.attach(
                            poly_no,
                            point_no,
                            transform_index,
                            attach_point,
                            1,
                            self.nr_attached - 1,
                        );
                        if !done {
                            let (p, pt, ti, ap) = self.detach(0);
                            poly_no = p;
                            point_no = pt;
                            transform_index = ti;
                            attach_point = ap;
                        }
                    }
                    if !done {
                        another_attachment_try =
                            self.next_attach_try(&mut poly_no, &mut point_no, &mut transform_index);
                    }
                }
            }

            if self.identical {
                if !done {
                    if self.nr_attached == 0 {
                        done = true;
                    } else {
                        let mut detach_until = self.nr_attached - 1;
                        if self.nr_attached < nr {
                            while detach_until > 0
                                && self.reason[self.nr_attached * nr + detach_until] == 0
                            {
                                detach_until -= 1;
                            }
                        }
                        while self.nr_attached > detach_until {
                            if self.rot180 {
                                // Outputs immediately overwritten by the detach(0) below.
                                self.detach(1);
                            }
                            let (p, pt, ti, ap) = self.detach(0);
                            poly_no = p;
                            point_no = pt;
                            transform_index = ti;
                            attach_point = ap;
                            let src = self.nr_attached + 1 + self.rot180 as usize;
                            if src < nr {
                                for i in 0..nr {
                                    self.reason[self.nr_attached * nr + i] |=
                                        self.reason[src * nr + i];
                                }
                            }
                        }
                        another_attachment_try =
                            self.next_attach_try(&mut poly_no, &mut point_no, &mut transform_index);
                    }
                }
            } else if !done {
                if self.nr_attached == 0 {
                    done = true;
                } else {
                    if self.rot180 {
                        // Outputs immediately overwritten by the detach(0) below.
                        self.detach(1);
                    }
                    let (p, pt, ti, ap) = self.detach(0);
                    poly_no = p;
                    point_no = pt;
                    transform_index = ti;
                    attach_point = ap;
                }
                another_attachment_try =
                    self.next_attach_try(&mut poly_no, &mut point_no, &mut transform_index);
            }
        }

        self.wait = if self.nr_attached == nr { 100 } else { 0 };
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if self.box_size <= 0 {
            return;
        }
        if self.use_bitmaps {
            self.draw_with_bitmaps(buffer, width, height);
        } else {
            self.draw_without_bitmaps(buffer, width, height);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.win_w = config.width;
        self.win_h = config.height;
        self.npixels = if config.ncolors <= 0 { DEF_NCOLORS } else { config.ncolors };
        self.cycles = if config.cycles <= 0 { DEF_CYCLES } else { config.cycles };
        self.delay_us = DEF_DELAY_US;
        self.init();
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
