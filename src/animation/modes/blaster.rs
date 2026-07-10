// blaster, Copyright (c) 1999 Jonathan H. Lin <jonlin@tesuji.org>
//
// Permission to use, copy, modify, distribute, and sell this software and its
// documentation for any purpose is hereby granted without fee, provided that
// the above copyright notice appear in all copies and that both that
// copyright notice and this permission notice appear in supporting
// documentation.  No representations are made about the suitability of this
// software for any purpose.  It is provided "as is" without express or
// implied warranty.
//
//  Robots that move randomly and shoot lasers at each other. If the
//  mothership is active, it will fly back and forth horizontally,
//  firing 8 lasers in the 8 cardinal directions. The explosions are
//  a 20 frame animation. Robots regenerate after the explosion is finished
//  and all of its lasers have left the screen.
//
// Rust port of xscreensaver's blaster.c. The mothership and random star
// movement are disabled by default in the original (*mother_ship: false,
// *move_stars_random: 0), so those dead code paths are not ported.

use crate::animation::primitives::{clear_buffer, draw_line, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};
use rand::Rng;

const BLACK: Color = Color { a: 255, r: 0, g: 0, b: 0 };
// *r_color0..5 defaults
const R_COLORS: [Color; 6] = [
    Color { a: 255, r: 0xFF, g: 0x00, b: 0xFF },
    Color { a: 255, r: 0xFF, g: 0xA5, b: 0x00 },
    Color { a: 255, r: 0xFF, g: 0xFF, b: 0x00 },
    Color { a: 255, r: 0xFF, g: 0xFF, b: 0xFF },
    Color { a: 255, r: 0x00, g: 0x00, b: 0xFF },
    Color { a: 255, r: 0x00, g: 0xFF, b: 0xFF },
];
// *l_color0/1 defaults
const L_COLORS: [Color; 2] = [
    Color { a: 255, r: 0x00, g: 0xFF, b: 0x00 },
    Color { a: 255, r: 0xFF, g: 0x00, b: 0x00 },
];
const EXPLODE_COLOR_1: Color = Color { a: 255, r: 0xFF, g: 0xFF, b: 0x00 };
const EXPLODE_COLOR_2: Color = Color { a: 255, r: 0xFF, g: 0xA5, b: 0x00 };
const STAR_COLOR: Color = Color { a: 255, r: 0xFF, g: 0xFF, b: 0xFF };

