//! Comet mode: a glowing comet orbits the ring on a wall-clock timer — a
//! bright gradient head with a smooth tail that tapers continuously in both
//! width and brightness along the circumference (no segment quantization).
//! Each keystroke triggers a cascading response (glow, then lunge, then tail
//! flare), so the on-screen state never discloses how many characters have
//! been typed. Leak-free.

use super::pen::Pen;
use super::IndicatorCtx;
use crate::app::AuthState;
use std::f64::consts::PI;
use tiny_skia::LineCap;

pub(super) fn draw(pen: &mut Pen, ctx: &IndicatorCtx) {
    let (x, y, radius_f, thickness, now) = (ctx.x, ctx.y, ctx.radius, ctx.thickness, ctx.now);

    let t = super::wall_clock(now);

    let rev_per_s = if ctx.verification_start.is_some() {
        1.1
    } else {
        0.45
    };

    // Typing response as overlapping action, so it reads organic instead of
    // a rigid twitch: light responds first (glow, peaks ~60ms), the body
    // leans after it (lunge, ~220ms), and the tail drags behind with its own
    // inertia (flare, ~350ms, lingering past the others). Each element is a
    // soft union (1 - Π(1-e)) of x²e^(2(1-x)) pulses — C¹ everywhere: zero
    // value AND zero slope at birth and at the peak, so the comet's angular
    // velocity never steps when a key lands (the earlier pulse shape had a
    // steep birth slope = an instant ~5x speed jump, the "mechanical" feel).
    // The lunge amplitude/relax are sized so the ease-back deceleration
    // stays ~25% of cruise — a glide back to idle speed, never a brake.
    // Saturating unions, never sums: fast typing holds each level rather
    // than stacking it, the resting position stays purely time-driven, and
    // the keystroke count can't reach the screen.
    let pulse = |age: f64, tau: f64| -> f64 {
        let x = age / tau;
        (x * x * (2.0 * (1.0 - x)).exp()).clamp(0.0, 1.0)
    };
    let (mut calm_glow, mut calm_lunge, mut calm_tail) = (1.0f64, 1.0f64, 1.0f64);
    for &k in ctx.keystrokes {
        let age = now.duration_since(k).as_secs_f64();
        if age >= 1.6 {
            continue;
        }
        calm_glow *= 1.0 - pulse(age, 0.06);
        calm_lunge *= 1.0 - pulse(age, 0.22);
        calm_tail *= 1.0 - pulse(age, 0.35);
    }
    let glow = 1.0 - calm_glow;
    let lunge = 1.0 - calm_lunge;
    let tail_flare = 1.0 - calm_tail;

    let head_angle = t * rev_per_s * 2.0 * PI + 0.28 * lunge;

    let color_phase = ctx
        .auth_complete_time
        .map(|c| (now.duration_since(c).as_secs_f64() / 0.4).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let (hl_r, hl_g, hl_b) = match ctx.auth_state {
        AuthState::Success => (1.0 - 0.7 * color_phase, 1.0, 1.0 - 0.7 * color_phase),
        AuthState::Invalid => (1.0, 1.0 - 0.7 * color_phase, 1.0 - 0.7 * color_phase),
        _ => (1.0, 1.0, 1.0),
    };

    // Brightness rides the fast glow; the swell and orbit push ride the
    // slower lunge — the light arrives first, the body follows.
    let push = radius_f * 0.05 * lunge;
    let orbit_r = radius_f + push;
    let brightness = 1.0 + 0.5 * glow;

    // Tail: sweep ~130 degrees behind the head (stretching toward ~175 at
    // full flare, on the slowest envelope so it keeps flaring after the head
    // has settled), rendered as many short arc strokes whose width and alpha
    // taper smoothly to nothing. Small steps + round caps make the taper
    // read as one continuous body, not segments.
    let tail_sweep = PI * (0.72 + 0.26 * tail_flare);
    let steps = 64usize;
    let head_w = thickness * (1.0 + 0.25 * lunge);
    pen.set_line_cap(LineCap::Round);
    for i in 0..steps {
        let f0 = i as f64 / steps as f64; // 0 at head .. 1 at tail tip
        let f1 = (i + 1) as f64 / steps as f64;
        // Quadratic falloff: the tail thins slowly near the head, faster
        // toward the tip — the teardrop silhouette of a comet.
        let fade = (1.0 - f0) * (1.0 - f0);
        let w = head_w * (0.15 + 0.85 * fade);
        let alpha = (0.85 * fade * brightness).min(1.0);
        if alpha < 0.01 {
            break;
        }
        let a1 = head_angle - f0 * tail_sweep;
        let a0 = head_angle - f1 * tail_sweep - 0.01; // slight overlap, no seams
        pen.set_line_width(w);
        pen.set_source_rgba(hl_r, hl_g, hl_b, alpha);
        pen.arc(x, y, orbit_r, a0, a1);
        pen.stroke();
    }

    // Sparks: each keystroke sheds a couple of embers where the head was at
    // that moment. They drift off the orbit line, shrinking and fading over
    // their short life. Geometry derives from the keystroke's timestamp hash,
    // so it's stable across frames; concurrent sparks reflect typing rhythm
    // only (the accepted `ripple`/`fade` semantics).
    const SPARK_S: f64 = 0.55;
    for &k in ctx.keystrokes {
        let age = now.duration_since(k).as_secs_f64();
        if age >= SPARK_S {
            continue;
        }
        let p = age / SPARK_S; // 0 birth .. 1 gone
        let bump = 16.0 * p * p * (1.0 - p) * (1.0 - p);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&k, &mut hasher);
        let h = std::hash::Hasher::finish(&hasher);
        // Where the head was when the key landed (surge ignored: ~0 at birth).
        let birth_angle = (t - age) * rev_per_s * 2.0 * PI;
        for s in 0..2u64 {
            let hs = h.wrapping_mul(s * 2 + 1);
            let lag = 0.06 + 0.10 * ((hs % 100) as f64 / 100.0); // behind the head
            let dir = if (hs >> 8) & 1 == 0 { 1.0 } else { -1.0 }; // off-orbit side
            let drift = radius_f * (0.06 + 0.10 * (((hs >> 16) % 100) as f64 / 100.0));
            let a = birth_angle - lag;
            let r = orbit_r + dir * drift * p;
            let sx = x + a.cos() * r;
            let sy = y + a.sin() * r;
            pen.set_source_rgba(hl_r, hl_g, hl_b, 0.7 * bump);
            pen.arc(sx, sy, head_w * 0.22 * (1.0 - 0.5 * p), 0.0, 2.0 * PI);
            pen.fill();
        }
    }

    // Head: a radial-gradient glow — bright core melting into a soft halo.
    let hx = x + head_angle.cos() * orbit_r;
    let hy = y + head_angle.sin() * orbit_r;
    let head_r = head_w * (1.6 + 0.4 * glow);
    pen.set_radial(
        hx,
        hy,
        head_r,
        &[
            (0.0, (1.0, 1.0, 1.0, (0.95 * brightness).min(1.0))),
            (0.35, (hl_r, hl_g, hl_b, (0.85 * brightness).min(1.0))),
            (1.0, (hl_r, hl_g, hl_b, 0.0)),
        ],
    );
    pen.arc(hx, hy, head_r, 0.0, 2.0 * PI);
    pen.fill();
}
