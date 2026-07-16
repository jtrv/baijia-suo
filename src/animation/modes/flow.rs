//! Flow of strange bees.
//
// Copyright (c) 1996 by Tim Auckland <tda10.geo AT yahoo.com>
// Incorporating some code from Stephen Davies Copyright (c) 2000
//
// Search code based on techniques described in "Strange Attractors:
// Creating Patterns in Chaos" by Julien C. Sprott
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
// Rust port of xlockmore/modes/flow.c.

#![allow(dead_code, unused_variables, unused_assignments, unused_imports)]
use rand::Rng;
use std::f64;

use crate::animation::primitives::{draw_line, Color};
use crate::animation::{AnimConfig, Animation};

const LOST_IN_SPACE: f64 = 2000.0;
const INITIALSTEP: f64 = 0.04;
const EYEHEIGHT: f64 = 0.005;
const MINTRAIL: usize = 2;
const BOX_L: usize = 36;

const BOX_PTS: [[f64; 3]; 32] = [
    [1.0, 1.0, 1.0],   // 0
    [1.0, 1.0, -1.0],  // 1
    [1.0, -1.0, -1.0], // 2
    [1.0, -1.0, 1.0],  // 3
    [-1.0, 1.0, 1.0],  // 4
    [-1.0, 1.0, -1.0], // 5
    [-1.0, -1.0, -1.0], // 6
    [-1.0, -1.0, 1.0],  // 7
    [1.0, 0.8, 0.8],
    [1.0, 0.8, -0.8],
    [1.0, -0.8, -0.8],
    [1.0, -0.8, 0.8],
    [0.8, 1.0, 0.8],
    [0.8, 1.0, -0.8],
    [-0.8, 1.0, -0.8],
    [-0.8, 1.0, 0.8],
    [0.8, 0.8, 1.0],
    [0.8, -0.8, 1.0],
    [-0.8, -0.8, 1.0],
    [-0.8, 0.8, 1.0],
    [-1.0, 0.8, 0.8],
    [-1.0, 0.8, -0.8],
    [-1.0, -0.8, -0.8],
    [-1.0, -0.8, 0.8],
    [0.8, -1.0, 0.8],
    [0.8, -1.0, -0.8],
    [-0.8, -1.0, -0.8],
    [-0.8, -1.0, 0.8],
    [0.8, 0.8, -1.0],
    [0.8, -0.8, -1.0],
    [-0.8, -0.8, -1.0],
    [-0.8, 0.8, -1.0],
];

const LINES: [[usize; 2]; 36] = [
    [0, 1], [1, 2], [2, 3], [3, 0],
    [4, 5], [5, 6], [6, 7], [7, 4],
    [0, 4], [1, 5], [2, 6], [3, 7],
    [8, 9], [9, 10], [10, 11], [11, 8],
    [12, 13], [13, 14], [14, 15], [15, 12],
    [16, 17], [17, 18], [18, 19], [19, 16],
    [20, 21], [21, 22], [22, 23], [23, 20],
    [24, 25], [25, 26], [26, 27], [27, 24],
    [28, 29], [29, 30], [30, 31], [31, 28],
];

const C: usize = 0;
const X: usize = 1;
const XX: usize = 2;
const XXX: usize = 3;
const XXY: usize = 4;
const XXZ: usize = 5;
const XY: usize = 6;
const XYY: usize = 7;
const XYZ: usize = 8;
const XZ: usize = 9;
const XZZ: usize = 10;
const Y: usize = 11;
const YY: usize = 12;
const YYY: usize = 13;
const YYZ: usize = 14;
const YZ: usize = 15;
const YZZ: usize = 16;
const Z: usize = 17;
const ZZ: usize = 18;
const ZZZ: usize = 19;
const SINY: usize = XY;

#[derive(Clone, Copy, Default)]
struct DVector {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Copy, PartialEq)]
#[derive(Default)]
enum OdeType {
    #[default]
    Cubic,
    Periodic,
}


#[derive(Clone, Copy, PartialEq)]
#[derive(Default)]
enum Chaseto {
    #[default]
    Orbit,
    Bee,
}


