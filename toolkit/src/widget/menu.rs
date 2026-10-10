//! A menu: a column of items to press, each with a check's or a radio's
//! mark, an icon, a shortcut; separators; submenus unfolded in place,
//! under their item. Up and Down move the focus among the items once one
//! has it, Enter and Space press it.
//!
//! It's the app's, as a toggle or a slider is: each item carries the
//! message its press sends, and whether a submenu is unfolded is the
//! app's to say (`MenuItem::submenu`), so it knows when one unfolds (a tray's
//! menu fetches its items then). The marks are the theme's: a `check`'s
//! and an `arrow`'s text is their `content`, as the item's classes pick
//! it (`item.radio.checked > check { content: "●" }`).

use std::borrow::Cow;

use super::flex::{button, column, row};
use super::focus::{Keys, arrow_scope};
use super::icon::{IconSource, icon};
use super::text::{mark, text};
use crate::Element;
use crate::style::Node;

/// One entry of a menu.
pub struct MenuItem<'a, Message> {
    entry: Option<Entry<'a, Message>>,
}

struct Entry<'a, Message> {
    label: Cow<'a, str>,
    on_press: Option<Message>,
    mark: Mark,
    icon: Option<IconSource<'a>>,
    shortcut: Option<Cow<'a, str>>,
    /// Unfolded or not, and its items.
    submenu: Option<(bool, Vec<MenuItem<'a, Message>>)>,
    classes: Vec<&'a str>,
}

#[derive(Clone, Copy, PartialEq)]
enum Mark {
    None,
    Check(bool),
    Radio(bool),
}

impl<'a, Message> MenuItem<'a, Message> {
    /// An item showing `label`, disabled until given [`MenuItem::on_press`].
    pub fn new(label: impl Into<Cow<'a, str>>) -> Self {
        Self {
            entry: Some(Entry {
                label: label.into(),
                on_press: None,
                mark: Mark::None,
                icon: None,
                shortcut: None,
                submenu: None,
                classes: Vec::new(),
            }),
        }
    }

    /// A line between items.
    pub fn separator() -> Self {
        Self { entry: None }
    }

    fn with(mut self, change: impl FnOnce(&mut Entry<'a, Message>)) -> Self {
        if let Some(entry) = &mut self.entry {
            change(entry);
        }
        self
    }

    /// What its press sends.
    pub fn on_press(self, message: Message) -> Self {
        self.with(|e| e.on_press = Some(message))
    }

    /// A check's mark, `.check`, `.checked` when it is.
    pub fn check(self, checked: bool) -> Self {
        self.with(|e| e.mark = Mark::Check(checked))
    }

    /// A radio's mark, `.radio`, `.checked` when it is.
    pub fn radio(self, checked: bool) -> Self {
        self.with(|e| e.mark = Mark::Radio(checked))
    }

    pub fn icon(self, source: impl Into<IconSource<'a>>) -> Self {
        let source = source.into();
        self.with(|e| e.icon = Some(source))
    }

    /// A key that does the same, shown after the label.
    pub fn shortcut(self, shortcut: impl Into<Cow<'a, str>>) -> Self {
        let shortcut = shortcut.into();
        self.with(|e| e.shortcut = Some(shortcut))
    }

    /// Its `items`, under it when `expanded`: `.submenu`, `.expanded`.
    /// Its press is the app's way to unfold and fold it.
    pub fn submenu(
        self,
        expanded: bool,
        items: impl IntoIterator<Item = MenuItem<'a, Message>>,
    ) -> Self {
        let items = items.into_iter().collect();
        self.with(|e| e.submenu = Some((expanded, items)))
    }

    /// A class of the app's on its element.
    pub fn class(self, class: &'a str) -> Self {
        self.with(|e| e.classes.push(class))
    }
}

/// `items` as a menu: `node`, a column.
pub fn menu<'a, Message: Clone + 'a>(
    node: Node<'a>,
    items: impl IntoIterator<Item = MenuItem<'a, Message>>,
) -> Element<'a, Message> {
    arrow_scope(
        Keys::UpDown,
        column(node, level(node, items.into_iter().collect())),
    )
    .into()
}

/// A level's elements: its items and separators, its unfolded submenus
/// under their items.
fn level<'a, Message: Clone + 'a>(
    node: Node<'a>,
    items: Vec<MenuItem<'a, Message>>,
) -> Vec<Element<'a, Message>> {
    let entries = || items.iter().filter_map(|item| item.entry.as_ref());
    // A column of marks, or of icons, when one item has one: the labels
    // line up.
    let marks = entries().any(|e| e.mark != Mark::None);
    let icons = entries().any(|e| e.icon.is_some());
    let count: usize = items
        .iter()
        .map(|item| match &item.entry {
            Some(Entry {
                submenu: Some((true, _)),
                ..
            }) => 2,
            _ => 1,
        })
        .sum();
    let mut out = Vec::with_capacity(count);
    for item in items {
        let at = out.len();
        let Some(entry) = item.entry else {
            out.push(row(node.child_at("separator", at, count), []).into());
            continue;
        };
        let mut item = node.child_at("item", at, count);
        for class in &entry.classes {
            item = item.class(class);
        }
        let (check, radio, checked) = match entry.mark {
            Mark::None => (false, false, false),
            Mark::Check(on) => (true, false, on),
            Mark::Radio(on) => (false, true, on),
        };
        let expanded = matches!(entry.submenu, Some((true, _)));
        let item = item
            .class_if("check", check)
            .class_if("radio", radio)
            .class_if("checked", checked)
            .class_if("submenu", entry.submenu.is_some())
            .class_if("expanded", expanded)
            .disabled_if(entry.on_press.is_none());
        let mut parts: Vec<Element<'a, Message>> = Vec::with_capacity(5);
        if marks {
            parts.push(mark(item.child("check")));
        }
        if icons {
            let slot = item.child("icon");
            parts.push(match entry.icon {
                Some(source) => icon(slot, source).into(),
                None => row(slot, []).into(),
            });
        }
        parts.push(text(item.child("label"), entry.label).into());
        if let Some(shortcut) = entry.shortcut {
            parts.push(text(item.child("shortcut"), shortcut).into());
        }
        if entry.submenu.is_some() {
            parts.push(mark(item.child("arrow")));
        }
        let mut pressed = button(item, parts);
        if let Some(message) = entry.on_press {
            pressed = pressed.on_press(message);
        }
        out.push(pressed.into());
        if let Some((true, items)) = entry.submenu {
            let submenu = node.child_at("submenu", out.len(), count);
            out.push(column(submenu, level(submenu, items)).into());
        }
    }
    out
}
