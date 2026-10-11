//! A value on or off: a button whose press, or Enter or Space while it
//! has the focus, asks for the other value; `.checked` while it's on. It
//! looks one of four ways: a button that stays pressed, a check box
//! (`.check`), a radio (`.radio`), a switch (`.switch`), the last three
//! with the toolkit's part before its icon and its label. Its classes
//! reach them all, whenever they're given. The switch's knob slides as
//! the theme says: the base moves it with a transform, by the switch's
//! width less its padding and the knob's. A check box's mark is the
//! toolkit's own drawing, a symbolic svg: it takes its `color` and its
//! size from the sheet, as any icon, and shows as its `opacity` says.
//!
//! A radio is a toggle that chooses: one among a few is on, a press on
//! any asks for it (`toggle(p == chosen, move |_| Choose(p))`), on the
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
use crate::style::{Direction, name};

/// How a toggle looks.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Look {
    Button,
    Check,
    Radio,
    Switch,
}

pub struct Toggle<'a, Message> {
    place: Place<'a>,
    on: bool,
    /// What a press sends: the other value's message.
    press: Message,
    look: Look,
    icon: Option<IconSource<'a>>,
    label: Option<Cow<'a, str>>,
}

/// A `toggle` on or off: a button that stays pressed, unless `check`,
/// `radio` or `switch` says otherwise. `on_toggle` makes what a press sends from the
/// value it asks for.
pub fn toggle<'a, Message: Clone + 'a>(
    on: bool,
    on_toggle: impl Fn(bool) -> Message,
) -> Toggle<'a, Message> {
    Toggle {
        place: Place::new(name!("toggle")),
        on,
        press: on_toggle(!on),
        look: Look::Button,
        icon: None,
        label: None,
    }
}

impl<'a, Message> Toggle<'a, Message> {
    placed!(place);

    /// A check box: `.check`, its `check > mark` first.
    pub fn check(mut self) -> Self {
        self.look = Look::Check;
        self
    }

    /// A radio: `.radio`, its `radio > dot` first.
    pub fn radio(mut self) -> Self {
        self.look = Look::Radio;
        self
    }

    /// A switch: `.switch`, its `switch > knob` first.
    pub fn switch(mut self) -> Self {
        self.look = Look::Switch;
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
            place.class("checked");
        }
        let part: Option<Element<'a, Message>> = match toggle.look {
            Look::Button => None,
            Look::Check => {
                place.class("check");
                Some(check_part())
            }
            Look::Radio => {
                place.class("radio");
                Some(radio_part())
            }
            Look::Switch => {
                place.class("switch");
                Some(
                    row([row([]).kind_of(name!("knob")).into()])
                        .kind_of(name!("switch"))
                        .into(),
                )
            }
        };
        let icon = toggle.icon.map(|source| icon(source).into());
        let label = toggle.label.map(|label| text(label).into());
        Flex::placed(
            place,
            Direction::Row,
            part.into_iter().chain(icon).chain(label),
        )
        .on_press(toggle.press)
        .into()
    }
}

/// A check box's mark: a tick, stroked in a 16 px square.
static TICK: LazyLock<svg::Handle> = LazyLock::new(|| {
    svg::Handle::from_memory(
        br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-6.5" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##
            .as_slice(),
    )
});

/// A check box's part, a toggle's or a menu item's: `check > mark`, the
/// mark an icon.
pub(super) fn check_part<'a, Message: Clone + 'a>() -> Element<'a, Message> {
    let tick = IconFile::Svg {
        handle: TICK.clone(),
        symbolic: true,
    };
    row([icon(tick).kind_of(name!("mark")).into()])
        .kind_of(name!("check"))
        .into()
}

/// A radio's part, a toggle's or a menu item's: `radio > dot`.
pub(super) fn radio_part<'a, Message: Clone + 'a>() -> Element<'a, Message> {
    row([row([]).kind_of(name!("dot")).into()])
        .kind_of(name!("radio"))
        .into()
}
