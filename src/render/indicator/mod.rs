//! Typing-indicator rendering.
//!
//! Each animation style is a `mode` in its own submodule, selected by
//! `--indicator-mode`. The dispatcher here draws the shared chrome — the inner
//! disk (whose color reflects auth/caps state) and the success/fail/caps text —
//! and delegates the animated element to the selected mode's `draw`.
//!
//! Leak-free house rule: a mode's on-screen state may depend on time,
//! `auth_state`, caps-lock, and *time since the last keystroke*, but never on
//! the *count* of keystrokes. Only `pin-tumbler` intentionally breaks this (a
//! documented, accepted partial leak).

use crate::app::AuthState;
use crate::render::DamageRect;
use pen::Pen;
use std::f64::consts::PI;
use std::sync::OnceLock;
use std::time::Instant;
use tiny_skia::Pixmap;

mod breath;
mod comet;
mod dots;
mod fade;
mod pen;
mod pin_tumbler;
mod ripple;
mod scope;

// ── Indicator clock ──────────────────────────────────────────────────────────
// The event loop (wayland.rs) keys its redraw/settle decisions off these via
// `App::indicator_active` / `App::auth_settled`; the modes' own animation math
// agrees with them (pin_tumbler's collapse and the shared ~400 ms fades).

/// Verifying rotate/snap collapse duration (pin-tumbler), in ms.
pub(crate) const VERIFY_COLLAPSE_MS: u64 = 800;
/// Post-completion color fade shared by the modes, in ms.
pub(crate) const COMPLETE_FADE_MS: u64 = 400;
/// Full verify/complete envelope: collapse, then fade.
pub(crate) const VERIFY_ENVELOPE_MS: u64 = VERIFY_COLLAPSE_MS + COMPLETE_FADE_MS;
/// How long a keystroke's pop/flash keeps animating after the key, in ms.
pub(crate) const KEYSTROKE_SETTLE_MS: u64 = 400;
/// Redraw cadence while the indicator is animating (~60 fps), in ms.
pub(crate) const FRAME_MS: u64 = 16;
/// Ring segment count used by the segmented modes and the shuffled sequence.
pub const NUM_SEGMENTS: usize = 24;

// ── Shared animation helpers ─────────────────────────────────────────────────

/// Monotonic seconds since the first indicator frame — one process-global
/// animation clock for all the time-driven modes, so their motion shares a
/// phase instead of each starting at its own first-draw instant.
pub(super) fn wall_clock(now: Instant) -> f64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    now.duration_since(*EPOCH.get_or_init(|| now)).as_secs_f64()
}

/// Cubic smoothstep on [0, 1] — zero slope at both ends.
pub(super) fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Full envelope span of one keystroke's energy (attack + decay).
pub(super) const ENV_S: f64 = 0.62;

/// One keystroke's energy contribution: smoothstep attack (~70ms) then
/// smoothstep decay. Zero value *and* slope at birth and death, so nothing on
/// screen ever jumps when a key lands or an envelope expires.
pub(super) fn key_env(age: f64) -> f64 {
    const ATTACK: f64 = 0.07;
    if age < ATTACK {
        smoothstep(age / ATTACK)
    } else {
        smoothstep(1.0 - (age - ATTACK) / (ENV_S - ATTACK))
    }
}

/// Which typing-indicator animation to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndicatorMode {
    /// Segments pop out like lock pins and stay set; ~1 in 4 keystrokes
    /// skipped to blur length. Lit count still trends with length — partial
    /// mitigation only.
    PinTumbler,
    /// Per-keystroke pop that fades back to an empty ring at rest. No leak.
    /// The default: unlike pin-tumbler it reveals nothing about length.
    #[default]
    Fade,
    /// A comet orbits the ring on a wall clock; keystroke = brief flash.
    Comet,
    /// Minimal breathing circle with a swell per keystroke. No leak.
    Breath,
    /// Fixed cluster of "typing…" dots that shimmer on input. No leak.
    Dots,
    /// Live oscilloscope trace; amplitude from last-keystroke recency. No leak.
    Scope,
    /// Still pool; each keystroke drops an expanding, fading ripple. No leak.
    Ripple,
}

impl std::str::FromStr for IndicatorMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            // "classic" and "decay" are the pre-rename aliases.
            "pin-tumbler" | "pintumbler" | "pin_tumbler" | "classic" => {
                Ok(IndicatorMode::PinTumbler)
            }
            "fade" | "decay" => Ok(IndicatorMode::Fade),
            "comet" => Ok(IndicatorMode::Comet),
            "breath" | "orb" => Ok(IndicatorMode::Breath),
            "dots" => Ok(IndicatorMode::Dots),
            "scope" => Ok(IndicatorMode::Scope),
            "ripple" => Ok(IndicatorMode::Ripple),
            _ => Err(format!(
                "unknown indicator mode: '{}' (pin-tumbler|fade|comet|breath|dots|scope|ripple)",
                s
            )),
        }
    }
}

