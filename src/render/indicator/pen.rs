//! A tiny stateful drawing surface over a tiny-skia `Pixmap` that mirrors the
//! subset of the cairo `Context` API the indicator modes use. Keeping the same
//! call shapes (`set_source_rgba`, `set_line_width`, `arc`, `move_to`,
//! `line_to`, `fill`, `stroke`, `save`/`translate`/`restore`, `clip`) lets the
//! ported modes read almost identically to their cairo originals, so the
//! animation logic stays verbatim and only the backend changes.
//!
//! State (source color/gradient, line width, caps/joins, transform, clip)
//! accumulates like cairo; `arc`/`move_to`/`line_to` build the current path;
//! `fill`/`stroke` consume it (`stroke_preserve` keeps it).

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};
use std::f64::consts::PI;
use std::rc::Rc;
use std::sync::OnceLock;
use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap,
    Point, RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

/// The one shared arc helper. tiny-skia has no arc primitive, so a full circle
/// becomes a true `push_circle` (crisp) and a partial arc is flattened to line
/// segments (~256 per full turn — sub-pixel chords at the indicator's radius).
/// Appends to `pb`, starting a subpath if empty (else connecting with a line,
/// matching cairo's `arc` "line to arc start" behavior).
fn append_arc(pb: &mut PathBuilder, cx: f64, cy: f64, r: f64, a0: f64, a1: f64) {
    let span = a1 - a0;
    if span.abs() >= 2.0 * PI - 1e-6 && pb.is_empty() {
        pb.push_circle(cx as f32, cy as f32, r as f32);
        return;
    }
    let segs = ((span.abs() / (2.0 * PI) * 256.0).ceil() as usize).max(2);
    for i in 0..=segs {
        let a = a0 + span * (i as f64 / segs as f64);
        let (x, y) = ((cx + r * a.cos()) as f32, (cy + r * a.sin()) as f32);
        if pb.is_empty() {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
}

fn color(r: f64, g: f64, b: f64, a: f64) -> Color {
    let to8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::from_rgba8(to8(r), to8(g), to8(b), to8(a))
}

/// A gradient stop as the modes express it: `(offset, (r, g, b, a))` in 0..1.
type ColorStop = (f64, (f64, f64, f64, f64));

/// Current paint source: solid, or a radial gradient rebuilt per paint (cheap;
/// the indicator draws once per frame).
enum Source {
    Solid(Color),
    Radial {
        center: Point,
        radius: f32,
        stops: Vec<GradientStop>,
    },
}

pub(super) struct Pen<'a> {
    pix: &'a mut Pixmap,
    pb: PathBuilder,
    source: Source,
    line_width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
    transform: Transform,
    clip: Option<Rc<Mask>>,
    stack: Vec<(Transform, Option<Rc<Mask>>)>,
}

/// Liberation Sans (SIL OFL), parsed at runtime by `skrifa` for the status
/// text; outlines are rasterized by the same tiny-skia path fill used
/// everywhere else in this file. Runtime rasterization (vs a fixed bitmap
/// atlas) keeps the text crisp at any configured indicator radius.
const FONT_BYTES: &[u8] = include_bytes!("../../../fonts/LiberationSans-Regular.ttf");

/// The parsed bundled font, or `None` if it failed to parse (text is then
/// skipped rather than crashing).
fn font() -> Option<&'static FontRef<'static>> {
    static FONT: OnceLock<Option<FontRef<'static>>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::new(FONT_BYTES).ok()).as_ref()
}

// Liberation Sans cap height is ~0.72 em, so em ≈ size / 0.72 for a target
// cap height of `size` px.
fn glyph_size(size: f64) -> Size {
    Size::new((size / 0.72) as f32)
}

/// Width in px of `text` rendered at cap height `size`.
fn text_width(size: f64, text: &str) -> f32 {
    let Some(font) = font() else { return 0.0 };
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(glyph_size(size), LocationRef::default());
    text.chars()
        .filter_map(|c| charmap.map(c))
        .filter_map(|gid| metrics.advance_width(gid))
        .sum()
}

/// Wrap `text` to lines no wider than `max_width` px at cap height `size`.
/// Sentences (split on `". "`, keeping the period) each start a fresh line, so
/// e.g. a faillock notice breaks at its sentence boundary rather than mid-
/// clause; within a sentence it falls back to greedy word wrap (a single word
/// wider than `max_width` still gets its own line).
pub(super) fn wrap(size: f64, max_width: f64, text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for sentence in split_sentences(text) {
        let mut cur = String::new();
        for word in sentence.split_whitespace() {
            let trial = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if cur.is_empty() || (text_width(size, &trial) as f64) <= max_width {
                cur = trial;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
    }
    lines
}

/// Split on `". "` (period followed by a space), re-attaching the period to the
/// sentence it ends. `"3.5"` and other in-word periods are untouched.
fn split_sentences(text: &str) -> Vec<String> {
    let parts: Vec<&str> = text.split(". ").collect();
    let n = parts.len();
    parts
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            if i + 1 < n {
                format!("{p}.")
            } else {
                p.to_string()
            }
        })
        .collect()
}

/// Feeds skrifa's scaled glyph outline commands into a tiny-skia
/// `PathBuilder`, positioned at pen `(x, baseline)` and flipped from font
/// convention (origin at the glyph's advance point, y-up) to device space
/// (y-down).
struct GlyphPen<'a> {
    pb: &'a mut PathBuilder,
    x: f32,
    baseline: f32,
}

