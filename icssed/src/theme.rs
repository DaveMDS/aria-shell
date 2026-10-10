//! The toolkit's iced `Theme`: not a palette, the draw-time context a
//! parent hands its children (the crate's ARCHITECTURE.md, "The
//! draw-time context"); and the `Catalog`s of the stock widgets the
//! toolkit embeds, which read their styles from the sheet through it.

use std::time::Instant;

use iced::border::Radius;
use iced::theme::{Base, Mode, Palette, Style};
use iced::widget::{container, scrollable, text_input};
use iced::{Background, Border, Color, Shadow};

use crate::paint::fade;
use crate::style::{Ancestors, Computed, Node, States};

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// The opacity so far, multiplied down the tree.
    pub opacity: f32,
    /// The states of the ancestors not at rest.
    pub ancestors: Ancestors,
    /// The last instant an ancestor's states changed.
    pub changed: Option<Instant>,
    /// What iced clears a window with.
    background: Color,
    /// iced's default text colour.
    text: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            ancestors: Ancestors::default(),
            changed: None,
            background: Color::TRANSPARENT,
            text: Color::BLACK,
        }
    }
}

impl Theme {
    /// A window's: cleared with `background` (a surface's root style
    /// gives it), `text` for what iced draws on its own.
    pub fn window(background: Color, text: Color) -> Self {
        Self {
            background,
            text,
            ..<Self as Default>::default()
        }
    }
}

impl Theme {
    /// The same, under the element at `depth` in `states`, which last
    /// changed at `since`.
    pub fn under(&self, depth: u16, states: States, since: Option<Instant>) -> Self {
        Self {
            ancestors: self.ancestors.push(depth, states),
            changed: self.changed.max(since),
            ..self.clone()
        }
    }

    /// The same, its subtree at `opacity`.
    pub fn with_opacity(&self, opacity: f32) -> Self {
        Self {
            opacity,
            ..self.clone()
        }
    }
}

impl Base for Theme {
    fn default(_: Mode) -> Self {
        <Self as Default>::default()
    }

    fn mode(&self) -> Mode {
        Mode::None
    }

    fn base(&self) -> Style {
        Style {
            background_color: self.background,
            text_color: self.text,
        }
    }

    fn palette(&self) -> Option<Palette> {
        None
    }

    fn name(&self) -> &str {
        "icssed"
    }
}

/// What a text input's colours come from: its field's node, and the
/// nodes of its placeholder and selection.
#[derive(Clone, Copy)]
pub struct FieldClass<'a> {
    pub(crate) field: Node<'a>,
    pub(crate) placeholder: Node<'a>,
    pub(crate) selection: Node<'a>,
}

impl<'a> FieldClass<'a> {
    pub(crate) fn new(field: Node<'a>) -> Self {
        Self {
            field,
            placeholder: field.child("placeholder"),
            selection: field.child("selection"),
        }
    }
}

/// iced's text input, as `icssed::input` embeds it: no box of its own (the
/// input's is), its colours its nodes', under the states of its
/// ancestors. Without a class (an iced text input used directly), black
/// on nothing.
impl text_input::Catalog for Theme {
    type Class<'a> = Option<FieldClass<'a>>;

    fn default<'a>() -> Self::Class<'a> {
        None
    }

    fn style(&self, class: &Self::Class<'_>, _status: text_input::Status) -> text_input::Style {
        let color = |node: Node<'_>, background: bool| {
            let s = node.resolve(States::NONE, &self.ancestors);
            let color = if background { s.background } else { s.color };
            fade(color, self.opacity * s.opacity)
        };
        let (value, placeholder, selection) = match class {
            Some(class) => (
                color(class.field, false),
                color(class.placeholder, false),
                color(class.selection, true),
            ),
            None => (Color::BLACK, Color::BLACK, Color::TRANSPARENT),
        };
        text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            icon: value,
            placeholder,
            value,
            selection,
        }
    }
}

