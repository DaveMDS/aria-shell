//! A value on or off: a button whose press, or Enter or Space while it
//! has the focus, asks for the other value; `:checked` while it's on.
//! Without `on_toggle` it shows its value and nothing more, as a button
//! without `on_press`: no press, no focus, a click going to what it's in
//! (a menu item's checkbox).
//!
//! One widget, four names for the theme to tell its looks apart: a
//! `toggle` (a button that stays pressed), a `checkbox`, a `radio`, a
//! `switch`. Each is the same tree, the look the theme's alone:
//!
//! ```text
//! toggle | checkbox | radio | switch [:checked]
//! ├─ indicator      the box, the circle, the switch's track
//! │  ╰─ mark        the tick, the dot, the switch's knob
//! ├─ icon           if it has one
//! ╰─ label          if it has one
//! ```
//!
//! A checkbox's mark is an icon, the toolkit's tick, a symbolic svg: it
//! takes its `color` and its size from the sheet, as any icon (a theme
//! drawing its own makes it `transparent`). The others' is an empty
//! box. The base hides a toggle's indicator (`display: none`) and slides
//! a switch's knob with a transform, by the indicator's width less its
//! padding and the knob's.
//!
//! A radio is a toggle that chooses: one among a few is on, a press on
//! any asks for it (`toggle(p == chosen).on_toggle(move |_| Choose(p))`), on the
//! chosen one too, a message that changes nothing. The pressed button
//! does the same, the radio's group as buttons side by side.

use std::borrow::Cow;
use std::sync::LazyLock;

use iced::advanced::svg;

use super::flex::{Flex, row};
use super::icon::{IconFile, IconSource, icon};
use super::place::{Place, placed};
use super::text::text;
use crate::Element;
use crate::style::{Direction, Name, name};

/// How a toggle looks.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Look {
    Button,
    Checkbox,
    Radio,
    Switch,
}

impl Look {
    fn name(self) -> Name {
        match self {
            Look::Button => name!("toggle"),
            Look::Checkbox => name!("checkbox"),
            Look::Radio => name!("radio"),
            Look::Switch => name!("switch"),
        }
    }
}

pub struct Toggle<'a, Message> {
    place: Place<'a>,
    on: bool,
    /// What a press sends: the other value's message.
    press: Option<Message>,
    look: Look,
    icon: Option<IconSource<'a>>,
    label: Option<Cow<'a, str>>,
}

/// A `toggle` on or off: a button that stays pressed, unless
/// `checkbox`, `radio` or `switch` says otherwise; pressed once given
/// [`Toggle::on_toggle`].
pub fn toggle<'a, Message: Clone + 'a>(on: bool) -> Toggle<'a, Message> {
    Toggle {
        place: Place::new(name!("toggle")),
        on,
        press: None,
        look: Look::Button,
        icon: None,
        label: None,
    }
}

impl<'a, Message> Toggle<'a, Message> {
    placed!(place);

    /// What a press sends, made from the value it asks for: the other.
    pub fn on_toggle(mut self, on_toggle: impl Fn(bool) -> Message) -> Self {
        self.press = Some(on_toggle(!self.on));
        self
    }

    /// A check box: a `checkbox`.
    pub fn checkbox(self) -> Self {
        self.look(Look::Checkbox)
    }

    /// A radio: a `radio`.
    pub fn radio(self) -> Self {
        self.look(Look::Radio)
    }

    /// A switch: a `switch`.
    pub fn switch(self) -> Self {
        self.look(Look::Switch)
    }

    fn look(mut self, look: Look) -> Self {
        self.look = look;
        self.place.kind(look.name());
        self
    }

    /// Its `icon`.
    pub fn icon(mut self, source: impl Into<IconSource<'a>>) -> Self {
        self.icon = Some(source.into());
        self
    }

    /// Its `label`.
    pub fn label(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl<'a, Message: Clone + 'a> From<Toggle<'a, Message>> for Element<'a, Message> {
    fn from(toggle: Toggle<'a, Message>) -> Self {
        let mut place = toggle.place;
        if toggle.on {
            place.checked();
        }
        let icon = toggle.icon.map(|source| icon(source).into());
        let label = toggle.label.map(|label| text(label).into());
        let flex = Flex::placed(
            place,
            Direction::Row,
            std::iter::once(indicator(toggle.look))
                .chain(icon)
                .chain(label),
        );
        match toggle.press {
            Some(press) => flex.on_press(press).into(),
            None => flex.into(),
        }
    }
}

/// `indicator > mark`, the mark a checkbox's tick.
fn indicator<'a, Message: Clone + 'a>(look: Look) -> Element<'a, Message> {
    let mark: Element<'a, Message> = match look {
        Look::Checkbox => icon(tick()).kind_of(name!("mark")).into(),
        _ => row([]).kind_of(name!("mark")).into(),
    };
    row([mark]).kind_of(name!("indicator")).into()
}

/// A check box's mark: a tick, stroked in a 16 px square.
static TICK: LazyLock<svg::Handle> = LazyLock::new(|| {
    svg::Handle::from_memory(
        br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-6.5" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##
            .as_slice(),
    )
});

/// The toolkit's tick, a checkbox's mark.
fn tick() -> IconFile {
    IconFile::Svg {
        handle: TICK.clone(),
        symbolic: true,
    }
}
