//! Fade mode: the same per-keystroke pop as `pin-tumbler`, but each segment
//! fades and retracts over a fixed lifetime, so once typing stops the ring
//! returns to a uniform faint circle. The number of lit segments reflects
//! recent typing rhythm, never the total character count — the at-rest
//! screen discloses no length. Leak-free.

use super::pen::Pen;
use super::IndicatorCtx;
use crate::auth::AuthState;
use std::f64::consts::PI;

pub(super) fn draw(pen: &mut Pen, ctx: &IndicatorCtx) {
    const LIFETIME_S: f64 = 0.9;
    let (x, y, radius_f, thickness, num_segments, now) = (
        ctx.x,
        ctx.y,
        ctx.radius,
        ctx.thickness,
        ctx.num_segments,
        ctx.now,
    );
    let segment_angle = 2.0 * PI / (num_segments as f64);

    // Tint the whole ring toward the state color once an attempt completes.
    let (tint_r, tint_g, tint_b) = match ctx.auth_state {
        AuthState::Success => (0.3, 1.0, 0.4),
        AuthState::Invalid => (1.0, 0.3, 0.3),
        _ => (1.0, 1.0, 1.0),
    };

    // Faint, always-present base ring (uniform across segments — no length info).
    pen.set_line_width(thickness);
    for i in 0..num_segments {
        let a0 = i as f64 * segment_angle;
        pen.set_source_rgba(tint_r, tint_g, tint_b, 0.18);
        pen.arc(x, y, radius_f, a0, a0 + segment_angle * 0.9);
        pen.stroke();
    }

    // Recent keystrokes pop their segment out and fade over LIFETIME_S. Iterating
    // keystrokes (not segments) means a segment simply returns to the base ring
    // once its keystroke ages out.
    for (i, &ts) in ctx.keystrokes.iter().enumerate() {
        let age = now.duration_since(ts).as_secs_f64();
        if age >= LIFETIME_S {
            continue;
        }
        let life = age / LIFETIME_S; // 0 fresh .. 1 gone
        let segment_idx = ctx.segment_sequence[i % ctx.segment_sequence.len()];
        let start_angle = segment_idx as f64 * segment_angle;
        let mid_angle = start_angle + segment_angle / 2.0;

        let pop = (age / 0.3).clamp(0.0, 1.0);
        let pop_phase = 1.0 - (1.0 - pop).powi(3);
        let push = radius_f * 0.3 * pop_phase * (1.0 - life);
        let opacity = 0.85 * (1.0 - life);

        pen.save();
        pen.translate(mid_angle.cos() * push, mid_angle.sin() * push);
        pen.set_source_rgba(tint_r, tint_g, tint_b, opacity);
        pen.set_line_width(thickness);
        pen.arc(
            x,
            y,
            radius_f,
            start_angle,
            start_angle + segment_angle * 0.9,
        );
        pen.stroke();
        pen.restore();
    }
}
