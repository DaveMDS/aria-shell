//! Tabs: a row of buttons, one per page, the page shown `.active`; the
//! row's box draws the line under them, each tab its own mark. Each tab
//! carries the message that shows its page. Left and Right move the
//! focus among the tabs once one has it, Enter and Space press it.

use std::borrow::Cow;

use super::flex::{button, row};
use super::focus::{Keys, arrow_scope};
use super::text::text;
use crate::Element;
use crate::style::Node;

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

/// `tabs` in a row: `node`, a `tab` each.
pub fn tabs<'a, Message: Clone + 'a>(
    node: Node<'a>,
    tabs: impl IntoIterator<Item = Tab<'a, Message>>,
) -> Element<'a, Message> {
    let tabs: Vec<Tab<'a, Message>> = tabs.into_iter().collect();
    let count = tabs.len();
    let buttons = tabs.into_iter().enumerate().map(|(i, tab)| {
        let mut node = node.child_at("tab", i, count);
        for class in &tab.classes {
            node = node.class(class);
        }
        let node = node.class_if("active", tab.active);
        button(node, [text(node.child("label"), tab.label).into()])
            .on_press(tab.on_press)
            .into()
    });
    arrow_scope(Keys::LeftRight, row(node, buttons)).into()
}
