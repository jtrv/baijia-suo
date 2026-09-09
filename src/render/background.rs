use crate::animation::primitives::{clear_buffer, Color};
use crate::render::DamageRect;

fn bgra(color: (f64, f64, f64, f64)) -> [u8; 4] {
    let to8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [to8(color.2), to8(color.1), to8(color.0), to8(color.3)]
}

/// Fill the raw BGRA buffer with a solid color: a premultiplied-opaque fill
/// via the animation pipeline's BGRA writer.
pub fn render_solid_color(buf: &mut [u8], color: (f64, f64, f64, f64)) -> Result<(), String> {
    let pixel = bgra(color);
    clear_buffer(buf, Color::new(pixel[3], pixel[2], pixel[1], pixel[0]));
    Ok(())
}

/// Restore a rectangular part of a tightly packed BGRA buffer to a solid color.
pub fn render_solid_color_rect(
    buf: &mut [u8],
    width: i32,
    height: i32,
    color: (f64, f64, f64, f64),
    rect: DamageRect,
) -> Result<(), String> {
    let needed = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| *bytes >= 0)
        .map(|bytes| bytes as usize)
        .ok_or("invalid background dimensions")?;
    if buf.len() < needed {
        return Err("background buffer is smaller than its surface".into());
    }

    let pixel = bgra(color);
    for y in rect.y..rect.y + rect.height {
        let start = ((y * width + rect.x) * 4) as usize;
        let end = start + rect.width as usize * 4;
        for dst in buf[start..end].as_chunks_mut::<4>().0 {
            dst.copy_from_slice(&pixel);
        }
    }
    Ok(())
}