fn gauss_rand(rng: &mut impl rand::Rng, a: f64) -> f64 {
    let mut x: f64;
    let mut y: f64;
    let mut w: f64;
    loop {
        x = 2.0 * rng.random_range(0.0..1.0) - 1.0;
        y = 2.0 * rng.random_range(0.0..1.0) - 1.0;
        w = x * x + y * y;
        if w < 1.0 && w > 0.0 {
            break;
        }
    }
    w = (-2.0 * w.ln() / w).sqrt();
    (a / 3.0) * x * w
}

fn balance_rand(rng: &mut impl rand::Rng, v: f64) -> f64 {
    rng.random_range(0.0..1.0) * v - (v / 2.0)
}

fn cubic(a: &[DVector; 20], x: f64, y: f64, z: f64) -> DVector {
    DVector {
        x: a[C].x + a[X].x*x + a[XX].x*x*x + a[XXX].x*x*x*x + a[XXY].x*x*x*y +
            a[XXZ].x*x*x*z + a[XY].x*x*y + a[XYY].x*x*y*y + a[XYZ].x*x*y*z +
            a[XZ].x*x*z + a[XZZ].x*x*z*z + a[Y].x*y + a[YY].x*y*y +
            a[YYY].x*y*y*y + a[YYZ].x*y*y*z + a[YZ].x*y*z + a[YZZ].x*y*z*z +
            a[Z].x*z + a[ZZ].x*z*z + a[ZZZ].x*z*z*z,
        y: a[C].y + a[X].y*x + a[XX].y*x*x + a[XXX].y*x*x*x + a[XXY].y*x*x*y +
            a[XXZ].y*x*x*z + a[XY].y*x*y + a[XYY].y*x*y*y + a[XYZ].y*x*y*z +
            a[XZ].y*x*z + a[XZZ].y*x*z*z + a[Y].y*y + a[YY].y*y*y +
            a[YYY].y*y*y*y + a[YYZ].y*y*y*z + a[YZ].y*y*z + a[YZZ].y*y*z*z +
            a[Z].y*z + a[ZZ].y*z*z + a[ZZZ].y*z*z*z,
        z: a[C].z + a[X].z*x + a[XX].z*x*x + a[XXX].z*x*x*x + a[XXY].z*x*x*y +
            a[XXZ].z*x*x*z + a[XY].z*x*y + a[XYY].z*x*y*y + a[XYZ].z*x*y*z +
            a[XZ].z*x*z + a[XZZ].z*x*z*z + a[Y].z*y + a[YY].z*y*y +
            a[YYY].z*y*y*y + a[YYZ].z*y*y*z + a[YZ].z*y*z + a[YZZ].z*y*z*z +
            a[Z].z*z + a[ZZ].z*z*z + a[ZZZ].z*z*z*z,
    }
}

fn periodic(a: &[DVector; 20], x: f64, y: f64, z: f64) -> DVector {
    DVector {
        x: a[C].x + a[X].x*x + a[XX].x*x*x + a[XXX].x*x*x*x +
            a[XXZ].x*x*x*z + a[XZ].x*x*z + a[XZZ].x*x*z*z + a[Z].x*z +
            a[ZZ].x*z*z + a[ZZZ].x*z*z*z + a[SINY].x * y.sin(),
        y: a[C].y,
        z: a[C].z + a[X].z*x + a[XX].z*x*x + a[XXX].z*x*x*x +
            a[XXZ].z*x*x*z + a[XZ].z*x*z + a[XZZ].z*x*z*z + a[Z].z*z +
            a[ZZ].z*z*z + a[ZZZ].z*z*z*z,
    }
}

