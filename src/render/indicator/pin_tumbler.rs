//! Pin-tumbler mode: the original accumulating ring. Like pins in a lock,
//! one segment per keystroke pops out at a shuffled position and stays set,
//! with a rotate/snap collapse on verification. `~1 in 4` keystrokes are
//! skipped to blur the length signal — partial mitigation only, not
//! leak-free (see the crate docs).

use super::pen::Pen;
use super::IndicatorCtx;
use crate::auth::AuthState;
use std::f64::consts::PI;
use std::hash::{Hash, Hasher};

pub(super) fn draw(pen: &mut Pen, ctx: &IndicatorCtx) {
    let IndicatorCtx {
        x,
        y,
        radius,
        thickness,
        num_segments,
        now,
        auth_state,
        keystrokes,
        verification_start,
        auth_complete_time,
        segment_sequence,
        ..
    } = *ctx;
    let (x, y, radius_f, thickness) = (x, y, radius, thickness);
    let segment_angle = 2.0 * PI / (num_segments as f64);

    let (mut active_spaces, mut inactive_spaces, mut active_is_cw) = (3, 1, true);
    if let Some(v_start) = verification_start {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        v_start.hash(&mut hasher);
        let h = hasher.finish();
        active_spaces = (h % 5) as usize;
        inactive_spaces = ((h / 5) % 5) as usize;
        active_is_cw = (h % 2) == 0;
    }

    // Map keystrokes to randomized positions, skipping ~1 in 4 (decided from the
    // keystroke's timestamp so it's stable across frames, not re-rolled per
    // frame). ponytail: partial mitigation — lit count still trends with length
    // and repeated-attempt averaging can recover it; accepted for this mode.
    let mut active_info = vec![None; num_segments];
    for (i, &ts) in keystrokes.iter().enumerate() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        ts.hash(&mut hasher);
        if hasher.finish() % 4 == 0 {
            continue;
        }
        let segment_idx = segment_sequence[i % segment_sequence.len()];
        active_info[segment_idx] = Some((ts, i));
    }

    let rotate_and_snap_elapsed = verification_start
        .map(|v| now.duration_since(v).as_millis() as f64)
        .unwrap_or(0.0);
    let collapse_phase = (rotate_and_snap_elapsed / 800.0).clamp(0.0, 1.0);
    let rotate_phase = (collapse_phase * 2.0).clamp(0.0, 1.0);
    let snap_phase = ((collapse_phase - 0.5) * 2.0).clamp(0.0, 1.0);

    let color_phase = if let Some(complete_time) = auth_complete_time {
        if rotate_and_snap_elapsed >= 800.0 {
            let time_since_800 = rotate_and_snap_elapsed - 800.0;
            let time_since_complete = now.duration_since(complete_time).as_millis() as f64;
            ((time_since_800.min(time_since_complete)) / 400.0).clamp(0.0, 1.0)
        } else {
            0.0
        }
    } else {
        0.0
    };

    let (mut val_r, mut val_g, mut val_b) = (1.0, 1.0, 1.0);
    if color_phase > 0.0 {
        match auth_state {
            AuthState::Success => {
                val_r = 1.0 - color_phase;
                val_b = 1.0 - color_phase;
            }
            AuthState::Invalid => {
                val_g = 1.0 - color_phase;
                val_b = 1.0 - color_phase;
            }
            _ => {}
        }
    }

    // Ring base (inactive segments)
    pen.set_line_width(thickness);
    let mut inactive_indices = Vec::new();
    for (i, info) in active_info.iter().enumerate() {
        if info.is_none() {
            inactive_indices.push(i);
        }
    }
    let num_inactive = inactive_indices.len();
    let base_opacity = 0.2 + (0.6 * snap_phase);

    for j in 0..num_inactive {
        let original_i = inactive_indices[j];
        let target_j = if active_is_cw {
            (j + num_inactive - (inactive_spaces % num_inactive)) % num_inactive
        } else {
            (j + inactive_spaces) % num_inactive
        };
        let target_i = inactive_indices[target_j];
        let angle_offset = if active_is_cw {
            let ccw_distance = (original_i + num_segments - target_i) % num_segments;
            -(ccw_distance as f64) * segment_angle * rotate_phase
        } else {
            let cw_distance = (target_i + num_segments - original_i) % num_segments;
            (cw_distance as f64) * segment_angle * rotate_phase
        };
        let start_angle = original_i as f64 * segment_angle + angle_offset;
        pen.set_source_rgba(val_r, val_g, val_b, base_opacity);
        pen.arc(x, y, radius_f, start_angle, start_angle + segment_angle);
        pen.stroke();
    }

    // Pulsing active segments
    let mut active_indices = Vec::new();
    for (i, info) in active_info.iter().enumerate() {
        if info.is_some() {
            active_indices.push(i);
        }
    }
    let k = active_indices.len();
    for (i, info) in active_info.iter().enumerate() {
        if let Some((ts, stroke_idx)) = *info {
            let j = active_indices.iter().position(|&v| v == i).unwrap_or(0);
            let target_j = if active_is_cw {
                (j + active_spaces) % k.max(1)
            } else {
                (j + k - (active_spaces % k.max(1))) % k.max(1)
            };
            let target_i = active_indices.get(target_j).copied().unwrap_or(i);
            let angle_offset = if active_is_cw {
                let cw_distance = (target_i + num_segments - i) % num_segments;
                (cw_distance as f64) * segment_angle * rotate_phase
            } else {
                let ccw_distance = (i + num_segments - target_i) % num_segments;
                -(ccw_distance as f64) * segment_angle * rotate_phase
            };
            let start_angle = i as f64 * segment_angle + angle_offset;
            let mid_angle = start_angle + segment_angle / 2.0;

            let t = (now.duration_since(ts).as_millis() as f64 / 300.0).clamp(0.0, 1.0);
            let pulse_phase = 1.0 - (1.0 - t).powi(3);
            let extent_factor = 0.15 + 0.25 * (((stroke_idx * 13) % 10) as f64 / 10.0);
            let push_distance = radius_f * extent_factor * pulse_phase * (1.0 - snap_phase);
            let dx = mid_angle.cos() * push_distance;
            let dy = mid_angle.sin() * push_distance;

            pen.save();
            pen.translate(dx, dy);
            pen.set_source_rgba(val_r, val_g, val_b, 0.8);
            pen.set_line_width(thickness);
            pen.arc(x, y, radius_f, start_angle, start_angle + segment_angle);
            pen.stroke();
            pen.restore();
        }
    }
}
