//! Painting a box: its outer shadow, background, border, inset shadow
//! and outline. See TOOLKIT.md, "Painting".
//!
//! A border whose sides are all alike is a quad's; one whose sides
//! differ is drawn as paths (the canvas's geometry, from inside the
//! widget): a ring when the sides that show share a colour, a region per
//! side otherwise, split at the corners along their diagonal as CSS
//! does. Inner corners are ellipses: a radius less each side's width.

use iced::advanced::graphics::geometry::{Renderer as _, frame::Backend as _};
use iced::advanced::image::{self, Renderer as _};
use iced::advanced::renderer::{Quad, Renderer as _};
use iced::advanced::svg::{self, Renderer as _};
use iced::border::Radius;
use iced::widget::canvas::{Fill, Path, Style, fill::Rule, path::Builder};
use iced::{Border, Color, Point, Radians, Rectangle, Shadow, Size, Vector};

use crate::Renderer;
use crate::style::{BackgroundSize, Computed, Drawn, Nine, Origin, Picture};

/// `color` at `opacity`.
pub(crate) fn fade(color: Color, opacity: f32) -> Color {
    Color {
        a: color.a * opacity,
        ..color
    }
}

/// A box's corner radii, scaled down as CSS does where they'd overlap:
/// top-left, top-right, bottom-right, bottom-left.
fn radii(s: &Computed, size: Size) -> [f32; 4] {
    let [tl, tr, br, bl] = s.border_radius;
    let fit = |side: f32, sum: f32| if sum > side { side / sum } else { 1.0 };
    let f = fit(size.width, tl + tr)
        .min(fit(size.width, bl + br))
        .min(fit(size.height, tl + bl))
        .min(fit(size.height, tr + br));
    [tl * f, tr * f, br * f, bl * f]
}

fn radius([tl, tr, br, bl]: [f32; 4]) -> Radius {
    Radius {
        top_left: tl,
        top_right: tr,
        bottom_right: br,
        bottom_left: bl,
    }
}

