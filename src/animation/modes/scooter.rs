//! Flying through a tunnel of doors (EGS Nightshift 'scooter').
//
//  Copyright (c) 2001 Sven Thoennissen <posse@gmx.net>
//
//  This program is based on the original "scooter", a blanker module from the
//  Nightshift screensaver which is part of EGS (Enhanced Graphics System) on
//  the Amiga computer. EGS has been developed by VIONA Development.
//
//
//  (now the obligatory stuff)
//
//  Permission to use, copy, modify, and distribute this software and its
//  documentation for any purpose and without fee is hereby granted,
//  provided that the above copyright notice appear in all copies and that
//  both that copyright notice and this permission notice appear in
//  supporting documentation.
//
//  This file is provided AS IS with no warranties of any kind.  The author
//  shall have no liability with respect to the infringement of copyrights,
//  trade secrets or any patents by this file or any part thereof.  In no
//  event will the author be liable for any lost revenue or profits or
//  other special, indirect and consequential damages.
//
// Rust port of xlockmore/modes/scooter.c.

use crate::animation::{AnimConfig, Animation};
use crate::animation::primitives::{draw_line, put_pixel, Color};
use rand::Rng;

const MIN_DOORS: i32 = 4;
const MIN_SPEED: i32 = 1;
const MAX_SPEED: i32 = 10;
const SPACE_XY_FACTOR: i32 = 10;
const DOOR_WIDTH: i32 = 600 * SPACE_XY_FACTOR;
const DOOR_HEIGHT: i32 = 400 * SPACE_XY_FACTOR;
const STAR_MIN_X: i32 = 1000 * SPACE_XY_FACTOR;
const STAR_MIN_Y: i32 = 750 * SPACE_XY_FACTOR;
const STAR_MAX_X: i32 = 10000 * SPACE_XY_FACTOR;
const STAR_MAX_Y: i32 = 7500 * SPACE_XY_FACTOR;
const STAR_SIZE_MIN: i32 = 2 * SPACE_XY_FACTOR;
const STAR_SIZE_MAX: i32 = 64 * SPACE_XY_FACTOR;
const DOOR_CURVEDNESS: i32 = 14;
const PROJECTION_DEGREE: f32 = 2.4;
const ASPECT_SCREENWIDTH: i32 = 1152;
const ASPECT_SCREENHEIGHT: i32 = 864;
const SINUSTABLE_SIZE: usize = 0x8000;
const SINUSTABLE_MASK: i32 = 0x7fff;

#[derive(Clone, Copy)]
struct Vec3D {
    x: i32,
    y: i32,
    z: i32,
}

#[derive(Clone, Copy)]
struct Angle3D {
    x: i32,
    y: i32,
    z: i32,
}

#[derive(Clone, Copy)]
struct ColorRGB {
    r: i32,
    g: i32,
    b: i32,
}

#[derive(Clone, Copy)]
struct Door {
    zelement: i32,
    color: Color,
}

#[derive(Clone, Copy)]
struct Star {
    xpos: i32,
    ypos: i32,
    width: i32,
    height: i32,
    zelement: i32,
    draw: i32,
}

#[derive(Clone, Copy)]
struct ZElement {
    pos: Vec3D,
    angle: Angle3D,
}

struct Rect {
    lefttop: (i32, i32),
    rightbottom: (i32, i32),
}

fn sgn(a: i32) -> i32 {
    if a < 0 {
        -1
    } else {
        1
    }
}

fn randomcolor(rng: &mut impl Rng) -> ColorRGB {
    let n = rng.random_range(0..0x1000000);
    ColorRGB {
        r: (n >> 16) << 8,
        g: ((n >> 8) & 0xff) << 8,
        b: (n & 0xff) << 8,
    }
}

