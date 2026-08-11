/*
 *  @(#) vermiculate.c
 *  @(#) Copyright (C) 2001 Tyler Pierce (tyler@alumni.brown.edu)
 *  The full program, with documentation, is available at:
 *    http://freshmeat.net/projects/fdm
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Rust port of xscreensaver's vermiculate.c (with the bright variant of
 * utils/colors.c's make_random_colormap ported inline).
 */

use rand::RngExt;

use crate::animation::primitives::{hsv_to_rgb, put_pixel, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const DEGS: i32 = 360;
const DEGS2: i32 = DEGS / 2;
const DEGS4: i32 = DEGS / 4;
const DEGS8: i32 = DEGS / 8;
const DTOR: f64 = 0.017_453_292_5; /* pi / degs2 */
const THRMAX: usize = 120;
const TAILMAX: usize = THRMAX * 2 + 1;
const TMODES: u32 = 7; /* '7' - '0' */
const RLMAX: usize = 200;
const SPEEDINC: i32 = 10;
const SPEEDMAX: i32 = 1000;

const SAMPLE_STRINGS: [(&str, i32); 10] = [
    ("AEBMN222222223#CAR9CAD4CAOV", 150),
    ("mn333#c23#f1#]]]]]]]]]]]3bc9#r9#c78#f9#ma4#", 600),
    ("AEBMN22222#CAD4CAORc1#f2#c1#r6", 100),
    ("aebmnrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr#", 500),
    ("mn6rrrrrrrrrrrrrrr#by1i#lcalc1#fnyav", 200),
    (
        "mn1rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr#by1i#lcalc1#fn",
        2000,
    ),
    ("baeMn333333333333333333333333#CerrYerCal", 800),
    (
        "baeMn1111111111111111111111111111111111111111111111111111111111#Cer9YesYevYerCal",
        1200,
    ),
    (
        "baMn111111222222333333444444555555#Ct1#lCt2#lCt3#lCt4#lCt5#lCerrYerYet",
        1400,
    ),
    (
        "baMn111111222222333333444444555555#Ct1#lCt2#lCt3#lCt4#lCt5#lCerrYerYetYt1i#lYt1i#sYt1#v",
        1400,
    ),
];

fn random1(rng: &mut impl RngExt, i: u32) -> u32 {
    rng.random_range(0..i.max(1))
}

fn wrap_i(val: &mut i32, lower: i32, upper: i32) {
    if *val >= upper {
        *val -= upper - lower;
    } else if *val < lower {
        *val += upper - lower;
    }
}

fn wrap_f(val: &mut f64, lower: f64, upper: f64) {
    if *val >= upper {
        *val -= upper - lower;
    } else if *val < lower {
        *val += upper - lower;
    }
}

// set point / get point. The pixel index buffer stays the authoritative
// state; `sp` also paints the matching BGRA canvas entry so render() can be
// a memcpy instead of a full-screen palette scan per frame. On palette
// changes the canvas is repainted wholesale from `point` (rare — that's the
// pass this removes from every frame).
fn sp(
    point: &mut [u8],
    canvas: &mut [u8],
    colors: &[Color; TAILMAX],
    wid: i32,
    x: i32,
    y: i32,
    c: u8,
) {
    let idx = (wid * y + x) as usize;
    if idx < point.len() {
        point[idx] = c;
        let cidx = idx * 4;
        if cidx + 3 < canvas.len() {
            let col = colors[(c as usize).min(TAILMAX - 1)];
            canvas[cidx] = col.b;
            canvas[cidx + 1] = col.g;
            canvas[cidx + 2] = col.r;
            canvas[cidx + 3] = col.a;
        }
    }
}

fn gp(point: &[u8], wid: i32, x: i32, y: i32) -> u8 {
    let idx = (wid * y + x) as usize;
    if idx < point.len() {
        point[idx]
    } else {
        0
    }
}


struct LineData {
    deg: i32,
    spiturn: i32,
    turnco: i32,
    turnsize: i32,
    col: u8,
    dead: bool,

    orichar: char,
    x: f64,
    y: f64,
    tmode: i32,
    tsc: i32,
    tslen: i32,
    tclim: i32,
    otslen: i32,
    ctinc: i32,
    reclen: i32,
    recpos: i32,
    circturn: i32,
    prey: i32,
    slice: i32,
    xrec: [i32; RLMAX + 1],
    yrec: [i32; RLMAX + 1],
    turnseq: [i32; 50],
    filled: bool,
    killwalls: bool,
    vhfollow: bool,
    selfbounce: bool,
    tailfollow: bool,
    realbounce: bool,
    little: bool,
}

impl LineData {
    fn new() -> Self {
        LineData {
            deg: 0,
            spiturn: 0,
            turnco: 0,
            turnsize: 0,
            col: 0,
            dead: false,
            orichar: 'R',
            x: 0.0,
            y: 0.0,
            tmode: 1,
            tsc: 0,
            tslen: 0,
            tclim: 0,
            otslen: 0,
            ctinc: 0,
            reclen: 0,
            recpos: 0,
            circturn: 0,
            prey: 0,
            slice: 0,
            xrec: [0; RLMAX + 1],
            yrec: [0; RLMAX + 1],
            turnseq: [0; 50],
            filled: false,
            killwalls: false,
            vhfollow: false,
            selfbounce: false,
            tailfollow: false,
            realbounce: false,
            little: false,
        }
    }
}

pub struct Vermiculate {
    hei: i32,
    wid: i32,
    speed: i32,
    erasing: bool,
    autopal: bool,
    instring: Vec<u8>,
    inpos: usize,

    sinof: [f64; DEGS as usize],
    cosof: [f64; DEGS as usize],
    tanof: [f64; DEGS as usize],
    point: Vec<u8>,
    /// BGRA mirror of `point` through the palette (see `sp`).
    canvas: Vec<u8>,

    thread: Vec<LineData>,
    bank: [u8; THRMAX],
    bnkt: i32,
    boxw: i32,
    boxh: i32,
    curviness: i32,
    gridden: i32,
    ogd: i32,
    bordcorn: i32,
    bordcol: u8,
    threads: u8,
    ch: char,

    reset_p: bool,
    cyc: i32,
    pscale: i32,
    mycolors: [Color; TAILMAX],
    delay_us: u64,
}

impl Vermiculate {
    fn wasakeypressed(&self) -> bool {
        self.inpos < self.instring.len()
    }

    fn readkey(&mut self) -> char {
        if self.inpos >= self.instring.len() {
            '#'
        } else {
            let c = self.instring[self.inpos];
            self.inpos += 1;
            (c as char).to_ascii_uppercase()
        }
    }

    fn waitabit(&mut self) -> u64 {
        let mut result = 0u64;
        self.cyc += self.threads as i32;
        while self.cyc > self.speed {
            result += 10_000;
            self.cyc -= self.speed;
        }
        result
    }

    fn clearscreen(&mut self) {
        self.point.iter_mut().for_each(|p| *p = 0);
        self.repaint_canvas();
    }

    /// Rebuild the BGRA canvas from the index buffer — needed whenever the
    /// palette changes under pixels already on screen. Rare (pattern resets
    /// and autopal), which is why the per-frame version of this scan was
    /// worth removing from render().
    fn repaint_canvas(&mut self) {
        for (i, &c) in self.point.iter().enumerate() {
            let col = self.mycolors[(c as usize).min(TAILMAX - 1)];
            let cidx = i * 4;
            if cidx + 3 < self.canvas.len() {
                self.canvas[cidx] = col.b;
                self.canvas[cidx + 1] = col.g;
                self.canvas[cidx + 2] = col.r;
                self.canvas[cidx + 3] = col.a;
            }
        }
    }

    fn randpal(&mut self, rng: &mut impl RngExt) {
        // make_random_colormap, bright_p variant.
        for c in 1..TAILMAX {
            let h = rng.random_range(0..360);
            let s = (rng.random_range(0..70) + 30) as f64 / 100.0;
            let v = (rng.random_range(0..34) + 66) as f64 / 100.0;
            let (r, g, b) = hsv_to_rgb(h, s, v);
            self.mycolors[c] = Color::new(255, (r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8);
        }
        self.repaint_canvas();
    }

    fn gridupdate(&mut self, interruptible: bool, rng: &mut impl RngExt) {
        let xmax = self.wid - 1;
        let ymax = self.hei - 1;
        if self.gridden > 0 {
            let mut x = 0;
            while x <= xmax && !(self.wasakeypressed() && interruptible) {
                let mut y = 0;
                while y <= ymax {
                    if (random1(rng, 15) as i32) < self.gridden {
                        let max = (x + self.boxw).min(xmax);
                        for xc in x..=max {
                            sp(&mut self.point, &mut self.canvas, &self.mycolors, self.wid, xc, y, 1);
                        }
                    }
                    if (random1(rng, 15) as i32) < self.gridden {
                        let max = (y + self.boxh).min(ymax);
                        for yc in y..=max {
                            sp(&mut self.point, &mut self.canvas, &self.mycolors, self.wid, x, yc, 1);
                        }
                    }
                    y += self.boxh;
                }
                x += self.boxw;
            }
        }
    }

    fn bordupdate(&mut self) {
        let xmax = self.wid - 1;
        let ymax = self.hei - 1;
        let ybord = if self.bordcorn == 0 || self.bordcorn == 1 {
            0
        } else {
            ymax
        };
        let xbord = if self.bordcorn == 0 || self.bordcorn == 3 {
            0
        } else {
            xmax
        };
        for x in 0..=xmax {
            sp(&mut self.point, &mut self.canvas, &self.mycolors, self.wid, x, ybord, self.bordcol);
        }
        for y in 0..=ymax {
            sp(&mut self.point, &mut self.canvas, &self.mycolors, self.wid, xbord, y, self.bordcol);
        }
    }

    fn inbank(&self, thr: u8) -> bool {
        if self.bnkt > 0 {
            for c in 1..=self.bnkt {
                if self.bank[c as usize - 1] == thr {
                    return true;
                }
            }
        }
        false
    }

    fn pickbank(&mut self) {
        let mut thr: i32 = 1;
        self.bnkt = 0;
        self.ch = '\0';
        loop {
            while self.inbank(thr as u8) {
                thr = thr % self.threads as i32 + 1;
            }

            self.ch = self.readkey();
            match self.ch {
                '+' | '-' => loop {
                    if self.ch == '+' {
                        thr += 1;
                    } else {
                        thr -= 1;
                    }
                    wrap_i(&mut thr, 1, self.threads as i32 + 1);
                    if !self.inbank(thr as u8) {
                        break;
                    }
                },
                ' ' => {
                    self.bnkt += 1;
                    self.bank[self.bnkt as usize - 1] = thr as u8;
                }
                '1'..='9' => {
                    self.bnkt += 1;
                    self.bank[self.bnkt as usize - 1] = self.ch as u8 - b'0';
                    if self.bank[self.bnkt as usize - 1] > self.threads {
                        self.bnkt -= 1;
                    }
                }
                'I' => {
                    let mut tbank = [0u8; THRMAX];
                    let mut tbankt = 0;
                    for c in 1..=self.threads {
                        if !self.inbank(c) {
                            tbankt += 1;
                            tbank[tbankt - 1] = c;
                        }
                    }
                    self.bnkt = tbankt as i32;
                    self.bank = tbank;
                }
                'T' => {
                    self.ch = self.readkey();
                    if let '1'..='9' = self.ch {
                        let d = self.ch as i32 - '0' as i32;
                        for c in 1..=self.threads {
                            if self.thread[c as usize - 1].tmode == d {
                                self.bnkt += 1;
                                self.bank[self.bnkt as usize - 1] = c;
                            }
                        }
                    }
                }
                'A' => {
                    for i in 1..=self.threads as usize {
                        self.bank[i - 1] = i as u8;
                    }
                    self.bnkt = self.threads as i32;
                }
                'E' => {
                    for i in 1..=THRMAX {
                        self.bank[i - 1] = i as u8;
                    }
                    self.bnkt = THRMAX as i32;
                }
                _ => {}
            }
            if self.bnkt >= self.threads as i32
                || self.ch == 'N'
                || self.ch == '\r'
                || self.ch == '#'
            {
                break;
            }
        }
        if self.bnkt == 0 && self.ch != 'N' {
            self.bnkt = 1;
            self.bank[0] = thr as u8;
        }
        // palupdate() is a no-op here: render() always repaints from `point`.
    }

    fn newonscreen(&mut self, thr: usize, rng: &mut impl RngExt) {
        let hei = self.hei;
        let wid = self.wid;
        let lp = &mut self.thread[thr - 1];
        lp.filled = false;
        lp.dead = false;
        lp.reclen = if lp.little {
            random1(rng, 10) as i32 + 5
        } else {
            random1(rng, RLMAX as u32 - 30) as i32 + 30
        };
        lp.deg = random1(rng, DEGS as u32) as i32;
        lp.y = random1(rng, hei as u32) as f64;
        lp.x = random1(rng, wid as u32) as f64;
        lp.recpos = 0;
        lp.turnco = 2;
        lp.turnsize = random1(rng, 4) as i32 + 2;
    }

    fn firstinit(&mut self, thr: usize, rng: &mut impl RngExt) {
        let t = thr as i32;
        let lp = &mut self.thread[thr - 1];
        lp.col = (thr + 1) as u8;
        lp.prey = 0;
        lp.tmode = 1;
        lp.slice = DEGS / 3;
        lp.orichar = 'R';
        lp.spiturn = 5;
        lp.selfbounce = false;
        lp.realbounce = false;
        lp.vhfollow = false;
        lp.tailfollow = false;
        lp.killwalls = false;
        lp.little = false;
        lp.ctinc = random1(rng, 2) as i32 * 2 - 1;
        lp.circturn = ((t % 2) * 2 - 1) * ((t - 1) % 7 + 1);
        lp.tsc = 1;
        lp.tslen = 6;
        lp.turnseq[0] = 6;
        lp.turnseq[1] = -6;
        lp.turnseq[2] = 6;
        lp.turnseq[3] = 6;
        lp.turnseq[4] = -6;
        lp.turnseq[5] = 6;
        lp.tclim = (DEGS as f64 / 2.0 / 12.0) as i32;
    }

    fn maininit(&mut self, rng: &mut impl RngExt) {
        if self.instring.is_empty() {
            let n = random1(rng, SAMPLE_STRINGS.len() as u32) as usize;
            self.instring = SAMPLE_STRINGS[n].0.as_bytes().to_vec();
            self.inpos = 0;
            self.speed = SAMPLE_STRINGS[n].1;
        }
        self.boxh = 10;
        self.boxw = 10;
        self.gridden = 0;
        self.bordcorn = 0;
        self.threads = 4;
        self.curviness = 30;
        self.bordcol = 1;
        self.ogd = 8;
        self.ch = '\0';
        self.erasing = true;
        for thr in 1..=THRMAX {
            self.firstinit(thr, rng);
            self.newonscreen(thr, rng);
        }
        for d in (0..DEGS as usize).rev() {
            self.sinof[d] = (d as f64 * DTOR).sin();
            self.cosof[d] = (d as f64 * DTOR).cos();
            if d % DEGS4 as usize == 0 {
                self.tanof[d] = self.tanof[(d + 1).min(DEGS as usize - 1)];
            } else {
                self.tanof[d] = (d as f64 * DTOR).tan();
            }
        }
        self.randpal(rng);
    }

    fn move_thread(&mut self, thr: usize, rng: &mut impl RngExt) -> bool {
        let (dead, prey, tailfollow) = {
            let lp = &self.thread[thr - 1];
            (lp.dead, lp.prey, lp.tailfollow)
        };
        if dead {
            return false;
        }

        // Prefetch the prey's position (may be this thread's own tail).
        let prey_pos = if prey != 0 {
            let pr = &self.thread[prey as usize - 1];
            if tailfollow || prey as usize == thr {
                Some((
                    pr.xrec[pr.recpos as usize] as f64,
                    pr.yrec[pr.recpos as usize] as f64,
                ))
            } else {
                Some((pr.x, pr.y))
            }
        } else {
            None
        };

        let wid = self.wid;
        let hei = self.hei;
        let xmax = wid - 1;
        let ymax = hei - 1;
        let gridden = self.gridden;
        let boxw = self.boxw;
        let boxh = self.boxh;
        let curviness = self.curviness;
        let erasing = self.erasing;

        let lp = &mut self.thread[thr - 1];

        if prey_pos.is_none() {
            match lp.tmode {
                1 => {
                    lp.deg +=
                        random1(rng, 2 * lp.turnsize as u32 + 1) as i32 - lp.turnsize;
                }
                2 => {
                    if lp.slice == DEGS || lp.slice == DEGS2 || lp.slice == DEGS4 {
                        if lp.orichar == 'D' {
                            if lp.deg % DEGS4 != DEGS8 {
                                lp.deg = DEGS4 * random1(rng, 4) as i32 + DEGS8;
                            }
                        } else if lp.orichar == 'V' && lp.deg % DEGS4 != 0 {
                            lp.deg = DEGS4 * random1(rng, 4) as i32;
                        }
                    }
                    if random1(rng, 100) == 0 {
                        if lp.slice == 0 {
                            lp.deg = lp.deg - DEGS4 + random1(rng, DEGS2 as u32) as i32;
                        } else {
                            lp.deg += (random1(rng, 2) as i32 * 2 - 1) * lp.slice;
                        }
                    }
                }
                3 => {
                    lp.deg += lp.circturn;
                }
                4 => {
                    if lp.spiturn.abs() > 11 {
                        lp.spiturn = 5;
                    } else {
                        lp.deg += lp.spiturn;
                    }
                    if random1(rng, (15 - lp.spiturn.abs()).max(1) as u32) == 0 {
                        lp.spiturn += lp.ctinc;
                        if lp.spiturn.abs() > 10 {
                            lp.ctinc *= -1;
                        }
                    }
                }
                5 => {
                    lp.turnco = lp.turnco.abs() - 1;
                    if lp.turnco == 0 {
                        lp.turnco = curviness + random1(rng, 10) as i32;
                        lp.circturn *= -1;
                    }
                    lp.deg += lp.circturn;
                }
                6 => {
                    if lp.turnco.abs() == 1 {
                        lp.turnco *=
                            -(random1(rng, (DEGS2 / lp.circturn.abs().max(1)).max(1) as u32)
                                as i32
                                + 5);
                    } else if lp.turnco == 0 {
                        lp.turnco = 2;
                    } else if lp.turnco > 0 {
                        lp.turnco -= 1;
                        lp.deg += lp.circturn;
                    } else {
                        lp.turnco += 1;
                    }
                }
                7 => {
                    lp.turnco += 1;
                    if lp.turnco > lp.tclim {
                        lp.turnco = 1;
                        lp.tsc = (lp.tsc % lp.tslen.max(1)) + 1;
                    }
                    lp.deg += lp.turnseq[(lp.tsc - 1).clamp(0, 49) as usize];
                }
                _ => {}
            }
        } else if let Some((px, py)) = prey_pos {
            let dx = px - lp.x;
            let dy = py - lp.y;
            let mut desdeg = if lp.vhfollow {
                if dx.abs() > dy.abs() {
                    if dx > 0.0 {
                        0
                    } else {
                        2 * DEGS4
                    }
                } else if dy > 0.0 {
                    DEGS4
                } else {
                    3 * DEGS4
                }
            } else if dx > 0.0 {
                if dy > 0.0 {
                    DEGS8
                } else {
                    7 * DEGS8
                }
            } else if dy > 0.0 {
                3 * DEGS8
            } else {
                5 * DEGS8
            };
            if desdeg - desdeg % DEGS4 != lp.deg - lp.deg % DEGS4 || lp.vhfollow {
                if !lp.vhfollow {
                    // Using atan2 here doesn't seem to slow things down:
                    desdeg = (dy.atan2(dx) / DTOR) as i32;
                    wrap_i(&mut desdeg, 0, DEGS);
                }
                if (desdeg - lp.deg).abs() <= lp.circturn.abs() {
                    lp.deg = desdeg;
                } else {
                    lp.deg += if desdeg > lp.deg {
                        if desdeg - lp.deg > DEGS2 {
                            -lp.circturn.abs()
                        } else {
                            lp.circturn.abs()
                        }
                    } else if lp.deg - desdeg > DEGS2 {
                        lp.circturn.abs()
                    } else {
                        -lp.circturn.abs()
                    };
                }
            } else {
                lp.deg += if self.tanof[lp.deg.rem_euclid(DEGS) as usize] > dy / dx {
                    -lp.circturn.abs()
                } else {
                    lp.circturn.abs()
                };
            }
        }

        wrap_i(&mut lp.deg, 0, DEGS);
        {
            let oldy = lp.y;
            let oldx = lp.x;
            lp.x += self.cosof[lp.deg as usize];
            wrap_f(&mut lp.x, 0.0, (xmax + 1) as f64);
            lp.y += self.sinof[lp.deg as usize];
            wrap_f(&mut lp.y, 0.0, (ymax + 1) as f64);
            let xi = lp.x as i32;
            let yi = lp.y as i32;

            let oldcol = gp(&self.point, wid, xi, yi);
            if oldcol != 0 {
                let mut vertwall = false;
                let mut horiwall = false;
                if oldcol == 1 && ((lp.killwalls && gridden > 0) || lp.realbounce) {
                    vertwall = gp(&self.point, wid, xi, oldy as i32) == 1;
                    horiwall = gp(&self.point, wid, oldx as i32, yi) == 1;
                }
                if oldcol == 1 && lp.realbounce && (vertwall || horiwall) {
                    if vertwall {
                        lp.deg = -lp.deg + DEGS2;
                    } else {
                        lp.deg = -lp.deg;
                    }
                } else if (oldcol != lp.col && lp.realbounce)
                    || (oldcol == lp.col && lp.selfbounce)
                {
                    lp.deg += DEGS4 * (random1(rng, 2) as i32 * 2 - 1);
                } else if oldcol != lp.col {
                    lp.deg += DEGS2;
                }
                if lp.killwalls && gridden > 0 && oldcol == 1 {
                    if vertwall && xi < xmax {
                        let mut yy = yi - yi % boxh;
                        while yy <= yi - yi % boxh + boxh && yy <= ymax {
                            if gp(&self.point, wid, xi + 1, yy) != 1 || yy == ymax {
                                sp(&mut self.point, &mut self.canvas, &self.mycolors, wid, xi, yy, 0);
                            }
                            yy += 1;
                        }
                    }
                    if horiwall && yi < ymax {
                        let mut xx = xi - xi % boxw;
                        while xx <= xi - xi % boxw + boxw && xx <= xmax {
                            if gp(&self.point, wid, xx, yi + 1) != 1 || xx == xmax {
                                sp(&mut self.point, &mut self.canvas, &self.mycolors, wid, xx, yi, 0);
                            }
                            xx += 1;
                        }
                    }
                }
                if oldcol != lp.col || lp.selfbounce {
                    lp.x = oldx;
                    lp.y = oldy;
                }
                wrap_i(&mut lp.deg, 0, DEGS);
            }
        }

        // In the C, xi/yi are macros: they re-read the (possibly restored) x/y.
        let xi = lp.x as i32;
        let yi = lp.y as i32;

        sp(&mut self.point, &mut self.canvas, &self.mycolors, wid, xi, yi, lp.col);
        if lp.filled {
            let (rx, ry) = (
                lp.xrec[lp.recpos as usize],
                lp.yrec[lp.recpos as usize],
            );
            if erasing {
                sp(&mut self.point, &mut self.canvas, &self.mycolors, wid, rx, ry, 0);
            } else {
                sp(
                    &mut self.point,
                    &mut self.canvas,
                    &self.mycolors,
                    wid,
                    rx,
                    ry,
                    (lp.col as usize + THRMAX).min(TAILMAX - 1) as u8,
                );
            }
        }
        lp.yrec[lp.recpos as usize] = yi;
        lp.xrec[lp.recpos as usize] = xi;
        if lp.recpos == lp.reclen - 1 {
            lp.filled = true;
        }
        if lp.filled && !erasing {
            let mut co = lp.recpos;
            lp.dead = true;
            loop {
                let mut nextco = co + 1;
                wrap_i(&mut nextco, 0, lp.reclen);
                if lp.yrec[co as usize] != lp.yrec[nextco as usize]
                    || lp.xrec[co as usize] != lp.xrec[nextco as usize]
                {
                    lp.dead = false;
                }
                co = nextco;
                if !lp.dead || co == lp.recpos {
                    break;
                }
            }
        }
        lp.recpos += 1;
        wrap_i(&mut lp.recpos, 0, lp.reclen);
        !lp.dead
    }

    fn bankmod(boolop: char, b: &mut bool) {
        match boolop {
            'T' => *b = !*b,
            'Y' => *b = true,
            'N' => *b = false,
            _ => {}
        }
    }

    fn consume_instring(&mut self, rng: &mut impl RngExt) {
        while self.wasakeypressed() {
            self.ch = self.readkey();
            match self.ch {
                'M' => {
                    self.ch = self.readkey();
                    if self.ch == 'A' || self.ch == 'N' {
                        let othreads = self.threads;
                        if self.ch == 'N' {
                            self.threads = 0;
                        }
                        loop {
                            self.ch = self.readkey();
                            match self.ch {
                                '1'..='9' => {
                                    self.threads += 1;
                                    self.thread[self.threads as usize - 1].tmode =
                                        self.ch as i32 - '0' as i32;
                                }
                                'R' => {
                                    self.threads += 1;
                                    self.thread[self.threads as usize - 1].tmode =
                                        random1(rng, TMODES) as i32 + 1;
                                }
                                _ => {}
                            }
                            if self.ch == '\r'
                                || self.ch == '#'
                                || self.threads as usize == THRMAX
                            {
                                break;
                            }
                        }
                        if self.threads == 0 {
                            self.threads = othreads;
                        }
                        self.reset_p = true;
                    }
                }
                'C' => {
                    self.pickbank();
                    if self.bnkt > 0 {
                        self.ch = self.readkey();
                        match self.ch {
                            'D' => {
                                self.ch = self.readkey();
                                match self.ch {
                                    '1'..='9' => {
                                        let d = self.ch as i32 - '0' as i32;
                                        for bankc in 1..=self.bnkt as usize {
                                            let ti = self.bank[bankc - 1] as usize - 1;
                                            self.thread[ti].slice = DEGS / d;
                                        }
                                    }
                                    'M' => {
                                        for bankc in 1..=self.bnkt as usize {
                                            let ti = self.bank[bankc - 1] as usize - 1;
                                            self.thread[ti].slice = 0;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            'S' => {
                                for bankc in 1..=self.bnkt as usize {
                                    let ti = self.bank[bankc - 1] as usize - 1;
                                    self.thread[ti].otslen = self.thread[ti].tslen;
                                    self.thread[ti].tslen = 0;
                                }
                                loop {
                                    let oldch = self.ch;
                                    self.ch = self.readkey();
                                    if self.ch.is_ascii_digit() {
                                        for bankc in 1..=self.bnkt as usize {
                                            let ti = self.bank[bankc - 1] as usize - 1;
                                            let l = &mut self.thread[ti];
                                            l.tslen += 1;
                                            let mut v = self.ch as i32 - '0' as i32;
                                            if oldch == '-' {
                                                v = -v;
                                            }
                                            if bankc % 2 == 0 {
                                                v = -v;
                                            }
                                            l.turnseq[(l.tslen - 1).clamp(0, 49) as usize] = v;
                                        }
                                    }
                                    if self.ch == '\r'
                                        || self.ch == '#'
                                        || self.thread[self.bank[0] as usize - 1].tslen == 50
                                    {
                                        break;
                                    }
                                }
                                for bankc in 1..=self.bnkt as usize {
                                    let ti = self.bank[bankc - 1] as usize - 1;
                                    let l = &mut self.thread[ti];
                                    if l.tslen == 0 {
                                        l.tslen = l.otslen;
                                    }
                                    let mut seq_sum = 0;
                                    for c in 1..=l.tslen {
                                        seq_sum += l.turnseq[(c - 1).clamp(0, 49) as usize];
                                    }
                                    if seq_sum == 0 {
                                        l.tclim = 1;
                                    } else {
                                        l.tclim = (DEGS2 as f64 / seq_sum.abs() as f64) as i32;
                                    }
                                    l.tsc = random1(rng, l.tslen.max(1) as u32) as i32 + 1;
                                }
                            }
                            'T' => {
                                self.ch = self.readkey();
                                for bankc in 1..=self.bnkt as usize {
                                    let ti = self.bank[bankc - 1] as usize - 1;
                                    match self.ch {
                                        '1'..='9' => {
                                            self.thread[ti].tmode =
                                                self.ch as i32 - '0' as i32;
                                        }
                                        'R' => {
                                            self.thread[ti].tmode =
                                                random1(rng, TMODES) as i32 + 1;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            'O' => {
                                self.ch = self.readkey();
                                for bankc in 1..=self.bnkt as usize {
                                    let ti = self.bank[bankc - 1] as usize - 1;
                                    self.thread[ti].orichar = self.ch;
                                }
                            }
                            'F' => {
                                let fbank = self.bank;
                                let fbnkt = self.bnkt;
                                self.pickbank();
                                for bankc in 1..=fbnkt as usize {
                                    let ti = fbank[bankc - 1] as usize - 1;
                                    if self.ch == 'N' {
                                        self.thread[ti].prey = 0;
                                    } else {
                                        self.thread[ti].prey = self.bank
                                            [(bankc - 1) % self.bnkt.max(1) as usize]
                                            as i32;
                                    }
                                }
                            }
                            'L' => {
                                for bankc in 1..=self.bnkt as usize {
                                    let ti = self.bank[bankc - 1] as usize - 1;
                                    self.thread[ti].prey =
                                        self.bank[bankc % self.bnkt as usize] as i32;
                                }
                            }
                            'R' => {
                                self.ch = self.readkey();
                                for bankc in 1..=self.bnkt as usize {
                                    let ti = self.bank[bankc - 1] as usize - 1;
                                    match self.ch {
                                        '1'..='9' => {
                                            self.thread[ti].circturn =
                                                10 - (self.ch as i32 - '0' as i32);
                                        }
                                        'R' => {
                                            self.thread[ti].circturn =
                                                random1(rng, 7) as i32 + 1;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                'T' | 'Y' | 'N' => {
                    let boolop = self.ch;
                    self.pickbank();
                    if self.bnkt > 0 {
                        self.ch = self.readkey();
                        for bankc in 1..=self.bnkt as usize {
                            let ti = self.bank[bankc - 1] as usize - 1;
                            let l = &mut self.thread[ti];
                            match self.ch {
                                'S' => Self::bankmod(boolop, &mut l.selfbounce),
                                'V' => Self::bankmod(boolop, &mut l.vhfollow),
                                'R' => Self::bankmod(boolop, &mut l.realbounce),
                                'L' => Self::bankmod(boolop, &mut l.little),
                                'T' => Self::bankmod(boolop, &mut l.tailfollow),
                                'K' => Self::bankmod(boolop, &mut l.killwalls),
                                _ => {}
                            }
                        }
                    }
                }
                'R'
                    if self.bordcol == 1 => {
                        self.bordcol = 0;
                        self.bordupdate();
                        self.bordcorn = (self.bordcorn + 1) % 4;
                        self.bordcol = 1;
                        self.bordupdate();
                    }
                '1'..='9' => {
                    let d = self.ch as i32 - '0' as i32;
                    for c in 0..THRMAX {
                        self.thread[c].tmode = d;
                    }
                }
                'E' => {
                    self.erasing = !self.erasing;
                }
                'P' => {
                    self.randpal(rng);
                }
                'G' => {
                    let mut dimch = 'B';
                    let mut gridchanged = true;
                    if self.gridden == 0 {
                        self.gridden = self.ogd;
                    }
                    loop {
                        let mut msize = 0;
                        if gridchanged {
                            self.clearscreen();
                            self.gridupdate(true, rng);
                        }
                        self.ch = self.readkey();
                        gridchanged = true;
                        match self.ch {
                            '+' => msize = 1,
                            '-' => msize = -1,
                            ']' => {
                                if self.gridden < 15 {
                                    self.gridden += 1;
                                }
                            }
                            '[' => {
                                if self.gridden > 0 {
                                    self.gridden -= 1;
                                }
                            }
                            'O' => {
                                self.ogd = self.gridden;
                                self.gridden = 0;
                            }
                            'S' => {
                                // C falls through: 'S' also sets dimch.
                                self.boxw = self.boxh;
                                dimch = self.ch;
                            }
                            'W' | 'H' | 'B' => {
                                dimch = self.ch;
                            }
                            _ => {
                                gridchanged = false;
                            }
                        }
                        if dimch == 'W' || dimch == 'B' {
                            self.boxw += msize;
                        }
                        if dimch == 'H' || dimch == 'B' {
                            self.boxh += msize;
                        }
                        if self.boxw == 0 {
                            self.boxw = 1;
                        }
                        if self.boxh == 0 {
                            self.boxh = 1;
                        }
                        if self.ch == '\r' || self.ch == '#' || self.ch == 'O' {
                            break;
                        }
                    }
                }
                'A' => {
                    self.autopal = !self.autopal;
                }
                'B' => {
                    self.bordcol = 1 - self.bordcol;
                    self.bordupdate();
                }
                '-' => {
                    self.speed -= SPEEDINC;
                    if self.speed < 1 {
                        self.speed = 1;
                    }
                }
                '+' => {
                    self.speed += SPEEDINC;
                    if self.speed > SPEEDMAX {
                        self.speed = SPEEDMAX;
                    }
                }
                '/'
                    if self.curviness > 5 => {
                        self.curviness -= 5;
                    }
                '*'
                    if self.curviness < 50 => {
                        self.curviness += 5;
                    }
                ']'
                    if (self.threads as usize) < THRMAX => {
                        self.threads += 1;
                        self.newonscreen(self.threads as usize, rng);
                    }
                '['
                    if self.threads > 1 => {
                        let ti = self.threads as usize - 1;
                        let lastpos = if self.thread[ti].filled {
                            self.thread[ti].reclen - 1
                        } else {
                            self.thread[ti].recpos
                        };
                        for c in 0..=lastpos.max(0) as usize {
                            let (x, y) = (self.thread[ti].xrec[c], self.thread[ti].yrec[c]);
                            sp(&mut self.point, &mut self.canvas, &self.mycolors, self.wid, x, y, 0);
                        }
                        self.threads -= 1;
                    }
                _ => {}
            }
        }
    }
}

impl Animation for Vermiculate {
    fn new(config: &AnimConfig) -> Self {
        let mut v = Vermiculate {
            hei: config.height as i32,
            wid: config.width as i32,
            speed: 1,
            erasing: true,
            autopal: false,
            instring: Vec::new(),
            inpos: 0,
            sinof: [0.0; DEGS as usize],
            cosof: [0.0; DEGS as usize],
            tanof: [0.0; DEGS as usize],
            point: Vec::new(),
            canvas: Vec::new(),
            thread: (0..THRMAX).map(|_| LineData::new()).collect(),
            bank: [0; THRMAX],
            bnkt: 0,
            boxw: 10,
            boxh: 10,
            curviness: 30,
            gridden: 0,
            ogd: 8,
            bordcorn: 0,
            bordcol: 1,
            threads: 4,
            ch: '\0',
            reset_p: true,
            cyc: 0,
            pscale: 1,
            mycolors: [Color::new(255, 0, 0, 0); TAILMAX],
            delay_us: 10_000,
        };
        v.reset(config);
        v
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();
        let mut this_delay = 0u64;
        let mut loop_count = 0;

        loop {
            if self.reset_p {
                self.reset_p = false;
                self.clearscreen();
                for thr in 1..=self.threads as usize {
                    self.newonscreen(thr, &mut rng);
                }
                if self.autopal {
                    self.randpal(&mut rng);
                }
                self.bordupdate();
                self.gridupdate(false, &mut rng);
            }

            let mut alltrap = true;
            for thr in 1..=self.threads as usize {
                if self.move_thread(thr, &mut rng) {
                    alltrap = false;
                }
            }
            if alltrap {
                // all threads are trapped
                self.reset_p = true;
            }
            if self.speed != SPEEDMAX {
                this_delay = self.waitabit();
            }

            // (the C's ticks/instring re-init branch is unreachable: `tick`
            // is a local there and the instring pointer is never nulled)

            if this_delay == 0 && loop_count < 1000 {
                loop_count += 1;
                continue;
            }
            break;
        }
        self.delay_us = this_delay;
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        if self.pscale == 1 {
            let w = (self.wid.min(width as i32) as usize) * 4;
            let h = self.hei.min(height as i32) as usize;
            for y in 0..h {
                let src = y * self.wid as usize * 4;
                let dst = y * width as usize * 4;
                buffer[dst..dst + w].copy_from_slice(&self.canvas[src..src + w]);
            }
        } else {
            // Legacy full palette scan for the >2560px pscale path: its
            // overlapping pscale x pscale blocks depend on scan order, which
            // the canvas (write-order) can't reproduce.
            let w = self.wid.min(width as i32);
            let h = self.hei.min(height as i32);
            for y in 0..h {
                let row = (self.wid * y) as usize;
                for x in 0..w {
                    let c = self.point[row + x as usize] as usize;
                    if c == 0 {
                        continue;
                    }
                    let color = self.mycolors[c.min(TAILMAX - 1)];
                    for dy in 0..self.pscale {
                        for dx in 0..self.pscale {
                            put_pixel(buffer, width, height, x + dx, y + dy, color);
                        }
                    }
                }
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.wid = config.width as i32;
        self.hei = config.height as i32;
        self.reset_p = true;
        self.speed = 1;
        self.autopal = false;
        self.cyc = 0;
        self.instring = Vec::new();
        self.inpos = 0;
        self.mycolors[0] = Color::new(255, 0, 0, 0);

        self.pscale = if config.width > 2560 || config.height > 2560 {
            3 // Retina displays
        } else {
            1
        };

        self.point = vec![0u8; (self.wid * self.hei).max(0) as usize];
        self.canvas = vec![0u8; (self.wid * self.hei).max(0) as usize * 4];
        self.repaint_canvas(); // seed alpha + background
        self.maininit(&mut rng);
        self.consume_instring(&mut rng);
        self.delay_us = 10_000;
    }

    fn render_policy(&self) -> RenderPolicy {
        if self.pscale != 1 {
            RenderPolicy::ClearThenRender
        } else {
            RenderPolicy::CompleteFrame
        }
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
