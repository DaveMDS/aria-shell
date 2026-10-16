//! The toolkit's iced `Theme`: not a palette, the draw-time context a
//! parent hands its children (the crate's ARCHITECTURE.md, "The
//! draw-time context"); and the `Catalog`s of the stock widgets the
//! toolkit embeds, which read their styles from the sheet through it.

use std::cell::Cell;
use std::time::Instant;

use iced::theme::{Base, Mode, Palette, Style};
use iced::widget::{container, scrollable, text_input};
use iced::{Background, Border, Color, Shadow};

use crate::paint::fade;
use crate::style::{Ancestors, Node, States};

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// The opacity so far, multiplied down the tree.
    pub opacity: f32,
    /// The states of the ancestors not at rest.
    pub ancestors: Ancestors,
    /// The last instant an ancestor's states changed.
    pub changed: Option<Instant>,
    /// Whether an element drawn so far with it painted `-debug: box`:
    /// a box's children tell it so, and it paints none of its own.
    pub(crate) debugged: Cell<bool>,
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
            debugged: Cell::new(false),
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

/// iced's scrollable, as `icssed::scroll` embeds it: it scrolls, its
/// bars never drawn (the scroll draws its own, boxes).
impl scrollable::Catalog for Theme {
    type Class<'a> = ();

    fn default<'a>() -> Self::Class<'a> {}

    fn style(&self, _class: &Self::Class<'_>, _status: scrollable::Status) -> scrollable::Style {
        let hidden = scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
            },
        };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: hidden,
            horizontal_rail: hidden,
            gap: None,
            // Its middle-click scrolling is never on.
            auto_scroll: scrollable::AutoScroll {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                shadow: Shadow::default(),
                icon: Color::TRANSPARENT,
            },
        }
    }
}
