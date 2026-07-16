/* molecule, Copyright © 2001-2023 Jamie Zawinski <jwz@jwz.org>
 * Draws molecules, based on coordinates from PDB (Protein Data Base) files.
 *
 * Permission to use, copy, modify, distribute, and sell this software and its
 * documentation for any purpose is hereby granted without fee, provided that
 * the above copyright notice appear in all copies and that both that
 * copyright notice and this permission notice appear in supporting
 * documentation.  No representations are made about the suitability of this
 * software for any purpose.  It is provided "as is" without express or
 * implied warranty.
 *
 * Rust port of xscreensaver's GL hack hacks/glx/molecule.c (plus the rotator
 * from hacks/glx/rotator.c).  Since this renderer is a software rasterizer,
 * not OpenGL, some GL-only niceties are approximated or dropped:
 *
 *   - Spheres are drawn as Lambert-shaded filled discs (draw_filled_sphere)
 *     lit by the C's GL_LIGHT0 direction (1, 0.4, 0.9), not lit unit_sphere()
 *     tessellations.
 *   - "Smooth tube" bonds (SMOOTH_TUBE / tube.c) are drawn as solid capsules
 *     (a perspective-projected cylinder body with hemispherical end caps) with
 *     per-pixel Lambert shading.  tube() scales a radius-1 unit tube, so the C's
 *     0.07*strength (capped 0.3) is the tube *radius* in world units.  Bonds are
 *     shortened in 3D to just inside their endpoint atoms so their ends don't
 *     z-fight the sphere they connect to; a thick bond's inset is deepened to
 *     sqrt(atom_r^2 - bond_r^2) so its rounded cap stays inside a small atom
 *     instead of poking square corners past the silhouette (tube.c's capped GL
 *     cylinders are round; the earlier flat quad caps were not).
 *   - Atom/bond overlaps are resolved with a small per-frame f32 depth buffer
 *     over the molecule's screen bounding box (view-space distance, near
 *     surface bulging toward the eye for spheres, linear-along-axis plus the
 *     cross-section bulge for bonds) instead of the GL depth buffer.  This
 *     replaces the earlier painter's sort / projected-disc clip for the
 *     geometry, so junctions transition per-pixel with no "pop" and a bond
 *     crossing in front of a sphere correctly draws over it.  A back-to-front
 *     list is retained only so labels (drawn without depth) still occlude.
 *   - Projected screen coordinates are kept in f64 (not rounded to whole
 *     pixels) and both spheres and bonds are edge-antialiased with a coverage
 *     blend, so slow rotation moves the shapes smoothly instead of snapping
 *     their edges pixel-by-pixel (the GL relied on MSAA/GL_LINE_SMOOTH here).
 *   - Atom labels (do_labels) and the molecule title (do_titles) use a
 *     built-in 5x7 bitmap font instead of texfont; billboarding degenerates
 *     to "centre the label on the atom's projected position" and depth
 *     occlusion falls out of the back-to-front draw order.
 *   - do_wander is ported (rotator get_position sinoid) but off by default,
 *     matching DEF_WANDER "False".
 *   - Electron shells (do_shells), the bounding box (do_bbox), wireframe mode,
 *     the trackball and external -molecule PDB file loading are dropped: this
 *     port only has the AnimConfig knobs, so those non-default features have
 *     no way to be switched on and are omitted.
 *   - Molecule cycling keeps the C timing: a -timeout second dwell, then a
 *     zoom-out / swap / zoom-in transition (mode 0/1/2, speed 4).
 */

use crate::animation::primitives::{clear_buffer, put_pixel, Color};
use crate::animation::{AnimConfig, Animation};
use rand::Rng;
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::time::{SystemTime, UNIX_EPOCH};

const MOLECULES_PDB: &str = include_str!("molecules.pdb");

// Feature toggles.  The C hack exposes these as X resources; this port has no
// per-mode option plumbing, so they are compile-time constants set to the C
// defaults (DEF_LABELS/DEF_TITLES "True", DEF_WANDER "False").
const DO_LABELS: bool = true;
const DO_TITLES: bool = true;
const DO_ATOMS: bool = true;
const DO_BONDS: bool = true;
const DO_WANDER: bool = false;

// Above this bounding-box size (angstroms) the C hack hides atom labels
// (noLabelThreshold, default 150).  All builtins are far smaller, but keep it.
const NO_LABEL_THRESHOLD: f64 = 150.0;

#[derive(Clone)]
struct AtomData {
    /// Rescaled radius (size2 in the C) used for ball-and-stick rendering.
    size2: f64,
    /// Ball color.
    color: Color,
    /// Label text color.
    text_color: Color,
}

/// The traditional atom colors and sizes, copied verbatim from molecule.c's
/// `all_atom_data[]` (size2 is the ball-and-stick radius, used because we
/// always draw bonds; `size` from the table is only used when bonds are off).
fn all_atom_data(name: &str) -> AtomData {
    let c = |r, g, b| Color::new(255, r, g, b);
    match name {
        "H" => AtomData { size2: 0.40, color: c(0xFF, 0xFF, 0xFF), text_color: c(0, 0, 0) },
        "C" => AtomData { size2: 0.58, color: c(0x99, 0x99, 0x99), text_color: c(0xFF, 0xFF, 0xFF) },
        "CA" => AtomData { size2: 0.60, color: c(0x00, 0x00, 0xFF), text_color: c(0xAD, 0xD8, 0xE6) },
        "N" => AtomData { size2: 0.52, color: c(0x42, 0x8D, 0xC3), text_color: c(0xEE, 0x99, 0xFF) },
        "O" => AtomData { size2: 0.47, color: c(0xFF, 0x00, 0x00), text_color: c(0xFF, 0xB6, 0xC1) },
        "P" => AtomData { size2: 0.43, color: c(0x93, 0x70, 0xDB), text_color: c(0xDB, 0x70, 0x93) },
        "S" => AtomData { size2: 0.60, color: c(0x8B, 0x8B, 0x00), text_color: c(0xFF, 0xFF, 0x00) },
        "bond" => AtomData { size2: 0.0, color: c(0xB3, 0xB3, 0xB3), text_color: c(0xFF, 0xFF, 0x00) },
        // "*" is the catch-all entry, exactly as in the C table.
        _ => AtomData { size2: 0.47, color: c(0x00, 0x8B, 0x00), text_color: c(0x90, 0xEE, 0x90) },
    }
}

