//! Tabs: a row of buttons, one per page, the page shown `.active`; the
//! row's box draws the line under them, each tab its own mark. Each tab
//! carries the message that shows its page. Left and Right move the
//! focus among the tabs once one has it, Enter and Space press it.

use std::borrow::Cow;

use super::flex::{Flex, button, row};
use super::focus::{Keys, arrow_scope};
use super::place::placed;
use super::text::text;
use crate::Element;
use crate::style::name;

/// One tab: its label, what its press sends.
pub struct Tab<'a, Message> {
    label: Cow<'a, str>,
    on_press: Message,
    active: bool,
    classes: Vec<&'a str>,
}

impl<'a, Message> Tab<'a, Message> {
    pub fn new(label: impl Into<Cow<'a, str>>, on_press: Message) -> Self {
        Self {
            label: label.into(),
            on_press,
            active: false,
            classes: Vec::new(),
        }
    }

    /// Its page shown: `.active`.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// A class of the app's on its element: its page's name.
    pub fn class(mut self, class: &'a str) -> Self {
        self.classes.push(class);
        self
    }
}

/// The `tabs`: a row, a `tab` each.
pub fn tabs<'a, Message: Clone + 'a>(
    tabs: impl IntoIterator<Item = Tab<'a, Message>>,
) -> Tabs<'a, Message> {
    let buttons = tabs.into_iter().map(|tab| {
        tab.classes
            .iter()
            .fold(
                button([text(tab.label).into()]).kind_of(name!("tab")),
                |b, class| b.class(class),
            )
            .class_if("active", tab.active)
            .on_press(tab.on_press)
            .into()
    });
    Tabs {
        row: row(buttons).kind_of(name!("tabs")),
    }
}

pub struct Tabs<'a, Message> {
    row: Flex<'a, Message>,
}

impl<'a, Message> Tabs<'a, Message> {
    placed!(row.boxed.place);
}

impl<'a, Message: Clone + 'a> From<Tabs<'a, Message>> for Element<'a, Message> {
    fn from(tabs: Tabs<'a, Message>) -> Self {
        arrow_scope(Keys::LeftRight, tabs.row).into()
    }
}
