//! A box's border. When its four sides look the same iced draws it,
//! rounded corners and all; when they differ it's four rectangles: top
//! and bottom across the whole width, left and right between them, so
//! none overlaps another (a translucent colour stays even at the
//! corners). Where two shown sides meet, the corner is the top's or the
//! bottom's, not split on the diagonal as CSS does.

use iced::{Color, Rectangle};

use super::super::{BorderSide, Sides};

/// The one side all four are, when they look the same (a side that
/// shows nothing is the same as any other that doesn't).
pub fn uniform(sides: &Sides<BorderSide>) -> Option<BorderSide> {
    let [a, b, c, d] = sides.each().map(|s| shown(s).unwrap_or_default());
    (a == b && b == c && c == d).then_some(a)
}

/// The rectangles of a border whose sides differ, in `bounds`, each
/// with its colour. A side wider than the box is cut to it.
pub fn rects(
    bounds: Rectangle,
    sides: &Sides<BorderSide>,
) -> impl Iterator<Item = (Rectangle, Color)> {
    let width = |s: &BorderSide| shown(s).map_or(0.0, |s| s.width);
    let top = width(&sides.top).min(bounds.height);
    let bottom = width(&sides.bottom).min(bounds.height - top);
    let left = width(&sides.left).min(bounds.width);
    let right = width(&sides.right).min(bounds.width - left);
    let middle = bounds.height - top - bottom;
    [
        (sides.top, bounds.x, bounds.y, bounds.width, top),
        (
            sides.bottom,
            bounds.x,
            bounds.y + bounds.height - bottom,
            bounds.width,
            bottom,
        ),
        (sides.left, bounds.x, bounds.y + top, left, middle),
        (
            sides.right,
            bounds.x + bounds.width - right,
            bounds.y + top,
            right,
            middle,
        ),
    ]
    .into_iter()
    .filter(|&(_, _, _, width, height)| width > 0.0 && height > 0.0)
    .filter_map(|(side, x, y, width, height)| {
        Some((
            Rectangle {
                x,
                y,
                width,
                height,
            },
            side.color?,
        ))
    })
}

fn shown(side: &BorderSide) -> Option<BorderSide> {
    side.shows().then_some(*side)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::from_rgb(1.0, 0.0, 0.0);
    const BLUE: Color = Color::from_rgb(0.0, 0.0, 1.0);

    fn side(width: f32, color: Color) -> BorderSide {
        BorderSide {
            width,
            color: Some(color),
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn uniform_sides() {
        assert_eq!(uniform(&Sides::all(side(1.0, RED))), Some(side(1.0, RED)));
        // Sides that show nothing are alike, whatever their colour.
        let none = Sides {
            top: side(0.0, RED),
            right: side(2.0, Color::TRANSPARENT),
            bottom: BorderSide::default(),
            left: side(0.0, BLUE),
        };
        assert_eq!(uniform(&none), Some(BorderSide::default()));
        let bottom = Sides {
            bottom: side(3.0, RED),
            ..Sides::default()
        };
        assert_eq!(uniform(&bottom), None);
        let colours = Sides {
            left: side(1.0, BLUE),
            ..Sides::all(side(1.0, RED))
        };
        assert_eq!(uniform(&colours), None);
    }

    #[test]
    fn one_side() {
        let bottom = Sides {
            bottom: side(3.0, RED),
            ..Sides::default()
        };
        let rects: Vec<_> = rects(rect(10.0, 20.0, 100.0, 30.0), &bottom).collect();
        assert_eq!(rects, [(rect(10.0, 47.0, 100.0, 3.0), RED)]);
    }

    #[test]
    fn four_sides_do_not_overlap() {
        let sides = Sides {
            top: side(1.0, RED),
            right: side(2.0, BLUE),
            bottom: side(3.0, RED),
            left: side(4.0, BLUE),
        };
        let rects: Vec<_> = rects(rect(0.0, 0.0, 100.0, 30.0), &sides).collect();
        assert_eq!(
            rects,
            [
                (rect(0.0, 0.0, 100.0, 1.0), RED),
                (rect(0.0, 27.0, 100.0, 3.0), RED),
                (rect(0.0, 1.0, 4.0, 26.0), BLUE),
                (rect(98.0, 1.0, 2.0, 26.0), BLUE),
            ]
        );
    }

    #[test]
    fn sides_are_cut_to_the_box() {
        let sides = Sides {
            top: side(8.0, RED),
            bottom: side(8.0, BLUE),
            left: side(1.0, RED),
            ..Sides::default()
        };
        let rects: Vec<_> = rects(rect(0.0, 0.0, 50.0, 10.0), &sides).collect();
        assert_eq!(
            rects,
            [
                (rect(0.0, 0.0, 50.0, 8.0), RED),
                (rect(0.0, 8.0, 50.0, 2.0), BLUE),
            ]
        );
    }
}