/// Port of `get_atom_data()`: strip non-alpha from both ends, then look up the
/// whole cleaned name case-insensitively.  Anything unrecognised falls through
/// to the "*" entry (green) -- e.g. "Cl", "Co" -- exactly as the C does.
fn get_atom_data(name: &str) -> AtomData {
    let clean: String = name
        .trim_matches(|c: char| !c.is_ascii_alphabetic())
        .to_string();
    for key in ["H", "C", "CA", "N", "O", "P", "S", "bond"] {
        if clean.eq_ignore_ascii_case(key) {
            return all_atom_data(key);
        }
    }
    all_atom_data("*")
}

#[derive(Clone)]
struct MoleculeAtom {
    id: i32,
    label: String,
    x: f64,
    y: f64,
    z: f64,
    data: AtomData,
}

#[derive(Clone)]
struct MoleculeBond {
    from: i32,
    to: i32,
    strength: i32,
}

#[derive(Clone)]
struct MoleculeDef {
    /// Title lines: description (from HEADER/COMPND) plus the chemical formula.
    title_lines: Vec<String>,
    atoms: Vec<MoleculeAtom>,
    bonds: Vec<MoleculeBond>,
    /// atom.id -> index into `atoms`, built once at parse time so render()'s
    /// CONECT bond lookups are O(1) instead of a linear scan per bond
    /// endpoint per frame. First occurrence wins, matching the old
    /// `.iter().find()` semantics for (in practice never occurring)
    /// duplicate ids.
    id_index: HashMap<i32, usize>,
}

