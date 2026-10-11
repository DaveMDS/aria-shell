//! A value on or off: a button whose press, or Enter or Space while it
//! has the focus, asks for the other value; `.checked` while it's on. It
//! looks one of three ways: a button that stays pressed, a check box
//! (`.check`), a switch (`.switch`), the last two with the toolkit's part
//! before its icon and its label. It makes all its nodes itself, as a
//! menu's items do, so its classes reach them all. The switch's knob
//! slides as the theme says: the base moves it with a transform, by the
//! switch's width less its padding and the knob's.

use std::borrow::Cow;

use super::flex::{button, row};
use super::icon::{IconSource, icon};
use super::text::{mark, text};
use crate::Element;
use crate::style::Node;

/// How a toggle looks.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Look {
    Button,
    Check,
    Switch,
}

pub struct Toggle<'a, Message> {
    node: Node<'a>,
    on: bool,
    /// What a press sends: the other value's message.
    press: Message,
    look: Look,
    icon: Option<IconSource<'a>>,
    label: Option<Cow<'a, str>>,
}

/// `node` on or off: a button that stays pressed, unless `check` or
/// `switch` says otherwise. `on_toggle` makes what a press sends from the
/// value it asks for.
pub fn toggle<'a, Message: Clone + 'a>(
    node: Node<'a>,
    on: bool,
    on_toggle: impl Fn(bool) -> Message,
) -> Toggle<'a, Message> {
    Toggle {
        node,
        on,
        press: on_toggle(!on),
        look: Look::Button,
        icon: None,
        label: None,
    }
}

impl<'a, Message> Toggle<'a, Message> {
    /// A check box: `.check`, its `check > mark` first.
    pub fn check(mut self) -> Self {
        self.look = Look::Check;
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
        let mut node = toggle.node.class_if("checked", toggle.on);
        let part: Option<Element<'a, Message>> = match toggle.look {
            Look::Button => None,
            Look::Check => {
                node = node.class("check");
                let check = node.child("check");
                Some(row(check, [mark(check.child("mark"))]).into())
            }
            Look::Switch => {
                node = node.class("switch");
                let switch = node.child("switch");
                Some(row(switch, [row(switch.child("knob"), []).into()]).into())
            }
        };
        let icon = toggle
            .icon
            .map(|source| icon(node.child("icon"), source).into());
        let label = toggle
            .label
            .map(|label| text(node.child("label"), label).into());
        button(node, part.into_iter().chain(icon).chain(label))
            .on_press(toggle.press)
            .into()
    }
}
