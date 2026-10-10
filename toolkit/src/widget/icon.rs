//! Icons and images: sized by the theme (`width`, `height`; a square of
//! the font size when neither is set), whether or not the file is found
//! yet. A symbolic svg takes the element's `color`, so an ancestor's
//! state (`button:hover`) reaches it.

use std::borrow::Cow;
use std::sync::Arc;

use iced::advanced::image::{self, Renderer as _};
use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::svg::{self, Renderer as _};
use iced::advanced::widget::{Tree, Widget};
use iced::border::Radius;
use iced::{Length, Radians, Rectangle, Size};

use crate::layout::snap;
use crate::paint::fade;
use crate::style::{Computed, Node, Size as Len, States};
use crate::{Element, Renderer, Theme};

/// Where named icons come from: the app's (a freedesktop icon theme,
/// its own files). Asked at draw, for the size and the scale the icon is
/// drawn at, so a png theme's `@2` directory serves a scale-2 output;
/// meant to answer from its own cache.
pub trait Icons {
    /// The icon `name` drawn `size` logical pixels wide at `scale`.
    fn lookup(&self, name: &str, size: f32, scale: f32) -> Option<IconFile>;
}

/// What an icon draws.
#[derive(Debug, Clone)]
pub enum IconFile {
    /// `symbolic`: recoloured by the element's `color`.
    Svg {
        handle: svg::Handle,
        symbolic: bool,
    },
    Image(image::Handle),
}

/// A file, or a name for an [`Icons`] to look up.
pub enum IconSource<'a> {
    File(IconFile),
    Named {
        icons: &'a dyn Icons,
        name: Cow<'a, str>,
    },
}

impl From<IconFile> for IconSource<'_> {
    fn from(file: IconFile) -> Self {
        Self::File(file)
    }
}

impl<'a, S: Into<Cow<'a, str>>> From<(&'a dyn Icons, S)> for IconSource<'a> {
    fn from((icons, name): (&'a dyn Icons, S)) -> Self {
        Self::Named {
            icons,
            name: name.into(),
        }
    }
}

pub struct Icon<'a> {
    node: Node<'a>,
    style: Arc<Computed>,
    source: IconSource<'a>,
}

pub fn icon<'a>(node: Node<'a>, source: impl Into<IconSource<'a>>) -> Icon<'a> {
    Icon {
        style: node.base(),
        node,
        source: source.into(),
    }
}

impl Icon<'_> {
    /// Its size when not `fill`: `width` and `height`, one of them for
    /// both, else the font size.
    fn natural(&self) -> Size {
        let s = &self.style;
        let scale = self.node.scale();
        let px = |len: Len| match len {
            Len::Px(v) => Some(snap(v, scale)),
            _ => None,
        };
        let square = snap(s.font_size, scale);
        match (px(s.width), px(s.height)) {
            (Some(w), Some(h)) => Size::new(w, h),
            (Some(w), None) => Size::new(w, w),
            (None, Some(h)) => Size::new(h, h),
            (None, None) => Size::new(square, square),
        }
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Icon<'_> {
    fn size(&self) -> Size<Length> {
        let natural = self.natural();
        let length = |len: Len, px: f32| match len {
            Len::Fill => Length::Fill,
            _ => Length::Fixed(px),
        };
        Size::new(
            length(self.style.width, natural.width),
            length(self.style.height, natural.height),
        )
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &Limits) -> layout::Node {
        let natural = self.natural();
        let axis = |len: Len, px: f32, min: f32, max: f32| match len {
            Len::Fill => max.max(min),
            // Its size whatever it's offered: it can't shrink.
            _ => px.max(min),
        };
        layout::Node::new(Size::new(
            axis(
                self.style.width,
                natural.width,
                limits.min().width,
                limits.max().width,
            ),
            axis(
                self.style.height,
                natural.height,
                limits.min().height,
                limits.max().height,
            ),
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
        let file = match &self.source {
            IconSource::File(file) => Some(file.clone()),
            IconSource::Named { icons, name } => {
                icons.lookup(name, bounds.width.max(bounds.height), self.node.scale())
            }
        };
        let Some(file) = file else {
            return;
        };
        let s = self.node.resolve(States::NONE, &theme.ancestors);
        let opacity = theme.opacity * s.opacity;
        match file {
            IconFile::Svg { handle, symbolic } => renderer.draw_svg(
                svg::Svg {
                    handle,
                    color: symbolic.then(|| fade(s.color, opacity)),
                    rotation: Radians(0.0),
                    opacity: if symbolic { 1.0 } else { opacity },
                },
                bounds,
                bounds,
            ),
            IconFile::Image(handle) => {
                let mut image = image::Image::new(handle);
                image.opacity = opacity;
                image.border_radius = Radius {
                    top_left: s.border_radius[0],
                    top_right: s.border_radius[1],
                    bottom_right: s.border_radius[2],
                    bottom_left: s.border_radius[3],
                };
                renderer.draw_image(image, bounds, bounds);
            }
        }
    }
}

impl<'a, Message> From<Icon<'a>> for Element<'a, Message> {
    fn from(icon: Icon<'a>) -> Self {
        Element::new(icon)
    }
}
