pub(crate) mod background;
pub mod indicator;
pub(crate) mod pool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DamageRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl DamageRect {
    pub fn full(width: i32, height: i32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    pub fn clipped(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        buf_width: i32,
        buf_height: i32,
    ) -> Option<Self> {
        let x0 = x.clamp(0, buf_width);
        let y0 = y.clamp(0, buf_height);
        let x1 = x.saturating_add(width).clamp(0, buf_width);
        let y1 = y.saturating_add(height).clamp(0, buf_height);
        (x1 > x0 && y1 > y0).then_some(Self {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }

    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BaseFrame {
    Solid,
    Animation { playlist: u64, generation: u64 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BufferContent {
    pub base: Option<BaseFrame>,
    pub indicator: Option<DamageRect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RenderOutcome {
    pub content: BufferContent,
    pub damage: Option<DamageRect>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_rect_clips_and_unions() {
        let a = DamageRect::clipped(-5, 10, 20, 10, 100, 100).unwrap();
        let b = DamageRect::clipped(10, 15, 20, 20, 100, 100).unwrap();
        assert_eq!(
            a,
            DamageRect {
                x: 0,
                y: 10,
                width: 15,
                height: 10
            }
        );
        assert_eq!(
            a.union(b),
            DamageRect {
                x: 0,
                y: 10,
                width: 30,
                height: 25
            }
        );
    }
}