fn clipline(p1: &mut (i32, i32), p2: &mut (i32, i32), rect: &Rect) -> bool {
    let mut new1 = *p1;
    let mut new2 = *p2;

    if (new1.0 >= rect.lefttop.0 && new1.0 <= rect.rightbottom.0)
        || (new1.1 >= rect.lefttop.1 && new1.1 <= rect.rightbottom.1)
        || (new2.0 >= rect.lefttop.0 && new2.0 <= rect.rightbottom.0)
        || (new2.1 >= rect.lefttop.1 && new2.1 <= rect.rightbottom.1)
    {
        return true;
    }

    if new1.1 > new2.1 {
        std::mem::swap(&mut new1, &mut new2);
    }

    if new2.1 < rect.lefttop.1 || new1.1 > rect.rightbottom.1 {
        return false;
    }

    let mut m = if new2.0 == new1.0 {
        0.0
    } else {
        (new2.1 - new1.1) as f32 / (new2.0 - new1.0) as f32
    };

    if new1.1 < rect.lefttop.1 {
        if m != 0.0 {
            new1.0 += ((rect.lefttop.1 - new1.1) as f32 / m) as i32;
        }
        new1.1 = rect.lefttop.1;
    }
    if new2.1 > rect.rightbottom.1 {
        if m != 0.0 {
            new2.0 -= ((new2.1 - rect.rightbottom.1) as f32 / m) as i32;
        }
        new2.1 = rect.rightbottom.1;
    }

    if new1.0 > new2.0 {
        std::mem::swap(&mut new1, &mut new2);
    }

    if new2.0 < rect.lefttop.0 || new1.0 > rect.rightbottom.0 {
        return false;
    }

    m = if new2.0 == new1.0 {
        0.0
    } else {
        (new2.1 - new1.1) as f32 / (new2.0 - new1.0) as f32
    };

    if new1.0 < rect.lefttop.0 {
        new1.1 += ((rect.lefttop.0 - new1.0) as f32 * m) as i32;
        new1.0 = rect.lefttop.0;
    }
    if new2.0 > rect.rightbottom.0 {
        new2.1 -= ((new2.0 - rect.rightbottom.0) as f32 * m) as i32;
        new2.0 = rect.rightbottom.0;
    }

    *p1 = new1;
    *p2 = new2;

    true
}

pub struct Scooter {
    stars: Vec<Star>,
    doors: Vec<Door>,
    zelements: Vec<ZElement>,
    doorcount: i32,
    ztotal: i32,
    speed: i32,
    zelements_per_door: i32,
    zelement_distance: i32,
    spectator_zelement: i32,
    projnorm_z: i32,
    rotation_duration: i32,
    rotation_step: i32,
    starcount: i32,
    current_rotation: Angle3D,
    rotation_delta: Angle3D,
    begincolor: ColorRGB,
    endcolor: ColorRGB,
    colorcount: i32,
    colorsteps: i32,
    delay_us: u64,
    sintable: Vec<f32>,
}

impl Scooter {
    fn sin(&self, a: i32) -> f32 {
        self.sintable[(a & SINUSTABLE_MASK) as usize]
    }

    fn cos(&self, a: i32) -> f32 {
        self.sintable[((a + (SINUSTABLE_SIZE as i32 / 4)) & SINUSTABLE_MASK) as usize]
    }

