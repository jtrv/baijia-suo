/* bouboule --- glob of spheres twisting and changing size */

/*-
 * Copyright 1996 by Jeremie PETIT <petit@eurecom.fr>, <jpetit@essi.fr>
 *
 * Permission to use, copy, modify, and distribute this software and its
 * documentation for any purpose and without fee is hereby granted,
 * provided that the above copyright notice appear in all copies and that
 * both that copyright notice and this permission notice appear in
 * supporting documentation.
 *
 * Rust port of xlockmore modes/bouboule.c (the 2D, non-use3d path).
 * Deliberate deviations: the old-arcs/adaptive erase machinery is replaced
 * by a full-frame clear each tick, and the xlockmore colormap (SMOOTH_COLORS,
 * 64 pixels) is emulated with an HSV hue wheel.
 */

use std::f64::consts::PI;
use rand::Rng;
use crate::animation::primitives::{put_pixel, Color};
use crate::animation::{AnimConfig, Animation, RenderPolicy};

const MINSTARS: i32 = 1;
const MINSIZE: i32 = 1;
const COLOR_CHANGES: i16 = 50;
const MAX_SIZEX_SIZEY: f64 = 2.0;
// Shrink factor on the size caps so the ball keeps padding around its
// drift path instead of grazing the screen edges at full throb.
const PAD: f64 = 0.95;

const THETACANRAND: i32 = 80;
const SIZECANRAND: i32 = 80;
const POSCANRAND: i32 = 80;

const VARRANDMIN: f64 = -70.0;
const VARRANDMAX: f64 = 70.0;

const MINZVAL: f64 = 100.0;
const MAXZVAL: f64 = 10000.0;

fn nrand(rng: &mut impl Rng, n: i32) -> i32 {
    if n <= 1 {
        0
    } else {
        rng.random_range(0..n)
    }
}

#[derive(Clone, Default)]
struct SinVariable {
    alpha: f64,
    step: f64,
    minimum: f64,
    maximum: f64,
    value: f64,
    mayrand: i32,
    varrand: Option<Box<SinVariable>>,
}

impl SinVariable {
    fn new(rng: &mut impl Rng, alpha: f64, step: f64, minimum: f64, maximum: f64, mayrand: i32) -> Self {
        let mut varrand = None;
        if mayrand != 0 {
            let vr_alpha = nrand(rng, (PI * 1000.0) as i32) as f64 / 1000.0;
            let vr_step = PI / (nrand(rng, 100) as f64 + 100.0);
            let mut vr = SinVariable::new(rng, vr_alpha, vr_step, VARRANDMIN, VARRANDMAX, 0);
            vr.vary(rng);
            varrand = Some(Box::new(vr));
        }
        let mut sv = Self {
            alpha,
            step,
            minimum,
            maximum,
            value: 0.0,
            mayrand,
            varrand,
        };
        sv.vary(rng);
        sv
    }

    fn vary(&mut self, rng: &mut impl Rng) {
        self.value = self.minimum + (self.maximum - self.minimum) * (self.alpha.sin() + 1.0) / 2.0;
        if self.mayrand == 0 {
            self.alpha += self.step;
        } else {
            let vaval = nrand(rng, 100);
            if vaval <= self.mayrand {
                if let Some(ref mut vr) = self.varrand {
                    vr.vary(rng);
                }
            }
            let vr_val = self.varrand.as_ref().map(|vr| vr.value).unwrap_or(0.0);
            self.alpha += (100.0 + vr_val) * self.step / 100.0;
        }
        if self.alpha > 2.0 * PI {
            self.alpha -= 2.0 * PI;
        }
    }
}

#[derive(Clone)]
struct Star {
    x: f64,
    y: f64,
    z: f64,
    size: i16,
}

#[derive(Clone)]
struct XArc {
    x: i32,
    y: i32,
    size: i32,
}

pub struct Bouboule {
    width: u32,
    height: u32,
    max_star_size: i16,
    x: SinVariable,
    y: SinVariable,
    z: SinVariable,
    sizex: SinVariable,
    sizey: SinVariable,
    thetax: SinVariable,
    thetay: SinVariable,
    thetaz: SinVariable,
    stars: Vec<Star>,
    xarcs: Vec<XArc>,
    color: Color,
    colorp: i32,
    nb_stars: i32,
    colorchange: i16,
    ncolors: i32,
}

impl Bouboule {
    // XFillArc over a size x size bounding box at (x, y): a filled disc.
    fn fill_arc(&self, buffer: &mut [u8], x: i32, y: i32, size: i32) {
        if size <= 0 {
            return;
        }
        let r = size as f32 / 2.0;
        let cx = x as f32 + r;
        let cy = y as f32 + r;
        for yy in y..y + size {
            for xx in x..x + size {
                let dx = xx as f32 + 0.5 - cx;
                let dy = yy as f32 + 0.5 - cy;
                if dx * dx + dy * dy <= r * r {
                    put_pixel(buffer, self.width, self.height, xx, yy, self.color);
                }
            }
        }
    }
}