fn iterate(p: &mut DVector, ode_type: OdeType, par: &[DVector; 20], step: f64) -> f64 {
    let k1 = match ode_type {
        OdeType::Cubic => cubic(par, p.x, p.y, p.z),
        OdeType::Periodic => periodic(par, p.x, p.y, p.z),
    };
    let k1x = k1.x * step;
    let k1y = k1.y * step;
    let k1z = k1.z * step;

    let k2 = match ode_type {
        OdeType::Cubic => cubic(par, p.x + k1x, p.y + k1y, p.z + k1z),
        OdeType::Periodic => periodic(par, p.x + k1x, p.y + k1y, p.z + k1z),
    };
    let k2x = k2.x * step;
    let k2y = k2.y * step;
    let k2z = k2.z * step;

    let k3x = (k1x + k2x) / 2.0;
    let k3y = (k1y + k2y) / 2.0;
    let k3z = (k1z + k2z) / 2.0;

    p.x += k3x;
    p.y += k3y;
    p.z += k3z;

    k3x * k3x + k3y * k3y + k3z * k3z
}

fn clip(nx: f64, ny: f64, nz: f64, d: f64, s: &mut DVector, e: &mut DVector) -> bool {
    let front1 = nx * s.x + ny * s.y + nz * s.z >= -d;
    let front2 = nx * e.x + ny * e.y + nz * e.z >= -d;
    if !front1 && !front2 {
        return true;
    }
    if front1 && front2 {
        return false;
    }
    let wx = e.x - s.x;
    let wy = e.y - s.y;
    let wz = e.z - s.z;
    
    let t = (-d - nx * s.x - ny * s.y - nz * s.z) / (nx * wx + ny * wy + nz * wz);
    
    let px = s.x + wx * t;
    let py = s.y + wy * t;
    let pz = s.z + wz * t;
    
    if front2 {
        s.x = px;
        s.y = py;
        s.z = pz;
    } else {
        e.x = px;
        e.y = py;
        e.z = pz;
    }
    false
}

#[derive(Default)]
pub struct Flow {
    cam: [DVector; 3],
    chasetime: f64,
    chaseto: Chaseto,
    circle: [DVector; 2],
    centre: DVector,
    beecount: usize,
    taillen: usize,
    range: DVector,
    yperiod: f64,
    ode_type: OdeType,
    par: [DVector; 20],
    p: Vec<Vec<DVector>>,
    count: usize,
    lyap: f64,
    size: f64,
    mid: DVector,
    step: f64,
    par2: [DVector; 20],
    p2: [DVector; 2],
    count2: usize,
    lyap2: f64,
    size2: f64,
    mid2: DVector,
    step2: f64,
    rotatep: bool,
    ridep: bool,
    boxp: bool,
    periodicp: bool,
    searchp: bool,
    ncolors: usize,
    cycles: usize,
    delay_us: u64,
    m: [[f64; 3]; 3],
    swarm_count: usize,
    breaks: Vec<usize>,
    palette: Vec<Color>,

    count_arg: i32,
    size_arg: i32,
}

