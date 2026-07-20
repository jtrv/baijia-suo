//! Spinning galaxies.
//
// Originally done by Uli Siegmund <uli@wombat.okapi.sub.org> on Amiga
//   for EGS in Cluster
// Port from Cluster/EGS to C/Intuition by Harald Backert
// Port to X11 and incorporation into xlockmore by Hubert Feyrer
//   <hubert.feyrer@rz.uni-regensburg.de>
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
// Rust port of xlockmore/modes/galaxy.c.

use rand::Rng;
use std::f64::consts::PI;
use crate::animation::{AnimConfig, Animation, RenderPolicy};
use crate::animation::primitives::{put_pixel, Color, clear_buffer};

const MINSIZE: i32 = 1;
const MINGALAXIES: i32 = 2;
const MAX_STARS: i32 = 300;
const MAX_IDELTAT: i32 = 50;

const EPSILON: f64 = 0.00000001;
const SQRT_EPSILON: f64 = 0.0001;

const DELTAT: f64 = (MAX_IDELTAT as f64) * 0.0001;
const GALAXYRANGESIZE: f64 = 0.1;
const GALAXYMINSIZE: f64 = 0.1;
const QCONS: f64 = 0.001;
const Z_OFFSET: f64 = 1.25;

const COLORS: i32 = 8;

#[derive(Clone, Default)]
struct Star {
    pos: [f64; 3],
    vel: [f64; 3],
    px: i32,
    py: i32,
    size: i32,
    old_px: i32,
    old_py: i32,
    old_size: i32,
    color_idx: i32,
    z_size: i32,
}

#[derive(Clone, Default)]
struct GalaxyStruct {
    mass: i32,
    nstars: i32,
    stars: Vec<Star>,
    pos: [f64; 3],
    vel: [f64; 3],
    galcol_idx: i32,
}

#[derive(Clone, Copy)]
struct GalaxyInfo {
    pos: [f64; 3],
    mass: i32,
    galcol_idx: i32,
}

pub struct Galaxy {
    galaxies: Vec<GalaxyStruct>,
    ngalaxies: i32,
    f_hititerations: i32,
    step: i32,
    tracks: bool,
    fisheye: bool,
    
    clip_left: i32,
    clip_right: i32,
    clip_top: i32,
    clip_bottom: i32,
    
    mat: [[f64; 3]; 3],
    scale: f64,
    midx: i32,
    midy: i32,
    size: f64,
    star_scale_z: f64,
    
    width: u32,
    height: u32,
    delay_us: u64,
    
    ncolors: i32,
    cycles: i32,
    count: i32,
    star_size_param: i32,
    
    palette: Vec<Color>,
}

fn draw_star(buffer: &mut [u8], width: u32, height: u32, x: i32, y: i32, size: i32, color: Color) {
    if size <= 1 {
        put_pixel(buffer, width, height, x, y, color);
    } else {
        let radius = size / 2;
        let cx = x + radius;
        let cy = y + radius;
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= r2 {
                    put_pixel(buffer, width, height, cx + dx, cy + dy, color);
                }
            }
        }
    }
}

