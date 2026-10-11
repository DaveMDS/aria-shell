//! An image (cover art, an avatar, a wallpaper's preview): its size the
//! theme's (`width`, `height`), else its own, a pixel a logical pixel,
//! one side given keeping its ratio; filling its box as `object-fit`
//! says, centred, clipped to the box and its corners.

use std::sync::Arc;

use iced::advanced::Renderer as _;
use iced::advanced::image::{self, Renderer as _};
use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::border::Radius;
use iced::{Event, Length, Point, Radians, Rectangle, Size};

use super::place::{Place, placed};
use crate::anim::Animated;
use crate::layout::snap;
use crate::query::Reported;
use crate::style::{Computed, Node, ObjectFit, Size as Len, States, name};
use crate::{Element, Renderer, Theme};

pub struct Image<'a> {
    place: Place<'a>,
    handle: image::Handle,
}

/// `handle`'s picture in an `image`.
pub fn image<'a>(handle: impl Into<image::Handle>) -> Image<'a> {
    Image {
        place: Place::new(name!("image")),
        handle: handle.into(),
    }
}

impl<'a> Image<'a> {
    placed!(place);

    fn node(&self) -> Node<'a> {
        self.place.node()
    }

    fn style(&self) -> &Arc<Computed> {
        self.place.style()
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
        operation.custom(None, layout.bounds(), &mut Reported::new(self.node()));
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Animated>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Animated::new(self.node()))
    }

    fn diff(&self, tree: &mut Tree) {
        tree.state.downcast_mut::<Animated>().diff(self.node());
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        tree.state.downcast_mut::<Animated>().update(event, shell);
    }

    fn size(&self) -> Size<Length> {
        let length = |len: Len| match len {
            Len::Auto => Length::Shrink,
            Len::Px(v) => Length::Fixed(v),
            Len::Fill => Length::Fill,
        };
        Size::new(length(self.style().width), length(self.style().height))
    }

    fn layout(&mut self, _tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let s = self.style();
        let scale = self.node().scale();
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
        tree: &Tree,
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
        let resolved = self.node().resolve(States::NONE, &theme.ancestors);
        let moving = tree
            .state
            .downcast_ref::<Animated>()
            .style(&resolved, theme);
        let s = moving.as_ref().unwrap_or(&resolved);
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
        image.rotation = Radians(s.rotate.to_radians());
        image.border_radius = Radius {
            top_left: s.border_radius[0],
            top_right: s.border_radius[1],
            bottom_right: s.border_radius[2],
            bottom_left: s.border_radius[3],
        };
        if bounds.contains(drawn.position())
            && bounds.contains(Point::new(drawn.x + drawn.width, drawn.y + drawn.height))
        {
            renderer.draw_image(image, drawn, bounds);
        } else {
            // Larger than its box (`cover`, `none`): clipped by a layer,
            // which every renderer honours (tiny-skia ignores an image's
            // own clip).
            renderer.with_layer(bounds, |renderer| renderer.draw_image(image, drawn, bounds));
        }
    }
}

impl<'a, Message> From<Image<'a>> for Element<'a, Message> {
    fn from(image: Image<'a>) -> Self {
        Element::new(image)
    }
}