/// What a box draws under its children, in its border box `bounds`.
pub(crate) fn draw_box(renderer: &mut Renderer, s: &Computed, bounds: Rectangle, opacity: f32) {
    let w = s.border_width;
    let widths = [w.top, w.right, w.bottom, w.left];
    let colors = s.border_color.map(|c| fade(c, opacity));
    let shows: Vec<usize> = (0..4)
        .filter(|&i| widths[i] > 0.0 && colors[i].a > 0.0)
        .collect();
    let alike = shows.iter().all(|&i| colors[i] == colors[shows[0]]);
    let uniform =
        !shows.is_empty() && shows.len() == 4 && alike && widths.iter().all(|x| *x == widths[0]);
    let shadow = s.shadow.map_or(Shadow::default(), |s| Shadow {
        color: fade(s.color, opacity),
        offset: Vector::new(s.x, s.y),
        blur_radius: s.blur,
    });
    let background = fade(s.background, opacity);
    let corners = radii(s, bounds.size());
    // A border image replaces the border; a background image goes
    // between the background colour and the border.
    let border_in_quad = uniform && s.border_image.is_none() && s.background_image.is_none();

    if background.a > 0.0 || shadow.color.a > 0.0 || border_in_quad {
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    color: if border_in_quad {
                        colors[0]
                    } else {
                        Color::TRANSPARENT
                    },
                    width: if border_in_quad { widths[0] } else { 0.0 },
                    radius: radius(corners),
                },
                shadow,
                snap: true,
            },
            background,
        );
    }
    if let Some(picture) = &s.background_image {
        draw_background(renderer, s, picture, bounds, corners, opacity);
    }
    if let Some(nine) = &s.border_image {
        draw_nine(renderer, nine, bounds, widths, opacity);
    } else if uniform && !border_in_quad {
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    color: colors[0],
                    width: widths[0],
                    radius: radius(corners),
                },
                shadow: Shadow::default(),
                snap: true,
            },
            Color::TRANSPARENT,
        );
    } else if !shows.is_empty() && !uniform {
        let square = corners.iter().all(|r| *r == 0.0);
        if square && alike {
            // Square and of one colour: four quads, no paths.
            for (i, side) in sides(bounds, widths).into_iter().enumerate() {
                if shows.contains(&i) {
                    fill(renderer, side, colors[i]);
                }
            }
        } else {
            draw_sides(renderer, bounds, widths, corners, colors, &shows, alike);
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
        let x = inset.x.clamp(-inner.width, inner.width);
        let y = inset.y.clamp(-inner.height, inner.height);
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

/// A background image: sized and placed in the padding box, tiled as
/// `background-repeat` says, clipped to the border box and its corners
/// (iced's image shader rounds at the clip).
fn draw_background(
    renderer: &mut Renderer,
    s: &Computed,
    picture: &Picture,
    bounds: Rectangle,
    corners: [f32; 4],
    opacity: f32,
) {
    let w = s.border_width;
    let area = Rectangle {
        x: bounds.x + w.left,
        y: bounds.y + w.top,
        width: (bounds.width - w.left - w.right).max(0.0),
        height: (bounds.height - w.top - w.bottom).max(0.0),
    };
    let drawn = picture.drawn();
    let own = match &drawn {
        Drawn::Raster { width, height, .. } => Size::new(*width as f32, *height as f32),
        Drawn::Svg(handle) => {
            let size = renderer.measure_svg(handle);
            Size::new(size.width as f32, size.height as f32)
        }
    };
    if own.width <= 0.0 || own.height <= 0.0 {
        return;
    }
    let size = match s.background_size {
        BackgroundSize::Auto => own,
        BackgroundSize::Cover => own * (area.width / own.width).max(area.height / own.height),
        BackgroundSize::Contain => own * (area.width / own.width).min(area.height / own.height),
        BackgroundSize::Px(Some(w), Some(h)) => Size::new(w, h),
        BackgroundSize::Px(Some(w), None) => Size::new(w, w * own.height / own.width),
        BackgroundSize::Px(None, Some(h)) => Size::new(h * own.width / own.height, h),
        BackgroundSize::Px(None, None) => own,
    };
    if size.width < 0.5 || size.height < 0.5 {
        return;
    }
    let place = |origin: Origin, start: f32, room: f32, size: f32| match origin {
        Origin::Fraction(f) => start + (room - size) * f,
        Origin::Px(v) => start + v,
    };
    let (x, y) = (
        place(s.background_position.0, area.x, area.width, size.width),
        place(s.background_position.1, area.y, area.height, size.height),
    );
    // The tiles from the first one reaching into the border box.
    let first = |at: f32, start: f32, step: f32, repeat: bool| {
        if repeat {
            at - ((at - start) / step).ceil() * step
        } else {
            at
        }
    };
    let (x0, y0) = (
        first(x, bounds.x, size.width, s.background_repeat.x),
        first(y, bounds.y, size.height, s.background_repeat.y),
    );
    // Clipped by a layer too: tiny-skia ignores an image's own clip.
    renderer.with_layer(bounds, |renderer| {
        tile(
            renderer,
            s,
            &drawn,
            (x0, y0),
            size,
            bounds,
            corners,
            opacity,
        );
    });
}

/// The tiles of a background image, from `(x0, y0)` over `bounds`.
#[allow(clippy::too_many_arguments)]
fn tile(
    renderer: &mut Renderer,
    s: &Computed,
    drawn: &Drawn,
    (x0, y0): (f32, f32),
    size: Size,
    bounds: Rectangle,
    corners: [f32; 4],
    opacity: f32,
) {
    const MOST: usize = 4096;
    let mut tiles = 0;
    let mut ty = y0;
    while ty < bounds.y + bounds.height && tiles < MOST {
        let mut tx = x0;
        while tx < bounds.x + bounds.width && tiles < MOST {
            let tile = Rectangle::new(Point::new(tx, ty), size);
            match drawn {
                Drawn::Raster { handle, .. } => {
                    let mut image = image::Image::new(handle.clone());
                    image.opacity = opacity;
                    image.border_radius = radius(corners);
                    renderer.draw_image(image, tile, bounds);
                }
                Drawn::Svg(handle) => renderer.draw_svg(
                    svg::Svg {
                        handle: handle.clone(),
                        color: None,
                        rotation: Radians(0.0),
                        opacity,
                    },
                    tile,
                    bounds,
                ),
            }
            tiles += 1;
            if !s.background_repeat.x {
                break;
            }
            tx += size.width;
        }
        if !s.background_repeat.y {
            break;
        }
        ty += size.height;
    }
}

/// A 9-patch in place of the border: the corners at the border's widths,
/// the edges and the middle stretched.
fn draw_nine(
    renderer: &mut Renderer,
    nine: &Nine,
    bounds: Rectangle,
    [wt, wr, wb, wl]: [f32; 4],
    opacity: f32,
) {
    let (x0, y0) = (bounds.x, bounds.y);
    let (x1, y1) = (x0 + bounds.width, y0 + bounds.height);
    let (mw, mh) = (
        (bounds.width - wl - wr).max(0.0),
        (bounds.height - wt - wb).max(0.0),
    );
    let regions = [
        Rectangle::new(Point::new(x0, y0), Size::new(wl, wt)),
        Rectangle::new(Point::new(x0 + wl, y0), Size::new(mw, wt)),
        Rectangle::new(Point::new(x1 - wr, y0), Size::new(wr, wt)),
        Rectangle::new(Point::new(x0, y0 + wt), Size::new(wl, mh)),
        Rectangle::new(Point::new(x0 + wl, y0 + wt), Size::new(mw, mh)),
        Rectangle::new(Point::new(x1 - wr, y0 + wt), Size::new(wr, mh)),
        Rectangle::new(Point::new(x0, y1 - wb), Size::new(wl, wb)),
        Rectangle::new(Point::new(x0 + wl, y1 - wb), Size::new(mw, wb)),
        Rectangle::new(Point::new(x1 - wr, y1 - wb), Size::new(wr, wb)),
    ];
    for (part, region) in nine.parts.iter().zip(regions) {
        if let Some(handle) = part
            && region.width > 0.0
            && region.height > 0.0
        {
            let mut image = image::Image::new(handle.clone());
            image.opacity = opacity;
            renderer.draw_image(image, region, region);
        }
    }
}

/// What a box draws over its children: its outline, around its border
/// box `bounds`, taking no room.
pub(crate) fn draw_outline(renderer: &mut Renderer, s: &Computed, bounds: Rectangle, opacity: f32) {
    let width = s.outline_width;
    let color = fade(s.outline_color, opacity);
    if width <= 0.0 || color.a <= 0.0 {
        return;
    }
    let grow = s.outline_offset + width;
    let outer = bounds.expand(grow);
    if outer.width <= 0.0 || outer.height <= 0.0 {
        return;
    }
    // It follows the border's corners, as browsers draw it.
    let corners = radii(s, bounds.size()).map(|r| if r > 0.0 { (r + grow).max(0.0) } else { 0.0 });
    renderer.fill_quad(
        Quad {
            bounds: outer,
            border: Border {
                color,
                width,
                radius: radius(corners),
            },
            shadow: Shadow::default(),
            snap: true,
        },
        Color::TRANSPARENT,
    );
}

/// The four sides of a square border, top and bottom whole.
fn sides(b: Rectangle, [top, right, bottom, left]: [f32; 4]) -> [Rectangle; 4] {
    let middle = (b.height - top - bottom).max(0.0);
    [
        Rectangle { height: top, ..b },
        Rectangle {
            x: b.x + b.width - right,
            y: b.y + top,
            width: right,
            height: middle,
        },
        Rectangle {
            y: b.y + b.height - bottom,
            height: bottom,
            ..b
        },
        Rectangle {
            y: b.y + top,
            width: left,
            height: middle,
            ..b
        },
    ]
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

/// One corner of a border: its outer arc, its inner one, the angles
/// they span (degrees, clockwise from +x, unwrapped so that the corners
/// follow each other) and where its two sides meet.
struct Corner {
    outer: (Point, f32, f32),
    inner: (Point, f32, f32),
    start: f32,
    end: f32,
    split: f32,
}

fn draw_sides(
    renderer: &mut Renderer,
    bounds: Rectangle,
    [wt, wr, wb, wl]: [f32; 4],
    [rtl, rtr, rbr, rbl]: [f32; 4],
    colors: [Color; 4],
    shows: &[usize],
    alike: bool,
) {
    let size = bounds.size();
    let (x0, y0, x1, y1) = (0.0, 0.0, size.width, size.height);
    let (ix0, iy0, ix1, iy1) = (wl, wt, x1 - wr, y1 - wb);
    let inner = |r: f32, wx: f32, wy: f32| ((r - wx).max(0.0), (r - wy).max(0.0));
    let deg = |y: f32, x: f32| y.atan2(x).to_degrees();
    let (tl, tr, br, bl) = (
        inner(rtl, wl, wt),
        inner(rtr, wr, wt),
        inner(rbr, wr, wb),
        inner(rbl, wl, wb),
    );
    let corners = [
        Corner {
            outer: (Point::new(x0 + rtl, y0 + rtl), rtl, rtl),
            inner: (Point::new(ix0 + tl.0, iy0 + tl.1), tl.0, tl.1),
            start: 180.0,
            end: 270.0,
            split: 270.0 - deg(wt, wl),
        },
        Corner {
            outer: (Point::new(x1 - rtr, y0 + rtr), rtr, rtr),
            inner: (Point::new(ix1 - tr.0, iy0 + tr.1), tr.0, tr.1),
            start: 270.0,
            end: 360.0,
            split: 270.0 + deg(wt, wr),
        },
        Corner {
            outer: (Point::new(x1 - rbr, y1 - rbr), rbr, rbr),
            inner: (Point::new(ix1 - br.0, iy1 - br.1), br.0, br.1),
            start: 360.0,
            end: 450.0,
            split: 450.0 - deg(wb, wr),
        },
        Corner {
            outer: (Point::new(x0 + rbl, y1 - rbl), rbl, rbl),
            inner: (Point::new(ix0 + bl.0, iy1 - bl.1), bl.0, bl.1),
            start: 450.0,
            end: 540.0,
            split: 450.0 + deg(wb, wl),
        },
    ];

    let mut frame = renderer.new_frame(Rectangle::with_size(size));
    if alike {
        // One ring: the outer outline and the inner one, even-odd.
        let ring = Path::new(|b| {
            contour(b, &corners, |c| c.outer);
            contour(b, &corners, |c| c.inner);
        });
        frame.fill(
            &ring,
            Fill {
                style: Style::Solid(colors[shows[0]]),
                rule: Rule::EvenOdd,
            },
        );
    } else {
        for &side in shows {
            let a = &corners[side];
            let b = &corners[(side + 1) % 4];
            // The left side goes from the bottom-left corner round to the
            // top-left one: one turn further.
            let turn = if side == 3 { 360.0 } else { 0.0 };
            let region = Path::new(|p| {
                p.move_to(at(a.outer, a.split));
                arc(p, a.outer, a.split, a.end);
                p.line_to(at(b.outer, b.start + turn));
                arc(p, b.outer, b.start + turn, b.split + turn);
                p.line_to(at(b.inner, b.split + turn));
                arc(p, b.inner, b.split + turn, b.start + turn);
                p.line_to(at(a.inner, a.end));
                arc(p, a.inner, a.end, a.split);
                p.close();
            });
            frame.fill(
                &region,
                Fill {
                    style: Style::Solid(colors[side]),
                    rule: Rule::NonZero,
                },
            );
        }
    }
    renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
        renderer.draw_geometry(frame.into_geometry());
    });
}

/// A closed outline through every corner's `arc`.
fn contour(b: &mut Builder, corners: &[Corner; 4], arc_of: impl Fn(&Corner) -> (Point, f32, f32)) {
    b.move_to(at(arc_of(&corners[0]), corners[0].start));
    for c in corners {
        b.line_to(at(arc_of(c), c.start));
        arc(b, arc_of(c), c.start, c.end);
    }
    b.close();
}

/// The point at `angle` (degrees) of the ellipse `(center, rx, ry)`.
fn at((center, rx, ry): (Point, f32, f32), angle: f32) -> Point {
    let a = angle.to_radians();
    Point::new(center.x + rx * a.cos(), center.y + ry * a.sin())
}

/// The ellipse's arc from `from` to `to` (degrees, either way), from the
/// current point, as cubic Béziers of a quarter turn at most. iced's own
/// `ellipse` starts a new subpath, which a contour can't have.
fn arc(b: &mut Builder, ellipse: (Point, f32, f32), from: f32, to: f32) {
    let (_, rx, ry) = ellipse;
    if rx <= 0.0 || ry <= 0.0 {
        b.line_to(at(ellipse, to));
        return;
    }
    let pieces = ((to - from).abs() / 90.0).ceil().max(1.0) as usize;
    let step = (to - from) / pieces as f32;
    let tangent = |angle: f32| {
        let a = angle.to_radians();
        Vector::new(-rx * a.sin(), ry * a.cos())
    };
    for i in 0..pieces {
        let a0 = from + step * i as f32;
        let a1 = a0 + step;
        let k = 4.0 / 3.0 * (step.to_radians() / 4.0).tan();
        let (p0, p1) = (at(ellipse, a0), at(ellipse, a1));
        let (t0, t1) = (tangent(a0), tangent(a1));
        b.bezier_curve_to(
            Point::new(p0.x + k * t0.x, p0.y + k * t0.y),
            Point::new(p1.x - k * t1.x, p1.y - k * t1.y),
            p1,
        );
    }
}