impl Galaxy {
    fn startover(&mut self) -> bool {
        let mut rng = rand::rng();
        self.step = 0;

        let count = self.count;
        if count < -MINGALAXIES {
            self.ngalaxies = rng.random_range(0..(-count - MINGALAXIES + 1)) + MINGALAXIES;
        } else if count < MINGALAXIES {
            self.ngalaxies = MINGALAXIES;
        } else {
            self.ngalaxies = count;
        }

        self.galaxies.clear();
        for _ in 0..self.ngalaxies {
            let mut gt = GalaxyStruct::default();

            let colorbase = self.ncolors / COLORS;
            if self.ncolors >= COLORS {
                loop {
                    gt.galcol_idx = rng.random_range(0..colorbase) * COLORS;
                    let green = 22 * self.ncolors / 64;
                    let notgreen = 7 * self.ncolors / 64;
                    let c = gt.galcol_idx + colorbase / 2;
                    if !(c < green + notgreen && c > green - notgreen) {
                        break;
                    }
                }
            } else {
                gt.galcol_idx = 0;
            }

            gt.nstars = rng.random_range(0..(MAX_STARS / 2)) + MAX_STARS / 2;
            gt.stars = vec![Star::default(); gt.nstars as usize];

            let w1 = 2.0 * PI * rng.random::<f64>();
            let w2 = 2.0 * PI * rng.random::<f64>();
            let sinw1 = w1.sin();
            let sinw2 = w2.sin();
            let cosw1 = w1.cos();
            let cosw2 = w2.cos();

            self.mat[0][0] = cosw2;
            self.mat[0][1] = -sinw1 * sinw2;
            self.mat[0][2] = cosw1 * sinw2;
            self.mat[1][0] = 0.0;
            self.mat[1][1] = cosw1;
            self.mat[1][2] = sinw1;
            self.mat[2][0] = -sinw2;
            self.mat[2][1] = -sinw1 * cosw2;
            self.mat[2][2] = cosw1 * cosw2;

            gt.vel[0] = rng.random::<f64>() * 2.0 - 1.0;
            gt.vel[1] = rng.random::<f64>() * 2.0 - 1.0;
            gt.vel[2] = rng.random::<f64>() * 2.0 - 1.0;
            gt.pos[0] = -gt.vel[0] * DELTAT * (self.f_hititerations as f64) + rng.random::<f64>() - 0.5;
            gt.pos[1] = -gt.vel[1] * DELTAT * (self.f_hititerations as f64) + rng.random::<f64>() - 0.5;
            gt.pos[2] = (-gt.vel[2] * DELTAT * (self.f_hititerations as f64) + rng.random::<f64>() - 0.5) + Z_OFFSET;

            gt.mass = (rng.random::<f64>() * 1000.0) as i32 + 1;

            self.size = GALAXYRANGESIZE * rng.random::<f64>() + GALAXYMINSIZE;

            for j in 0..gt.nstars as usize {
                let st = &mut gt.stars[j];
                let w = 2.0 * PI * rng.random::<f64>();
                let sinw = w.sin();
                let cosw = w.cos();
                let d = rng.random::<f64>() * self.size;
                let mut h = rng.random::<f64>() * (-2.0 * (d / self.size)).exp() / 5.0 * self.size;
                if rng.random::<bool>() {
                    h = -h;
                }

                st.pos[0] = self.mat[0][0] * d * cosw + self.mat[1][0] * d * sinw + self.mat[2][0] * h + gt.pos[0];
                st.pos[1] = self.mat[0][1] * d * cosw + self.mat[1][1] * d * sinw + self.mat[2][1] * h + gt.pos[1];
                st.pos[2] = self.mat[0][2] * d * cosw + self.mat[1][2] * d * sinw + self.mat[2][2] * h + gt.pos[2];

                let v = ((gt.mass as f64) * QCONS / (d * d + h * h).sqrt()).sqrt();
                st.vel[0] = -self.mat[0][0] * v * sinw + self.mat[1][0] * v * cosw + gt.vel[0];
                st.vel[1] = -self.mat[0][1] * v * sinw + self.mat[1][1] * v * cosw + gt.vel[1];
                st.vel[2] = -self.mat[0][2] * v * sinw + self.mat[1][2] * v * cosw + gt.vel[2];

                st.vel[0] *= DELTAT;
                st.vel[1] *= DELTAT;
                st.vel[2] *= DELTAT;

                st.px = 0;
                st.py = 0;
                st.old_px = 0;
                st.old_py = 0;

                if self.star_size_param < -MINSIZE {
                    st.size = rng.random_range(0..(-self.star_size_param - MINSIZE + 1)) + MINSIZE;
                } else if self.star_size_param < MINSIZE {
                    st.size = MINSIZE;
                } else {
                    st.size = self.star_size_param;
                }
                st.z_size = st.size;
                st.old_size = st.size;
            }

            self.galaxies.push(gt);
        }
        
        true
    }
}