/// Everything a mode's `draw` needs, built by the caller from app state.
/// Geometry is precomputed; timing is read from the fields (a mode uses only
/// what it needs).
pub struct IndicatorCtx<'a> {
    pub x: f64,
    pub y: f64,
    /// Ring radius in pixels (the animated element lives at/around this).
    pub radius: f64,
    /// Stroke width used by the ring modes; a sensible unit for others.
    pub thickness: f64,
    /// Number of ring segments the segmented modes use.
    pub num_segments: usize,
    pub now: Instant,
    pub auth_state: AuthState,
    /// Keystroke timestamps, oldest first. Modes may read recency/`.last()` but
    /// must not let the *count* reach the screen (except `classic`).
    pub keystrokes: &'a [Instant],
    pub verification_start: Option<Instant>,
    pub auth_complete_time: Option<Instant>,
    /// Shuffled segment order the ring modes map keystrokes onto.
    pub segment_sequence: &'a [usize],
    /// Number of failed attempts so far, shown as a running tally below the
    /// ring (persists between attempts once non-zero).
    pub failed_attempts: u32,
    /// A message PAM surfaced on the last failure (e.g. a faillock lockout
    /// notice), shown wrapped below the ring; `None` when there is none.
    pub message: Option<&'a str>,
}

/// Presentation of the indicator: which animation, chrome opacity, the
/// idle/typing color override, and whether caps-lock warning state shows.
pub struct IndicatorStyle {
    pub mode: IndicatorMode,
    /// Overall opacity of the indicator chrome (the inner disk), 0.0-1.0.
    pub opacity: f64,
    /// Override for the idle/typing disk color (RGB). Semantic state colors
    /// (success green, invalid red, caps amber) are unaffected.
    pub color: Option<(f64, f64, f64)>,
    pub caps_lock: bool,
}