impl Flow {
    fn discover(&mut self) -> bool {
        let mut rng = rand::rng();
        if self.count2 == 0 {
            self.p2[0].x = gauss_rand(&mut rng, self.range.x);
            self.p2[0].y = if self.yperiod > 0.0 {
                balance_rand(&mut rng, self.range.y)
            } else {
                gauss_rand(&mut rng, self.range.y)
            };
            self.p2[0].z = gauss_rand(&mut rng, self.range.z);

            for _ in 0..1000 {
                iterate(&mut self.p2[0], self.ode_type, &self.par2, self.step2);
                if self.yperiod > 0.0 && self.p2[0].y > self.yperiod {
                    self.p2[0].y -= self.yperiod;
                }
                if self.p2[0].x.abs() > LOST_IN_SPACE ||
                   self.p2[0].y.abs() > LOST_IN_SPACE ||
                   self.p2[0].z.abs() > LOST_IN_SPACE {
                    return false;
                }
                self.count2 += 1;
            }
            self.p2[1].x = self.p2[0].x + 0.000001;
            self.p2[1].y = self.p2[0].y;
            self.p2[1].z = self.p2[0].z;
        }

        let mut min = self.p2[0];
        let mut max = self.p2[0];

        let mut lsum = 0.0;
        let mut nl = 0;
        let mut maxv2 = 0.0;
        let mut l = 0.0;

        for _ in 0..5000 {
            for i in 0..2 {
                let v2 = iterate(&mut self.p2[i], self.ode_type, &self.par2, self.step2);
                if self.yperiod > 0.0 && self.p2[i].y > self.yperiod {
                    self.p2[i].y -= self.yperiod;
                }
                if self.p2[i].x.abs() > LOST_IN_SPACE ||
                   self.p2[i].y.abs() > LOST_IN_SPACE ||
                   self.p2[i].z.abs() > LOST_IN_SPACE {
                    return false;
                }
                if v2 > maxv2 {
                    maxv2 = v2;
                }
            }

            if self.p2[0].x < min.x { min.x = self.p2[0].x; }
            else if self.p2[0].x > max.x { max.x = self.p2[0].x; }
            
            if self.p2[0].y < min.y { min.y = self.p2[0].y; }
            else if self.p2[0].y > max.y { max.y = self.p2[0].y; }
            
            if self.p2[0].z < min.z { min.z = self.p2[0].z; }
            else if self.p2[0].z > max.z { max.z = self.p2[0].z; }

            let dlx = self.p2[1].x - self.p2[0].x;
            let dly = self.p2[1].y - self.p2[0].y;
            let dlz = self.p2[1].z - self.p2[0].z;
            let dl2 = dlx * dlx + dly * dly + dlz * dlz;

            if dl2 > 0.0 {
                let df = 1e12 * dl2;
                let rs = 1.0 / df.sqrt();
                self.p2[1].x = self.p2[0].x + rs * dlx;
                self.p2[1].y = self.p2[0].y + rs * dly;
                self.p2[1].z = self.p2[0].z + rs * dlz;
                lsum += df.ln();
                nl += 1;
                l = std::f64::consts::LOG2_E / 2.0 * lsum / (nl as f64) / self.step2;
            }
            self.count2 += 1;
        }

        self.lyap2 = l;
        self.size2 = max.x - min.x;
        let mut s = max.y - min.y;
        if s > self.size2 { self.size2 = s; }
        s = max.z - min.z;
        if s > self.size2 { self.size2 = s; }

        self.mid2.x = (max.x + min.x) / 2.0;
        self.mid2.y = (max.y + min.y) / 2.0;
        self.mid2.z = (max.z + min.z) / 2.0;

        if maxv2.sqrt() > self.size2 * 0.2 {
            self.step2 /= 2.0;
        }
        true
    }

    fn restart_flow(&mut self) {
        let mut rng = rand::rng();
        self.count = 0;
        for b in 0..self.beecount {
            self.p[b][0].x = gauss_rand(&mut rng, self.range.x);
            self.p[b][0].y = if self.yperiod > 0.0 {
                balance_rand(&mut rng, self.range.y)
            } else {
                gauss_rand(&mut rng, self.range.y)
            };
            self.p[b][0].z = gauss_rand(&mut rng, self.range.z);
        }
    }