/// Normalise an element symbol to title case ("CL" -> "Cl", "c" -> "C"),
/// mirroring the C parser's tolower() of everything after the first char.
fn normalize_element(raw: &str) -> String {
    let mut out = String::new();
    for (i, ch) in raw.chars().enumerate() {
        if i == 0 {
            out.extend(ch.to_uppercase());
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// The convention for a molecular formula: carbon first, hydrogen second, then
/// the other elements alphabetically.  Port of `cmp_atoms()` / the formula
/// half of `generate_molecule_formula()` (without the special-case table).
fn generate_formula(atoms: &[MoleculeAtom]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for a in atoms {
        // Leading alphabetic run of the label is the element symbol.
        let elem: String = a
            .label
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        if elem.is_empty() {
            continue;
        }
        if let Some(e) = counts.iter_mut().find(|(name, _)| *name == elem) {
            e.1 += 1;
        } else {
            counts.push((elem, 1));
        }
    }
    counts.sort_by(|a, b| {
        let rank = |s: &str| match s {
            "C" => 0,
            "H" => 1,
            _ => 2,
        };
        rank(&a.0).cmp(&rank(&b.0)).then_with(|| a.0.cmp(&b.0))
    });
    let mut s = String::new();
    for (elem, count) in counts {
        s.push_str(&elem);
        if count > 1 {
            s.push_str(&count.to_string());
        }
    }
    s
}

/// Split a description on ", " / "; " / ": " into stacked lines, matching
/// `insert_vertical_whitespace()`.
fn split_title(desc: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = desc.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len()
            && matches!(chars[i], ',' | ';' | ':')
            && chars[i + 1] == ' '
        {
            lines.push(std::mem::take(&mut cur).trim().to_string());
            i += 2;
        } else {
            cur.push(chars[i]);
            i += 1;
        }
    }
    let last = cur.trim().to_string();
    if !last.is_empty() {
        lines.push(last);
    }
    lines
}

fn parse_molecules() -> Vec<MoleculeDef> {
    let mut defs: Vec<MoleculeDef> = Vec::new();
    let mut atoms: Vec<MoleculeAtom> = Vec::new();
    let mut bonds: Vec<MoleculeBond> = Vec::new();
    let mut desc: String = String::new();

    let push = |defs: &mut Vec<MoleculeDef>,
                atoms: &mut Vec<MoleculeAtom>,
                bonds: &mut Vec<MoleculeBond>,
                desc: &mut String| {
        if !atoms.is_empty() {
            let mut title_lines = split_title(desc);
            let formula = generate_formula(atoms);
            if !formula.is_empty() {
                title_lines.push(formula);
            }
            let mut id_index = HashMap::new();
            for (i, a) in atoms.iter().enumerate() {
                id_index.entry(a.id).or_insert(i);
            }
            defs.push(MoleculeDef {
                title_lines,
                atoms: std::mem::take(atoms),
                bonds: std::mem::take(bonds),
                id_index,
            });
        }
        atoms.clear();
        bonds.clear();
        desc.clear();
    };

    for raw in MOLECULES_PDB.lines() {
        let line = raw.trim_end();
        // END terminates a molecule block.
        if line.starts_with("END") {
            push(&mut defs, &mut atoms, &mut bonds, &mut desc);
            continue;
        }

        if line.starts_with("HEADER") || line.starts_with("COMPND") {
            if desc.is_empty() && line.len() > 6 {
                desc = line[6..].trim().to_string();
            }
        } else if line.starts_with("ATOM") || line.starts_with("HETATM") {
            // PDB-style fixed-column format. C uses:
            //   ATOM: id from s+7, name from s+12..s+15, coords from s+32.
            //   HETATM: id from s+7, name from s+12..s+15, coords from s+30.
            let is_hetatm = line.starts_with("HETATM");
            let coord_start = if is_hetatm { 30 } else { 32 };
            if line.len() <= coord_start {
                continue;
            }
            // Element label: columns 12..15 (sometimes more for residue codes).
            let name_end = line.len().min(16);
            let name = line[12..name_end].trim().to_string();
            // ID: columns 6..11 (right-justified integer).
            let id_str = line[6..11.min(line.len())].trim();
            let coords: Vec<&str> = line[coord_start..].split_whitespace().collect();
            if coords.len() < 3 {
                continue;
            }
            let Ok(id) = id_str.parse::<i32>() else { continue };
            let Ok(x) = coords[0].parse::<f64>() else { continue };
            let Ok(y) = coords[1].parse::<f64>() else { continue };
            let Ok(z) = coords[2].parse::<f64>() else { continue };

            // Element label may be like "C", "CA", "OG1", "H", or "O   UNL".
            // The leading token is the element symbol; normalise its case.
            let element_label = normalize_element(
                name.split_whitespace().next().unwrap_or(&name),
            );
            let data = get_atom_data(&element_label);
            atoms.push(MoleculeAtom { id, label: element_label, x, y, z, data });
        } else if let Some(rest) = line.strip_prefix("CONECT") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            let Ok(from) = parts[0].parse::<i32>() else { continue };
            for to_str in &parts[1..] {
                let Ok(to) = to_str.parse::<i32>() else { continue };
                if to <= 0 {
                    continue;
                }
                let mut found = false;
                for b in bonds.iter_mut() {
                    if (b.from == from && b.to == to) || (b.from == to && b.to == from) {
                        b.strength += 1;
                        found = true;
                        break;
                    }
                }
                if !found {
                    bonds.push(MoleculeBond { from, to, strength: 1 });
                }
            }
        }
        // Everything else (TER, END, MASTER, AUTHOR, REMARK, ...) is ignored.
    }

    // Flush the last molecule (no trailing separator).
    push(&mut defs, &mut atoms, &mut bonds, &mut desc);

    defs
}

#[derive(Clone, Copy)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

fn rotate_x(v: Vec3, a: f64) -> Vec3 {
    let (s, c) = a.sin_cos();
    Vec3 { x: v.x, y: v.y * c - v.z * s, z: v.y * s + v.z * c }
}

fn rotate_y(v: Vec3, a: f64) -> Vec3 {
    let (s, c) = a.sin_cos();
    Vec3 { x: v.x * c + v.z * s, y: v.y, z: -v.x * s + v.z * c }
}

fn rotate_z(v: Vec3, a: f64) -> Vec3 {
    let (s, c) = a.sin_cos();
    Vec3 { x: v.x * c - v.y * s, y: v.x * s + v.y * c, z: v.z }
}

/// Faithful port of rotator.c's `rotate_1()` for jitter-y, never-quite-stops
/// rotation around an axis. `pos` is in turns (range -1..1), `v` is velocity,
/// `dv` is acceleration, `max_v` clamps velocity magnitude.
fn rotate_axis(pos: &mut f64, v: &mut f64, dv: &mut f64, max_v: f64) {
    let mut rng = rand::rng();
    let mut ppos = *pos;
    if ppos < 0.0 {
        ppos = -(ppos + *v);
    } else {
        ppos += *v;
    }
    // CLAMP: keep ppos in [0, 1).
    while ppos < 0.0 {
        ppos += 1.0;
    }
    while ppos >= 1.0 {
        ppos -= 1.0;
    }
    *pos = if *pos > 0.0 { ppos } else { -ppos };

    *v += *dv;
    if *v > max_v || *v < -max_v {
        *dv = -*dv;
    } else if *v < 0.0 {
        if rng.random_range(0..4) != 0 {
            *v = 0.0;
            if rng.random_range(0..2) != 0 {
                *dv = 0.0;
            } else if *dv < 0.0 {
                *dv = -*dv;
            }
        } else {
            *v = -*v;
            *dv = -*dv;
            *pos = -*pos;
        }
    }
    if rng.random_range(0..120) == 0 {
        *dv = -*dv;
    }
    if rng.random_range(0..200) == 0 {
        if *dv == 0.0 {
            *dv = 0.00001;
        } else if rng.random_range(0..2) != 0 {
            *dv *= 1.2;
        } else {
            *dv *= 0.8;
        }
    }
}

// Molecule-swap animation: dwell in mode 0, then zoom out (mode 1) / zoom in
// (mode 2).  speed 4 => 80/4 = 20 ticks each, matching draw_molecule().
const MODE_TICKS: i32 = 20;

pub struct Molecule {
    width: u32,
    height: u32,
    delay_us: u64,

    rotx: f64,
    roty: f64,
    rotz: f64,
    dx: f64,
    dy: f64,
    dz: f64,
    ddx: f64,
    ddy: f64,
    ddz: f64,
    d_max: f64,
    wander_frame: u32,

    which: usize,
    molecules: Vec<MoleculeDef>,

    mode: i32,      // 0 = normal, 1 = zooming out, 2 = zooming in
    mode_tick: i32,
    draw_tick: i32,
    last_change_time: u64,
    timeout_secs: u64,

    /// Per-pixel depth buffer scratch for render(&self), persistent so a
    /// steady-state frame doesn't reallocate; grown (never shrunk) to the
    /// largest bounding box seen, and only the frame's used prefix is
    /// re-filled with f32::INFINITY each render.
    depth_scratch: RefCell<Vec<f32>>,
}

impl Molecule {
    fn pick_new_molecule(&mut self, rng: &mut rand::rngs::ThreadRng) {
        if self.molecules.len() <= 1 {
            self.which = 0;
            return;
        }
        let mut n = self.which;
        while n == self.which {
            n = rng.random_range(0..self.molecules.len());
        }
        self.which = n;
    }
}

impl Animation for Molecule {
    fn new(config: &AnimConfig) -> Self {
        let mut m = Molecule {
            width: config.width.max(1),
            height: config.height.max(1),
            delay_us: if config.delay_us == 0 { 20_000 } else { config.delay_us },

            rotx: 0.0,
            roty: 0.0,
            rotz: 0.0,
            dx: 0.0,
            dy: 0.0,
            dz: 0.0,
            ddx: 0.0,
            ddy: 0.0,
            ddz: 0.0,
            d_max: 0.0,
            wander_frame: 0,

            which: 0,
            molecules: parse_molecules(),

            mode: 0,
            mode_tick: 0,
            draw_tick: 0,
            last_change_time: 0,
            // DEF_TIMEOUT is 20 seconds.
            timeout_secs: if config.cycles <= 0 { 20 } else { config.cycles as u64 },

            depth_scratch: RefCell::new(Vec::new()),
        };
        m.reset(config);
        m
    }

    fn reset(&mut self, config: &AnimConfig) {
        let mut rng = rand::rng();
        self.width = config.width.max(1);
        self.height = config.height.max(1);

        // make_rotator(spin=0.5 per axis, accel=0.3, wander).
        let spin = 0.5_f64;
        let accel = 0.3_f64;
        let d = 0.006_f64;
        let dd = 0.00006_f64;
        let sign = |r: &mut rand::rngs::ThreadRng| if r.random::<bool>() { 1.0 } else { -1.0 };
        let bell = |r: &mut rand::rngs::ThreadRng| {
            (r.random::<f64>() + r.random::<f64>() + r.random::<f64>()) / 3.0
        };

        // Initial angles: random turns in (-1..1), sign carries direction.
        self.rotx = rng.random::<f64>() * sign(&mut rng);
        self.roty = rng.random::<f64>() * sign(&mut rng);
        self.rotz = rng.random::<f64>() * sign(&mut rng);

        // dx = BELLRAND(d * spin); d_max is derived from dx alone (rotator.c).
        self.dx = bell(&mut rng) * d * spin;
        self.dy = bell(&mut rng) * d * spin;
        self.dz = bell(&mut rng) * d * spin;
        self.d_max = self.dx * 2.0;

        self.ddx = (dd + rng.random::<f64>() * (dd + dd)) * spin * accel;
        self.ddy = (dd + rng.random::<f64>() * (dd + dd)) * spin * accel;
        self.ddz = (dd + rng.random::<f64>() * (dd + dd)) * spin * accel;

        self.wander_frame = if DO_WANDER { rng.random_range(0..0xFFFF) } else { 0 };

        if !self.molecules.is_empty() {
            self.which = rng.random_range(0..self.molecules.len());
        }

        self.mode = 0;
        self.mode_tick = 0;
        self.draw_tick = 0;
        self.last_change_time = now_secs();
    }

    fn tick(&mut self) {
        let now = now_secs();
        let mut rng = rand::rng();

        match self.mode {
            0 => {
                self.draw_tick += 1;
                if self.draw_tick > 10 {
                    self.draw_tick = 0;
                    if self.molecules.len() > 1
                        && now >= self.last_change_time + self.timeout_secs
                    {
                        self.mode = 1; // zoom out
                        self.mode_tick = MODE_TICKS;
                        self.last_change_time = now;
                    }
                }
            }
            1 => {
                self.mode_tick -= 1;
                if self.mode_tick <= 0 {
                    self.mode_tick = MODE_TICKS;
                    self.mode = 2; // zoom in
                    self.pick_new_molecule(&mut rng);
                }
            }
            _ => {
                self.mode_tick -= 1;
                if self.mode_tick <= 0 {
                    self.mode = 0;
                }
            }
        }

        rotate_axis(&mut self.rotx, &mut self.dx, &mut self.ddx, self.d_max);
        rotate_axis(&mut self.roty, &mut self.dy, &mut self.ddy, self.d_max);
        rotate_axis(&mut self.rotz, &mut self.dz, &mut self.ddz, self.d_max);

        if DO_WANDER {
            self.wander_frame = self.wander_frame.wrapping_add(1);
        }
    }

    fn render(&self, buffer: &mut [u8], width: u32, height: u32) {
        clear_buffer(buffer, Color::new(255, 0, 0, 0));
        if self.molecules.is_empty() {
            return;
        }
        let m = &self.molecules[self.which.min(self.molecules.len() - 1)];
        if m.atoms.is_empty() {
            return;
        }

        // Bounding box -> centroid + size.
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut min_z = f64::INFINITY;
        let mut max_z = f64::NEG_INFINITY;
        for a in &m.atoms {
            min_x = min_x.min(a.x);
            max_x = max_x.max(a.x);
            min_y = min_y.min(a.y);
            max_y = max_y.max(a.y);
            min_z = min_z.min(a.z);
            max_z = max_z.max(a.z);
        }
        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        let cz = (min_z + max_z) * 0.5;
        let bb_size = (max_x - min_x)
            .max(max_y - min_y)
            .max(max_z - min_z)
            .max(1e-6);

        let labels_on = DO_LABELS && bb_size <= NO_LABEL_THRESHOLD;

        // Fit the molecule into a virtual cube of side `max_size` (matches
        // ensure_bounding_box_visible: max_size = 10).
        let max_size = 10.0_f64;
        let overall_scale = if bb_size > max_size { max_size / bb_size } else { 1.0 };

        // The C applies glScalef(1.1) around everything, plus the mode zoom.
        let mode_scale = match self.mode {
            1 => self.mode_tick as f64 / MODE_TICKS as f64,
            2 => (MODE_TICKS - self.mode_tick + 1) as f64 / MODE_TICKS as f64,
            _ => 1.0,
        };
        let world_scale = overall_scale * 1.1 * mode_scale;

        // Rotation angles (turns -> radians). rotator returns |rot|.
        let rx = self.rotx.abs() * 2.0 * PI;
        let ry = self.roty.abs() * 2.0 * PI;
        let rz = self.rotz.abs() * 2.0 * PI;

        // Wander offset (get_position sinoid), applied post-rotation.  Off by
        // default; the *9 magnitude matches draw_molecule's translate.
        let (wx, wy, wz) = if DO_WANDER {
            let wf = self.wander_frame as f64;
            let ws = 0.01_f64;
            let sinoid = |f: f64| (1.0 + (wf * f / 2.0 * PI).sin()) / 2.0;
            (
                (sinoid(0.71 * ws) - 0.5) * 9.0 * world_scale,
                (sinoid(0.53 * ws) - 0.5) * 9.0 * world_scale,
                (sinoid(0.37 * ws) - 0.5) * 9.0 * world_scale,
            )
        } else {
            (0.0, 0.0, 0.0)
        };

        // gluLookAt(0,0,30, 0,0,0, 0,1,0): the camera sits 30 units out.
        let view_z = 30.0_f64;
        let transform = |x: f64, y: f64, z: f64| -> Vec3 {
            let v = Vec3 {
                x: (x - cx) * world_scale,
                y: (y - cy) * world_scale,
                z: (z - cz) * world_scale,
            };
            let v = rotate_z(v, rz);
            let v = rotate_y(v, ry);
            let v = rotate_x(v, rx);
            Vec3 { x: v.x + wx, y: v.y + wy, z: v.z + wz - view_z }
        };

        // Pinhole projection with vertical FOV = 30 degrees.
        let fovy_rad = 30.0_f64.to_radians();
        let f = 1.0 / (fovy_rad / 2.0).tan();
        let aspect = (width as f64) / (height as f64).max(1.0);
        let near = 0.1_f64;
        // Keep screen coords in f64: rounding them to whole pixels here makes
        // quad corners and disc centres snap frame-to-frame under slow spin.
        let project = |v: Vec3| -> Option<(f64, f64, f64)> {
            if -v.z < near {
                return None;
            }
            let inv_w = 1.0 / -v.z;
            let ndc_x = v.x * f / aspect * inv_w;
            let ndc_y = v.y * f * inv_w;
            let sx = (ndc_x + 1.0) * 0.5 * width as f64;
            let sy = (-ndc_y + 1.0) * 0.5 * height as f64;
            // Pixels per world unit at this depth (for sizing spheres).
            let ppu = f * inv_w * 0.5 * height as f64;
            Some((sx, sy, ppu))
        };

        // Borrows its label from `m.atoms` instead of cloning a String per
        // atom per render.
        struct ProjAtom<'a> {
            world: Vec3,
            sx: f64,
            sy: f64,
            ppu: f64,
            data: AtomData,
            label: &'a str,
            visible: bool,
        }
        let mut projected: Vec<ProjAtom<'_>> = Vec::with_capacity(m.atoms.len());
        for a in &m.atoms {
            let world = transform(a.x, a.y, a.z);
            let (sx, sy, ppu, visible) = match project(world) {
                Some((sx, sy, ppu)) => (sx, sy, ppu, true),
                None => (0.0, 0.0, 0.0, false),
            };
            projected.push(ProjAtom {
                world,
                sx,
                sy,
                ppu,
                data: a.data.clone(),
                label: a.label.as_str(),
                visible,
            });
        }

        // Build a depth-sorted draw list: back-to-front so nearer atoms occlude
        // farther ones (and their labels).
        enum DrawItem<'a> {
            Atom(&'a ProjAtom<'a>),
            Bond(&'a ProjAtom<'a>, &'a ProjAtom<'a>, i32),
        }
        let mut draw_list: Vec<(f64, DrawItem)> = Vec::new();
        if DO_ATOMS {
            for a in &projected {
                draw_list.push((a.world.z, DrawItem::Atom(a)));
            }
        }
        // atom.id -> index into `projected` (built in the same order as
        // `m.atoms`), via the molecule's precomputed id_index: O(1) instead
        // of a linear scan per bond endpoint per frame.
        let find_by_id = |id: i32| m.id_index.get(&id).map(|&i| &projected[i]);
        if DO_BONDS {
            for b in &m.bonds {
                if let (Some(p1), Some(p2)) = (find_by_id(b.from), find_by_id(b.to)) {
                    let z = (p1.world.z + p2.world.z) * 0.5;
                    draw_list.push((z, DrawItem::Bond(p1, p2, b.strength)));
                }
            }
        }
        draw_list.sort_by(|a, b| {
            a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
        });

        let bond_color = all_atom_data("bond").color;

        // Per-pixel depth buffer over the molecule's screen bounding box.
        // Geometry is resolved by depth test, so draw order no longer decides
        // atom/bond overlaps; the back-to-front list is kept only so labels
        // (drawn without depth) still occlude correctly.
        let atom_screen_radius = |a: &ProjAtom| -> f64 {
            (a.data.size2 * world_scale * a.ppu)
                .max(2.0)
                .min((height as f64 / 4.0).max(2.0))
        };
        let (mut bx0, mut by0, mut bx1, mut by1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for a in &projected {
            if !a.visible {
                continue;
            }
            let r = atom_screen_radius(a);
            bx0 = bx0.min((a.sx - r - 1.0).floor() as i32);
            by0 = by0.min((a.sy - r - 1.0).floor() as i32);
            bx1 = bx1.max((a.sx + r + 1.0).ceil() as i32);
            by1 = by1.max((a.sy + r + 1.0).ceil() as i32);
        }
        let dx0 = bx0.max(0);
        let dy0 = by0.max(0);
        let dx1 = bx1.min(width as i32 - 1);
        let dy1 = by1.min(height as i32 - 1);
        let needed = ((dx1 - dx0 + 1).max(0) * (dy1 - dy0 + 1).max(0)) as usize;
        let mut depth_data = self.depth_scratch.borrow_mut();
        if depth_data.len() < needed {
            depth_data.resize(needed, f32::INFINITY);
        }
        depth_data[..needed].fill(f32::INFINITY);
        let mut depth = DepthBuf::new(&mut depth_data[..needed], dx0, dy0, dx1, dy1);

        for (_, item) in &draw_list {
            match item {
                DrawItem::Atom(a) => {
                    if !a.visible {
                        continue;
                    }
                    let world_r = a.data.size2 * world_scale;
                    let radius_f = (world_r * a.ppu)
                        .max(2.0)
                        .min((height as f64 / 4.0).max(2.0));

                    draw_filled_sphere(
                        buffer, width, height, a.sx, a.sy, radius_f, a.data.color,
                        &mut depth, world_r, -a.world.z,
                    );

                    if labels_on {
                        draw_atom_label(
                            buffer, width, height,
                            a.sx.round() as i32, a.sy.round() as i32,
                            radius_f.round() as i32, &a.label, a.data.text_color,
                        );
                    }
                }
                DrawItem::Bond(p1, p2, strength) => {
                    if !p1.visible || !p2.visible {
                        continue;
                    }
                    // tube radius: base 0.07 per bond strength, capped 0.3
                    // (tube() scales a radius-1 unit tube by this value).
                    let radius_w = (0.07 * *strength as f64).min(0.3) * world_scale;
                    // Clip the tube back to just inside each atom's sphere so
                    // it never paints over its endpoint atoms (the GL hides
                    // the buried part with the depth buffer).
                    let dx = p2.world.x - p1.world.x;
                    let dy = p2.world.y - p1.world.y;
                    let dz = p2.world.z - p1.world.z;
                    let len3 = (dx * dx + dy * dy + dz * dz).sqrt();
                    // Inset each end to just inside its atom.  The tube now has
                    // rounded (capsule) caps of radius `radius_w`; for the cap to
                    // stay hidden inside the sphere the cap centre must sit within
                    // sqrt(atom_r^2 - radius_w^2) of the atom centre (the cap base
                    // rim then lands on the silhouette).  Thin bonds keep the old
                    // 0.9r; only bonds thick enough that a flat cap would poke past
                    // a small atom get pulled deeper (clamped to 0 when thicker
                    // than the atom).
                    let inset = |atom_r: f64| -> f64 {
                        (0.9 * atom_r)
                            .min((atom_r * atom_r - radius_w * radius_w).max(0.0).sqrt())
                    };
                    let r1 = inset(p1.data.size2 * world_scale);
                    let r2 = inset(p2.data.size2 * world_scale);
                    if len3 <= r1 + r2 {
                        continue; // atoms overlap: no visible tube
                    }
                    let w1 = Vec3 {
                        x: p1.world.x + dx * (r1 / len3),
                        y: p1.world.y + dy * (r1 / len3),
                        z: p1.world.z + dz * (r1 / len3),
                    };
                    let w2 = Vec3 {
                        x: p2.world.x - dx * (r2 / len3),
                        y: p2.world.y - dy * (r2 / len3),
                        z: p2.world.z - dz * (r2 / len3),
                    };
                    let (Some((sx1, sy1, ppu1)), Some((sx2, sy2, ppu2))) =
                        (project(w1), project(w2))
                    else {
                        continue;
                    };
                    // Depth is interpolated along the bond from the two
                    // (shortened) endpoints; the per-pixel test cleanly resolves
                    // the junction (the bond's rounded cap sits inside the sphere,
                    // so the sphere's near surface wins there) and lets a bond
                    // cross in front of a non-endpoint atom draw over it.
                    draw_solid_bond(
                        buffer, width, height,
                        sx1, sy1, (radius_w * ppu1).max(0.75),
                        sx2, sy2, (radius_w * ppu2).max(0.75),
                        bond_color,
                        &mut depth, -w1.z, -w2.z, radius_w,
                    );
                }
            }
        }

        // Title (do_titles): molecule name + formula, top-left, in the bond
        // text color (yellow), only when not mid-transition (mode 0).
        if DO_TITLES && self.mode == 0 {
            let title_color = all_atom_data("bond").text_color;
            let scale = 2;
            let line_h = (FONT_H + 2) * scale;
            let mut y = FONT_H * scale + 2;
            for line in &m.title_lines {
                draw_text(buffer, width, height, 2, y, line, title_color, scale);
                y += line_h;
            }
        }
    }

    fn clears_each_frame(&self) -> bool {
        true
    }
    fn frame_delay_us(&self) -> u64 {
        self.delay_us.max(10_000)
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// GL_LIGHT0's position (1.0, 0.4, 0.9, 0) from gl_init(), normalized: a
/// directional light from the upper right, in front of the scene.
const LIGHT: (f64, f64, f64) = (0.712_47, 0.284_99, 0.641_22);

/// Source-over blends `color` onto the buffer at `coverage` (0..1) opacity.
/// The scene draws back-to-front on an opaque background, so blending edge
/// pixels against what is already there gives smooth (antialiased) silhouettes.
fn blend_pixel(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    color: Color,
    coverage: f64,
) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let c = coverage.clamp(0.0, 1.0);
    if c >= 1.0 {
        put_pixel(buffer, width, height, x, y, color);
        return;
    }
    if c <= 0.0 {
        return;
    }
    let stride = (width * 4) as usize;
    let idx = (y as usize) * stride + (x as usize) * 4;
    if idx + 3 >= buffer.len() {
        return;
    }
    let inv = 1.0 - c;
    buffer[idx] = (color.b as f64 * c + buffer[idx] as f64 * inv).round() as u8;
    buffer[idx + 1] = (color.g as f64 * c + buffer[idx + 1] as f64 * inv).round() as u8;
    buffer[idx + 2] = (color.r as f64 * c + buffer[idx + 2] as f64 * inv).round() as u8;
    buffer[idx + 3] = 255;
}

/// Per-frame depth buffer covering the molecule's screen bounding box, storing
/// view-space distance from the camera (smaller = nearer).  Atoms and bonds are
/// resolved by this per-pixel depth test rather than draw order, so junctions
/// transition per-pixel and a bond crossing in front of a sphere draws over it.
struct DepthBuf<'a> {
    /// Borrowed from the caller's persistent depth_scratch buffer (sized to
    /// exactly this frame's bbox) rather than a fresh per-frame Vec.
    data: &'a mut [f32],
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
}
impl<'a> DepthBuf<'a> {
    fn new(data: &'a mut [f32], x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        let w = (x1 - x0 + 1).max(0);
        let h = (y1 - y0 + 1).max(0);
        DepthBuf { data, x0, y0, w, h }
    }
    /// Depth-test pixel (x,y) at `depth`; true if nearer-or-equal to what's
    /// stored, in which case it becomes the new nearest depth.  Callers only
    /// test a pixel they are about to draw (coverage already > 0), so every
    /// drawn pixel is the nearest visible surface there and must record depth --
    /// including the antialiased rim, otherwise a later shape that is actually
    /// *behind* would pass the (still-empty) rim test and paint a fringe over
    /// the silhouette.  Pixels outside the bbox always pass (untracked).
    #[inline]
    fn test(&mut self, x: i32, y: i32, depth: f32) -> bool {
        let (lx, ly) = (x - self.x0, y - self.y0);
        if lx < 0 || ly < 0 || lx >= self.w || ly >= self.h {
            return true;
        }
        let idx = (ly * self.w + lx) as usize;
        if depth <= self.data[idx] {
            self.data[idx] = depth;
            true
        } else {
            false
        }
    }
}