    fn nextdoorcolor(&mut self, door_index: usize, rng: &mut impl Rng) {
        if self.colorcount >= self.colorsteps {
            self.colorcount = 0;
            self.colorsteps = 8 + rng.random_range(0..32);
            self.begincolor = self.endcolor;
            self.endcolor = randomcolor(rng);
        }

        let r = self.begincolor.r + (self.endcolor.r - self.begincolor.r) * self.colorcount / self.colorsteps;
        let g = self.begincolor.g + (self.endcolor.g - self.begincolor.g) * self.colorcount / self.colorsteps;
        let b = self.begincolor.b + (self.endcolor.b - self.begincolor.b) * self.colorcount / self.colorsteps;

        self.colorcount += 1;

        self.doors[door_index].color = Color::new(255, (r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8);
    }

    fn projection(&self, zval: i32) -> f32 {
        self.projnorm_z as f32 / (PROJECTION_DEGREE * zval as f32)
    }

    fn rotate_3d(&self, src: &Vec3D, dest: &mut Vec3D, angle: &Angle3D) {
        let cosa = self.cos(angle.x);
        let cosb = self.cos(angle.y);
        let cosc = self.cos(angle.z);
        let sina = self.sin(angle.x);
        let sinb = self.sin(angle.y);
        let sinc = self.sin(angle.z);

        let tmp_z = src.z;
        let tmp_y = src.y;
        dest.z = (tmp_z as f32 * cosa - tmp_y as f32 * sina) as i32;
        dest.y = (tmp_z as f32 * sina + tmp_y as f32 * cosa) as i32;

        let tmp_z2 = dest.z;
        let tmp_x = src.x;
        dest.z = (tmp_z2 as f32 * cosb - tmp_x as f32 * sinb) as i32;
        dest.x = (tmp_z2 as f32 * sinb + tmp_x as f32 * cosb) as i32;

        let tmp_x2 = dest.x;
        let tmp_y2 = dest.y;
        dest.x = (tmp_x2 as f32 * cosc - tmp_y2 as f32 * sinc) as i32;
        dest.y = (tmp_x2 as f32 * sinc + tmp_y2 as f32 * cosc) as i32;
    }

    fn calc_new_element(&mut self, rng: &mut impl Rng) {
        let rot = self.sin((SINUSTABLE_SIZE as i32 / 2) * self.rotation_step / self.rotation_duration);

        let old_step = self.rotation_step;
        self.rotation_step += 1;

        if old_step >= self.rotation_duration {
            // max(1) guards the division: the player always resolves delay_us
            // to a nonzero mode default, but a 0 here would abort the locker.
            let fps = (1_000_000 / self.delay_us.max(1)) as i32;
            let fps_val = if fps == 0 { 1 } else { fps };
            self.rotation_duration = 10 * fps_val + rng.random_range(0..(20 * fps_val).max(1));

            self.rotation_delta.x = rng.random_range(0..(DOOR_CURVEDNESS * 2 + 1)) - DOOR_CURVEDNESS;
            self.rotation_delta.y = rng.random_range(0..(DOOR_CURVEDNESS * 2 + 1)) - DOOR_CURVEDNESS;
            self.rotation_delta.z = rng.random_range(0..(DOOR_CURVEDNESS * 2 + 1)) - DOOR_CURVEDNESS;

            self.rotation_step = 0;
        }

        self.current_rotation.x += (rot * self.rotation_delta.x as f32) as i32;
        self.current_rotation.y += (rot * self.rotation_delta.y as f32) as i32;
        self.current_rotation.z += (rot * self.rotation_delta.z as f32) as i32;

        self.current_rotation.x &= SINUSTABLE_MASK;
        self.current_rotation.y &= SINUSTABLE_MASK;
        self.current_rotation.z &= SINUSTABLE_MASK;
    }

    fn shift_elements(&mut self, rng: &mut impl Rng) {
        for i in self.speed..self.ztotal {
            self.zelements[(i - self.speed) as usize].angle = self.zelements[i as usize].angle;
        }
        for i in (self.ztotal - self.speed)..self.ztotal {
            self.calc_new_element(rng);
            self.zelements[i as usize].angle = self.current_rotation;
        }

        self.zelements[self.spectator_zelement as usize].pos.x = 0;
        self.zelements[self.spectator_zelement as usize].pos.y = 0;
        self.zelements[self.spectator_zelement as usize].pos.z = self.zelement_distance * self.spectator_zelement;

        for i in (0..self.spectator_zelement).rev() {
            let iprev = i + 1;
            let tmpvec = Vec3D { x: 0, y: 0, z: -self.zelement_distance };
            let tmpangle = Angle3D {
                x: self.zelements[i as usize].angle.x - self.zelements[self.spectator_zelement as usize].angle.x,
                y: self.zelements[i as usize].angle.y - self.zelements[self.spectator_zelement as usize].angle.y,
                z: self.zelements[i as usize].angle.z - self.zelements[self.spectator_zelement as usize].angle.z,
            };
            let mut new_pos = self.zelements[i as usize].pos;
            self.rotate_3d(&tmpvec, &mut new_pos, &tmpangle);
            self.zelements[i as usize].pos = new_pos;
            self.zelements[i as usize].pos.x += self.zelements[iprev as usize].pos.x;
            self.zelements[i as usize].pos.y += self.zelements[iprev as usize].pos.y;
            self.zelements[i as usize].pos.z += self.zelements[iprev as usize].pos.z;
        }

        for i in (self.spectator_zelement + 1)..self.ztotal {
            let iprev = i - 1;
            let tmpvec = Vec3D { x: 0, y: 0, z: self.zelement_distance };
            let tmpangle = Angle3D {
                x: self.zelements[i as usize].angle.x - self.zelements[self.spectator_zelement as usize].angle.x,
                y: self.zelements[i as usize].angle.y - self.zelements[self.spectator_zelement as usize].angle.y,
                z: self.zelements[i as usize].angle.z - self.zelements[self.spectator_zelement as usize].angle.z,
            };
            let mut new_pos = self.zelements[i as usize].pos;
            self.rotate_3d(&tmpvec, &mut new_pos, &tmpangle);
            self.zelements[i as usize].pos = new_pos;
            self.zelements[i as usize].pos.x += self.zelements[iprev as usize].pos.x;
            self.zelements[i as usize].pos.y += self.zelements[iprev as usize].pos.y;
            self.zelements[i as usize].pos.z += self.zelements[iprev as usize].pos.z;
        }

        for i in 0..self.doorcount {
            self.doors[i as usize].zelement -= self.speed;
            if self.doors[i as usize].zelement < 0 {
                self.doors[i as usize].zelement += self.ztotal;
                self.nextdoorcolor(i as usize, rng);
            }
        }

        for i in 0..self.starcount {
            self.stars[i as usize].zelement -= self.speed;
            if self.stars[i as usize].zelement < 0 {
                self.stars[i as usize].zelement += self.ztotal;
                self.stars[i as usize].draw = 1;

                let rnd_x = rng.random_range(0..(2 * (STAR_MAX_X - STAR_MIN_X))) - (STAR_MAX_X - STAR_MIN_X);
                self.stars[i as usize].xpos = rnd_x + STAR_MIN_X * sgn(rnd_x);

                let rnd_y = rng.random_range(0..(2 * (STAR_MAX_Y - STAR_MIN_Y))) - (STAR_MAX_Y - STAR_MIN_Y);
                self.stars[i as usize].ypos = rnd_y + STAR_MIN_Y * sgn(rnd_y);

                let rnd_size = rng.random_range(0..(STAR_SIZE_MAX - STAR_SIZE_MIN)) + STAR_SIZE_MIN;
                self.stars[i as usize].width = rnd_size;
                self.stars[i as usize].height = rnd_size * 3 / 4;
            }
        }
    }

    fn get_door_coords(&self, door_index: usize) -> [Vec3D; 4] {
        let ze = &self.zelements[self.doors[door_index].zelement as usize];
        let mut coords = [Vec3D { x: 0, y: 0, z: 0 }; 4];
        let tmpangle = Angle3D {
            x: ze.angle.x - self.zelements[self.spectator_zelement as usize].angle.x,
            y: ze.angle.y - self.zelements[self.spectator_zelement as usize].angle.y,
            z: ze.angle.z - self.zelements[self.spectator_zelement as usize].angle.z,
        };

        let mut src = Vec3D { x: -DOOR_WIDTH / 2, y: DOOR_HEIGHT / 2, z: 0 };
        self.rotate_3d(&src, &mut coords[0], &tmpangle);
        coords[0].x += ze.pos.x;
        coords[0].y += ze.pos.y;
        coords[0].z += ze.pos.z;

        src.x = DOOR_WIDTH / 2;
        self.rotate_3d(&src, &mut coords[1], &tmpangle);
        coords[1].x += ze.pos.x;
        coords[1].y += ze.pos.y;
        coords[1].z += ze.pos.z;

        src.y = -DOOR_HEIGHT / 2;
        self.rotate_3d(&src, &mut coords[2], &tmpangle);
        coords[2].x += ze.pos.x;
        coords[2].y += ze.pos.y;
        coords[2].z += ze.pos.z;

        src.x = -DOOR_WIDTH / 2;
        self.rotate_3d(&src, &mut coords[3], &tmpangle);
        coords[3].x += ze.pos.x;
        coords[3].y += ze.pos.y;
        coords[3].z += ze.pos.z;

        coords
    }

    fn drawstars(&self, buffer: &mut [u8], width: u32, height: u32, aspect_scale: f32) {
        let midx = (width / 2) as i32;
        let midy = (height / 2) as i32;

        for i in 0..self.starcount {
            if self.stars[i as usize].draw == 0 {
                continue;
            }

            let ze = &self.zelements[self.stars[i as usize].zelement as usize];
            let mut coords = Vec3D { x: 0, y: 0, z: 0 };
            let tmpangle = Angle3D {
                x: ze.angle.x - self.zelements[self.spectator_zelement as usize].angle.x,
                y: ze.angle.y - self.zelements[self.spectator_zelement as usize].angle.y,
                z: ze.angle.z - self.zelements[self.spectator_zelement as usize].angle.z,
            };

            let tmpvec = Vec3D {
                x: self.stars[i as usize].xpos,
                y: self.stars[i as usize].ypos,
                z: 0,
            };

            self.rotate_3d(&tmpvec, &mut coords, &tmpangle);
            coords.x += ze.pos.x;
            coords.y += ze.pos.y;
            coords.z += ze.pos.z;

            if coords.z <= 0 {
                continue;
            }

            let proj = self.projection(coords.z) * aspect_scale;

            let mut lefttop_x = midx + ((coords.x - self.stars[i as usize].width / 2) as f32 * proj / SPACE_XY_FACTOR as f32).round() as i32;
            let mut lefttop_y = midy - ((coords.y + self.stars[i as usize].height / 2) as f32 * proj / SPACE_XY_FACTOR as f32).round() as i32;

            if lefttop_x < 0 {
                lefttop_x = 0;
            } else if lefttop_x >= width as i32 {
                continue;
            }

            if lefttop_y < 0 {
                lefttop_y = 0;
            } else if lefttop_y >= height as i32 {
                continue;
            }

            let mut rightbottom_x = midx + ((coords.x + self.stars[i as usize].width / 2) as f32 * proj / SPACE_XY_FACTOR as f32).round() as i32;
            let mut rightbottom_y = midy - ((coords.y - self.stars[i as usize].height / 2) as f32 * proj / SPACE_XY_FACTOR as f32).round() as i32;

            if rightbottom_x < 0 {
                continue;
            } else if rightbottom_x >= width as i32 {
                rightbottom_x = width as i32 - 1;
            }

            if rightbottom_y < 0 {
                continue;
            } else if rightbottom_y >= height as i32 {
                rightbottom_y = height as i32 - 1;
            }

            let color = Color::new(255, 255, 255, 255);

            if lefttop_x == rightbottom_x && lefttop_y == rightbottom_y {
                put_pixel(buffer, width, height, lefttop_x, lefttop_y, color);
            } else if (rightbottom_x - lefttop_x) + (rightbottom_y - lefttop_y) == 1 {
                put_pixel(buffer, width, height, lefttop_x, lefttop_y, color);
                put_pixel(buffer, width, height, rightbottom_x, rightbottom_y, color);
            } else if (rightbottom_x - lefttop_x == 1) && (rightbottom_y - lefttop_y == 1) {
                put_pixel(buffer, width, height, lefttop_x, lefttop_y, color);
                put_pixel(buffer, width, height, rightbottom_x, lefttop_y, color);
                put_pixel(buffer, width, height, lefttop_x, rightbottom_y, color);
                put_pixel(buffer, width, height, rightbottom_x, rightbottom_y, color);
            } else {
                for yy in lefttop_y..=rightbottom_y {
                    for xx in lefttop_x..=rightbottom_x {
                        put_pixel(buffer, width, height, xx, yy, color);
                    }
                }
            }
        }
    }

    fn drawdoors(&self, buffer: &mut [u8], width: u32, height: u32, aspect_scale: f32) {
        let midx = (width / 2) as i32;
        let midy = (height / 2) as i32;

        let rect = Rect {
            lefttop: (0, 0),
            rightbottom: (width as i32 - 1, height as i32 - 1),
        };

        for i in 0..self.doorcount {
            let coords = self.get_door_coords(i as usize);

            let door = &self.doors[i as usize];
            let mut lines = [(0, 0); 4];
            let mut j = 0;

            while j < 4 {
                if coords[j].z <= 0 {
                    break;
                }
                let proj = self.projection(coords[j].z) * aspect_scale;
                lines[j].0 = midx + (coords[j].x as f32 * proj / SPACE_XY_FACTOR as f32).round() as i32;
                lines[j].1 = midy - (coords[j].y as f32 * proj / SPACE_XY_FACTOR as f32).round() as i32;
                j += 1;
            }

            if j < 4 {
                continue;
            }

            let color = door.color;

            for j in 0..4 {
                let mut clip1 = lines[j];
                let mut clip2 = lines[(j + 1) % 4];
                if clipline(&mut clip1, &mut clip2, &rect) {
                    draw_line(buffer, width, height, clip1.0, clip1.1, clip2.0, clip2.1, color);
                }
            }
        }
    }
}

impl Animation for Scooter {
    fn new(config: &AnimConfig) -> Self {
        let mut sintable = vec![0.0; SINUSTABLE_SIZE];
        for i in 0..SINUSTABLE_SIZE {
            sintable[i] = ((std::f32::consts::PI * 2.0 / SINUSTABLE_SIZE as f32) * i as f32).sin();
        }

        let mut s = Scooter {
            stars: Vec::new(),
            doors: Vec::new(),
            zelements: Vec::new(),
            doorcount: 0,
            ztotal: 0,
            speed: 0,
            zelements_per_door: 0,
            zelement_distance: 0,
            spectator_zelement: 0,
            projnorm_z: 0,
            rotation_duration: 0,
            rotation_step: 0,
            starcount: 0,
            current_rotation: Angle3D { x: 0, y: 0, z: 0 },
            rotation_delta: Angle3D { x: 0, y: 0, z: 0 },
            begincolor: ColorRGB { r: 0, g: 0, b: 0 },
            endcolor: ColorRGB { r: 0, g: 0, b: 0 },
            colorcount: 0,
            colorsteps: 0,
            delay_us: config.delay_us,
            sintable,
        };

        s.reset(config);
        s
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();
        self.shift_elements(&mut rng);
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        let aspect_scale = if (width as f32 / height as f32) >= (ASPECT_SCREENWIDTH as f32 / ASPECT_SCREENHEIGHT as f32) {
            height as f32 / ASPECT_SCREENHEIGHT as f32
        } else {
            width as f32 / ASPECT_SCREENWIDTH as f32
        };

        self.drawstars(buffer, width, height, aspect_scale);
        self.drawdoors(buffer, width, height, aspect_scale);
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();

        self.delay_us = config.delay_us;

        let count = if config.count <= 0 { 24 } else { config.count };
        self.doorcount = count.max(MIN_DOORS);

        let speed = if config.cycles <= 0 { 3 } else { config.cycles };
        self.speed = speed.clamp(MIN_SPEED, MAX_SPEED);

        let mut starcount = if config.size <= 0 { 100 } else { config.size };
        if starcount < 1 {
            starcount = 1;
        }

        self.zelements_per_door = 60;
        self.zelement_distance = 300;
        self.ztotal = self.doorcount * self.zelements_per_door;

        if starcount > self.ztotal {
            starcount = self.ztotal;
        }
        self.starcount = starcount;

        self.doors.resize(self.doorcount as usize, Door {
            zelement: 0,
            color: Color::new(255, 255, 255, 255),
        });

        self.zelements.resize(self.ztotal as usize, ZElement {
            pos: Vec3D { x: 0, y: 0, z: 0 },
            angle: Angle3D { x: 0, y: 0, z: 0 },
        });

        self.stars.resize(self.starcount as usize, Star {
            xpos: 0,
            ypos: 0,
            width: 0,
            height: 0,
            zelement: 0,
            draw: 0,
        });

        self.endcolor = randomcolor(&mut rng);
        self.colorcount = 0;
        self.colorsteps = 0;

        for i in 0..self.doorcount {
            self.doors[i as usize].zelement = (self.zelements_per_door * (i + 1)) - 1;
            self.nextdoorcolor(i as usize, &mut rng);
        }

        for i in 0..self.ztotal {
            self.zelements[i as usize].angle.x = 0;
            self.zelements[i as usize].angle.y = 0;
            self.zelements[i as usize].angle.z = 0;
        }

        for i in 0..self.starcount {
            self.stars[i as usize].zelement = self.ztotal * i / self.starcount;
            self.stars[i as usize].draw = 0;
        }

        self.projnorm_z = 50 * 240;
        self.spectator_zelement = self.zelements_per_door;

        self.current_rotation.x = 0;
        self.current_rotation.y = 0;
        self.current_rotation.z = 0;

        self.rotation_delta.x = 0;
        self.rotation_delta.y = 0;
        self.rotation_delta.z = 0;

        self.rotation_duration = 1;
        self.rotation_step = 0;
    }

    fn clears_each_frame(&self) -> bool {
        true
    }

    fn frame_delay_us(&self) -> u64 {
        20_000 // match xlockmore default
    }
}
