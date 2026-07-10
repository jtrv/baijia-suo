//! Dots mode: the chat "typing…" affordance — exactly three dots resting
//! slightly below the disk center (clear of the status text). One wave runs
//! forever on the wall clock; what typing controls is its *amplitude* — a
//! smooth, saturating energy over the recent keystrokes. Fast typing keeps
//! the wave lively (like a chat peer typing back), a single key raises one
//! graceful swell, and rest lets it settle flat. Because the phase never
//! restarts, no keystroke can snap the animation. Verifying eases the
//! amplitude to full on the same phase. Energy saturates and dies within
//! ~0.6s of the last key (rhythm-only, same accepted semantics as
//! `ripple`/`decay`). Success merges the dots into a steady green pill,
//! invalid gives a quick red horizontal shake. The dot count is a constant
//! three, so nothing on screen tracks password length. Leak-free.

use super::pen::Pen;
use super::{key_env, smoothstep, wall_clock, IndicatorCtx, ENV_S};
use crate::app::AuthState;
use std::f64::consts::PI;

pub(super) fn draw(pen: &mut Pen, ctx: &IndicatorCtx) {
    let (x, y, r, now) = (ctx.x, ctx.y, ctx.radius, ctx.now);
    let t = wall_clock(now);

    let dot_r = r * 0.10;
    let spacing = r * 0.30;
    // Below center so the dots coexist with the centered status text.
    let base_y = y + r * 0.42;
    let bounce_h = r * 0.16;

    // Typing energy: soft union (1 - Π(1-e)) of the live keystroke envelopes.
    // Saturates near 1 under a stream of keys instead of restarting per key,
    // and settles to exactly 0 within ~0.6s of the last one.
    let mut calm = 1.0f64;
    for &ts in ctx.keystrokes {
        let age = now.duration_since(ts).as_secs_f64();
        if age < ENV_S {
            calm *= 1.0 - key_env(age);
        }
    }
    let energy = 1.0 - calm;

    // Wave amplitude: typing energy, or eased to full while verifying. The
    // phase below runs on the wall clock either way, so state changes and
    // keystrokes only ever scale the wave — its motion is never disturbed.
    let verify_amp = ctx.verification_start.map_or(0.0, |v| {
        smoothstep(now.duration_since(v).as_secs_f64() / 0.35)
    });
    let amp = energy.max(verify_amp);
    let phase = 2.0 * PI * 1.4 * t;

    // 0 → 1 over ~400ms after an attempt completes (settle transition).
    let settle = ctx
        .auth_complete_time
        .map(|c| (now.duration_since(c).as_secs_f64() / 0.4).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    let shimmer = 0.06 * (t * 2.0).sin();

    for i in 0..3 {
        let (mut dx, mut sp) = (0.0, spacing);
        let (mut lift, mut op) = (0.0, 0.35 + shimmer);
        let (mut cr_r, mut cr_g, mut cr_b) = (1.0, 1.0, 1.0);

        match ctx.auth_state {
            AuthState::Success => {
                // Slide together into a steady bright green pill.
                sp = spacing * (1.0 - 0.6 * settle);
                cr_r = 1.0 - 0.65 * settle;
                cr_b = 1.0 - 0.55 * settle;
                op = 0.35 + 0.6 * settle;
            }
            AuthState::Invalid => {
                // Quick horizontal jitter-shake that damps out as it settles.
                let ts = ctx
                    .auth_complete_time
                    .map_or(0.0, |c| now.duration_since(c).as_secs_f64());
                dx = r * 0.08 * (ts * 45.0).sin() * (1.0 - settle);
                cr_g = 1.0 - 0.65 * settle;
                cr_b = 1.0 - 0.65 * settle;
                op = 0.35 + 0.55 * settle;
            }
            _ => {
                // Idle / typing / verifying: the one continuous wave, scaled
                // by `amp`. The sine is remapped to 0..1 then raised to a
                // power — a rounded crest that dwells at the baseline, with
                // no clipped kinks anywhere in the cycle.
                let s = 0.5 + 0.5 * (phase - i as f64 * (2.0 * PI / 3.0)).sin();
                let w = s.powf(2.5);
                lift = amp * bounce_h * w;
                op = (op + amp * (0.20 + 0.45 * w)).min(1.0);
            }
        }

        pen.set_source_rgba(cr_r, cr_g, cr_b, op);
        pen.arc(
            x + (i as f64 - 1.0) * sp + dx,
            base_y - lift,
            dot_r,
            0.0,
            2.0 * PI,
        );
        pen.fill();
    }
}
