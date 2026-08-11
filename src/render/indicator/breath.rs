//! Breath mode: a minimal breathing circle — a flat filled core with one thin
//! concentric ring, no gradients. The core slowly "breathes" (scale on a
//! wall-clock sine) to signal "alive and listening." Typing feeds a smooth
//! *energy* level — a saturating soft-union of per-keystroke envelopes — so
//! the circle swells and the ring expands while keys keep coming, relaxing
//! when they stop rather than twitching per key. Energy saturates (a burst
//! of 5 keys reads like a burst of 3) and decays fully within ~0.6s, so rest
//! frames carry no length information — the same accepted rhythm-only
//! semantics as `ripple`/`fade`. The breath quickens while verifying;
//! success settles the circle calm and green; invalid shudders it red.
//! Leak-free.

use super::pen::Pen;
use super::{key_env, wall_clock, IndicatorCtx, ENV_S};
use crate::auth::AuthState;
use std::f64::consts::PI;

fn mix(a: f64, b: f64, k: f64) -> f64 {
    a + (b - a) * k
}

pub(super) fn draw(pen: &mut Pen, ctx: &IndicatorCtx) {
    let t = wall_clock(ctx.now);

    // Breath: smooth sine, 0 (exhaled) .. 1 (inhaled); faster while verifying.
    let breath_hz = if ctx.verification_start.is_some() {
        1.3
    } else {
        0.65
    };
    let osc = 0.5 + 0.5 * (2.0 * PI * breath_hz * t).sin();

    // Typing energy: soft union (1 - Π(1-e)) of the live keystroke envelopes.
    // Sustained fast typing holds it near 1 — continuously energized — and it
    // breathes back down within ~0.6s of the last key. It only brightens the
    // bloom and nudges the core baseline; the visible per-key response is the
    // traveling rings drawn below, so nothing ever jolts the breath itself.
    let mut calm = 1.0f64;
    for &ts in ctx.keystrokes {
        let age = ctx.now.duration_since(ts).as_secs_f64();
        if age >= ENV_S {
            continue;
        }
        calm *= 1.0 - key_env(age);
    }
    let energy = 1.0 - calm;

    // Auth-complete fade (~400ms), like comet.
    let fade = ctx
        .auth_complete_time
        .map(|c| (ctx.now.duration_since(c).as_secs_f64() / 0.4).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let tau = ctx
        .auth_complete_time
        .map(|c| ctx.now.duration_since(c).as_secs_f64())
        .unwrap_or(0.0);

    // State changes motion, not just color: success eases the breath to a
    // steady half-full calm; invalid shudders side-to-side a couple of times.
    let (osc, dx) = match ctx.auth_state {
        AuthState::Success => (mix(osc, 0.45, fade), 0.0),
        AuthState::Invalid => {
            let inner = ctx.radius - ctx.thickness;
            (
                osc,
                inner * 0.12 * (2.0 * PI * 6.0 * tau).sin() * (1.0 - fade),
            )
        }
        _ => (osc, 0.0),
    };

    // Glow tint: cool white at rest, green/red once an attempt completes.
    let (base_r, base_g, base_b) = (0.72, 0.82, 1.0);
    let (tr, tg, tb) = match ctx.auth_state {
        AuthState::Success => (0.25, 1.0, 0.45),
        AuthState::Invalid => (1.0, 0.3, 0.25),
        _ => (base_r, base_g, base_b),
    };
    let k = if matches!(ctx.auth_state, AuthState::Success | AuthState::Invalid) {
        0.45 + 0.55 * fade // tint immediately, deepen over the fade
    } else {
        0.0
    };
    let (cr_, cg_, cb_) = (mix(base_r, tr, k), mix(base_g, tg, k), mix(base_b, tb, k));

    // Geometry: breathing-app anatomy — a solid core plus a bloom of
    // concentric translucent circles that expand with the inhale and gather
    // back into the core on the exhale. Outer layers travel further than
    // inner ones, giving the blooming-flower read. Everything peaks within
    // ~0.85*inner at full inhale + full typing energy.
    let inner = ctx.radius - ctx.thickness;
    let cx = ctx.x + dx;
    let cy = ctx.y;
    let grow = osc + 0.20 * energy;
    let core_r = inner * (0.18 + 0.14 * grow.min(1.2));

    // Bloom layers: translucent filled circles behind the core. At exhale
    // they sit just outside the core; at full inhale they spread out.
    for i in (1..=3u32).rev() {
        let spread = 1.0 + (0.22 * i as f64) * (0.25 + 0.75 * grow.min(1.2));
        let a = (0.16 - 0.03 * i as f64 + 0.06 * energy).max(0.04);
        pen.set_source_rgba(cr_, cg_, cb_, a);
        pen.arc(cx, cy, core_r * spread, 0.0, 2.0 * PI);
        pen.fill();
    }

    // Typing response: each keystroke births a thin ring at the core's edge
    // that travels outward through the bloom and dissolves — the breath is
    // never disturbed, and typing reads as waves moving through the circles
    // (concurrent rings reflect rhythm only, the accepted `ripple`/`fade`
    // semantics). The p²(1-p)² bump has zero value and slope at both ends,
    // so rings appear and vanish without a pop.
    const WAVE_S: f64 = 0.7;
    for &ts in ctx.keystrokes {
        let age = ctx.now.duration_since(ts).as_secs_f64();
        if age >= WAVE_S {
            continue;
        }
        let p = age / WAVE_S; // 0 at birth .. 1 gone
        let bump = 16.0 * p * p * (1.0 - p) * (1.0 - p); // peaks at 1.0
        let wr = core_r * (1.05 + 1.9 * p);
        pen.set_line_width((inner * 0.035 * (1.0 - 0.5 * p)).max(1.5));
        pen.set_source_rgba(cr_, cg_, cb_, 0.55 * bump);
        pen.arc(cx, cy, wr, 0.0, 2.0 * PI);
        pen.stroke();
    }

    // Core: one flat filled circle.
    pen.set_source_rgba(cr_, cg_, cb_, 0.92);
    pen.arc(cx, cy, core_r, 0.0, 2.0 * PI);
    pen.fill();
}
