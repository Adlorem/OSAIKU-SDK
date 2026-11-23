//! Geometry of `ui-switch`: a pill-shaped track with a round knob as tall as
//! the content box, at the left end when off and the right end when on. The
//! knob overhangs the track, which is a little thinner, as on a phone.
use crate::layout::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub track: Rect,
    /// The knob when off and when on.
    pub off: Rect,
    pub on: Rect,
}

/// None when the content box is too small to draw a switch in.
pub fn geometry(content: Rect) -> Option<Geometry> {
    // The knob keeps at least two fifths of the width to travel.
    let knob = content.height.min(content.width * 3 / 5);
    if knob < 6 {
        return None;
    }
    // A track about seven tenths of the knob, its ends under the knob.
    let thickness = (knob * 7 / 10).max(4);
    let inset = (knob - thickness) / 2;
    let y = content.y + ((content.height - knob) / 2) as i32;
    let off = Rect {
        x: content.x,
        y,
        width: knob,
        height: knob,
    };
    Some(Geometry {
        track: Rect {
            x: content.x + inset as i32,
            y: content.y + ((content.height - thickness) / 2) as i32,
            width: content.width - 2 * inset,
            height: thickness,
        },
        off,
        on: Rect {
            x: content.right() - knob as i32,
            ..off
        },
    })
}

impl Geometry {
    pub fn knob(&self, on: bool) -> Rect {
        if on {
            self.on
        } else {
            self.off
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_knob_sits_at_either_end_over_a_thinner_track() {
        let g = geometry(Rect {
            x: 10,
            y: 20,
            width: 44,
            height: 24,
        })
        .unwrap();
        assert_eq!(
            g.off,
            Rect {
                x: 10,
                y: 20,
                width: 24,
                height: 24
            }
        );
        assert_eq!(
            g.on,
            Rect {
                x: 30,
                y: 20,
                width: 24,
                height: 24
            }
        );
        assert_eq!(g.track.height, 16);
        assert_eq!((g.track.x, g.track.right()), (14, 50));
        assert_eq!(g.track.y + g.track.height as i32 / 2, 32);
        // A tall, narrow box keeps a round knob and room to move it.
        let g = geometry(Rect {
            x: 0,
            y: 0,
            width: 30,
            height: 40,
        })
        .unwrap();
        assert_eq!(
            (g.off.width, g.off.height, g.on.x, g.on.right()),
            (18, 18, 12, 30)
        );
        assert!(geometry(Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 4
        })
        .is_none());
    }
}
