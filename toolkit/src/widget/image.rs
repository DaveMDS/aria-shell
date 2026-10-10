//! An image (cover art, an avatar, a wallpaper's preview): its size the
//! theme's (`width`, `height`), else its own, a pixel a logical pixel,
//! one side given keeping its ratio; filling its box as `object-fit`
//! says, centred, clipped to the box and its corners.

use std::sync::Arc;

use iced::advanced::image::{self, Renderer as _};
use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::widget::{Tree, Widget};
use iced::border::Radius;
use iced::{Length, Point, Rectangle, Size};

use crate::layout::snap;
use crate::style::{Computed, Node, ObjectFit, Size as Len, States};
use crate::{Element, Renderer, Theme};

pub struct Image<'a> {
    node: Node<'a>,
    style: Arc<Computed>,
    handle: image::Handle,
}

pub fn image<'a>(node: Node<'a>, handle: impl Into<image::Handle>) -> Image<'a> {
    Image {
        style: node.base(),
        node,
        handle: handle.into(),
    }
}

impl Image<'_> {
    fn own(&self, renderer: &Renderer) -> Size {
        renderer
            .measure_image(&self.handle)
            .map_or(Size::ZERO, |s| Size::new(s.width as f32, s.height as f32))
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Image<'_> {
    fn size(&self) -> Size<Length> {
        let length = |len: Len| match len {
            Len::Auto => Length::Shrink,
            Len::Px(v) => Length::Fixed(v),
            Len::Fill => Length::Fill,
        };
        Size::new(length(self.style.width), length(self.style.height))
    }

    fn layout(&mut self, _tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let s = &self.style;
        let scale = self.node.scale();
        let own = self.own(renderer);
        let ratio = if own.height > 0.0 {
            own.width / own.height
        } else {
            1.0
        };
        let given = |len: Len, max: f32, compressed: bool| match len {
            Len::Px(v) => Some(snap(v, scale)),
            Len::Fill if !compressed => Some(max),
            _ => None,
        };
        let compressed = limits.compression();
        let width = given(s.width, limits.max().width, compressed.width);
        let height = given(s.height, limits.max().height, compressed.height);
        let size = match (width, height) {
            (Some(w), Some(h)) => Size::new(w, h),
            (Some(w), None) => Size::new(w, snap(w / ratio, scale)),
            (None, Some(h)) => Size::new(snap(h * ratio, scale), h),
            (None, None) => Size::new(snap(own.width, scale), snap(own.height, scale)),
        };
        layout::Node::new(Size::new(
            size.width.max(limits.min().width),
            size.height.max(limits.min().height),
        ))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let own = self.own(renderer);
        if own.width <= 0.0 || own.height <= 0.0 {
            return;
        }
        let s = self.node.resolve(States::NONE, &theme.ancestors);
        let fit = |f: fn(f32, f32) -> f32| f(bounds.width / own.width, bounds.height / own.height);
        let size = match s.object_fit {
            ObjectFit::Fill => bounds.size(),
            ObjectFit::Contain => own * fit(f32::min),
            ObjectFit::Cover => own * fit(f32::max),
            ObjectFit::None => own,
            ObjectFit::ScaleDown => own * fit(f32::min).min(1.0),
        };
        let drawn = Rectangle::new(
            Point::new(
                bounds.x + (bounds.width - size.width) / 2.0,
                bounds.y + (bounds.height - size.height) / 2.0,
            ),
            size,
        );
        let mut image = image::Image::new(self.handle.clone());
        image.opacity = theme.opacity * s.opacity;
        image.border_radius = Radius {
            top_left: s.border_radius[0],
            top_right: s.border_radius[1],
            bottom_right: s.border_radius[2],
            bottom_left: s.border_radius[3],
        };
        renderer.draw_image(image, drawn, bounds);
    }
}

impl<'a, Message> From<Image<'a>> for Element<'a, Message> {
    fn from(image: Image<'a>) -> Self {
        Element::new(image)
    }
}