    fn do_init(&mut self) {
        let mut rng = rand::rng();
        
        self.count2 = 0;
        
        let size_arg = if self.size_arg == 0 { -10 } else { self.size_arg };
        if size_arg < -(MINTRAIL as i32) {
            let max_val = (-size_arg - (MINTRAIL as i32) + 1) as f64;
            let r = rng.random_range(0..(max_val.sqrt() as i32 + 1));
            self.taillen = (r * r) as usize + MINTRAIL;
        } else if size_arg < (MINTRAIL as i32) {
            self.taillen = MINTRAIL;
        } else {
            self.taillen = size_arg as usize;
        }
        
        if !self.rotatep && !self.ridep {
            self.rotatep = true;
        }
        
        if self.rotatep {
            self.chaseto = Chaseto::Orbit;
        } else {
            self.chaseto = Chaseto::Bee;
        }
        self.chasetime = 1.0;
        
        self.lyap = 0.0;
        self.yperiod = 0.0;
        self.step2 = INITIALSTEP;
        self.par2 = [DVector::default(); 20];
        
        let examples = if self.periodicp { 5 } else { 3 };
        let choice = rng.random_range(0..examples);
        
        match choice {
            0 => { // Lorentz
                self.par2[Y].x = 10.0 + balance_rand(&mut rng, 0.0);
                self.par2[X].x = -self.par2[Y].x;
                self.par2[X].y = 28.0 + balance_rand(&mut rng, 0.0);
                self.par2[XZ].y = -1.0;
                self.par2[Y].y = -1.0;
                self.par2[XY].z = 1.0;
                self.par2[Z].z = -2.0 + balance_rand(&mut rng, 0.0);
            },
            1 => { // Rossler
                self.par2[Y].x = -1.0;
                self.par2[Z].x = -2.0 + balance_rand(&mut rng, 1.0);
                self.par2[X].y = 1.0;
                self.par2[Y].y = 0.2 + balance_rand(&mut rng, 0.1);
                self.par2[C].z = 0.2 + balance_rand(&mut rng, 0.1);
                self.par2[XZ].z = 1.0;
                self.par2[Z].z = -5.7;
            },
            2 => { // RosslerCone
                self.par2[Y].x = -1.0;
                self.par2[Z].x = -2.0;
                self.par2[X].y = 1.0;
                self.par2[Y].y = 0.2;
                self.par2[ZZ].y = -0.331 + balance_rand(&mut rng, 0.01);
                self.par2[C].z = 0.2;
                self.par2[XZ].z = 1.0;
                self.par2[Z].z = -5.7;
            },
            3 => { // Birkhoff
                self.par2[Z].x = -1.0;
                self.par2[SINY].x = 0.35 + balance_rand(&mut rng, 0.25);
                self.par2[C].y = 1.57;
                self.par2[X].z = 0.7;
                self.par2[Z].z = 1.0 + balance_rand(&mut rng, 0.5);
                self.par2[XXZ].z = -10.0 * self.par2[Z].z;
                self.yperiod = 2.0 * std::f64::consts::PI;
            },
            _ => { // Duffing
                self.par2[X].x = -0.2 + balance_rand(&mut rng, 0.1);
                self.par2[Z].x = -0.5;
                self.par2[ZZZ].x = -0.125;
                self.par2[SINY].x = 27.0 + balance_rand(&mut rng, 3.0);
                self.par2[C].y = 1.33;
                self.par2[X].z = 2.0;
                self.yperiod = 2.0 * std::f64::consts::PI;
            }
        }
        
        self.range.x = 5.0;
        self.range.z = 5.0;
        if self.yperiod > 0.0 {
            self.ode_type = OdeType::Periodic;
            self.range.y = if rng.random_bool(0.5) { self.yperiod } else { 0.0 };
        } else {
            self.range.y = 5.0;
            self.ode_type = OdeType::Cubic;
        }
        
        self.discover();
        
        self.lyap = self.lyap2;
        self.size = self.size2;
        self.mid = self.mid2;
        self.step = self.step2;
        self.par = self.par2;
        self.count2 = 0;
        
        let count_arg = if self.count_arg == 0 { 3000 } else { self.count_arg };
        if count_arg < 0 {
            let max_val = (-count_arg) as usize;
            self.beecount = rng.random_range(0..max_val) + 1;
        } else {
            self.beecount = count_arg as usize;
        }
        
        self.p = vec![vec![DVector::default(); self.taillen]; self.beecount];
        
        self.restart_flow();
        
        if self.beecount > 0 && self.taillen > 1 {
            self.p[0][1] = DVector::default();
            self.cam[1] = DVector::default();
        }
    }
}