impl OutlinePen for GlyphPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.x + x, self.baseline - y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.x + x, self.baseline - y);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.pb.quad_to(
            self.x + cx0,
            self.baseline - cy0,
            self.x + x,
            self.baseline - y,
        );
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.pb.cubic_to(
            self.x + cx0,
            self.baseline - cy0,
            self.x + cx1,
            self.baseline - cy1,
            self.x + x,
            self.baseline - y,
        );
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

impl<'a> Pen<'a> {
    pub(super) fn new(pix: &'a mut Pixmap) -> Self {
        Pen {
            pix,
            pb: PathBuilder::new(),
            source: Source::Solid(Color::BLACK),
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            transform: Transform::identity(),
            clip: None,
            stack: Vec::new(),
        }
    }

    // ── source ────────────────────────────────────────────────────────────
    pub(super) fn set_source_rgba(&mut self, r: f64, g: f64, b: f64, a: f64) {
        self.source = Source::Solid(color(r, g, b, a));
    }

    /// A radial gradient centered at `(cx, cy)` out to `r`; `stops` are
    /// `(offset, (r, g, b, a))` in 0..1 — the cairo `RadialGradient` +
    /// `add_color_stop_rgba` replacement (comet head glow).
    pub(super) fn set_radial(&mut self, cx: f64, cy: f64, r: f64, stops: &[ColorStop]) {
        let center = Point::from_xy(cx as f32, cy as f32);
        let stops = stops
            .iter()
            .map(|(off, (r, g, b, a))| GradientStop::new(*off as f32, color(*r, *g, *b, *a)))
            .collect();
        self.source = Source::Radial {
            center,
            radius: (r as f32).max(0.01),
            stops,
        };
    }

    // ── stroke params ────────────────────────────────────────────────────
    pub(super) fn set_line_width(&mut self, w: f64) {
        self.line_width = w as f32;
    }
    pub(super) fn set_line_cap(&mut self, c: LineCap) {
        self.line_cap = c;
    }
    pub(super) fn set_line_join(&mut self, j: LineJoin) {
        self.line_join = j;
    }

