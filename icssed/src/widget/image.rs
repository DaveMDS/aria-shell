//! An image (cover art, an avatar, a wallpaper's preview): a box, its
//! size the theme's (`width`, `height`, its border box as every box's),
//! else its picture's, a pixel a logical pixel, its padding and border
//! around; one side given keeping the picture's ratio. The picture
//! fills the content box as `object-fit` says, centred, clipped to it
//! and its corners.

use iced::advanced::Renderer as _;
use iced::advanced::image::{self, Renderer as _};
use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::{Event, Length, Point, Radians, Rectangle, Size, Vector};

use super::boxed::{BoxState, Boxed, border_box, frame, inner_radius};
use super::place::{Place, placed};
use crate::layout::{content_box, snap};
use crate::query::Reported;
use crate::style::{Node, ObjectFit, Size as Len, States, name};
use crate::{Element, Renderer, Theme};

pub struct Image<'a> {
    boxed: Boxed<'a>,
    handle: image::Handle,
}

/// `handle`'s picture in an `image`.
pub fn image<'a>(handle: impl Into<image::Handle>) -> Image<'a> {
    Image {
        boxed: Boxed::placed(Place::new(name!("image"))),
        handle: handle.into(),
    }
}

impl<'a> Image<'a> {
    placed!(boxed.place);

    fn node(&self) -> Node<'a> {
        self.boxed.node()
    }

    fn own(&self, renderer: &Renderer) -> Size {
        renderer
            .measure_image(&self.handle)
            .map_or(Size::ZERO, |s| Size::new(s.width as f32, s.height as f32))
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Image<'_> {
    fn operate(
        &mut self,
        _tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let bounds = self.boxed.border_box(layout.bounds());
        operation.custom(None, bounds, &mut Reported::new(self.node()));
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<BoxState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(self.boxed.state())
    }

    fn diff(&self, tree: &mut Tree) {
        tree.state
            .downcast_mut::<BoxState>()
            .animated
            .diff(self.node());
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<BoxState>();
        state.animated.update(event, shell);
        let before = state.states;
        let over = state
            .local(cursor)
            .is_over(self.boxed.border_box(layout.bounds()));
        let tracked = self.boxed.tracked(States::NONE);
        self.boxed.settle(state, before, tracked, over, shell);
    }

    fn size(&self) -> Size<Length> {
        self.boxed.size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let s = self
            .boxed
            .layout_style(tree.state.downcast_ref::<BoxState>().states);
        let scale = self.node().scale();
        let own = self.own(renderer);
        let ratio = if own.height > 0.0 {
            own.width / own.height
        } else {
            1.0
        };
        let m = s.margin;
        let sn = |v: f32| snap(v, scale);
        let margins = Size::new(sn(m.left) + sn(m.right), sn(m.top) + sn(m.bottom));
        let frame = frame(&s, scale);
        // Its border box: given, filling what's offered, or its
        // picture's with the frame around.
        let given = |len: Len, max: f32, margins: f32, compressed: bool| match len {
            Len::Px(v) => Some(snap(v, scale)),
            Len::Fill if !compressed => Some(max - margins),
            _ => None,
        };
        let compressed = limits.compression();
        let width = given(s.width, limits.max().width, margins.width, compressed.width);
        let height = given(
            s.height,
            limits.max().height,
            margins.height,
            compressed.height,
        );
        // The picture's side from the other, keeping its ratio.
        let across = |side: f32, inner: f32, outer: f32, by: f32| {
            snap((side - inner).max(0.0) * by, scale) + outer
        };
        let size = match (width, height) {
            (Some(w), Some(h)) => Size::new(w, h),
            (Some(w), None) => Size::new(w, across(w, frame.width, frame.height, 1.0 / ratio)),
            (None, Some(h)) => Size::new(across(h, frame.height, frame.width, ratio), h),
            (None, None) => Size::new(
                snap(own.width, scale) + frame.width,
                snap(own.height, scale) + frame.height,
            ),
        };
        layout::Node::new(Size::new(
            (size.width + margins.width).max(limits.min().width),
            (size.height + margins.height).max(limits.min().height),
        ))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let own = self.own(renderer);
        let scale = self.node().scale();
        let state = tree.state.downcast_ref::<BoxState>();
        self.boxed.draw(
            state,
            renderer,
            theme,
            bounds,
            *viewport,
            cursor,
            |renderer, theme, _, _, s| {
                if own.width <= 0.0 || own.height <= 0.0 {
                    return;
                }
                let area = content_box(bounds.size(), s, scale) + Vector::new(bounds.x, bounds.y);
                let fit =
                    |f: fn(f32, f32) -> f32| f(area.width / own.width, area.height / own.height);
                let size = match s.object_fit {
                    ObjectFit::Fill => area.size(),
                    ObjectFit::Contain => own * fit(f32::min),
                    ObjectFit::Cover => own * fit(f32::max),
                    ObjectFit::None => own,
                    ObjectFit::ScaleDown => own * fit(f32::min).min(1.0),
                };
                let drawn = Rectangle::new(
                    Point::new(
                        area.x + (area.width - size.width) / 2.0,
                        area.y + (area.height - size.height) / 2.0,
                    ),
                    size,
                );
                let mut image = image::Image::new(self.handle.clone());
                // The box's opacity is the theme's by now.
                image.opacity = theme.opacity;
                image.rotation = Radians(s.rotate.to_radians());
                image.border_radius = inner_radius(s, border_box(bounds, s, scale), scale);
                if area.contains(drawn.position())
                    && area.contains(Point::new(drawn.x + drawn.width, drawn.y + drawn.height))
                {
                    renderer.draw_image(image, drawn, area);
                } else {
                    // Larger than its box (`cover`, `none`): clipped by a
                    // layer, which every renderer honours (tiny-skia
                    // ignores an image's own clip).
                    renderer.with_layer(area, |renderer| renderer.draw_image(image, drawn, area));
                }
            },
        );
    }
}

impl<'a, Message> From<Image<'a>> for Element<'a, Message> {
    fn from(image: Image<'a>) -> Self {
        Element::new(image)
    }
}
