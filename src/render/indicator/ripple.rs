//! Ripple mode: the inner disk is a still pool of water. Each keystroke drops
//! a ripple — an expanding, fading circle from a point hashed from the
//! keystroke's timestamp — that decays fully within `LIFETIME_S`. At rest the
//! pool is glassy and carries no length information; concurrent ripples
//! reflect recent typing rhythm only (the same accepted semantics as `decay`).
//! Verifying rains gentle time-driven drops; success glows the pool green and
//! stills; invalid sends one hard red ripple from the center. Leak-free.

use super::pen::Pen;
use super::IndicatorCtx;
use crate::app::AuthState;
use std::f64::consts::PI;
use std::hash::{Hash, Hasher};

/// How long a ripple lives, birth to gone.
const LIFETIME_S: f64 = 1.2;
/// The crests of one ripple: (delay s, radius scale, alpha scale). The primary
/// crest plus two smaller, fainter, slightly delayed trailing rings — this is
/// what makes it read as water rather than expanding circles.
const CRESTS: [(f64, f64, f64); 3] = [(0.0, 1.0, 1.0), (0.11, 0.70, 0.45), (0.22, 0.48, 0.22)];

fn hash_u64<T: Hash>(v: T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Map a hash to a stable point within `spread` of the pool center (sqrt on
/// the distance for uniform-by-area, so drops don't bunch at the center).
fn origin(h: u64, x: f64, y: f64, spread: f64) -> (f64, f64) {
    let ang = (h % 4096) as f64 / 4096.0 * 2.0 * PI;
    let d = (((h >> 12) % 4096) as f64 / 4096.0).sqrt() * spread;
    (x + ang.cos() * d, y + ang.sin() * d)
}

/// One soft crest: two concentric strokes (tight bright core + wide faint
/// halo) so the ring reads as a swell of water, not a hard circle.
fn crest(pen: &mut Pen, ox: f64, oy: f64, r: f64, w: f64, rgb: (f64, f64, f64), a: f64) {
    if r <= 0.1 || a <= 0.004 {
        return;
    }
    for (wf, af) in [(1.0, 1.0), (2.4, 0.32)] {
        pen.set_line_width(w * wf);
        pen.set_source_rgba(rgb.0, rgb.1, rgb.2, a * af);
        pen.arc(ox, oy, r, 0.0, 2.0 * PI);
        pen.stroke();
    }
}

/// A full ripple at `age`: radius grows ease-out (fast at birth, slowing as it
/// dies — how water moves), alpha fades to nothing over `LIFETIME_S`.
#[allow(clippy::too_many_arguments)]
fn ripple(
    pen: &mut Pen,
    ox: f64,
    oy: f64,
    age: f64,
    max_r: f64,
    w: f64,
    rgb: (f64, f64, f64),
    strength: f64,
) {
    for &(delay, r_scale, a_scale) in &CRESTS {
        let c_age = age - delay;
        if c_age < 0.0 {
            continue;
        }
        let life = c_age / LIFETIME_S;
        if life >= 1.0 {
            continue;
        }
        let r = max_r * r_scale * (1.0 - (1.0 - life).powi(2));
        let fade = (1.0 - life).powf(1.5);
        let ramp = (c_age / 0.05).clamp(0.0, 1.0); // no pop at birth
        crest(pen, ox, oy, r, w, rgb, strength * a_scale * fade * ramp);
    }
}

pub(super) fn draw(pen: &mut Pen, ctx: &IndicatorCtx) {
    let (x, y) = (ctx.x, ctx.y);
    let pool_r = ctx.radius - ctx.thickness; // the dispatcher's disk = the pool
    let w = ctx.thickness * 0.35;

    // Wall clock for the ambient shimmer and the verifying rain.
    let t = super::wall_clock(ctx.now);

    pen.save();
    pen.arc(x, y, pool_r, 0.0, 2.0 * PI);
    pen.clip(); // nothing spills past the water's edge

    // Waterline rim — constant; keeps the pool reading as a bounded element
    // over any background.
    pen.set_line_width(w);
    pen.set_source_rgba(1.0, 1.0, 1.0, 0.20);
    pen.arc(x, y, pool_r - w, 0.0, 2.0 * PI);
    pen.stroke();

    match ctx.auth_state {
        AuthState::Success => {
            // The whole pool glows green and stills: a soft bloom easing in,
            // plus one gentle green swell from the center as it settles.
            let birth = ctx.auth_complete_time.unwrap_or(ctx.now);
            let age = ctx.now.duration_since(birth).as_secs_f64();
            let phase = (age / 0.6).clamp(0.0, 1.0);
            for (rs, af) in [(1.0, 0.10), (0.68, 0.10), (0.42, 0.12)] {
                pen.set_source_rgba(0.5, 1.0, 0.6, af * phase);
                pen.arc(x, y, pool_r * rs, 0.0, 2.0 * PI);
                pen.fill();
            }
            ripple(pen, x, y, age, pool_r * 0.9, w, (0.6, 1.0, 0.65), 0.5);
        }
        AuthState::Invalid => {
            // ONE hard red ripple from the center, then still.
            let birth = ctx.auth_complete_time.unwrap_or(ctx.now);
            let age = ctx.now.duration_since(birth).as_secs_f64();
            ripple(
                pen,
                x,
                y,
                age,
                pool_r * 1.02,
                w * 1.6,
                (1.0, 0.42, 0.36),
                1.0,
            );
        }
        _ => {
            // Verifying: gentle continuous rain. Drop points come from hashing
            // the wall-clock tick index — never from keystrokes.
            if ctx.verification_start.is_some() {
                const DROP_EVERY_S: f64 = 0.33;
                let tick = (t / DROP_EVERY_S).floor() as i64;
                for k in 0..((LIFETIME_S / DROP_EVERY_S).ceil() as i64 + 1) {
                    let i = tick - k;
                    if i < 0 {
                        continue;
                    }
                    let age = t - i as f64 * DROP_EVERY_S;
                    let (ox, oy) = origin(hash_u64(i), x, y, pool_r * 0.7);
                    ripple(pen, ox, oy, age, pool_r * 0.42, w, (1.0, 1.0, 1.0), 0.45);
                }
            }

            // Each live keystroke drops one ripple that decays fully; the
            // origin hashes the timestamp so it's stable across frames.
            // Concurrent ripples reflect recent rhythm only — the count never
            // survives to a rest frame.
            let window = LIFETIME_S + CRESTS[CRESTS.len() - 1].0;
            for &ts in ctx.keystrokes {
                let age = ctx.now.duration_since(ts).as_secs_f64();
                if age >= window {
                    continue;
                }
                let (ox, oy) = origin(hash_u64(ts), x, y, pool_r * 0.6);
                ripple(pen, ox, oy, age, pool_r * 0.55, w, (1.0, 1.0, 1.0), 0.7);
            }
        }
    }

    pen.restore();
}
