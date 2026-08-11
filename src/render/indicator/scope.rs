//! Scope mode: a live oscilloscope trace across the inner disk. The trace is
//! REGENERATED every frame from layered sines whose amplitude is driven by
//! wall-clock time and the age of the *most recent* keystroke only — no
//! history buffer, no scrolling, so the screen never encodes how many
//! characters were typed. Leak-free.

use super::pen::Pen;
use super::IndicatorCtx;
use crate::auth::AuthState;
use std::f64::consts::PI;
use tiny_skia::{LineCap, LineJoin};

pub(super) fn draw(pen: &mut Pen, ctx: &IndicatorCtx) {
    let t = super::wall_clock(ctx.now);

    let inner = ctx.radius - ctx.thickness; // inner-disk radius
    let half_w = inner * 0.75; // trace spans 1.5 * inner

    // Excitation: max-combined envelopes over the recent keystrokes (attack +
    // quadratic ring-down). Max, never sum — one key's peak is the ceiling, so
    // fast typing holds a steady level instead of re-spiking per key (reading
    // only last() caused a dip-and-slam on every keystroke), and the count
    // still can't reach the screen. Identical for 1 char or 20.
    let mut excite = 0.0f64;
    for &k in ctx.keystrokes {
        let age = ctx.now.duration_since(k).as_secs_f64();
        if age >= 0.7 {
            continue;
        }
        let attack = (age / 0.09).min(1.0);
        let decay = 1.0 - age / 0.7;
        excite = excite.max(attack * decay * decay);
    }

    // Post-auth transition (0 = just completed, 1 = settled), ~400ms.
    let settle = ctx
        .auth_complete_time
        .map(|c| (ctx.now.duration_since(c).as_secs_f64() / 0.4).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    // Faint slow idle wave so the line reads as a live signal, never dead-flat.
    let idle_amp = inner * (0.065 + 0.035 * (t * 0.7).sin());

    // Time anchored to the CURRENT state, for the extra phase drift below.
    // Anchoring to state entry (not process uptime) keeps the wave velocity
    // bounded and continuous: the old `t * speed` phases jumped by t*Δspeed
    // whenever speed changed, so the wave got progressively wilder the longer
    // the locker ran.
    let state_t = match ctx.auth_state {
        AuthState::Verifying => ctx
            .verification_start
            .map(|v| ctx.now.duration_since(v).as_secs_f64())
            .unwrap_or(0.0),
        AuthState::Success | AuthState::Invalid => ctx
            .auth_complete_time
            .map(|c| ctx.now.duration_since(c).as_secs_f64())
            .unwrap_or(0.0),
        _ => 0.0,
    };

    // (amplitude, extra phase drift, hard-clip?, color, dim-for-status-text)
    let (amp, drift, clipped, (col_r, col_g, col_b), dim) = match ctx.auth_state {
        // Settle smoothly to a calm, near-flat green line.
        AuthState::Success => (
            inner * (0.20 * (1.0 - settle) + 0.012),
            state_t * 0.35,
            false,
            (0.35, 1.0, 0.5),
            settle,
        ),
        // Harsh red clipped/square-ish burst, then flat.
        AuthState::Invalid => (
            inner * (0.48 * (1.0 - settle).powi(2) + 0.012),
            state_t * 2.7,
            settle < 0.85,
            (1.0, 0.28, 0.22),
            settle,
        ),
        // Continuous medium-amplitude activity: signal "processing".
        AuthState::Verifying => (
            inner * (0.20 + 0.055 * (t * 5.0).sin()),
            state_t * 0.35,
            false,
            (0.55, 0.85, 1.0),
            0.0,
        ),
        // Idle / typing: ease from the idle wave toward the SAME character as
        // the verifying animation (steady medium amplitude with gentle
        // modulation) as typing energy rises — sustained typing looks like
        // verifying, just green. No speed coupling to excite: amplitude
        // carries the response, the drift stays constant and calm.
        _ => (
            idle_amp + (inner * (0.20 + 0.055 * (t * 5.0).sin()) - idle_amp) * excite,
            0.0,
            false,
            (0.55, 1.0, 0.75),
            0.0,
        ),
    };

    // 2-3 layered sines at non-harmonic spatial frequencies with
    // time-varying phases — organic, not a textbook sine. Frequencies stay
    // untightened in every state; tightening them under typing was a major
    // part of the old frantic feel. Base velocity is constant; `drift` adds
    // bounded state-anchored motion.
    let (f1, f2, f3) = (2.3, 3.7, 6.1);
    let (p1, p2, p3) = (
        t * 10.0 + drift * 10.0,
        -(t * 15.6 + drift * 15.6) + 1.7,
        t * 23.8 + drift * 23.8 + 4.2,
    );

    // Dim the trace while status text occupies the disk center.
    let alpha_mul = 1.0 - 0.6 * dim;

    pen.save();
    pen.arc(ctx.x, ctx.y, inner, 0.0, 2.0 * PI);
    pen.clip();

    const N: usize = 96;
    for i in 0..=N {
        let u = i as f64 / N as f64 * 2.0 - 1.0; // -1 .. 1 across the trace
        let mut v = 0.50 * (u * PI * f1 + p1).sin()
            + 0.34 * (u * PI * f2 + p2).sin()
            + 0.16 * (u * PI * f3 + p3).sin();
        if clipped {
            v = (v * 2.8).clamp(-1.0, 1.0); // square-ish flattened tops
        }
        let window = 1.0 - 0.3 * u * u; // slight taper toward the edges
        let px = ctx.x + u * half_w;
        let py = ctx.y - amp * window * v;
        if i == 0 {
            pen.move_to(px, py);
        } else {
            pen.line_to(px, py);
        }
    }

    pen.set_line_join(LineJoin::Round);
    pen.set_line_cap(LineCap::Round);
    // Wide low-alpha glow under a bright core so the trace survives visually.
    pen.set_source_rgba(col_r, col_g, col_b, 0.25 * alpha_mul);
    pen.set_line_width(ctx.thickness * 1.3);
    pen.stroke_preserve();
    pen.set_source_rgba(col_r, col_g, col_b, 0.95 * alpha_mul);
    pen.set_line_width((ctx.thickness * 0.45).max(1.2));
    pen.stroke();

    pen.restore();
}
