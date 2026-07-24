//! Bouncing polyhedra.
//
// Copyright (c) 1987  X Consortium
//
// Permission to use, copy, modify, and distribute this software and its
// documentation for any purpose and without fee is hereby granted,
// provided that the above copyright notice appear in all copies and that
// both that copyright notice and this permission notice appear in
// supporting documentation.
//
// This file is provided AS IS with no warranties of any kind.  The author
// shall have no liability with respect to the infringement of copyrights,
// trade secrets or any patents by this file or any part thereof.  In no
// event will the author be liable for any lost revenue or profits or
// other special, indirect and consequential damages.
//
// Rust port of xlockmore/modes/ico.c.

#![allow(dead_code, unused_assignments)]
use rand::Rng;
use std::f64::consts::PI;
use crate::animation::primitives::{draw_line, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const MAXVERTS: usize = 120;
const POLYSIZE: usize = 9;
const MINSIZE: i32 = 5;
const DEFAULT_DELTAX: i32 = 13;
const DEFAULT_DELTAY: i32 = 9;
// Render at 60fps for smooth tumbling. Upstream ico.c ticks at 10fps and
// rotates 5°/tick (50°/s); its slow clock beats against the 60Hz frame
// callback and reads as stutter, and the 5° jumps look fast/chunky. We tick
// 6x as often and scale per-tick rotation by 1/6, so real-time angular
// velocity is unchanged but the motion is smooth.
//
// Tick a hair under the player's 16_666us REFRESH_US so it uses the
// sub-refresh catch-up path (max_ticks > 1). At exactly 16_666 ico sat on
// the knife-edge with max_ticks == 1: any frame callback that jittered even
// a few us early produced a 0-tick frame (a repeated frame = visible
// stutter) that the once-per-frame path could never catch up. The ~666us
// margin covers normal callback jitter. ~62fps nominal; imperceptible.
const ICO_FPS: u64 = 60;
const ICO_TICK_US: u64 = 16_000;
const FPS_SCALE: f64 = 10.0 / ICO_FPS as f64; // upstream 10fps → our 60fps

#[derive(Clone, Copy, Default)]
struct Point3D {
    x: f64,
    y: f64,
    z: f64,
}

struct Polyinfo {
    numverts: usize,
    numedges: usize,
    numfaces: usize,
    v: &'static [Point3D],
    f: &'static [usize],
}

type Transform3D = [[f64; 4]; 4];

fn ident_mat(m: &mut Transform3D) {
    for i in 0..4 {
        for j in 0..4 {
            m[i][j] = 0.0;
        }
        m[i][i] = 1.0;
    }
}

fn format_rotate_mat(axis: char, angle: f64, m: &mut Transform3D) {
    ident_mat(m);
    let s = angle.sin();
    let c = angle.cos();
    match axis {
        'x' => {
            m[1][1] = c; m[2][2] = c;
            m[1][2] = s; m[2][1] = -s;
        }
        'y' => {
            m[0][0] = c; m[2][2] = c;
            m[2][0] = s; m[0][2] = -s;
        }
        'z' => {
            m[0][0] = c; m[1][1] = c;
            m[0][1] = s; m[1][0] = -s;
        }
        _ => {}
    }
}

fn concat_mat(l: &Transform3D, r: &Transform3D, m: &mut Transform3D) {
    for i in 0..4 {
        for j in 0..4 {
            m[i][j] = l[i][0] * r[0][j]
                    + l[i][1] * r[1][j]
                    + l[i][2] * r[2][j]
                    + l[i][3] * r[3][j];
        }
    }
}

fn partial_non_hom_transform(n: usize, m: &Transform3D, input: &[Point3D], output: &mut [Point3D]) {
    for i in 0..n {
        let in_pt = &input[i];
        output[i].x = in_pt.x * m[0][0] + in_pt.y * m[1][0] + in_pt.z * m[2][0];
        output[i].y = in_pt.x * m[0][1] + in_pt.y * m[1][1] + in_pt.z * m[2][1];
        output[i].z = in_pt.x * m[0][2] + in_pt.y * m[1][2] + in_pt.z * m[2][2];
    }
}

static POLYGONS: &[Polyinfo] = &[
    Polyinfo {
        numverts: 4, numedges: 6, numfaces: 4,
        v: &[
            Point3D { x: 0.57735, y: 0.57735, z: 0.57735 },
            Point3D { x: 0.57735, y: -0.57735, z: -0.57735 },
            Point3D { x: -0.57735, y: 0.57735, z: -0.57735 },
            Point3D { x: -0.57735, y: -0.57735, z: 0.57735 },
        ],
        f: &[
            3, 2, 1, 0,
            3, 1, 3, 0,
            3, 3, 2, 0,
            3, 2, 3, 1,
        ],
    },
    Polyinfo {
        numverts: 8, numedges: 12, numfaces: 6,
        v: &[
            Point3D { x: 0.57735, y: 0.57735, z: 0.57735 },
            Point3D { x: 0.57735, y: 0.57735, z: -0.57735 },
            Point3D { x: 0.57735, y: -0.57735, z: -0.57735 },
            Point3D { x: 0.57735, y: -0.57735, z: 0.57735 },
            Point3D { x: -0.57735, y: 0.57735, z: 0.57735 },
            Point3D { x: -0.57735, y: 0.57735, z: -0.57735 },
            Point3D { x: -0.57735, y: -0.57735, z: -0.57735 },
            Point3D { x: -0.57735, y: -0.57735, z: 0.57735 },
        ],
        f: &[
            4, 0, 1, 2, 3,
            4, 7, 6, 5, 4,
            4, 1, 0, 4, 5,
            4, 3, 2, 6, 7,
            4, 2, 1, 5, 6,
            4, 0, 3, 7, 4,
        ],
    },
    Polyinfo {
        numverts: 6, numedges: 12, numfaces: 8,
        v: &[
            Point3D { x: 1.0, y: 0.0, z: 0.0 },
            Point3D { x: -1.0, y: 0.0, z: 0.0 },
            Point3D { x: 0.0, y: 1.0, z: 0.0 },
            Point3D { x: 0.0, y: -1.0, z: 0.0 },
            Point3D { x: 0.0, y: 0.0, z: 1.0 },
            Point3D { x: 0.0, y: 0.0, z: -1.0 },
        ],
        f: &[
            3, 0, 4, 2,
            3, 0, 2, 5,
            3, 0, 5, 3,
            3, 0, 3, 4,
            3, 1, 2, 4,
            3, 1, 5, 2,
            3, 1, 3, 5,
            3, 1, 4, 3,
        ],
    },
    Polyinfo {
        numverts: 20, numedges: 30, numfaces: 12,
        v: &[
            Point3D { x: 0.0, y: 0.3090169943749474241, z: 0.8090169943749474241 },
            Point3D { x: 0.0, y: -0.3090169943749474241, z: 0.8090169943749474241 },
            Point3D { x: 0.0, y: -0.3090169943749474241, z: -0.8090169943749474241 },
            Point3D { x: 0.0, y: 0.3090169943749474241, z: -0.8090169943749474241 },
            Point3D { x: 0.8090169943749474241, y: 0.0, z: 0.3090169943749474241 },
            Point3D { x: -0.8090169943749474241, y: 0.0, z: 0.3090169943749474241 },
            Point3D { x: -0.8090169943749474241, y: 0.0, z: -0.3090169943749474241 },
            Point3D { x: 0.8090169943749474241, y: 0.0, z: -0.3090169943749474241 },
            Point3D { x: 0.3090169943749474241, y: 0.8090169943749474241, z: 0.0 },
            Point3D { x: -0.3090169943749474241, y: 0.8090169943749474241, z: 0.0 },
            Point3D { x: -0.3090169943749474241, y: -0.8090169943749474241, z: 0.0 },
            Point3D { x: 0.3090169943749474241, y: -0.8090169943749474241, z: 0.0 },
            Point3D { x: 0.5, y: 0.5, z: 0.5 },
            Point3D { x: -0.5, y: 0.5, z: 0.5 },
            Point3D { x: -0.5, y: -0.5, z: 0.5 },
            Point3D { x: 0.5, y: -0.5, z: 0.5 },
            Point3D { x: 0.5, y: -0.5, z: -0.5 },
            Point3D { x: 0.5, y: 0.5, z: -0.5 },
            Point3D { x: -0.5, y: 0.5, z: -0.5 },
            Point3D { x: -0.5, y: -0.5, z: -0.5 },
        ],
        f: &[
            5, 12, 8, 17, 7, 4,
            5, 5, 6, 18, 9, 13,
            5, 14, 10, 19, 6, 5,
            5, 12, 4, 15, 1, 0,
            5, 13, 9, 8, 12, 0,
            5, 1, 14, 5, 13, 0,
            5, 16, 7, 17, 3, 2,
            5, 19, 10, 11, 16, 2,
            5, 3, 18, 6, 19, 2,
            5, 15, 11, 10, 14, 1,
            5, 3, 17, 8, 9, 18,
            5, 4, 7, 16, 11, 15,
        ],
    },
    Polyinfo {
        numverts: 12, numedges: 30, numfaces: 20,
        v: &[
            Point3D { x: 0.0, y: 0.0, z: -0.9510565162951535721 },
            Point3D { x: 0.0, y: 0.8506508083520399322, z: -0.42532537 },
            Point3D { x: 0.8090169943749474241, y: 0.26286556, z: -0.42532537 },
            Point3D { x: 0.5, y: -0.68819095, z: -0.42532537 },
            Point3D { x: -0.5, y: -0.68819095, z: -0.42532537 },
            Point3D { x: -0.8090169943749474241, y: 0.26286556, z: -0.42532537 },
            Point3D { x: 0.5, y: 0.68819095, z: 0.42532537 },
            Point3D { x: 0.8090169943749474241, y: -0.26286556, z: 0.42532537 },
            Point3D { x: 0.0, y: -0.8506508083520399322, z: 0.42532537 },
            Point3D { x: -0.8090169943749474241, y: -0.26286556, z: 0.42532537 },
            Point3D { x: -0.5, y: 0.68819095, z: 0.42532537 },
            Point3D { x: 0.0, y: 0.0, z: 0.9510565162951535721 },
        ],
        f: &[
            3, 0, 2, 1,
            3, 0, 3, 2,
            3, 0, 4, 3,
            3, 0, 5, 4,
            3, 0, 1, 5,
            3, 1, 6, 10,
            3, 1, 2, 6,
            3, 2, 7, 6,
            3, 2, 3, 7,
            3, 3, 8, 7,
            3, 3, 4, 8,
            3, 4, 9, 8,
            3, 4, 5, 9,
            3, 5, 10, 9,
            3, 5, 1, 10,
            3, 10, 6, 11,
            3, 6, 7, 11,
            3, 7, 8, 11,
            3, 8, 9, 11,
            3, 9, 10, 11,
        ],
    },
    Polyinfo {
        numverts: 14, numedges: 24, numfaces: 12,
        v: &[
            Point3D { x: 1.0, y: 0.0, z: 0.0 },
            Point3D { x: 0.0, y: 1.0, z: 0.0 },
            Point3D { x: 0.0, y: 0.0, z: 1.0 },
            Point3D { x: 0.5, y: 0.5, z: 0.5 },
            Point3D { x: 0.5, y: 0.5, z: -0.5 },
            Point3D { x: 0.5, y: -0.5, z: 0.5 },
            Point3D { x: 0.5, y: -0.5, z: -0.5 },
            Point3D { x: -0.5, y: 0.5, z: 0.5 },
            Point3D { x: -0.5, y: 0.5, z: -0.5 },
            Point3D { x: -0.5, y: -0.5, z: 0.5 },
            Point3D { x: -0.5, y: -0.5, z: -0.5 },
            Point3D { x: -1.0, y: 0.0, z: 0.0 },
            Point3D { x: 0.0, y: -1.0, z: 0.0 },
            Point3D { x: 0.0, y: 0.0, z: -1.0 },
        ],
        f: &[
            4, 0, 3, 2, 5,
            4, 2, 9, 12, 5,
            4, 2, 7, 11, 9,
            4, 1, 7, 2, 3,
            4, 0, 4, 1, 3,
            4, 0, 5, 12, 6,
            4, 9, 11, 10, 12,
            4, 1, 8, 11, 7,
            4, 0, 6, 13, 4,
            4, 6, 12, 10, 13,
            4, 8, 13, 10, 11,
            4, 1, 4, 13, 8,
        ],
    },
    Polyinfo {
        numverts: 32, numedges: 60, numfaces: 30,
        v: &[
            Point3D { x: 0.0, y: 0.3090169943749474241, z: 0.8090169943749474241 },
            Point3D { x: 0.0, y: -0.3090169943749474241, z: 0.8090169943749474241 },
            Point3D { x: 0.0, y: -0.3090169943749474241, z: -0.8090169943749474241 },
            Point3D { x: 0.0, y: 0.3090169943749474241, z: -0.8090169943749474241 },
            Point3D { x: 0.8090169943749474241, y: 0.0, z: 0.3090169943749474241 },
            Point3D { x: -0.8090169943749474241, y: 0.0, z: 0.3090169943749474241 },
            Point3D { x: -0.8090169943749474241, y: 0.0, z: -0.3090169943749474241 },
            Point3D { x: 0.8090169943749474241, y: 0.0, z: -0.3090169943749474241 },
            Point3D { x: 0.3090169943749474241, y: 0.8090169943749474241, z: 0.0 },
            Point3D { x: -0.3090169943749474241, y: 0.8090169943749474241, z: 0.0 },
            Point3D { x: -0.3090169943749474241, y: -0.8090169943749474241, z: 0.0 },
            Point3D { x: 0.3090169943749474241, y: -0.8090169943749474241, z: 0.0 },
            Point3D { x: 0.5, y: 0.5, z: 0.5 },
            Point3D { x: -0.5, y: 0.5, z: 0.5 },
            Point3D { x: -0.5, y: -0.5, z: 0.5 },
            Point3D { x: 0.5, y: -0.5, z: 0.5 },
            Point3D { x: 0.5, y: -0.5, z: -0.5 },
            Point3D { x: 0.5, y: 0.5, z: -0.5 },
            Point3D { x: -0.5, y: 0.5, z: -0.5 },
            Point3D { x: -0.5, y: -0.5, z: -0.5 },
            Point3D { x: 0.0, y: 0.8090169943749474241, z: 0.5 },
            Point3D { x: 0.0, y: -0.8090169943749474241, z: 0.5 },
            Point3D { x: 0.0, y: 0.8090169943749474241, z: -0.5 },
            Point3D { x: 0.0, y: -0.8090169943749474241, z: -0.5 },
            Point3D { x: 0.5, y: 0.0, z: 0.8090169943749474241 },
            Point3D { x: -0.5, y: 0.0, z: 0.8090169943749474241 },
            Point3D { x: 0.5, y: 0.0, z: -0.8090169943749474241 },
            Point3D { x: -0.5, y: 0.0, z: -0.8090169943749474241 },
            Point3D { x: 0.8090169943749474241, y: 0.5, z: 0.0 },
            Point3D { x: -0.8090169943749474241, y: 0.5, z: 0.0 },
            Point3D { x: 0.8090169943749474241, y: -0.5, z: 0.0 },
            Point3D { x: -0.8090169943749474241, y: -0.5, z: 0.0 },
        ],
        f: &[
            4, 8, 20, 12, 28,
            4, 0, 24, 12, 20,
            4, 4, 28, 12, 24,
            4, 8, 22, 9, 20,
            4, 8, 28, 17, 22,
            4, 9, 29, 13, 20,
            4, 0, 20, 13, 25,
            4, 0, 25, 1, 24,
            4, 5, 25, 13, 29,
            4, 9, 22, 18, 29,
            4, 3, 27, 18, 22,
            4, 3, 22, 17, 26,
            4, 6, 29, 18, 27,
            4, 7, 26, 17, 28,
            4, 2, 27, 3, 26,
            4, 4, 30, 7, 28,
            4, 4, 24, 15, 30,
            4, 7, 30, 16, 26,
            4, 1, 21, 15, 24,
            4, 1, 25, 14, 21,
            4, 11, 30, 15, 21,
            4, 11, 23, 16, 30,
            4, 2, 26, 16, 23,
            4, 10, 23, 11, 21,
            4, 2, 23, 19, 27,
            4, 10, 31, 19, 23,
            4, 10, 21, 14, 31,
            4, 5, 31, 14, 25,
            4, 5, 29, 6, 31,
            4, 6, 27, 19, 31,
        ],
    },
    Polyinfo {
        numverts: 24, numedges: 36, numfaces: 14,
        v: &[
            Point3D { x: 0.8506508083520399322, y: 0.42532537, z: 0.0 },
            Point3D { x: 0.8506508083520399322, y: -0.42532537, z: 0.0 },
            Point3D { x: 0.8506508083520399322, y: 0.0, z: 0.42532537 },
            Point3D { x: 0.8506508083520399322, y: 0.0, z: -0.42532537 },
            Point3D { x: -0.8506508083520399322, y: 0.42532537, z: 0.0 },
            Point3D { x: -0.8506508083520399322, y: -0.42532537, z: 0.0 },
            Point3D { x: -0.8506508083520399322, y: 0.0, z: 0.42532537 },
            Point3D { x: -0.8506508083520399322, y: 0.0, z: -0.42532537 },
            Point3D { x: 0.42532537, y: 0.8506508083520399322, z: 0.0 },
            Point3D { x: -0.42532537, y: 0.8506508083520399322, z: 0.0 },
            Point3D { x: 0.0, y: 0.8506508083520399322, z: 0.42532537 },
            Point3D { x: 0.0, y: 0.8506508083520399322, z: -0.42532537 },
            Point3D { x: 0.42532537, y: -0.8506508083520399322, z: 0.0 },
            Point3D { x: -0.42532537, y: -0.8506508083520399322, z: 0.0 },
            Point3D { x: 0.0, y: -0.8506508083520399322, z: 0.42532537 },
            Point3D { x: 0.0, y: -0.8506508083520399322, z: -0.42532537 },
            Point3D { x: 0.42532537, y: 0.0, z: 0.8506508083520399322 },
            Point3D { x: -0.42532537, y: 0.0, z: 0.8506508083520399322 },
            Point3D { x: 0.0, y: 0.42532537, z: 0.8506508083520399322 },
            Point3D { x: 0.0, y: -0.42532537, z: 0.8506508083520399322 },
            Point3D { x: 0.42532537, y: 0.0, z: -0.8506508083520399322 },
            Point3D { x: -0.42532537, y: 0.0, z: -0.8506508083520399322 },
            Point3D { x: 0.0, y: 0.42532537, z: -0.8506508083520399322 },
            Point3D { x: 0.0, y: -0.42532537, z: -0.8506508083520399322 },
        ],
        f: &[
            4, 0, 2, 1, 3,
            4, 4, 7, 5, 6,
            4, 8, 11, 9, 10,
            4, 12, 14, 13, 15,
            4, 16, 18, 17, 19,
            4, 20, 23, 21, 22,
            6, 2, 0, 8, 10, 18, 16,
            6, 0, 3, 20, 22, 11, 8,
            6, 1, 2, 16, 19, 14, 12,
            6, 3, 1, 12, 15, 23, 20,
            6, 4, 6, 17, 18, 10, 9,
            6, 7, 4, 9, 11, 22, 21,
            6, 6, 5, 13, 14, 19, 17,
            6, 5, 7, 21, 23, 15, 13,
        ],
    },
    Polyinfo {
        numverts: 12, numedges: 24, numfaces: 14,
        v: &[
            Point3D { x: -0.5, y: -0.86602540, z: 0.0 },
            Point3D { x: -1.0, y: 0.0, z: 0.0 },
            Point3D { x: -0.5, y: 0.86602540, z: 0.0 },
            Point3D { x: 0.5, y: 0.86602540, z: 0.0 },
            Point3D { x: 1.0, y: 0.0, z: 0.0 },
            Point3D { x: 0.5, y: -0.86602540, z: 0.0 },
            Point3D { x: -0.5, y: -0.28867513, z: -0.81649658 },
            Point3D { x: 0.0, y: 0.57735027, z: -0.81649658 },
            Point3D { x: 0.5, y: -0.28867513, z: -0.81649658 },
            Point3D { x: 0.0, y: -0.57735027, z: 0.81649658 },
            Point3D { x: 0.5, y: 0.28867513, z: 0.81649658 },
            Point3D { x: -0.5, y: 0.28867513, z: 0.81649658 },
        ],
        f: &[
            3, 0, 1, 6,
            4, 0, 6, 8, 5,
            3, 0, 5, 9,
            4, 0, 9, 11, 1,
            4, 1, 2, 7, 6,
            3, 1, 11, 2,
            3, 2, 3, 7,
            4, 2, 11, 10, 3,
            4, 3, 4, 8, 7,
            3, 3, 10, 4,
            3, 4, 5, 8,
            4, 4, 10, 9, 5,
            3, 6, 7, 8,
            3, 9, 10, 11,
        ],
    },
];

pub struct Ico {
    width: u32,
    height: u32,
    loopcount: i32,
    object: usize,
    linewidth: i32,
    color_idx: usize,
    poly_w: i32,
    poly_h: i32,
    curr_x: f64,
    curr_y: f64,
    prev_x: f64,
    prev_y: f64,
    poly_delta_x: f64,
    poly_delta_y: f64,
    faces: bool,
    edges: bool,
    opaque: bool,
    xv_buffer: usize,
    xform: Transform3D,
    xv: [[Point3D; MAXVERTS]; 2],
    wo2: f64,
    ho2: f64,
    color_offset: usize,
    cycles: i32,
    ncolors: i32,
}

impl Ico {
    fn init_poly(&mut self, init: bool) {
        let poly = &POLYGONS[self.object];
        let nv = poly.numverts;

        let mut r1 = [[0.0; 4]; 4];
        let mut r2 = [[0.0; 4]; 4];

        let roll = 5.0 * FPS_SCALE * PI / 180.0;

        if (self.poly_delta_x < 0.0 && self.poly_delta_y < 0.0) || (self.poly_delta_x > 0.0 && self.poly_delta_y > 0.0) {
            format_rotate_mat('x', if self.poly_delta_x > 0.0 { -roll } else { roll }, &mut r1);
            format_rotate_mat('y', if self.poly_delta_y < 0.0 { -roll } else { roll }, &mut r2);
        } else {
            format_rotate_mat('x', if self.poly_delta_x < 0.0 { -roll } else { roll }, &mut r1);
            format_rotate_mat('y', if self.poly_delta_y > 0.0 { -roll } else { roll }, &mut r2);
        }

        concat_mat(&r1, &r2, &mut self.xform);

        if init {
            for i in 0..nv {
                self.xv[0][i] = poly.v[i];
            }
            self.xv_buffer = 0;
            self.wo2 = self.poly_w as f64 / 2.0;
            self.ho2 = self.poly_h as f64 / 2.0;
        }
    }
}

impl Animation for Ico {
    fn new(config: &AnimConfig) -> Self {
        let mut i = Ico {
            width: config.width,
            height: config.height,
            loopcount: 0,
            object: 0,
            linewidth: 0,
            color_idx: 0,
            poly_w: 0,
            poly_h: 0,
            curr_x: 0.0,
            curr_y: 0.0,
            prev_x: 0.0,
            prev_y: 0.0,
            poly_delta_x: 0.0,
            poly_delta_y: 0.0,
            faces: false,
            edges: true,
            opaque: true,
            xv_buffer: 0,
            xform: [[0.0; 4]; 4],
            xv: [[Point3D::default(); MAXVERTS]; 2],
            wo2: 0.0,
            ho2: 0.0,
            color_offset: 0,
            cycles: config.cycles,
            ncolors: config.ncolors,
        };
        i.reset(config);
        i
    }

    fn tick(&mut self) {
        let _rng = rand::rng();

        self.loopcount += 1;
        if self.cycles > 0 && self.loopcount > self.cycles {
            // Need a dummy config to reuse logic if we wanted, or just call reset with partial setup.
            // In XLockMore, it does init_ico(mi).
            // We'll just reset here.
            
            // XLockMore behavior: pick next object if count <= 0.
            self.object = (self.object + 1) % POLYSIZE;
            self.loopcount = 0;
            self.init_poly(true);
        }

        self.prev_x = self.curr_x;
        self.prev_y = self.curr_y;

        self.curr_x += self.poly_delta_x;
        if self.curr_x < 0.0 || self.curr_x + self.poly_w as f64 > self.width as f64 {
            self.curr_x -= 2.0 * self.poly_delta_x;
            self.poly_delta_x = -self.poly_delta_x;
            self.init_poly(false);
        }

        self.curr_y += self.poly_delta_y;
        if self.curr_y < 0.0 || self.curr_y + self.poly_h as f64 > self.height as f64 {
            self.curr_y -= 2.0 * self.poly_delta_y;
            self.poly_delta_y = -self.poly_delta_y;
            self.init_poly(false);
        }

        let poly = &POLYGONS[self.object];
        let nv = poly.numverts;

        self.xv_buffer = 1 - self.xv_buffer;
        
        let _prev_buf = 1 - self.xv_buffer;
        
        let (src, dest) = if self.xv_buffer == 1 {
            let (first, second) = self.xv.split_at_mut(1);
            (&first[0], &mut second[0])
        } else {
            let (first, second) = self.xv.split_at_mut(1);
            (&second[0], &mut first[0])
        };

        partial_non_hom_transform(nv, &self.xform, src, dest);
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let poly = &POLYGONS[self.object];
        let nv = poly.numverts;
        let nf = poly.numfaces;

        let pxv = &self.xv[self.xv_buffer];
        let mut v2 = [(0i32, 0i32); MAXVERTS];

        for i in 0..nv {
            v2[i] = (
                (((pxv[i].x + 1.0) * self.wo2) + self.curr_x).round() as i32,
                (((pxv[i].y + 1.0) * self.ho2) + self.curr_y).round() as i32,
            );
        }

        let mut drawn_edges = [[false; MAXVERTS]; MAXVERTS];
        let mut drawn_points = [false; MAXVERTS];

        let mut pf_idx = 0;
        for i in (0..nf).rev() {
            let pcount = poly.f[pf_idx];
            pf_idx += 1;

            let mut pxvz = 0.0;
            for j in 0..pcount {
                let p0 = poly.f[pf_idx + j];
                pxvz += pxv[p0].z;
            }

            if pxvz < 0.0 && (self.faces || self.opaque) {
                pf_idx += pcount;
                continue;
            }

            // In our rust port, we only do wireframe (edges) since we don't have FillPolygon easily accessible in primitives.
            // xlockmore supports faces if available. We will just draw edges for all non-backfacing.
            if self.edges {
                for j in 0..pcount {
                    let k = if j < pcount - 1 { j + 1 } else { 0 };
                    let p0 = poly.f[pf_idx + j];
                    let p1 = poly.f[pf_idx + k];
                    if !drawn_edges[p0][p1] {
                        drawn_edges[p0][p1] = true;
                        drawn_edges[p1][p0] = true;

                        let color = if self.ncolors > 2 {
                            let hue = (self.color_idx as f32 / self.ncolors as f32) + (i as f32 / nf as f32);
                            Color::from_hsl(hue.fract(), 1.0, 0.5)
                        } else {
                            Color::new(255, 255, 255, 255)
                        };

                        draw_line(
                            buffer, width, height,
                            v2[p0].0, v2[p0].1,
                            v2[p1].0, v2[p1].1,
                            color
                        );
                    }
                }
            } else {
                for j in 0..pcount {
                    let p0 = poly.f[pf_idx + j];
                    if !drawn_points[p0] {
                        drawn_points[p0] = true;

                        let color = if self.ncolors > 2 {
                            Color::from_hsl(self.color_idx as f32 / self.ncolors as f32, 1.0, 0.5)
                        } else {
                            Color::new(255, 255, 255, 255)
                        };
                        draw_line(
                            buffer, width, height,
                            v2[p0].0, v2[p0].1,
                            v2[p0].0, v2[p0].1,
                            color
                        );
                    }
                }
            }
            pf_idx += pcount;
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;
        self.linewidth = rng.random_range(2..((self.width + self.height) / 200 + 3)) as i32;

        self.edges = true; // xlockmore default unless random
        self.faces = false; // We don't implement fill_polygon
        self.opaque = true;

        let _size = 0; // Use default
        self.poly_w = (self.width.min(self.height) / 4).max(MINSIZE as u32) as i32;
        self.poly_h = self.poly_w;

        // Translation is deliberately NOT scaled by FPS_SCALE: rotation is
        // matched to upstream's real-time angular velocity, but the
        // drift/bounce reads better at the port's full 60fps px/tick rate
        // (owner preference — upstream's true drift felt sluggish here).
        self.poly_delta_x = ((self.poly_w as f64 / DEFAULT_DELTAY as f64 + 1.0) / 6.0).round().max(1.0);
        self.poly_delta_y = ((self.poly_h as f64 / DEFAULT_DELTAX as f64 + 1.0) / 6.0).round().max(1.0);

        self.curr_x = rng.random_range(0..((self.width as i32 - self.poly_w).max(1))) as f64;
        self.curr_y = rng.random_range(0..((self.height as i32 - self.poly_h).max(1))) as f64;

        self.poly_delta_x *= if rng.random::<bool>() { 1.0 } else { -1.0 };
        self.poly_delta_y *= if rng.random::<bool>() { 1.0 } else { -1.0 };

        self.loopcount = 0;

        // Choose random object if count <= 0
        self.object = if config.count <= 0 { rng.random_range(0..POLYSIZE) } else { config.count as usize % POLYSIZE };

        if self.ncolors > 2 {
            self.color_idx = rng.random_range(0..self.ncolors as usize);
        }

        self.color_offset = rng.random_range(0..self.ncolors.max(1) as usize);

        self.init_poly(true);
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        ICO_TICK_US
    }
}