/// Scales a color by a Lambert shade factor (clamped to 1.0).
fn shade_color(base: Color, shade: f64) -> Color {
    let s = shade.clamp(0.0, 1.0);
    Color::new(
        base.a,
        (base.r as f64 * s) as u8,
        (base.g as f64 * s) as u8,
        (base.b as f64 * s) as u8,
    )
}

/// Fills a circle with per-pixel Lambert shading against LIGHT so atoms read
/// as lit 3D spheres (approximating the GL's lit unit_sphere).
#[allow(clippy::too_many_arguments)]
fn draw_filled_sphere(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    cx: f64,
    cy: f64,
    radius: f64,
    base: Color,
    depth: &mut DepthBuf<'_>,
    // World-space sphere radius and the view-space depth of its centre; the near
    // surface bulges toward the eye by world_r*sqrt(1-(d/radius)^2).
    world_r: f64,
    center_depth: f64,
) {
    if radius <= 0.0 {
        return;
    }
    // 1px margin for the antialiased fringe; sample at pixel centres (x+0.5).
    let x0 = (cx - radius - 1.0).floor() as i32;
    let x1 = (cx + radius + 1.0).ceil() as i32;
    let y0 = (cy - radius - 1.0).floor() as i32;
    let y1 = (cy + radius + 1.0).ceil() as i32;
    if x1 < 0 || y1 < 0 || x0 >= width as i32 || y0 >= height as i32 {
        return;
    }
    for y in y0..=y1 {
        if y < 0 || y >= height as i32 {
            continue;
        }
        for x in x0..=x1 {
            if x < 0 || x >= width as i32 {
                continue;
            }
            let ddx = x as f64 + 0.5 - cx;
            let ddy = y as f64 + 0.5 - cy;
            let dist = (ddx * ddx + ddy * ddy).sqrt();
            // Coverage: full inside, ramps to 0 across the 1px edge band.
            let coverage = (radius + 0.5 - dist).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            // Near-surface depth at this pixel (bulges toward the eye); rim
            // pixels (dist>=radius) sit at the centre depth.
            let frac = (dist / radius).min(1.0);
            let zc = (center_depth - world_r * (1.0 - frac * frac).sqrt()) as f32;
            if !depth.test(x, y, zc) {
                continue;
            }
            // Sphere surface normal: (nx, ny, nz), screen y points down so
            // world y flips sign.
            let nx = ddx / radius;
            let ny = ddy / radius;
            let nz = (1.0 - (nx * nx + ny * ny)).max(0.0).sqrt();
            let ndl = nx * LIGHT.0 - ny * LIGHT.1 + nz * LIGHT.2;
            let shade = 0.2 + 0.8 * ndl.max(0.0);
            blend_pixel(buffer, width, height, x, y, shade_color(base, shade), coverage);
        }
    }
}

