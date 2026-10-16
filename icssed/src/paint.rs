//! Painting a box: its outer shadow, background, border, inset shadow
//! and outline. See the crate's ARCHITECTURE.md, "Painting".
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
use crate::style::{
    BackgroundOrigin, BackgroundSize, Computed, Drawn, Layer, LengthPercentage, Nine,
};

/// `color` at `opacity`.
pub(crate) fn fade(color: Color, opacity: f32) -> Color {
    Color {
        a: color.a * opacity,
        ..color
    }
}

fn radius([tl, tr, br, bl]: [f32; 4]) -> Radius {
    Radius {
        top_left: tl,
        top_right: tr,
        bottom_right: br,
        bottom_left: bl,
    }
}

/// A box in its border box `bounds`: its background, its border, then
/// `over` (its children, its outline) over them. A background image is
/// drawn in a layer of its own, and wgpu draws layers in the order
/// they're made, over what's already in the layer around them: what
/// comes after it goes in a layer made after it, clipped to `viewport`
/// (a new layer's clip isn't the one around it), or it's drawn under
/// the picture.
pub(crate) fn draw_box(
    renderer: &mut Renderer,
    s: &Computed,
    bounds: Rectangle,
    viewport: Rectangle,
    opacity: f32,
    over: impl FnOnce(&mut Renderer),
) {
    let w = s.border_width;
    let widths = [w.top, w.right, w.bottom, w.left];
    let colors = s.border_color.map(|c| fade(c, opacity));
    let shows: Vec<usize> = (0..4)
        .filter(|&i| widths[i] > 0.0 && colors[i].a > 0.0)
        .collect();
    let alike = shows.iter().all(|&i| colors[i] == colors[shows[0]]);
    let uniform =
        !shows.is_empty() && shows.len() == 4 && alike && widths.iter().all(|x| *x == widths[0]);
    let mut shadow = s.shadow.map_or(Shadow::default(), |s| Shadow {
        color: fade(s.color, opacity),
        offset: Vector::new(s.x, s.y),
        blur_radius: s.blur,
    });
    let background = fade(s.background, opacity);
    let corners = s.radii(bounds.size());
    // A spread shadow is cast by a box of its own, the box shrunk: iced
    // draws a shadow only outside its quad, so this one goes under the
    // box (seen through a translucent background, unlike CSS).
    if let Some(cast) = s.shadow.filter(|c| c.spread < 0.0 && shadow.color.a > 0.0) {
        let by = -cast.spread;
        renderer.fill_quad(
            Quad {
                bounds: Rectangle {
                    x: bounds.x + by,
                    y: bounds.y + by,
                    width: (bounds.width - 2.0 * by).max(0.0),
                    height: (bounds.height - 2.0 * by).max(0.0),
                },
                border: Border {
                    radius: radius(corners.map(|r| (r - by).max(0.0))),
                    ..Border::default()
                },
                shadow,
                snap: true,
            },
            Color::TRANSPARENT,
        );
        shadow = Shadow::default();
    }
    // A border image replaces the border; the background's images go
    // between the background colour and the border.
    let border_in_quad = uniform && s.border_image.is_none() && s.backgrounds.is_empty();

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
    // The border, or the 9-patch in its place, and the inset shadow, then
    // what goes over the box.
    let edges = |renderer: &mut Renderer| {
        if let Some(nine) = &s.border_image {
            draw_nine(renderer, nine, bounds, widths, opacity, None);
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
        over(renderer);
    };
    if s.backgrounds.is_empty() {
        edges(renderer);
    } else {
        // Clipped by a layer too: tiny-skia ignores an image's own
        // clip. To what shows of the box: a new layer's clip isn't the
        // one around it (a scroll's), and a box scrolled out of sight
        // would show its pictures. The last layer first, the first on
        // top.
        if let Some(clip) = bounds.intersection(&viewport) {
            renderer.with_layer(clip, |renderer| {
                for layer in s.backgrounds.iter().rev() {
                    draw_background(renderer, s, layer, bounds, corners, opacity);
                }
            });
        }
        renderer.with_layer(viewport, |renderer| {
            // TEMPORARY, until iced draws a layer's images in order:
            // a layer that starts with images is merged into the one
            // before it (same clip), and there iced_wgpu 0.14 draws
            // every nearest-filtered image (an svg) before every
            // linear one (this picture), whatever came first. A quad
            // first keeps the layers apart. See ARCHITECTURE.md and
            // https://github.com/iced-rs/iced/issues/3512.
            renderer.fill_quad(Quad::default(), Color::TRANSPARENT);
            edges(renderer);
        });
    }
}

/// A background layer: sized and placed in its origin's box (the
/// padding box, or the border box), tiled as its `background-repeat`
/// says, each tile its 9-patch when it's sliced, clipped to the border
/// box and its corners (iced's image shader rounds at the clip).
fn draw_background(
    renderer: &mut Renderer,
    s: &Computed,
    layer: &Layer,
    bounds: Rectangle,
    corners: [f32; 4],
    opacity: f32,
) {
    let area = match layer.origin {
        BackgroundOrigin::BorderBox => bounds,
        BackgroundOrigin::PaddingBox => {
            let w = s.border_width;
            Rectangle {
                x: bounds.x + w.left,
                y: bounds.y + w.top,
                width: (bounds.width - w.left - w.right).max(0.0),
                height: (bounds.height - w.top - w.bottom).max(0.0),
            }
        }
    };
    let drawn = layer.picture.drawn();
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
    let size = match layer.size {
        BackgroundSize::Auto => own,
        BackgroundSize::Cover => own * (area.width / own.width).max(area.height / own.height),
        BackgroundSize::Contain => own * (area.width / own.width).min(area.height / own.height),
        BackgroundSize::Length(w, h) => {
            let (w, h) = (w.map(|w| w.of(area.width)), h.map(|h| h.of(area.height)));
            match (w, h) {
                (Some(w), Some(h)) => Size::new(w, h),
                (Some(w), None) => Size::new(w, w * own.height / own.width),
                (None, Some(h)) => Size::new(h * own.width / own.height, h),
                (None, None) => own,
            }
        }
    };
    if size.width < 0.5 || size.height < 0.5 {
        return;
    }
    // A percentage is of the room left, as in CSS: 100% puts the
    // image's end at the area's.
    let place =
        |origin: LengthPercentage, start: f32, room: f32, size: f32| start + origin.of(room - size);
    let (x, y) = (
        place(layer.position.0, area.x, area.width, size.width),
        place(layer.position.1, area.y, area.height, size.height),
    );
    // The tiles from the first one reaching into the border box.
    let first = |at: f32, start: f32, step: f32, repeat: bool| {
        if repeat {
            at - ((at - start) / step).ceil() * step
        } else {
            at
        }
    };
    let repeat = layer.repeat;
    let (x0, y0) = (
        first(x, bounds.x, size.width, repeat.x),
        first(y, bounds.y, size.height, repeat.y),
    );
    const MOST: usize = 4096;
    let mut tiles = 0;
    let mut ty = y0;
    while ty < bounds.y + bounds.height && tiles < MOST {
        let mut tx = x0;
        while tx < bounds.x + bounds.width && tiles < MOST {
            let tile = Rectangle::new(Point::new(tx, ty), size);
            match (&layer.nine, &drawn) {
                (Some(nine), _) => {
                    // Its corners at their own size, smaller only where
                    // the tile can't hold two of them.
                    let [top, right, bottom, left] = nine.slices.map(|s| s as f32);
                    let fit = |side: f32, sum: f32| if sum > side { side / sum } else { 1.0 };
                    let f = fit(size.width, left + right).min(fit(size.height, top + bottom));
                    let widths = [top, right, bottom, left].map(|w| w * f);
                    draw_nine(
                        renderer,
                        nine,
                        tile,
                        widths,
                        opacity,
                        Some((bounds, corners)),
                    );
                }
                (None, Drawn::Raster { handle, .. }) => {
                    let mut image = image::Image::new(handle.clone());
                    image.opacity = opacity;
                    image.border_radius = radius(corners);
                    renderer.draw_image(image, tile, bounds);
                }
                (None, Drawn::Svg(handle)) => renderer.draw_svg(
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
            if !repeat.x {
                break;
            }
            tx += size.width;
        }
        if !repeat.y {
            break;
        }
        ty += size.height;
    }
}

/// A 9-patch over `bounds`: the corners at `widths` (top, right,
/// bottom, left; a border's), the edges and the middle stretched. Each
/// part clipped to its own region, or to `clip` and its corners (a
/// background's border box).
fn draw_nine(
    renderer: &mut Renderer,
    nine: &Nine,
    bounds: Rectangle,
    [wt, wr, wb, wl]: [f32; 4],
    opacity: f32,
    clip: Option<(Rectangle, [f32; 4])>,
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
            match clip {
                Some((clip, corners)) => {
                    image.border_radius = radius(corners);
                    renderer.draw_image(image, region, clip);
                }
                None => renderer.draw_image(image, region, region),
            }
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
    let corners = s
        .radii(bounds.size())
        .map(|r| if r > 0.0 { (r + grow).max(0.0) } else { 0.0 });
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

/// What `-debug: box` paints over an element: its margin box, border
/// box, padding box and content box (`boxes`, each inside the one
/// before), each ring in the colour browsers' inspectors give it.
pub(crate) fn draw_debug(renderer: &mut Renderer, boxes: [Rectangle; 4]) {
    const COLORS: [Color; 4] = [
        Color::from_rgba8(246, 178, 107, 0.66),
        Color::from_rgba8(255, 229, 153, 0.66),
        Color::from_rgba8(147, 196, 125, 0.55),
        Color::from_rgba8(111, 168, 220, 0.66),
    ];
    for (i, color) in COLORS.into_iter().enumerate().take(3) {
        let (outer, inner) = (boxes[i], boxes[i + 1]);
        let widths = [
            inner.y - outer.y,
            outer.x + outer.width - inner.x - inner.width,
            outer.y + outer.height - inner.y - inner.height,
            inner.x - outer.x,
        ];
        for side in sides(outer, widths.map(|w| w.max(0.0))) {
            if side.width > 0.0 && side.height > 0.0 {
                fill(renderer, side, color);
            }
        }
    }
    fill(renderer, boxes[3], COLORS[3]);
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