impl Animation for Galaxy {
    fn new(config: &AnimConfig) -> Self {
        let mut s = Galaxy {
            galaxies: Vec::new(),
            ngalaxies: 0,
            f_hititerations: 0,
            step: 0,
            tracks: false,
            fisheye: false,
            clip_left: 0,
            clip_right: 0,
            clip_top: 0,
            clip_bottom: 0,
            mat: [[0.0; 3]; 3],
            scale: 0.0,
            midx: 0,
            midy: 0,
            size: 0.0,
            star_scale_z: 0.0,
            width: config.width,
            height: config.height,
            delay_us: config.delay_us,
            ncolors: 64,
            cycles: 250,
            count: -5,
            star_size_param: -3,
            palette: Vec::new(),
        };
        s.reset(config);
        s
    }

    fn tick(&mut self) {
        self.step += 1;
        if self.step > self.f_hititerations * 4 {
            self.startover();
            return;
        }

        let mut gal_infos = Vec::with_capacity(self.ngalaxies as usize);
        for g in &self.galaxies {
            gal_infos.push(GalaxyInfo {
                pos: g.pos,
                mass: g.mass,
                galcol_idx: g.galcol_idx,
            });
        }

        for i in 0..self.ngalaxies as usize {
            let gt_galcol_idx = gal_infos[i].galcol_idx;

            for j in 0..self.galaxies[i].nstars as usize {
                let st = &mut self.galaxies[i].stars[j];

                let mut v0 = st.vel[0];
                let mut v1 = st.vel[1];
                let mut v2 = st.vel[2];

                for k in 0..self.ngalaxies as usize {
                    let gtk = &gal_infos[k];
                    let d0 = gtk.pos[0] - st.pos[0];
                    let d1 = gtk.pos[1] - st.pos[1];
                    let d2 = gtk.pos[2] - st.pos[2];

                    let mut d = d0 * d0 + d1 * d1 + d2 * d2;
                    let gtk_mass = gtk.mass as f64;
                    if d > EPSILON {
                        d = gtk_mass / (d * d.sqrt()) * DELTAT * DELTAT * QCONS;
                    } else {
                        d = gtk_mass / (EPSILON * SQRT_EPSILON) * DELTAT * DELTAT * QCONS;
                    }
                    v0 += d0 * d;
                    v1 += d1 * d;
                    v2 += d2 * d;
                }

                st.vel[0] = v0;
                st.vel[1] = v1;
                st.vel[2] = v2;

                let colorbase = self.ncolors / COLORS;
                let d = (v0 * v0 + v1 * v1 + v2 * v2) / (3.0 * DELTAT * DELTAT);
                if d > colorbase as f64 {
                    st.color_idx = gt_galcol_idx + colorbase - 1;
                } else {
                    st.color_idx = gt_galcol_idx + (d as i32) % colorbase;
                }

                st.pos[0] += v0;
                st.pos[1] += v1;
                st.pos[2] += v2;

                st.old_px = st.px;
                st.old_py = st.py;
                st.old_size = st.size;

                if self.fisheye {
                    if st.pos[2] > 0.0 {
                        st.px = ((st.pos[0] * self.scale) / st.pos[2]) as i32 + self.midx;
                        st.py = ((st.pos[1] * self.scale) / st.pos[2]) as i32 + self.midy;
                        st.size = (self.star_scale_z / st.pos[2]) as i32 + st.z_size;
                        if st.size > 12 {
                            st.size = 12;
                        }
                    }
                } else {
                    st.px = (st.pos[0] * self.scale) as i32 + self.midx;
                    st.py = (st.pos[1] * self.scale) as i32 + self.midy;
                }
            }

            for k in (i + 1)..self.ngalaxies as usize {
                let d0 = self.galaxies[k].pos[0] - self.galaxies[i].pos[0];
                let d1 = self.galaxies[k].pos[1] - self.galaxies[i].pos[1];
                let d2 = self.galaxies[k].pos[2] - self.galaxies[i].pos[2];

                let mut d = d0 * d0 + d1 * d1 + d2 * d2;
                let mass_i = self.galaxies[i].mass as f64;
                let mass_k = self.galaxies[k].mass as f64;

                if d > EPSILON {
                    d = mass_i * mass_i / (d * d.sqrt()) * DELTAT * QCONS;
                } else {
                    d = mass_i * mass_i / (EPSILON * SQRT_EPSILON) * DELTAT * QCONS;
                }
                let d0_adj = d0 * d;
                let d1_adj = d1 * d;
                let d2_adj = d2 * d;

                self.galaxies[i].vel[0] += d0_adj / mass_i;
                self.galaxies[i].vel[1] += d1_adj / mass_i;
                self.galaxies[i].vel[2] += d2_adj / mass_i;
                self.galaxies[k].vel[0] -= d0_adj / mass_k;
                self.galaxies[k].vel[1] -= d1_adj / mass_k;
                self.galaxies[k].vel[2] -= d2_adj / mass_k;
            }

            self.galaxies[i].pos[0] += self.galaxies[i].vel[0] * DELTAT;
            self.galaxies[i].pos[1] += self.galaxies[i].vel[1] * DELTAT;
            self.galaxies[i].pos[2] += self.galaxies[i].vel[2] * DELTAT;
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let black = Color::new(255, 0, 0, 0);
        let white = Color::new(255, 255, 255, 255);

        if self.step == 0 {
            clear_buffer(buffer, black);
        }

        for i in 0..self.ngalaxies as usize {
            let gt = &self.galaxies[i];
            for j in 0..gt.nstars as usize {
                let st = &gt.stars[j];

                // Erase old star if not fisheye
                if !self.fisheye
                    && st.old_px >= self.clip_left && st.old_px <= self.clip_right - st.old_size 
                        && st.old_py >= self.clip_top && st.old_py <= self.clip_bottom - st.old_size {
                        draw_star(buffer, width, height, st.old_px, st.old_py, st.old_size, black);
                    }

                let mut clipped = true;
                if self.fisheye {
                    if st.pos[2] > 0.0 {
                        clipped = false;
                    }
                } else {
                    if st.px >= self.clip_left && st.px <= self.clip_right - st.size 
                        && st.py >= self.clip_top && st.py <= self.clip_bottom - st.size {
                        clipped = false;
                    }
                }

                if !clipped {
                    let color = if self.ncolors >= COLORS {
                        // Clamp color index to bounds just in case
                        let idx = (st.color_idx as usize) % self.palette.len();
                        self.palette[idx]
                    } else {
                        white
                    };

                    if self.tracks {
                        draw_star(buffer, width, height, st.px + 1, st.py, st.size, color);
                    } else {
                        draw_star(buffer, width, height, st.px, st.py, st.size, color);
                    }
                }
            }
        }
    }

    fn reset(&mut self, config: &AnimConfig) {
        self.width = config.width;
        self.height = config.height;
        self.clip_left = 0;
        self.clip_top = 0;
        self.clip_right = config.width as i32;
        self.clip_bottom = config.height as i32;

        self.scale = (self.clip_right + self.clip_bottom) as f64 / 8.0;
        self.midx = self.clip_right / 2;
        self.midy = self.clip_bottom / 2;

        self.cycles = if config.cycles <= 0 { 250 } else { config.cycles };
        self.f_hititerations = self.cycles;
        self.count = if config.count == 0 { -5 } else { config.count };
        self.star_size_param = if config.size == 0 { -3 } else { config.size };
        self.ncolors = if config.ncolors < 8 { 64 } else { config.ncolors };
        self.delay_us = config.delay_us;

        self.palette = (0..self.ncolors).map(|i| {
            Color::from_hsl(i as f32 / self.ncolors as f32, 1.0, 0.5)
        }).collect();

        let mut rng = rand::rng();
        self.fisheye = rng.random_range(0..3) == 0;
        self.tracks = false;
        if !self.fisheye {
            self.tracks = rng.random::<bool>();
        }

        if self.fisheye {
            self.scale *= Z_OFFSET;
            self.star_scale_z = self.scale * 0.005;
        }

        self.startover();
    }

    fn render_policy(&self) -> RenderPolicy {
        if self.fisheye {
            RenderPolicy::ClearThenRender
        } else {
            RenderPolicy::Incremental
        }
    }

    fn frame_delay_us(&self) -> u64 {
        20_000 // tuned for modern machines (50 FPS)
    }
}
