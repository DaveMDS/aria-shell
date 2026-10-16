//! A menu: a column of items to press, each with a check box or a radio
//! (a toggle's), an icon, a shortcut; separators; submenus unfolded in place,
//! under their item. Up and Down move the focus among the items once one
//! has it, Enter and Space press it.
//!
//! It's the app's, as a toggle or a slider is: each item carries the
//! message its press sends, and whether a submenu is unfolded is the
//! app's to say (`MenuItem::submenu`), so it knows when one unfolds (a tray's
//! menu fetches its items then). A check box and a radio are the
//! toggle's parts, `check > mark` and `radio > dot`, drawn as a toggle's
//! are; a submenu's `arrow` is a mark, its text the theme's `content`.

use std::borrow::Cow;

use super::flex::{Flex, button, column, row};
use super::focus::{Keys, arrow_scope};
use super::icon::{IconSource, icon};
use super::place::placed;
use super::text::{mark, text};
use super::toggle::tick;
use crate::Element;
use crate::style::name;

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

/// `items` as a `menu`, a column.
pub fn menu<'a, Message: Clone + 'a>(
    items: impl IntoIterator<Item = MenuItem<'a, Message>>,
) -> Menu<'a, Message> {
    Menu {
        column: column(level(items.into_iter().collect())).kind_of(name!("menu")),
    }
}

pub struct Menu<'a, Message> {
    column: Flex<'a, Message>,
}

impl<'a, Message> Menu<'a, Message> {
    placed!(column.boxed.place);
}

impl<'a, Message: Clone + 'a> From<Menu<'a, Message>> for Element<'a, Message> {
    fn from(menu: Menu<'a, Message>) -> Self {
        arrow_scope(Keys::UpDown, menu.column).into()
    }
}

/// A level's elements: its items and separators, its unfolded submenus
/// under their items.
fn level<'a, Message: Clone + 'a>(items: Vec<MenuItem<'a, Message>>) -> Vec<Element<'a, Message>> {
    let entries = || items.iter().filter_map(|item| item.entry.as_ref());
    // One column before the labels when an item has a mark or an icon:
    // its mark, else its icon, else an empty room, so the labels line
    // up. Two only when an item has both: the marks', the icons'.
    let marks = entries().any(|e| e.mark != Mark::None);
    let icons = entries().any(|e| e.icon.is_some());
    let both = entries().any(|e| e.mark != Mark::None && e.icon.is_some());
    let empty_check = || row([]).kind_of(name!("check")).class("empty").into();
    let empty_icon = || row([]).kind_of(name!("icon")).into();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Some(entry) = item.entry else {
            out.push(row([]).kind_of(name!("separator")).into());
            continue;
        };
        let (check, radio, checked) = match entry.mark {
            Mark::None => (false, false, false),
            Mark::Check(on) => (true, false, on),
            Mark::Radio(on) => (false, true, on),
        };
        let expanded = matches!(entry.submenu, Some((true, _)));
        let mut parts: Vec<Element<'a, Message>> = Vec::with_capacity(5);
        let lead = match entry.mark {
            Mark::Check(_) => Some(check_part()),
            Mark::Radio(_) => Some(radio_part()),
            Mark::None => None,
        };
        let icon = entry.icon.map(|source| icon(source).into());
        if both {
            parts.push(lead.unwrap_or_else(empty_check));
            parts.push(icon.unwrap_or_else(empty_icon));
        } else if let Some(part) = lead.or(icon) {
            parts.push(part);
        } else if marks {
            parts.push(empty_check());
        } else if icons {
            parts.push(empty_icon());
        }
        parts.push(text(entry.label).into());
        if let Some(shortcut) = entry.shortcut {
            parts.push(text(shortcut).kind_of(name!("shortcut")).into());
        }
        if entry.submenu.is_some() {
            parts.push(mark().kind_of(name!("arrow")).into());
        }
        let mut pressed = entry
            .classes
            .iter()
            .fold(button(parts).kind_of(name!("item")), |b, class| {
                b.class(class)
            })
            .class_if("check", check)
            .class_if("radio", radio)
            .class_if("checked", checked)
            .class_if("submenu", entry.submenu.is_some())
            .class_if("expanded", expanded)
            .disabled_if(entry.on_press.is_none());
        if let Some(message) = entry.on_press {
            pressed = pressed.on_press(message);
        }
        out.push(pressed.into());
        if let Some((true, items)) = entry.submenu {
            out.push(column(level(items)).kind_of(name!("submenu")).into());
        }
    }
    out
}

/// A check item's part: `check > mark`, the mark the toolkit's tick.
fn check_part<'a, Message: Clone + 'a>() -> Element<'a, Message> {
    row([icon(tick()).kind_of(name!("mark")).into()])
        .kind_of(name!("check"))
        .into()
}

/// A radio item's part: `radio > dot`.
fn radio_part<'a, Message: Clone + 'a>() -> Element<'a, Message> {
    row([row([]).kind_of(name!("dot")).into()])
        .kind_of(name!("radio"))
        .into()
}
