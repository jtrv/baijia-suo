use crate::animation::primitives::{clear_buffer, Color};

/// Fill the raw BGRA buffer with a solid color: a premultiplied-opaque fill
/// via the animation pipeline's BGRA writer.
pub fn render_solid_color(buf: &mut [u8], color: (f64, f64, f64, f64)) -> Result<(), String> {
    let to8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    clear_buffer(
        buf,
        Color::new(to8(color.3), to8(color.0), to8(color.1), to8(color.2)),
    );
    Ok(())
}