/// Draws a bond as a solid capsule between two projected endpoints: a
/// cylindrical body with hemispherical end caps, per-endpoint half-widths
/// (perspective) and per-pixel Lambert shading, so it reads as tube.c's solid
/// smooth tube.  The caps replace the old flat quad ends, which -- once a bond
/// grew thick relative to a small endpoint atom -- poked square corners past the
/// atom's silhouette; a rounded cap stays within the sphere (given the caller's
/// containment inset) and terminates cleanly where it doesn't.
#[allow(clippy::too_many_arguments)]
fn draw_solid_bond(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    x0: f64,
    y0: f64,
    h0: f64,
    x1: f64,
    y1: f64,
    h1: f64,
    base: Color,
    depth: &mut DepthBuf<'_>,
    // View-space depth at each endpoint and the tube's world radius: depth is
    // interpolated linearly along the axis, with the cross-section bulge added.
    depth0: f64,
    depth1: f64,
    world_r: f64,
) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 {
        return; // end-on tube: hidden inside its atoms anyway
    }
    let ax = dx / len;
    let ay = dy / len;
    // Screen-space perpendicular.
    let px = -ay;
    let py = ax;

    // 1px margin so the antialiased fringe is covered; sample pixel centres.
    // The bounding box already pads by hmax on every side of the endpoint box,
    // which also covers the hemispherical caps (they extend at most `hw` past an
    // endpoint in any direction).
    let hmax = h0.max(h1) + 1.0;
    let min_x = ((x0.min(x1) - hmax).floor() as i32).max(0);
    let max_x = ((x0.max(x1) + hmax).ceil() as i32).min(width as i32 - 1);
    let min_y = ((y0.min(y1) - hmax).floor() as i32).max(0);
    let max_y = ((y0.max(y1) + hmax).ceil() as i32).min(height as i32 - 1);

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let rx = x as f64 + 0.5 - x0;
            let ry = y as f64 + 0.5 - y0;
            let s = rx * ax + ry * ay;
            // Nearest point on the axis segment; `along` is the overshoot into a
            // cap (0 within the body).  Half-width tapers along the body only.
            let sc = s.clamp(0.0, len);
            let along = s - sc;
            let hw = h0 + (h1 - h0) * (sc / len);
            let t = rx * px + ry * py;
            // Capsule coverage: distance to the nearest axis point (|t| in the
            // body, sqrt(along^2+t^2) in the caps), ramping to 0 across a 1px
            // band, so both the long edges and the round caps move smoothly
            // sub-pixel instead of snapping.
            let r2d = (along * along + t * t).sqrt();
            let coverage = (hw + 0.5 - r2d).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            // Unified sphere/cylinder normal: radial component (screen-space,
            // toward the nearest axis point) plus out-of-screen nz.  In the body
            // along=0 so this reduces to the cylinder's perpendicular normal; in
            // the caps it is the hemisphere's normal.  Screen y points down, so
            // the world y of the normal flips sign in the dot product.
            let rn = (r2d / hw).min(1.0);
            let nz = (1.0 - rn * rn).max(0.0).sqrt();
            let nsx = (along * ax + t * px) / hw;
            let nsy = (along * ay + t * py) / hw;
            // View-space depth: linear along the axis (endpoint depth in the
            // caps) plus the near-side bulge of the cross-section / cap.
            let base_depth = depth0 + (depth1 - depth0) * (sc / len);
            let zc = (base_depth - world_r * nz) as f32;
            if !depth.test(x, y, zc) {
                continue;
            }
            let ndl = nsx * LIGHT.0 - nsy * LIGHT.1 + nz * LIGHT.2;
            let shade = 0.2 + 0.8 * ndl.max(0.0);
            blend_pixel(buffer, width, height, x, y, shade_color(base, shade), coverage);
        }
    }
}

