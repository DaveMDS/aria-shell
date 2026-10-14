//! Icons: boxes sized by the theme (`width`, `height`, their border box
//! as every box's; a square of the font size when neither is set, its
//! padding and border around; one side `fill` and the other not given,
//! that one following it, a square picture), whether or not the file
//! is found yet, the picture in their content box. A symbolic svg takes the element's
//! `color`, so an ancestor's state (`button:hover`) reaches it.

use std::borrow::Cow;

use iced::advanced::image::{self, Renderer as _};
use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::svg::{self, Renderer as _};
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::{Event, Length, Radians, Rectangle, Size, Vector};

use super::boxed::{BoxState, Boxed, bare, border_box, frame, inner_radius};
use super::place::{Place, placed};
use crate::layout::{content_box, snap};
use crate::paint::fade;
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
    boxed: Boxed<'a>,
    source: IconSource<'a>,
}

/// `source` in an `icon`.
pub fn icon<'a>(source: impl Into<IconSource<'a>>) -> Icon<'a> {
    Icon {
        boxed: Boxed::placed(Place::new(name!("icon"))),
        source: source.into(),
    }
}

impl<'a> Icon<'a> {
    placed!(boxed.place);

    fn node(&self) -> Node<'a> {
        self.boxed.node()
    }

    /// Its border box when not `fill`: `width` and `height`; one of them
    /// alone makes the picture square; neither, a square of the font
    /// size with its padding and border around.
    fn natural(&self, s: &Computed) -> Size {
        let scale = self.node().scale();
        let px = |len: Len| match len {
            Len::Px(v) => Some(snap(v, scale)),
            _ => None,
        };
        let frame = frame(s, scale);
        let square = snap(s.font_size, scale);
        match (px(s.width), px(s.height)) {
            (Some(w), Some(h)) => Size::new(w, h),
            (Some(w), None) => Size::new(w, (w - frame.width).max(0.0) + frame.height),
            (None, Some(h)) => Size::new((h - frame.height).max(0.0) + frame.width, h),
            (None, None) => Size::new(square + frame.width, square + frame.height),
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
        let s = self.boxed.style();
        let natural = self.natural(s);
        let m = s.margin;
        // A side following a `fill` one is known only in layout.
        let follows = |len: Len, other: Len| len == Len::Auto && other == Len::Fill;
        let length = |len: Len, other: Len, px: f32, margins: f32| match len {
            Len::Fill => Length::Fill,
            _ if follows(len, other) => Length::Shrink,
            _ => Length::Fixed(px + margins),
        };
        Size::new(
            length(s.width, s.height, natural.width, m.horizontal()),
            length(s.height, s.width, natural.height, m.vertical()),
        )
    }

    fn layout(&mut self, tree: &mut Tree, _renderer: &Renderer, limits: &Limits) -> layout::Node {
        let s = self
            .boxed
            .layout_style(tree.state.downcast_ref::<BoxState>().states);
        let scale = self.node().scale();
        let natural = self.natural(&s);
        let m = s.margin;
        let sn = |v: f32| snap(v, scale);
        let axis = |len: Len, px: f32, min: f32, max: f32| match len {
            Len::Fill => max.max(min),
            // Its size whatever it's offered: it can't shrink.
            _ => px.max(min),
        };
        let margins = Size::new(sn(m.left) + sn(m.right), sn(m.top) + sn(m.bottom));
        let mut size = Size::new(
            axis(
                s.width,
                natural.width + margins.width,
                limits.min().width,
                limits.max().width,
            ),
            axis(
                s.height,
                natural.height + margins.height,
                limits.min().height,
                limits.max().height,
            ),
        );
        // One side `fill`, the other not given: the picture square, as
        // big as the `fill` side makes it (a dock's icons as tall as the
        // bar).
        let frame = frame(&s, scale);
        let square = |side: f32, margins: f32, frame: f32| (side - margins - frame).max(0.0);
        match (s.width, s.height) {
            (Len::Auto, Len::Fill) => {
                let picture = square(size.height, margins.height, frame.height);
                size.width = (picture + frame.width + margins.width).max(limits.min().width);
            }
            (Len::Fill, Len::Auto) => {
                let picture = square(size.width, margins.width, frame.width);
                size.height = (picture + frame.height + margins.height).max(limits.min().height);
            }
            _ => {}
        }
        layout::Node::new(size)
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
        let scale = self.node().scale();
        let state = tree.state.downcast_ref::<BoxState>();
        // The picture in the content box, at `opacity` (the box's in it),
        // a symbolic one in `color`.
        let picture = |renderer: &mut Renderer, opacity: f32, color: iced::Color, s: &Computed| {
            let content = content_box(bounds.size(), s, scale) + Vector::new(bounds.x, bounds.y);
            let file = match &self.source {
                IconSource::File(file) => Some(file.clone()),
                IconSource::Named { icons, name } => {
                    icons.lookup(name, content.width.max(content.height), scale)
                }
            };
            let Some(file) = file else {
                return;
            };
            let rotation = Radians(s.rotate.to_radians());
            match file {
                // iced recolours a symbolic svg's RGB alone, its
                // alpha left out: the colour's alpha (the box's
                // opacity in it) goes to the svg's opacity.
                IconFile::Svg { handle, symbolic } => renderer.draw_svg(
                    svg::Svg {
                        handle,
                        color: symbolic.then_some(color),
                        rotation,
                        opacity: if symbolic { color.a } else { opacity },
                    },
                    content,
                    content,
                ),
                IconFile::Image(handle) => {
                    let mut image = image::Image::new(handle);
                    image.opacity = opacity;
                    image.rotation = rotation;
                    image.border_radius = inner_radius(s, border_box(bounds, s, scale), scale);
                    renderer.draw_image(image, content, content);
                }
            }
        };
        // Most icons paint no box: their picture alone, at no box's cost.
        let resolved = self.node().resolve(state.states, &theme.ancestors);
        let moving = state.animated.style(&resolved, theme);
        let s = moving.as_ref().unwrap_or(&resolved);
        if bare(s) {
            state.drawn_untransformed();
            let opacity = theme.opacity * s.opacity;
            picture(renderer, opacity, fade(s.color, opacity), s);
            return;
        }
        self.boxed.draw(
            state,
            renderer,
            theme,
            bounds,
            *viewport,
            cursor,
            // The box's opacity is the theme's by now.
            |renderer, theme, style, _, s| picture(renderer, theme.opacity, style.text_color, s),
        );
    }
}

impl<'a, Message> From<Icon<'a>> for Element<'a, Message> {
    fn from(icon: Icon<'a>) -> Self {
        Element::new(icon)
    }
}
