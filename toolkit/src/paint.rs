//! Painting a box: its outer shadow, background, border and inset
//! shadow. See TOOLKIT.md, "Painting".

use iced::advanced::renderer::{Quad, Renderer as _};
use iced::border::Radius;
use iced::{Border, Color, Rectangle, Shadow, Vector};

use crate::Renderer;
use crate::style::Computed;

/// `color` at `opacity`.
pub(crate) fn fade(color: Color, opacity: f32) -> Color {
    Color {
        a: color.a * opacity,
        ..color
    }
}

/// What a box draws under its children, in its border box `bounds`.
pub(crate) fn draw_box(renderer: &mut Renderer, s: &Computed, bounds: Rectangle, opacity: f32) {
    let w = s.border_width;
    let c = s.border_color;
    let uniform =
        w.top == w.right && w.top == w.bottom && w.top == w.left && c.iter().all(|x| *x == c[0]);
    let border_shows = [w.top, w.right, w.bottom, w.left]
        .iter()
        .zip(c)
        .any(|(w, c)| *w > 0.0 && c.a > 0.0);
    let shadow = s.shadow.map_or(Shadow::default(), |s| Shadow {
        color: fade(s.color, opacity),
        offset: Vector::new(s.x, s.y),
        blur_radius: s.blur,
    });
    let background = fade(s.background, opacity);
    let radius = Radius {
        top_left: s.border_radius[0],
        top_right: s.border_radius[1],
        bottom_right: s.border_radius[2],
        bottom_left: s.border_radius[3],
    };

    let whole = uniform && border_shows;
    if background.a > 0.0 || shadow.color.a > 0.0 || whole {
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    color: if whole {
                        fade(c[0], opacity)
                    } else {
                        Color::TRANSPARENT
                    },
                    width: if whole { w.top } else { 0.0 },
                    radius,
                },
                shadow,
                snap: true,
            },
            background,
        );
    }
    if border_shows && !uniform {
        // Square: where two sides meet, the corner is the top's or the
        // bottom's.
        let sides = [
            (
                Rectangle {
                    height: w.top,
                    ..bounds
                },
                c[0],
            ),
            (
                Rectangle {
                    y: bounds.y + bounds.height - w.bottom,
                    height: w.bottom,
                    ..bounds
                },
                c[2],
            ),
            (
                Rectangle {
                    y: bounds.y + w.top,
                    width: w.left,
                    height: bounds.height - w.top - w.bottom,
                    ..bounds
                },
                c[3],
            ),
            (
                Rectangle {
                    x: bounds.x + bounds.width - w.right,
                    y: bounds.y + w.top,
                    width: w.right,
                    height: bounds.height - w.top - w.bottom,
                },
                c[1],
            ),
        ];
        for (side, color) in sides {
            if side.width > 0.0 && side.height > 0.0 && color.a > 0.0 {
                fill(renderer, side, fade(color, opacity));
            }
        }
    }
    if let Some(inset) = s.inset_shadow {
        // Bands inside the border, along the sides the offset points
        // away from.
        let inner = Rectangle {
            x: bounds.x + w.left,
            y: bounds.y + w.top,
            width: (bounds.width - w.left - w.right).max(0.0),
            height: (bounds.height - w.top - w.bottom).max(0.0),
        };
        let color = fade(inset.color, opacity);
        let (x, y) = (inset.x.min(inner.width), inset.y.min(inner.height));
        let (x, y) = (x.max(-inner.width), y.max(-inner.height));
        if y > 0.0 {
            fill(renderer, Rectangle { height: y, ..inner }, color);
        } else if y < 0.0 {
            fill(
                renderer,
                Rectangle {
                    y: inner.y + inner.height + y,
                    height: -y,
                    ..inner
                },
                color,
            );
        }
        if x > 0.0 {
            fill(renderer, Rectangle { width: x, ..inner }, color);
        } else if x < 0.0 {
            fill(
                renderer,
                Rectangle {
                    x: inner.x + inner.width + x,
                    width: -x,
                    ..inner
                },
                color,
            );
        }
    }
}

fn fill(renderer: &mut Renderer, bounds: Rectangle, color: Color) {
    renderer.fill_quad(
        Quad {
            bounds,
            snap: true,
            ..Quad::default()
        },
        color,
    );
}
