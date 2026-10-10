//! The toolkit's iced `Theme`: not a palette, the draw-time context a
//! parent hands its children (TOOLKIT.md, "The draw-time context").

use std::time::Instant;

use iced::Color;
use iced::theme::{Base, Mode, Palette, Style};

use crate::style::{Ancestors, States};

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
        "tk"
    }
}
