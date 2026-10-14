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
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::border::Radius;
use iced::{Event, Length, Radians, Rectangle, Size};

use super::place::{Place, placed};
use crate::anim::Animated;
use crate::layout::snap;
use crate::query::Reported;
use crate::style::{Computed, Node, Size as Len, States, name};
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
    place: Place<'a>,
    source: IconSource<'a>,
}

/// `source` in an `icon`.
pub fn icon<'a>(source: impl Into<IconSource<'a>>) -> Icon<'a> {
    Icon {
        place: Place::new(name!("icon")),
        source: source.into(),
    }
}

impl<'a> Icon<'a> {
    placed!(place);

    fn node(&self) -> Node<'a> {
        self.place.node()
    }

    fn style(&self) -> &Arc<Computed> {
        self.place.style()
    }

    /// Its size when not `fill`: `width` and `height`, one of them for
    /// both, else the font size.
    fn natural(&self) -> Size {
        let s = self.style();
        let scale = self.node().scale();
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
        let natural = self.natural();
        let length = |len: Len, px: f32| match len {
            Len::Fill => Length::Fill,
            _ => Length::Fixed(px),
        };
        Size::new(
            length(self.style().width, natural.width),
            length(self.style().height, natural.height),
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
                self.style().width,
                natural.width,
                limits.min().width,
                limits.max().width,
            ),
            axis(
                self.style().height,
                natural.height,
                limits.min().height,
                limits.max().height,
            ),
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
        let file = match &self.source {
            IconSource::File(file) => Some(file.clone()),
            IconSource::Named { icons, name } => {
                icons.lookup(name, bounds.width.max(bounds.height), self.node().scale())
            }
        };
        let Some(file) = file else {
            return;
        };
        let resolved = self.node().resolve(States::NONE, &theme.ancestors);
        let moving = tree
            .state
            .downcast_ref::<Animated>()
            .style(&resolved, theme);
        let s = moving.as_ref().unwrap_or(&resolved);
        let opacity = theme.opacity * s.opacity;
        let rotation = Radians(s.rotate.to_radians());
        match file {
            // iced recolours a symbolic svg's RGB alone, its alpha left
            // out: the colour's alpha goes to the svg's opacity.
            IconFile::Svg { handle, symbolic } => renderer.draw_svg(
                svg::Svg {
                    handle,
                    color: symbolic.then_some(s.color),
                    rotation,
                    opacity: if symbolic {
                        opacity * s.color.a
                    } else {
                        opacity
                    },
                },
                bounds,
                bounds,
            ),
            IconFile::Image(handle) => {
                let mut image = image::Image::new(handle);
                image.opacity = opacity;
                image.rotation = rotation;
                image.border_radius = {
                    let [top_left, top_right, bottom_right, bottom_left] = s.radii(bounds.size());
                    Radius {
                        top_left,
                        top_right,
                        bottom_right,
                        bottom_left,
                    }
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
