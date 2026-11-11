//! Geometry of `ui-slider`, shared by painting and pointer input so a press
//! lands where the thumb is drawn.
use crate::layout::Rect;

/// A round thumb two thirds as tall as the content box travels along a thin
/// track; the whole box stays the press target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub track: Rect,
    /// Thumb diameter.
    pub knob: u32,
    /// Thumb centre at value zero.
    pub start: i32,
    /// Centre travel from zero to 10000.
    pub travel: u32,
}

/// None when the content box has no room to move a thumb.
pub fn geometry(content: Rect) -> Option<Geometry> {
    let knob = (content.height * 2 / 3).min(content.width);
    if knob < 4 || content.width <= knob {
        return None;
    }
    let thickness = (knob / 3).max(2);
    Some(Geometry {
        track: Rect {
            x: content.x,
            y: content.y + ((content.height - thickness) / 2) as i32,
            width: content.width,
            height: thickness,
        },
        knob,
        start: content.x + (knob / 2) as i32,
        travel: content.width - knob,
    })
}

impl Geometry {
    /// Thumb centre for a value in basis points (clamped to 10000).
    pub fn centre(&self, value: u16) -> i32 {
        self.start + (u64::from(self.travel) * u64::from(value.min(10000)) / 10000) as i32
    }
    /// The thumb box for a value.
    pub fn thumb(&self, value: u16) -> Rect {
        let y = self.track.y + (self.track.height as i32 - self.knob as i32) / 2;
        Rect {
            x: self.centre(value) - (self.knob / 2) as i32,
            y,
            width: self.knob,
            height: self.knob,
        }
    }
    /// The value whose thumb centre is nearest to `x`, clamped to the track.
    pub fn value(&self, x: i32) -> u16 {
        let travel = i64::from(self.travel.max(1));
        let at = (i64::from(x) - i64::from(self.start)).clamp(0, travel);
        ((at * 10000 + travel / 2) / travel) as u16
    }
}