/* ---- tiny built-in 5x7 bitmap font (bit 0x10 = leftmost column) ---- */

const FONT_W: i32 = 5;
const FONT_H: i32 = 7;

fn glyph(ch: char) -> Option<[u8; 7]> {
    Some(match ch {
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        'a' => [0x00, 0x00, 0x0E, 0x01, 0x0F, 0x11, 0x0F],
        'b' => [0x10, 0x10, 0x1E, 0x11, 0x11, 0x11, 0x1E],
        'c' => [0x00, 0x00, 0x0E, 0x10, 0x10, 0x11, 0x0E],
        'd' => [0x01, 0x01, 0x0F, 0x11, 0x11, 0x11, 0x0F],
        'e' => [0x00, 0x00, 0x0E, 0x11, 0x1F, 0x10, 0x0E],
        'f' => [0x06, 0x09, 0x08, 0x1C, 0x08, 0x08, 0x08],
        'g' => [0x00, 0x0F, 0x11, 0x11, 0x0F, 0x01, 0x0E],
        'h' => [0x10, 0x10, 0x16, 0x19, 0x11, 0x11, 0x11],
        'i' => [0x04, 0x00, 0x0C, 0x04, 0x04, 0x04, 0x0E],
        'j' => [0x02, 0x00, 0x06, 0x02, 0x02, 0x12, 0x0C],
        'k' => [0x10, 0x10, 0x12, 0x14, 0x18, 0x14, 0x12],
        'l' => [0x0C, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'm' => [0x00, 0x00, 0x1A, 0x15, 0x15, 0x11, 0x11],
        'n' => [0x00, 0x00, 0x16, 0x19, 0x11, 0x11, 0x11],
        'o' => [0x00, 0x00, 0x0E, 0x11, 0x11, 0x11, 0x0E],
        'p' => [0x00, 0x00, 0x1E, 0x11, 0x1E, 0x10, 0x10],
        'q' => [0x00, 0x00, 0x0D, 0x13, 0x0F, 0x01, 0x01],
        'r' => [0x00, 0x00, 0x16, 0x19, 0x10, 0x10, 0x10],
        's' => [0x00, 0x00, 0x0F, 0x10, 0x0E, 0x01, 0x1E],
        't' => [0x08, 0x08, 0x1C, 0x08, 0x08, 0x09, 0x06],
        'u' => [0x00, 0x00, 0x11, 0x11, 0x11, 0x13, 0x0D],
        'v' => [0x00, 0x00, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'w' => [0x00, 0x00, 0x11, 0x11, 0x15, 0x15, 0x0A],
        'x' => [0x00, 0x00, 0x11, 0x0A, 0x04, 0x0A, 0x11],
        'y' => [0x00, 0x00, 0x11, 0x11, 0x0F, 0x01, 0x0E],
        'z' => [0x00, 0x00, 0x1F, 0x02, 0x04, 0x08, 0x1F],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x0C, 0x04, 0x08],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        ';' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x04, 0x08],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
        '[' => [0x0E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0E],
        ']' => [0x0E, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0E],
        _ => return None,
    })
}