/// Draw the typing indicator in the style's `mode`.
///
/// The indicator is rendered into a small tiny-skia [`Pixmap`] (RGBA,
/// premultiplied) sized to its bounding box, then alpha-composited onto the
/// main `buf` — which stays BGRA (`wl_shm` Argb8888, little-endian), exactly
/// as the animation pipeline writes it. The composite is where the one and
/// only format conversion happens: an R↔B swizzle plus source-over blend.
pub(crate) fn render_indicator(
    buf: &mut [u8],
    buf_w: i32,
    buf_h: i32,
    ctx: &IndicatorCtx,
    style: &IndicatorStyle,
) -> Result<Option<DamageRect>, String> {
    let radius_f = ctx.radius;
    let auth_state = ctx.auth_state;

    // Secondary text below the ring: the attempt tally, then PAM's message
    // (small/wide enough that a typical faillock notice wraps to two lines).
    // Built first so the pixmap can grow to fit it.
    let below_size = radius_f * 0.14;
    let wrap_w = radius_f * 4.5;
    let gap = radius_f * 0.22;
    let line_h = below_size * 1.35;
    let mut below_lines: Vec<String> = Vec::new();
    if ctx.failed_attempts > 0 {
        let plural = if ctx.failed_attempts == 1 { "" } else { "s" };
        below_lines.push(format!("{} failed attempt{plural}", ctx.failed_attempts));
    }
    if let Some(msg) = ctx.message {
        below_lines.extend(pen::wrap(below_size, wrap_w, msg));
    }

    // Bounding box: the ring's own pushes (pin-tumbler ~1.4r) / glows (~1.25r)
    // stay inside 1.5r. Keep the pixmap square and ring-centered — pointing
    // (ctx.x, ctx.y) at (half, half) reuses each mode's absolute math verbatim
    // — but grow `half` when a below-ring block needs the room.
    let ring_half = radius_f * 1.5;
    let half = if below_lines.is_empty() {
        ring_half
    } else {
        let block_h = below_lines.len() as f64 * line_h;
        let need_v = radius_f + gap + block_h + radius_f * 0.25;
        let need_h = wrap_w / 2.0 + radius_f * 0.25;
        ring_half.max(need_v).max(need_h)
    }
    .ceil();
    let side = ((2.0 * half).ceil() as u32).max(1);
    let mut pix = Pixmap::new(side, side).ok_or("Failed to allocate indicator pixmap")?;

    let lctx = IndicatorCtx {
        x: half,
        y: half,
        radius: ctx.radius,
        thickness: ctx.thickness,
        num_segments: ctx.num_segments,
        now: ctx.now,
        auth_state: ctx.auth_state,
        keystrokes: ctx.keystrokes,
        verification_start: ctx.verification_start,
        auth_complete_time: ctx.auth_complete_time,
        segment_sequence: ctx.segment_sequence,
        failed_attempts: ctx.failed_attempts,
        message: ctx.message,
    };

    {
        let mut pen = Pen::new(&mut pix);

        // Inner disk — color reflects auth/caps state. Caps-lock overrides while
        // idle/typing so the user always sees it. `color` (--indicator-color)
        // overrides only the non-semantic idle/typing states; success green,
        // invalid red, and caps amber are fixed. Fully opaque unless the user
        // dials it down with --indicator-opacity.
        // Palette rationale: the non-semantic states are three luminance steps of
        // one cool-tinted neutral (idle darkest, typing mid, verifying lightest)
        // — hue is reserved for meaning, so the semantic colors (gold=warning,
        // emerald=success, crimson=error) stand out harder. Values are tuned so
        // status text hits WCAG contrast on every disk, success stays brighter
        // than invalid for a color-blind-safe luminance difference on top of
        // motion+text, and nothing glares in a dark room.
        let caps_showing =
            style.caps_lock && matches!(auth_state, AuthState::Idle | AuthState::Typing);
        let (bg_r, bg_g, bg_b) = if caps_showing {
            (0.85, 0.62, 0.05) // gold — warning; carries near-black text below
        } else {
            match auth_state {
                AuthState::Success => (0.05, 0.52, 0.28), // emerald, 4.7:1 w/ white
                AuthState::Invalid => (0.72, 0.11, 0.20), // crimson, 6.5:1 w/ white
                AuthState::Idle => style.color.unwrap_or((0.13, 0.15, 0.18)), // near-black slate
                AuthState::Verifying => style.color.unwrap_or((0.41, 0.43, 0.46)), // light gray, 5.1:1 w/ white
                _ => style.color.unwrap_or((0.31, 0.33, 0.36)), // mid gray, 7.5:1 w/ white
            }
        };
        pen.set_source_rgba(bg_r, bg_g, bg_b, style.opacity);
        pen.arc(half, half, radius_f - ctx.thickness, 0.0, 2.0 * PI);
        pen.fill();

        match style.mode {
            IndicatorMode::PinTumbler => pin_tumbler::draw(&mut pen, &lctx),
            IndicatorMode::Fade => fade::draw(&mut pen, &lctx),
            IndicatorMode::Comet => comet::draw(&mut pen, &lctx),
            IndicatorMode::Breath => breath::draw(&mut pen, &lctx),
            IndicatorMode::Dots => dots::draw(&mut pen, &lctx),
            IndicatorMode::Scope => scope::draw(&mut pen, &lctx),
            IndicatorMode::Ripple => ripple::draw(&mut pen, &lctx),
        }

        // The primary status word, on the contrast-controlled disk. Black-on-
        // gold is the canonical warning combination (8.8:1); other states carry
        // white text on their deep disks.
        let status = if caps_showing {
            "Caps Lock"
        } else {
            match auth_state {
                AuthState::Success => "Success",
                AuthState::Invalid => "Wrong",
                _ => "",
            }
        };
        if !status.is_empty() {
            let fg = if caps_showing {
                (0.05, 0.05, 0.05)
            } else {
                (1.0, 1.0, 1.0)
            };
            pen.draw_text_centered(half, half, radius_f * 0.35, status, fg);
        }

        // Secondary block below the ring (PAM message + attempt tally). Drawn
        // with a drop-shadow since it sits on the background, not the disk.
        if !below_lines.is_empty() {
            let top = half + radius_f + gap;
            pen.draw_lines_centered(half, top, below_size, &below_lines, (0.95, 0.95, 0.95));
        }
    }

    // Composite the RGBA (premultiplied) pixmap onto the BGRA (premultiplied)
    // main buffer at the indicator's screen position, source-over, swapping R↔B.
    let ox = (ctx.x - half).round() as i32;
    let oy = (ctx.y - half).round() as i32;
    let src = pix.data();
    let sw = side as i32;
    // Clip once to the source rows/columns that land inside the buffer,
    // instead of bounds-testing every pixel.
    let sy0 = (-oy).max(0);
    let sy1 = (buf_h - oy).min(sw);
    let sx0 = (-ox).max(0);
    let sx1 = (buf_w - ox).min(sw);
    for sy in sy0..sy1 {
        let dy = oy + sy;
        for sx in sx0..sx1 {
            let dx = ox + sx;
            let si = ((sy * sw + sx) * 4) as usize;
            let sa = src[si + 3] as u32;
            if sa == 0 {
                continue;
            }
            let (sr, sg, sb) = (src[si] as u32, src[si + 1] as u32, src[si + 2] as u32);
            let di = ((dy * buf_w + dx) * 4) as usize;
            let inv = 255 - sa;
            // buf is BGRA; src is RGBA — R↔B swap in the assignment.
            buf[di] = (sb + buf[di] as u32 * inv / 255) as u8; // B
            buf[di + 1] = (sg + buf[di + 1] as u32 * inv / 255) as u8; // G
            buf[di + 2] = (sr + buf[di + 2] as u32 * inv / 255) as u8; // R
            buf[di + 3] = (sa + buf[di + 3] as u32 * inv / 255) as u8; // A
        }
    }
    Ok(DamageRect::clipped(
        ox,
        oy,
        side as i32,
        side as i32,
        buf_w,
        buf_h,
    ))
}