impl Animation for Flow {
    fn new(config: &AnimConfig) -> Self {
        let mut f = Flow {
            rotatep: true,
            ridep: true,
            boxp: true,
            periodicp: true,
            searchp: true,
            ..Default::default()
        };
        f.reset(config);
        f
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        if self.searchp {
            if self.count2 == 0 {
                self.step2 = INITIALSTEP;
                for i in 0..20 {
                    self.par2[i].x = gauss_rand(&mut rng, 1.0);
                    self.par2[i].y = gauss_rand(&mut rng, 1.0);
                    self.par2[i].z = gauss_rand(&mut rng, 1.0);
                }
            }
            if !self.discover() {
                self.count2 = 0;
            } else {
                if self.lyap2 < 0.0 {
                    self.count2 = 0;
                } else if self.count2 > 1000000 {
                    self.count2 = 0;
                    self.lyap = self.lyap2;
                    self.size = self.size2;
                    self.mid = self.mid2;
                    self.step = self.step2;
                    self.par = self.par2;

                    if self.chaseto == Chaseto::Bee && self.rotatep {
                        self.chaseto = Chaseto::Orbit;
                        self.chasetime = 100.0;
                    }
                    self.restart_flow();
                }
            }
        }

        self.circle[1] = self.circle[0];
        let count_f = self.count as f64;
        self.circle[0].x = self.size * 2.0 * (count_f / 100.0).sin() * (-0.6 + 0.4 * (count_f / 500.0).cos()) + self.mid.x;
        self.circle[0].y = self.size * 2.0 * (count_f / 100.0).cos() * (0.6 + 0.4 * (count_f / 500.0).cos()) + self.mid.y;
        self.circle[0].z = self.size * 2.0 * (count_f / 421.0).sin() + self.mid.z;

        if self.rotatep && self.ridep {
            if self.chaseto == Chaseto::Bee && rng.random_range(0..1000) == 0 {
                self.chaseto = Chaseto::Orbit;
                self.chasetime = 100.0;
            } else if rng.random_range(0..4000) == 0 {
                self.chaseto = Chaseto::Bee;
                self.chasetime = 100.0;
            }
        }

        if self.chasetime > 1.0 {
            self.chasetime -= 1.0;
        }
        let ct = self.chasetime;

        if self.chaseto == Chaseto::Bee {
            self.cam[0].x += (self.p[0][0].x - self.cam[0].x) / ct;
            self.cam[0].y += (self.p[0][0].y - self.cam[0].y) / ct;
            self.cam[0].z += (self.p[0][0].z - self.cam[0].z) / ct;

            self.cam[1].x += (self.p[0][1].x - self.cam[1].x) / ct;
            self.cam[1].y += (self.p[0][1].y - self.cam[1].y) / ct;
            self.cam[1].z += (self.p[0][1].z - self.cam[1].z) / ct;

            if self.beecount > 1 {
                self.cam[2].x += (self.p[1][0].x - self.cam[2].x) / ct;
                self.cam[2].y += (self.p[1][0].y - self.cam[2].y) / ct;
                self.cam[2].z += (self.p[1][0].z - self.cam[2].z) / ct;
            }
        } else {
            self.cam[0].x += (self.circle[0].x - self.cam[0].x) / ct;
            self.cam[0].y += (self.circle[0].y - self.cam[0].y) / ct;
            self.cam[0].z += (self.circle[0].z - self.cam[0].z) / ct;

            self.cam[1].x += (2.0 * self.circle[0].x - self.mid.x - self.cam[1].x) / ct;
            self.cam[1].y += (2.0 * self.circle[0].y - self.mid.y - self.cam[1].y) / ct;
            self.cam[1].z += (2.0 * self.circle[0].z - self.mid.z - self.cam[1].z) / ct;

            self.cam[2].x += (self.circle[1].x - self.cam[2].x) / ct;
            self.cam[2].y += (self.circle[1].y - self.cam[2].y) / ct;
            self.cam[2].z += (self.circle[1].z - self.cam[2].z) / ct;
        }

        self.centre = self.cam[1];

        let x = [
            self.cam[0].x - self.cam[1].x,
            self.cam[0].y - self.cam[1].y,
            self.cam[0].z - self.cam[1].z,
        ];
        let p = [
            self.cam[2].x - self.cam[1].x,
            self.cam[2].y - self.cam[1].y,
            self.cam[2].z - self.cam[1].z,
        ];

        let mut m = [[0.0; 3]; 3];
        let mut x2 = 0.0;
        let mut xp = 0.0;

        for i in 0..3 {
            x2 += x[i] * x[i];
            xp += x[i] * p[i];
            m[0][i] = x[i];
        }
        for i in 0..3 {
            m[1][i] = x2 * p[i] - xp * x[i];
        }
        m[2][0] = x[1] * p[2] - x[2] * p[1];
        m[2][1] = -x[0] * p[2] + x[2] * p[0];
        m[2][2] = x[0] * p[1] - x[1] * p[0];

        for j in 0..3 {
            let mut a = 0.0;
            for i in 0..3 {
                a += m[j][i] * m[j][i];
            }
            a = a.sqrt();
            if a > 0.0 {
                for i in 0..3 {
                    m[j][i] /= a;
                }
            }
        }
        
        self.m = m;

        if self.chaseto == Chaseto::Bee && self.beecount > 1 {
            self.p[1][0].x = self.p[0][0].x + m[1][0] * self.step;
            self.p[1][0].y = self.p[0][0].y + m[1][1] * self.step;
            self.p[1][0].z = self.p[0][0].z + m[1][2] * self.step;
        }

        let mut swarm = 0;
        for b in 0..self.beecount {
            if self.p[b][0].x.abs() > LOST_IN_SPACE ||
               self.p[b][0].y.abs() > LOST_IN_SPACE ||
               self.p[b][0].z.abs() > LOST_IN_SPACE {
                if self.chaseto == Chaseto::Bee && b == 0 {
                    let max_b = (self.beecount as i32 - 1).max(1) as usize;
                    let newb = 1 + rng.random_range(0..max_b);
                    self.p[0][0].x = self.p[newb][0].x + 0.001;
                    self.p[0][0].y = self.p[newb][0].y;
                    self.p[0][0].z = self.p[newb][0].z;
                }
                continue;
            }

            for i in (1..self.taillen).rev() {
                self.p[b][i] = self.p[b][i - 1];
            }
            
            iterate(&mut self.p[b][0], self.ode_type, &self.par, self.step);
            swarm += 1;
        }
        self.swarm_count = swarm;
        
        self.breaks.clear();
        self.breaks.resize(self.beecount, self.taillen);
        for b in 0..self.beecount {
            for i in 0..self.taillen {
                if self.yperiod > 0.0 && self.p[b][i].y > self.yperiod {
                    self.p[b][i].y -= self.yperiod;
                    for j in i..self.taillen {
                        self.p[b][j].y = self.p[b][i].y;
                    }
                    self.breaks[b] = i;
                    break;
                }
            }
        }

        self.count += 1;
        if (self.count > 1 && self.swarm_count == 0) || self.count > self.cycles {
            self.do_init();
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let m = self.m;
        let hw = (width / 2) as f64;
        let hh = (height / 2) as f64;
        let w_f = width as f64;
        let h_f = height as f64;
        
        if self.boxp {
            for b in 0..BOX_L {
                let p1 = LINES[b][0];
                let p2 = LINES[b][1];
                
                let x1 = BOX_PTS[p1][0] * self.size / 2.0 + self.mid.x - self.centre.x;
                let y1 = BOX_PTS[p1][1] * self.size / 2.0 + self.mid.y - self.centre.y;
                let z1 = BOX_PTS[p1][2] * self.size / 2.0 + self.mid.z - self.centre.z;
                
                let x2 = BOX_PTS[p2][0] * self.size / 2.0 + self.mid.x - self.centre.x;
                let y2 = BOX_PTS[p2][1] * self.size / 2.0 + self.mid.y - self.centre.y;
                let z2 = BOX_PTS[p2][2] * self.size / 2.0 + self.mid.z - self.centre.z;
                
                let mut a1 = DVector {
                    x: m[0][0]*x1 + m[0][1]*y1 + m[0][2]*z1,
                    y: m[1][0]*x1 + m[1][1]*y1 + m[1][2]*z1,
                    z: m[2][0]*x1 + m[2][1]*y1 + m[2][2]*z1 + EYEHEIGHT * self.size,
                };
                let mut a2 = DVector {
                    x: m[0][0]*x2 + m[0][1]*y2 + m[0][2]*z2,
                    y: m[1][0]*x2 + m[1][1]*y2 + m[1][2]*z2,
                    z: m[2][0]*x2 + m[2][1]*y2 + m[2][2]*z2 + EYEHEIGHT * self.size,
                };
                
                let aspect = w_f / h_f;
                if clip(1.0, 0.0, 0.0, -1.0, &mut a1, &mut a2) ||
                   clip(1.0, 2.0, 0.0, 0.0, &mut a1, &mut a2) ||
                   clip(1.0, -2.0, 0.0, 0.0, &mut a1, &mut a2) ||
                   clip(1.0, 0.0, 2.0 * aspect, 0.0, &mut a1, &mut a2) ||
                   clip(1.0, 0.0, -2.0 * aspect, 0.0, &mut a1, &mut a2) {
                    continue;
                }
                
                let col_idx = b % (self.ncolors.max(2) - 1);
                let color = self.palette[col_idx];

                let px1 = hw + w_f * a1.y / a1.x;
                let py1 = hh + w_f * a1.z / a1.x;
                let px2 = hw + w_f * a2.y / a2.x;
                let py2 = hh + w_f * a2.z / a2.x;
                
                draw_line(buffer, width, height, px1 as i32, py1 as i32, px2 as i32, py2 as i32, color);
            }
        }
        
        for b in 0..self.beecount {
            if self.p[b][0].x.abs() > LOST_IN_SPACE ||
               self.p[b][0].y.abs() > LOST_IN_SPACE ||
               self.p[b][0].z.abs() > LOST_IN_SPACE {
                continue;
            }
            
            if self.chaseto == Chaseto::Bee && b == 1 {
                continue;
            }
            
            let col_idx = b % (self.ncolors.max(2) - 1);
            let color = self.palette[col_idx];

            let end = self.taillen.min(self.count);
            let break_idx = if self.breaks.len() > b { self.breaks[b] } else { self.taillen };
            
            let mut prev_abs: Option<(i32, i32)> = None;
            for i in 0..end {
                if i == break_idx {
                    prev_abs = None;
                    break;
                }
                
                let x = self.p[b][i].x - self.centre.x;
                let y_fac = if self.yperiod < 0.0 { self.size / self.yperiod } else { 1.0 };
                let y = self.p[b][i].y * y_fac - self.centre.y;
                let z = self.p[b][i].z - self.centre.z;
                
                let xm = m[0][0]*x + m[0][1]*y + m[0][2]*z;
                let ym = m[1][0]*x + m[1][1]*y + m[1][2]*z;
                let zm = m[2][0]*x + m[2][1]*y + m[2][2]*z + EYEHEIGHT * self.size;
                
                if xm <= 0.0 {
                    prev_abs = None;
                    continue;
                }
                
                // Particles only: clamp the projection basis to a 16:9 width
                // so the attractor doesn't balloon/crop on ultrawide. The
                // wireframe box above keeps the C's raw-width basis on
                // purpose — clamping it letterboxes the frame, and the box
                // reads better full-bleed even if its top/bottom clip.
                let p_f = (width.min(height * 16 / 9)) as f64;
                let absx = hw + p_f * ym / xm;
                let absy = hh + p_f * zm / xm;

                if absx <= 0.0 || absx >= w_f || absy <= 0.0 || absy >= h_f {
                    prev_abs = None;
                    continue;
                }
                
                let cur_abs = (absx as i32, absy as i32);
                if let Some(prev) = prev_abs {
                    draw_line(buffer, width, height, prev.0, prev.1, cur_abs.0, cur_abs.1, color);
                }
                prev_abs = Some(cur_abs);
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.count_arg = config.count;
        self.size_arg = config.size;
        self.ncolors = config.ncolors.max(2) as usize;
        self.cycles = if config.cycles <= 0 { 10000 } else { config.cycles as usize };
        self.delay_us = config.delay_us;
        let n = self.ncolors - 1;
        self.palette = (0..n).map(|i| Color::from_hsl(i as f32 / n as f32, 1.0, 0.5)).collect();
        self.do_init();
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