/// Draws text with its top-left at approximately (x, y_baseline - FONT_H*scale),
/// i.e. `y` is the glyph baseline.  Unknown chars render as blank space.
fn draw_text(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    scale: i32,
) {
    let char_w = (FONT_W + 1) * scale;
    let mut cx = x;
    for ch in text.chars() {
        if let Some(rows) = glyph(ch) {
            for (ry, row) in rows.iter().enumerate() {
                for bx in 0..FONT_W {
                    if row & (0x10 >> bx) != 0 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                put_pixel(
                                    buffer,
                                    width,
                                    height,
                                    cx + bx * scale + sx,
                                    y - FONT_H * scale + ry as i32 * scale + sy,
                                    color,
                                );
                            }
                        }
                    }
                }
            }
        }
        cx += char_w;
    }
}

/// Draws an atom's element label centered on its projected position, sized
/// relative to the atom radius (billboarding degenerates to 2D centering).
fn draw_atom_label(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    cx: i32,
    cy: i32,
    radius: i32,
    label: &str,
    color: Color,
) {
    if label.is_empty() || radius < 4 {
        return;
    }
    let scale = (radius / 8).clamp(1, 3);
    let char_w = (FONT_W + 1) * scale;
    let text_w = label.chars().count() as i32 * char_w;
    let text_h = FONT_H * scale;
    let x = cx - text_w / 2;
    let y = cy + text_h / 2; // baseline
    draw_text(buffer, width, height, x, y, label, color, scale);
}