    // ── path building ────────────────────────────────────────────────────
    pub(super) fn arc(&mut self, cx: f64, cy: f64, r: f64, a0: f64, a1: f64) {
        append_arc(&mut self.pb, cx, cy, r, a0, a1);
    }
    pub(super) fn move_to(&mut self, x: f64, y: f64) {
        self.pb.move_to(x as f32, y as f32);
    }
    pub(super) fn line_to(&mut self, x: f64, y: f64) {
        self.pb.line_to(x as f32, y as f32);
    }
    /// Draw one line of `text` left-aligned at `x_left`, on the given
    /// `baseline`, cap height `size`, alpha-blended into the pixmap.
    fn draw_line(
        &mut self,
        x_left: f32,
        baseline: f32,
        size: f64,
        text: &str,
        rgb: (f64, f64, f64),
    ) {
        let Some(font) = font() else { return };
        let scale = glyph_size(size);
        let location = LocationRef::default();
        let charmap = font.charmap();
        let metrics = font.glyph_metrics(scale, location);
        let outlines = font.outline_glyphs();
        let paint = Paint {
            shader: Shader::SolidColor(color(rgb.0, rgb.1, rgb.2, 1.0)),
            anti_alias: true,
            ..Default::default()
        };
        let mut x = x_left;
        for c in text.chars() {
            let Some(gid) = charmap.map(c) else { continue };
            if let Some(outline) = outlines.get(gid) {
                let mut pb = PathBuilder::new();
                let mut pen = GlyphPen {
                    pb: &mut pb,
                    x,
                    baseline,
                };
                if outline
                    .draw(DrawSettings::unhinted(scale, location), &mut pen)
                    .is_ok()
                {
                    if let Some(path) = pb.finish() {
                        self.pix.fill_path(
                            &path,
                            &paint,
                            FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
            x += metrics.advance_width(gid).unwrap_or(0.0);
        }
    }

    /// Draw `text` centered at `(cx, cy)` with cap height `size` px, in `rgb`.
    /// For the status word on the (contrast-controlled) disk.
    pub(super) fn draw_text_centered(
        &mut self,
        cx: f64,
        cy: f64,
        size: f64,
        text: &str,
        rgb: (f64, f64, f64),
    ) {
        let w = text_width(size, text);
        self.draw_line(
            cx as f32 - w / 2.0,
            cy as f32 + size as f32 / 2.0, // cap box centered on cy
            size,
            text,
            rgb,
        );
    }

    /// Draw a stack of `lines`, each centered horizontally at `cx`, starting at
    /// `top` and descending. Rendered with a dark drop-shadow so it stays
    /// legible over the arbitrary background it sits on (below the ring, off
    /// the disk). Lines are already wrapped by the caller (see [`wrap`]).
    pub(super) fn draw_lines_centered(
        &mut self,
        cx: f64,
        top: f64,
        size: f64,
        lines: &[String],
        rgb: (f64, f64, f64),
    ) {
        let line_h = (size * 1.35) as f32;
        let off = (size * 0.06).max(1.0) as f32; // shadow offset
        let mut baseline = top as f32 + size as f32;
        for line in lines {
            let w = text_width(size, line);
            let x = cx as f32 - w / 2.0;
            self.draw_line(x + off, baseline + off, size, line, (0.0, 0.0, 0.0));
            self.draw_line(x, baseline, size, line, rgb);
            baseline += line_h;
        }
    }

    // ── paint ────────────────────────────────────────────────────────────
    fn paint(&self) -> Paint<'static> {
        let shader = match &self.source {
            Source::Solid(c) => Shader::SolidColor(*c),
            Source::Radial {
                center,
                radius,
                stops,
            } => RadialGradient::new(
                // tiny-skia 0.12's two-circle form: a point radius (0) growing
                // out to `radius`, both centered — an ordinary radial gradient.
                *center,
                0.0,
                *center,
                *radius,
                stops.clone(),
                SpreadMode::Pad,
                Transform::identity(),
            )
            .unwrap_or(Shader::SolidColor(Color::TRANSPARENT)),
        };
        Paint {
            shader,
            anti_alias: true,
            ..Default::default()
        }
    }

    pub(super) fn fill(&mut self) {
        let Some(path) = std::mem::take(&mut self.pb).finish() else {
            return;
        };
        let paint = self.paint();
        self.pix.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            self.transform,
            self.clip.as_deref(),
        );
    }

    pub(super) fn stroke(&mut self) {
        let Some(path) = std::mem::take(&mut self.pb).finish() else {
            return;
        };
        self.stroke_path(&path);
    }

    /// Stroke the current path without clearing it (cairo `stroke_preserve`).
    pub(super) fn stroke_preserve(&mut self) {
        let Some(path) = self.pb.clone().finish() else {
            return;
        };
        self.stroke_path(&path);
    }

    fn stroke_path(&mut self, path: &Path) {
        let paint = self.paint();
        let mut stroke = Stroke {
            width: self.line_width.max(0.01),
            ..Stroke::default()
        };
        stroke.line_cap = self.line_cap;
        stroke.line_join = self.line_join;
        self.pix
            .stroke_path(path, &paint, &stroke, self.transform, self.clip.as_deref());
    }

    // ── transform / clip stack ───────────────────────────────────────────
    pub(super) fn save(&mut self) {
        self.stack.push((self.transform, self.clip.clone()));
    }
    pub(super) fn restore(&mut self) {
        if let Some((t, c)) = self.stack.pop() {
            self.transform = t;
            self.clip = c;
        }
    }
    pub(super) fn translate(&mut self, dx: f64, dy: f64) {
        self.transform = self.transform.pre_translate(dx as f32, dy as f32);
    }
    /// Clip subsequent drawing to the current path (rasterized to a `Mask`).
    /// Modes set the clip circle in device space before any `translate`, so
    /// the mask is built with the current transform.
    pub(super) fn clip(&mut self) {
        let Some(path) = std::mem::take(&mut self.pb).finish() else {
            return;
        };
        if let Some(mut mask) = Mask::new(self.pix.width(), self.pix.height()) {
            mask.fill_path(&path, FillRule::Winding, true, self.transform);
            self.clip = Some(Rc::new(mask));
        }
    }
}