// XFillArc equivalent: fill the ellipse inscribed in the bounding box.
fn fill_ellipse(buf: &mut [u8], width: u32, height: u32, x: i32, y: i32, ew: i32, eh: i32, color: Color) {
    if ew <= 0 || eh <= 0 {
        return;
    }
    let rx = ew as f32 / 2.0;
    let ry = eh as f32 / 2.0;
    let cx = x as f32 + rx;
    let cy = y as f32 + ry;
    for py in y..y + eh {
        for px in x..x + ew {
            let dx = (px as f32 + 0.5 - cx) / rx;
            let dy = (py as f32 + 0.5 - cy) / ry;
            if dx * dx + dy * dy <= 1.0 {
                put_pixel(buf, width, height, px, py, color);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Laser {
    active: bool,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
}

const LASER_OFF: Laser = Laser { active: false, start_x: 0, start_y: 0, end_x: 0, end_y: 0 };

struct Robot {
    alive: bool,
    death: i32,
    move_style: i32,
    target: usize,
    old_x: i32,
    old_y: i32,
    new_x: i32,
    new_y: i32,
    radius: i32,
    robot_color: Color,
    laser_color: Color,
    lasers: Vec<Laser>,
}

#[derive(Clone, Copy)]
struct Star {
    x: i32,
    y: i32,
    d: i32,
}

pub struct Blaster {
    width: i32,
    height: i32,
    scale: i32,
    num_robots: usize,
    num_lasers: usize,
    explode_size_1: i32,
    explode_size_2: i32,
    explode_size_3: i32,
    num_stars: usize,
    move_stars_x: i32,
    move_stars_y: i32,
    stars: Vec<Star>,
    robots: Vec<Robot>,
    canvas: Vec<u8>,
    delay_us: u64,
}

impl Blaster {
    fn arc(&mut self, x: i32, y: i32, ew: i32, eh: i32, color: Color) {
        fill_ellipse(&mut self.canvas, self.width as u32, self.height as u32, x, y, ew, eh, color);
    }

    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        draw_line(&mut self.canvas, self.width as u32, self.height as u32, x0, y0, x1, y1, color);
    }

    // Creates a new robot on one of the edges with no initial velocity and a
    // random target. Only respawns once all of its lasers have left the screen.
    fn make_new_robot(&mut self, rng: &mut impl Rng, index: usize) {
        if self.robots[index].lasers.iter().any(|l| l.active) {
            return;
        }
        let r = &mut self.robots[index];
        r.alive = true;
        r.radius = (7 + rng.random_range(0..7)) * self.scale;
        r.move_style = rng.random_range(0..2);
        if rng.random_range(0..2) == 0 {
            r.new_x = rng.random_range(0..(self.width - r.radius).max(1));
            r.old_x = r.new_x;
            if rng.random_range(0..2) == 0 {
                r.new_y = 0;
                r.old_y = 0;
            } else {
                r.new_y = self.height - r.radius;
                r.old_y = r.new_y;
            }
        } else {
            r.new_y = rng.random_range(0..(self.height - r.radius).max(1));
            r.old_y = r.new_y;
            if rng.random_range(0..2) != 0 {
                r.new_x = 0;
                r.old_x = 0;
            } else {
                r.new_x = self.width - r.radius;
                r.old_x = r.new_x;
            }
        }
        r.robot_color = R_COLORS[rng.random_range(0..6)];
        r.laser_color = L_COLORS[rng.random_range(0..2)];
        if self.num_robots > 1 {
            let mut target = rng.random_range(0..self.num_robots);
            while target == index {
                target = rng.random_range(0..self.num_robots);
            }
            self.robots[index].target = target;
        }
    }

    // Moves each robot, randomly changing its direction and velocity.
    // At random a laser is shot toward that robot's target, and at random
    // the target can change.
    fn move_robots(&mut self, rng: &mut impl Rng) {
        for x in 0..self.num_robots {
            if !self.robots[x].alive {
                if self.robots[x].death == 0 {
                    self.make_new_robot(rng, x);
                }
                continue;
            }

            if self.robots[x].new_x == self.robots[x].old_x && self.robots[x].new_y == self.robots[x].old_y {
                if self.robots[x].new_x == 0 {
                    self.robots[x].old_x = -(rng.random_range(0..3) + 1) * self.scale;
                } else {
                    self.robots[x].old_x += (rng.random_range(0..3) + 1) * self.scale;
                }
                if self.robots[x].new_y == 0 {
                    self.robots[x].old_y = -(rng.random_range(0..3) + 1) * self.scale;
                } else {
                    self.robots[x].old_y += (rng.random_range(0..3) + 1) * self.scale;
                }
            }

            if self.robots[x].move_style == 0 {
                // LINE_MOVE_STYLE
                let mut dx = self.robots[x].new_x - self.robots[x].old_x;
                let mut dy = self.robots[x].new_y - self.robots[x].old_y;
                dx = dx.clamp(-3, 3);
                dy = dy.clamp(-3, 3);
                self.robots[x].old_x = self.robots[x].new_x;
                self.robots[x].old_y = self.robots[x].new_y;
                self.robots[x].new_x += dx * self.scale;
                self.robots[x].new_y += dy * self.scale;
            } else {
                // RANDOM_MOVE_STYLE
                let mut dx = self.robots[x].new_x - self.robots[x].old_x;
                let mut dy = self.robots[x].new_y - self.robots[x].old_y;
                let y = rng.random_range(0..3);
                if y == 0 {
                    dx -= rng.random_range(0..7) + 1;
                } else if y == 1 {
                    dx += rng.random_range(0..7) + 1;
                } else {
                    dx = -dx;
                }
                dx = dx.clamp(-3, 3);
                let y = rng.random_range(0..3);
                if y == 0 {
                    dy -= (rng.random_range(0..7) + 1) * self.scale;
                } else if y == 1 {
                    dy += (rng.random_range(0..7) + 1) * self.scale;
                } else {
                    dx = -dx; // sic: the C flips dx here, not dy
                }
                dy = dy.clamp(-3, 3);
                self.robots[x].old_x = self.robots[x].new_x;
                self.robots[x].old_y = self.robots[x].new_y;
                self.robots[x].new_x += dx * self.scale;
                self.robots[x].new_y += dy * self.scale;
            }

            // bounds corrections
            let radius = self.robots[x].radius;
            if self.robots[x].new_x >= self.width - radius {
                self.robots[x].new_x = self.width - radius;
            } else if self.robots[x].new_x < 0 {
                self.robots[x].new_x = 0;
            }
            if self.robots[x].new_y >= self.height - radius {
                self.robots[x].new_y = self.height - radius;
            } else if self.robots[x].new_y < 0 {
                self.robots[x].new_y = 0;
            }

            self.robots[x].move_style = if rng.random_range(0..10) == 0 { 1 } else { 0 };

            if self.num_robots > 1 && rng.random_range(0..2) == 0 {
                let s7 = 7 * self.scale;
                if rng.random_range(0..200) == 0 {
                    // retarget and fire a random diagonal laser
                    let mut target = rng.random_range(0..self.num_robots);
                    while target == x {
                        target = rng.random_range(0..self.num_robots);
                    }
                    self.robots[x].target = target;
                    let (nx, ny, r) = (self.robots[x].new_x, self.robots[x].new_y, self.robots[x].radius);
                    for y in 0..self.num_lasers {
                        if self.robots[x].lasers[y].active {
                            continue;
                        }
                        let l = &mut self.robots[x].lasers[y];
                        l.active = true;
                        if rng.random_range(0..2) == 0 {
                            if rng.random_range(0..2) == 0 {
                                l.start_x = nx + r;
                                l.start_y = ny + r;
                                l.end_x = l.start_x + s7;
                                l.end_y = l.start_y + s7;
                            } else {
                                l.start_x = nx - r;
                                l.start_y = ny + r;
                                l.end_x = l.start_x - s7;
                                l.end_y = l.start_y + s7;
                            }
                        } else if rng.random_range(0..2) == 0 {
                            l.start_x = nx - r;
                            l.start_y = ny - r;
                            l.end_x = l.start_x - s7;
                            l.end_y = l.start_y - s7;
                        } else {
                            l.start_x = nx + r;
                            l.start_y = ny - r;
                            l.end_x = l.start_x + s7;
                            l.end_y = l.start_y - s7;
                        }
                        break;
                    }
                } else {
                    // fire a laser aimed at the current target
                    let target_x = self.robots[self.robots[x].target].new_x;
                    let target_y = self.robots[self.robots[x].target].new_y;
                    let (nx, ny, r) = (self.robots[x].new_x, self.robots[x].new_y, self.robots[x].radius);
                    for y in 0..self.num_lasers {
                        if self.robots[x].lasers[y].active {
                            continue;
                        }
                        let mut l = LASER_OFF;
                        if target_x - nx != 0 {
                            let slope = (target_y - ny) as f64 / (target_x - nx) as f64;
                            if slope < 1.0 && slope > -1.0 {
                                if target_x > nx {
                                    l.start_x = r;
                                    l.end_x = l.start_x + s7;
                                } else {
                                    l.start_x = -r;
                                    l.end_x = l.start_x - s7;
                                }
                                l.start_y = (l.start_x as f64 * slope) as i32;
                                l.end_y = (l.end_x as f64 * slope) as i32;
                            } else {
                                // sic: integer division, as in the C
                                let slope = ((target_x - nx) / (target_y - ny)) as f64;
                                if target_y > ny {
                                    l.start_y = r;
                                    l.end_y = l.start_y + s7;
                                } else {
                                    l.start_y = -r;
                                    l.end_y = l.start_y - s7;
                                }
                                l.start_x = (l.start_y as f64 * slope) as i32;
                                l.end_x = (l.end_y as f64 * slope) as i32;
                            }
                            l.start_x += nx;
                            l.start_y += ny;
                            l.end_x += nx;
                            l.end_y += ny;
                        } else if target_y > ny {
                            l.start_x = nx;
                            l.start_y = ny + r;
                            l.end_x = nx;
                            l.end_y = l.start_y + s7;
                        } else {
                            l.start_x = nx;
                            l.start_y = ny - r;
                            l.end_x = nx;
                            l.end_y = l.start_y - s7;
                        }

                        let big_x = l.start_x - l.end_x > s7 || l.end_x - l.start_x > s7;
                        let big_y = l.start_y - l.end_y > s7 || l.end_y - l.start_y > s7;
                        if big_x && big_y {
                            // degenerate laser: leave it inactive (C keeps looping,
                            // recomputing the same result for each remaining slot)
                            self.robots[x].lasers[y] = l;
                        } else {
                            l.active = true;
                            self.robots[x].lasers[y] = l;
                            break;
                        }
                    }
                }
            }
        }
    }

    // Moves a single laser one frame, checking for collisions with other robots.
    fn move_laser(&mut self, rindex: usize, lindex: usize) {
        let mut laser = self.robots[rindex].lasers[lindex];
        if !laser.active {
            return;
        }
        for x in 0..self.num_robots {
            if x == rindex || !self.robots[x].alive {
                continue;
            }
            let (rx, ry, rr) = (self.robots[x].new_x, self.robots[x].new_y, self.robots[x].radius);
            let start_hit = (laser.start_y - ry).abs() < rr - 1 && (laser.start_x - rx).abs() < rr - 1;
            let end_hit = (laser.end_y - ry).abs() < rr - 1 && (laser.end_x - rx).abs() < rr - 1;
            if start_hit || end_hit {
                self.robots[x].alive = false;
                self.robots[x].death = 20;
                let (ox, oy) = (self.robots[x].old_x, self.robots[x].old_y);
                self.arc(ox, oy, rr, rr, BLACK);
                self.arc(rx, ry, rr, rr, BLACK);
                laser.active = false;
                break;
            }
        }
        if laser.active {
            let dx = laser.start_x - laser.end_x;
            let dy = laser.start_y - laser.end_y;
            laser.start_x = laser.end_x;
            laser.start_y = laser.end_y;
            laser.end_x -= dx;
            laser.end_y -= dy;
            if laser.end_x < 0 || laser.end_x >= self.width || laser.end_y < 0 || laser.end_y >= self.height {
                laser.active = false;
            }
        }
        self.robots[rindex].lasers[lindex] = laser;
    }

    // Draws all robots, explosions and active lasers.
    fn draw_robots(&mut self) {
        for x in 0..self.num_robots {
            let r = &self.robots[x];
            let (alive, death, ox, oy, nx, ny, radius, color) =
                (r.alive, r.death, r.old_x, r.old_y, r.new_x, r.new_y, r.radius, r.robot_color);
            if alive {
                self.arc(ox, oy, radius, radius, BLACK);
                self.arc(nx, ny, radius, radius, color);
            } else {
                self.arc(ox, oy, radius, radius, BLACK);
                if death > 0 {
                    let ex = nx + radius / 3;
                    let ey = ny + radius / 3;
                    let ex17 = (nx as f64 + 1.7 * radius as f64 / 2.0) as i32;
                    let ey17 = (ny as f64 + 1.7 * radius as f64 / 2.0) as i32;
                    let (e1, e2, e3) = (self.explode_size_1, self.explode_size_2, self.explode_size_3);
                    match death {
                        20 | 17 | 12 | 8 => self.arc(ex, ey, e1, e1, EXPLODE_COLOR_1),
                        18 | 14 | 10 => self.arc(ex, ey, e1, e1, EXPLODE_COLOR_2),
                        15 | 13 | 11 | 9 | 7 => self.arc(ex, ey, e1, e1, BLACK),
                        6 => self.arc(ex, ey, e2, e2, EXPLODE_COLOR_2),
                        4 => self.arc(ex, ey, e2, e2, BLACK),
                        3 => self.arc(ex, ey, e2, e2, EXPLODE_COLOR_1),
                        2 => {
                            self.arc(ex, ey, e2, e2, BLACK);
                            self.arc(ex17, ey17, e3, e3, EXPLODE_COLOR_2);
                        }
                        1 => self.arc(ex17, ey17, e3, e3, BLACK),
                        _ => {}
                    }
                    self.robots[x].death -= 1;
                }
            }
        }

        for x in 0..self.num_robots {
            for y in 0..self.num_lasers {
                if !self.robots[x].lasers[y].active {
                    continue;
                }
                let l = self.robots[x].lasers[y];
                self.line(l.start_x, l.start_y, l.end_x, l.end_y, BLACK);
                self.move_laser(x, y);
                let l = self.robots[x].lasers[y];
                if l.active {
                    let c = self.robots[x].laser_color;
                    self.line(l.start_x, l.start_y, l.end_x, l.end_y, c);
                } else {
                    self.line(l.start_x, l.start_y, l.end_x, l.end_y, BLACK);
                }
            }
        }
    }
}

impl Animation for Blaster {
    fn new(config: &AnimConfig) -> Self {
        let mut b = Blaster {
            width: config.width as i32,
            height: config.height as i32,
            scale: 1,
            num_robots: 5,
            num_lasers: 3,
            explode_size_1: 27,
            explode_size_2: 19,
            explode_size_3: 7,
            num_stars: 50,
            move_stars_x: 2,
            move_stars_y: 1,
            stars: Vec::new(),
            robots: Vec::new(),
            canvas: Vec::new(),
            delay_us: 10_000, // *delay: 10000
        };
        b.reset(config);
        b
    }

    fn tick(&mut self) {
        let mut rng = rand::rng();

        // star field: erase, move (*move_stars: true), redraw
        if self.num_stars > 0 {
            for i in 0..self.stars.len() {
                let s = self.stars[i];
                self.arc(s.x, s.y, s.d, s.d, BLACK);
            }
            for s in self.stars.iter_mut() {
                s.x += self.move_stars_x;
                s.y += self.move_stars_y;
                if s.x < 0 {
                    s.x += self.width;
                } else if s.x > self.width {
                    s.x -= self.width;
                }
                if s.y < 0 {
                    s.y += self.height;
                } else if s.y > self.height {
                    s.y -= self.height;
                }
            }
            for i in 0..self.stars.len() {
                let s = self.stars[i];
                self.arc(s.x, s.y, s.d, s.d, STAR_COLOR);
            }
        }

        self.move_robots(&mut rng);
        self.draw_robots();
    }

    fn render(&self, buffer: &mut [u8], _width: u32, _height: u32) {
        let len = self.canvas.len().min(buffer.len());
        buffer[..len].copy_from_slice(&self.canvas[..len]);
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();
        self.width = config.width as i32;
        self.height = config.height as i32;
        self.scale = if config.width > 2560 || config.height > 2560 { 3 } else { 1 };
        self.explode_size_1 = 27 * self.scale;
        self.explode_size_2 = 19 * self.scale;
        self.explode_size_3 = 7 * self.scale;

        self.stars = (0..self.num_stars)
            .map(|_| Star {
                x: rng.random_range(0..self.width.max(1)),
                y: rng.random_range(0..self.height.max(1)),
                d: (rng.random_range(0..4) + 1) * self.scale,
            })
            .collect();

        self.robots = (0..self.num_robots)
            .map(|_| Robot {
                alive: false,
                death: 0,
                move_style: 0,
                target: 0,
                old_x: 0,
                old_y: 0,
                new_x: 0,
                new_y: 0,
                radius: 0,
                robot_color: BLACK,
                laser_color: BLACK,
                lasers: vec![LASER_OFF; self.num_lasers],
            })
            .collect();

        self.canvas = vec![0u8; (config.width * config.height * 4) as usize];
        clear_buffer(&mut self.canvas, BLACK);
    }

    fn clears_each_frame(&self) -> bool {
        false
    }

    fn frame_delay_us(&self) -> u64 {
        self.delay_us
    }
}