impl Animation for Bouboule {
    fn new(config: &AnimConfig) -> Self {
        let mut b = Bouboule {
            width: config.width,
            height: config.height,
            max_star_size: 0,
            x: SinVariable::default(),
            y: SinVariable::default(),
            z: SinVariable::default(),
            sizex: SinVariable::default(),
            sizey: SinVariable::default(),
            thetax: SinVariable::default(),
            thetay: SinVariable::default(),
            thetaz: SinVariable::default(),
            stars: Vec::new(),
            xarcs: Vec::new(),
            color: Color::new(255, 255, 255, 255),
            colorp: 0,
            nb_stars: 0,
            colorchange: 0,
            ncolors: 64,
        };
        b.reset(config);
        b
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        self.thetax.vary(&mut rng);
        self.thetay.vary(&mut rng);
        self.thetaz.vary(&mut rng);

        self.x.vary(&mut rng);
        self.y.vary(&mut rng);

        // Same 16:9 basis clamp + PAD as reset(): this per-tick recompute is
        // what actually governs the throb bounds — without the clamp here the
        // init-time padding is overwritten on the first frame.
        let wbasis = (self.width.min(self.height * 16 / 9)) as f64;
        self.sizex.maximum = ((self.width as f64) - self.x.value)
            .min(self.x.value)
            .min(wbasis / 2.0)
            * PAD;
        self.sizex.minimum = self.sizex.maximum / 3.0;

        self.sizey.minimum = (self.sizex.value / MAX_SIZEX_SIZEY).max(self.sizey.maximum / 3.0);
        self.sizey.maximum = (self.sizex.value * MAX_SIZEX_SIZEY)
            .min(((self.height as f64) - self.y.value).min(self.y.value) * PAD);
        // The C leaves minimum > maximum possible when the ball drifts near a
        // horizontal edge (minimum tracks sizex, maximum tracks edge distance),
        // which lets the throb overshoot the cap and touch the edge. Clamp.
        self.sizey.minimum = self.sizey.minimum.min(self.sizey.maximum);

        self.sizex.vary(&mut rng);
        self.sizey.vary(&mut rng);

        let cx = self.thetax.value.cos();
        let sx = self.thetax.value.sin();
        let cy = self.thetay.value.cos();
        let sy = self.thetay.value.sin();
        let cz = self.thetaz.value.cos();
        let sz = self.thetaz.value.sin();

        for i in 0..self.nb_stars as usize {
            let star = &self.stars[i];
            
            let nx = self.sizex.value
                * ((cy * cz - sx * sy * sz) * star.x
                    + (-cx * sz) * star.y
                    + (sy * cz + sz * sx * cy) * star.z)
                + self.x.value;
            let ny = self.sizey.value
                * ((cy * sz + sx * sy * cz) * star.x
                    + (cx * cz) * star.y
                    + (sy * sz - sx * cy * cz) * star.z)
                + self.y.value;

            // The C code truncates to short, then shifts the bounding box
            // up-left by the star size (XArc x/y is the bbox corner).
            let mut ax = nx as i32;
            let mut ay = ny as i32;
            if star.size != 0 {
                ax -= star.size as i32;
                ay -= star.size as i32;
            }
            self.xarcs[i].x = ax;
            self.xarcs[i].y = ay;
            self.xarcs[i].size = star.size as i32;
        }

        if self.ncolors > 2 {
            self.colorchange += 1;
            if self.colorchange >= COLOR_CHANGES {
                self.colorchange = 0;
                self.colorp += 1;
                if self.colorp >= self.ncolors {
                    self.colorp = 0;
                }
                self.color = Color::from_hsl(self.colorp as f32 / self.ncolors as f32, 1.0, 0.5);
            }
        }
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        for arc in &self.xarcs {
            // XFillArc bbox is (2 + size) square: size-0 stars are 2x2 discs.
            self.fill_arc(buffer, arc.x, arc.y, 2 + arc.size);
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.width = config.width;
        self.height = config.height;

        // xlockmore defaults: *size: 15, *count: 100
        let size = if config.size == 0 { 15 } else { config.size };
        self.max_star_size = if size < -MINSIZE {
            nrand(&mut rng, -size - MINSIZE + 1) as i16 + MINSIZE as i16
        } else if size < MINSIZE {
            MINSIZE as i16
        } else {
            size as i16
        };

        let count = if config.count == 0 { 100 } else { config.count };
        self.nb_stars = if count < -MINSTARS {
            nrand(&mut rng, -count - MINSTARS + 1) + MINSTARS
        } else if count < MINSTARS {
            MINSTARS
        } else {
            count
        };

        let amp = nrand(&mut rng, 3142) as f64 / 1000.0;
        let freq = PI / (nrand(&mut rng, 100) as f64 + 100.0);
        self.x = SinVariable::new(
            &mut rng,
            amp,
            freq,
            (self.width as f64) / 4.0,
            3.0 * (self.width as f64) / 4.0,
            POSCANRAND,
        );
        let amp = nrand(&mut rng, 3142) as f64 / 1000.0;
        let freq = PI / (nrand(&mut rng, 100) as f64 + 100.0);
        self.y = SinVariable::new(
            &mut rng,
            amp,
            freq,
            (self.height as f64) / 4.0,
            3.0 * (self.height as f64) / 4.0,
            POSCANRAND,
        );
        // z range and x radius derive from raw width in the C, which balloons
        // the ball on ultrawide; clamp the basis to a 16:9 width (same fix as
        // life3d). The edge-distance min() below still uses the real width so
        // the ball never overruns the sides.
        let wbasis = (self.width.min(self.height * 16 / 9)) as f64;
        let amp = nrand(&mut rng, 3142) as f64 / 1000.0;
        let freq = PI / (nrand(&mut rng, 100) as f64 + 100.0);
        self.z = SinVariable::new(
            &mut rng,
            amp,
            freq,
            wbasis / 2.0 + MINZVAL,
            wbasis / 2.0 + MAXZVAL,
            POSCANRAND,
        );

        let amp = nrand(&mut rng, 3142) as f64 / 1000.0;
        let freq = PI / (nrand(&mut rng, 100) as f64 + 100.0);
        let max_sx = ((self.width as f64) - self.x.value)
            .min(self.x.value)
            .min(wbasis / 2.0)
            * PAD;
        self.sizex = SinVariable::new(
            &mut rng,
            amp,
            freq,
            max_sx / 5.0,
            max_sx,
            SIZECANRAND,
        );
        
        let sizey_max = (self.sizex.value * MAX_SIZEX_SIZEY)
            .min(((self.height as f64) - self.y.value).min(self.y.value) * PAD);
        let sizey_min = (self.sizex.value / MAX_SIZEX_SIZEY)
            .max(self.sizey.maximum / 5.0)
            .min(sizey_max);
        
        let amp = nrand(&mut rng, 3142) as f64 / 1000.0;
        let freq = PI / (nrand(&mut rng, 100) as f64 + 100.0);
        self.sizey = SinVariable::new(
            &mut rng,
            amp,
            freq,
            sizey_min,
            sizey_max,
            SIZECANRAND,
        );

        let amp = nrand(&mut rng, 3142) as f64 / 1000.0;
        let freq = PI / (nrand(&mut rng, 200) as f64 + 200.0);
        self.thetax = SinVariable::new(
            &mut rng,
            amp,
            freq,
            -PI,
            PI,
            THETACANRAND,
        );
        let amp = nrand(&mut rng, 3142) as f64 / 1000.0;
        let freq = PI / (nrand(&mut rng, 200) as f64 + 200.0);
        self.thetay = SinVariable::new(
            &mut rng,
            amp,
            freq,
            -PI,
            PI,
            THETACANRAND,
        );
        let amp = nrand(&mut rng, 3142) as f64 / 1000.0;
        let freq = PI / (nrand(&mut rng, 400) as f64 + 400.0);
        self.thetaz = SinVariable::new(
            &mut rng,
            amp,
            freq,
            -PI,
            PI,
            THETACANRAND,
        );

        self.stars.clear();
        self.xarcs.clear();

        for _ in 0..self.nb_stars {
            let theta = (nrand(&mut rng, 1800) as f64 / 10.0 - 90.0) * PI / 180.0;
            let omega = (nrand(&mut rng, 3600) as f64 / 10.0 - 180.0) * PI / 180.0;

            let x = theta.cos() * omega.sin();
            let y = omega.sin() * theta.sin();
            let z = omega.cos();

            let mut star_size = nrand(&mut rng, 2 * self.max_star_size as i32) as i16;
            if star_size < self.max_star_size {
                star_size = 0;
            } else {
                star_size -= self.max_star_size;
            }

            self.stars.push(Star { x, y, z, size: star_size });
            self.xarcs.push(XArc { x: 0, y: 0, size: star_size as i32 });
        }

        self.ncolors = if config.ncolors <= 0 { 64 } else { config.ncolors };

        if self.ncolors > 2 {
            self.colorp = nrand(&mut rng, self.ncolors);
            self.color = Color::from_hsl(self.colorp as f32 / self.ncolors as f32, 1.0, 0.5);
        } else {
            self.color = Color::new(255, 255, 255, 255);
        }
        self.colorchange = 0;
    }

    fn render_policy(&self) -> RenderPolicy {
        RenderPolicy::ClearThenRender
    }

    fn frame_delay_us(&self) -> u64 {
        10_000 // match xlockmore default
    }
}