/// What a scroll's scrollbars look like: its node, and its bars'.
#[derive(Clone, Copy)]
pub struct ScrollClass<'a> {
    pub(crate) scroll: Node<'a>,
    pub(crate) vertical: Node<'a>,
    pub(crate) horizontal: Node<'a>,
    vertical_thumb: Node<'a>,
    horizontal_thumb: Node<'a>,
}

impl<'a> ScrollClass<'a> {
    pub(crate) fn new(scroll: Node<'a>) -> Self {
        let vertical = scroll.child("scrollbar").class("vertical");
        let horizontal = scroll.child("scrollbar").class("horizontal");
        Self {
            scroll,
            vertical,
            horizontal,
            vertical_thumb: vertical.child("thumb"),
            horizontal_thumb: horizontal.child("thumb"),
        }
    }
}

/// A uniform border as iced draws one: the top side's.
fn border(s: &Computed, opacity: f32) -> Border {
    let [top_left, top_right, bottom_right, bottom_left] = s.border_radius;
    Border {
        color: fade(s.border_color[0], opacity),
        width: s.border_width.top,
        radius: Radius {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        },
    }
}

/// iced's scrollable, as `icssed::scroll` embeds it: no box of its own, its
/// scrollbars and thumbs their nodes', `:hover` and `:active` as iced
/// reports them; the scroll `:hover` under the pointer. Without a class
/// (an iced scrollable used directly), bars that aren't drawn.
impl scrollable::Catalog for Theme {
    type Class<'a> = Option<ScrollClass<'a>>;

    fn default<'a>() -> Self::Class<'a> {
        None
    }

    fn style(&self, class: &Self::Class<'_>, status: scrollable::Status) -> scrollable::Style {
        let hidden = scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
            },
        };
        // Its middle-click scrolling is never on.
        let auto_scroll = scrollable::AutoScroll {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            shadow: Shadow::default(),
            icon: Color::TRANSPARENT,
        };
        let Some(class) = class else {
            return scrollable::Style {
                container: container::Style::default(),
                vertical_rail: hidden,
                horizontal_rail: hidden,
                gap: None,
                auto_scroll,
            };
        };
        let on = |hovered: bool, dragged: bool| {
            States::NONE
                .with(States::HOVER, hovered)
                .with(States::ACTIVE, dragged)
        };
        let (scroll, vertical, horizontal) = match status {
            scrollable::Status::Active { .. } => (States::NONE, States::NONE, States::NONE),
            scrollable::Status::Hovered {
                is_horizontal_scrollbar_hovered,
                is_vertical_scrollbar_hovered,
                ..
            } => (
                States::HOVER,
                on(is_vertical_scrollbar_hovered, false),
                on(is_horizontal_scrollbar_hovered, false),
            ),
            scrollable::Status::Dragged {
                is_horizontal_scrollbar_dragged,
                is_vertical_scrollbar_dragged,
                ..
            } => (
                States::HOVER,
                on(false, is_vertical_scrollbar_dragged),
                on(false, is_horizontal_scrollbar_dragged),
            ),
        };
        let under = self.ancestors.push(class.scroll.depth(), scroll);
        let rail = |bar: Node<'_>, thumb: Node<'_>, states: States| {
            let s = bar.resolve(states, &under);
            let opacity = self.opacity * s.opacity;
            let t = thumb.resolve(States::NONE, &under.push(bar.depth(), states));
            let thumb_opacity = opacity * t.opacity;
            scrollable::Rail {
                background: Some(Background::Color(fade(s.background, opacity))),
                border: border(&s, opacity),
                scroller: scrollable::Scroller {
                    background: Background::Color(fade(t.background, thumb_opacity)),
                    border: border(&t, thumb_opacity),
                },
            }
        };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail(class.vertical, class.vertical_thumb, vertical),
            horizontal_rail: rail(class.horizontal, class.horizontal_thumb, horizontal),
            gap: None,
            auto_scroll,
        }
    }
}
