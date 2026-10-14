//! A menu of the host's items (labels, toggles, separators, submenus
//! that unfold in place), for a popup: what the toolkit's menu shows,
//! and which submenus are unfolded, which the toolkit leaves to its
//! host. A reusable component, not a gadget: the host keeps a [`Menu`],
//! builds the [`Item`]s from its own state (the tray service builds
//! them from an app's menu), calls [`Menu::update`] with the menu's
//! [`Message`] and `.map()`s [`Menu::view`].
//!
//! Element tree: the toolkit's (`menu > item[.check|.radio][.checked]
//! [.submenu][.expanded] > (check > mark | radio > dot | label | arrow)`,
//! `menu > separator`, `menu > submenu > item …`).

use std::collections::BTreeSet;

use icssed::{Element, MenuItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Check,
    Radio,
}

/// One entry. Ids are the host's (a dbusmenu node id, an index): they
/// come back in the messages and must be unique across levels.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub toggle: Option<Toggle>,
    /// The toggle state; `None` for "indeterminate" (or no toggle).
    pub checked: Option<bool>,
    pub icon_name: String,
    /// Has (or may have, once opened) children.
    pub submenu: bool,
    pub children: Vec<Item>,
}

impl Item {
    /// A plain, enabled row.
    pub fn new(id: i32, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            enabled: true,
            ..Self::default()
        }
    }

    pub fn separator(id: i32) -> Self {
        Self {
            id,
            separator: true,
            ..Self::default()
        }
    }

    pub fn check(self, checked: bool) -> Self {
        Self {
            toggle: Some(Toggle::Check),
            checked: Some(checked),
            ..self
        }
    }

    pub fn radio(self, checked: bool) -> Self {
        Self {
            toggle: Some(Toggle::Radio),
            checked: Some(checked),
            ..self
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A row was clicked (never a submenu's own row).
    Click(i32),
    /// A submenu row was clicked: it folds or unfolds.
    ToggleSubmenu(i32),
}

/// What [`Menu::update`] tells the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    None,
    Clicked(i32),
    /// A submenu just unfolded (a chance to fetch its children).
    Unfolded(i32),
}

#[derive(Default)]
pub struct Menu {
    expanded: BTreeSet<i32>,
}

impl Menu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold everything (the host reuses the menu for other items).
    pub fn reset(&mut self) {
        self.expanded.clear();
    }

    pub fn update(&mut self, message: Message) -> Event {
        match message {
            Message::Click(id) => Event::Clicked(id),
            Message::ToggleSubmenu(id) => {
                if self.expanded.remove(&id) {
                    Event::None
                } else {
                    self.expanded.insert(id);
                    Event::Unfolded(id)
                }
            }
        }
    }

    /// The menu, a `menu`. Items are taken by value: hosts
    /// build them from their state in `view`.
    pub fn view<'a>(&self, items: Vec<Item>) -> Element<'a, Message> {
        icssed::menu(self.entries(items)).into()
    }

    /// One level's entries, an unfolded submenu's under its item.
    fn entries<'a>(&self, items: Vec<Item>) -> Vec<MenuItem<'a, Message>> {
        items
            .into_iter()
            .map(|item| {
                if item.separator {
                    return MenuItem::separator();
                }
                let mut entry = MenuItem::new(item.label);
                entry = match (item.toggle, item.checked) {
                    (Some(Toggle::Check), checked) => entry.check(checked == Some(true)),
                    (Some(Toggle::Radio), checked) => entry.radio(checked == Some(true)),
                    (None, _) => entry,
                };
                if item.submenu {
                    let expanded = self.expanded.contains(&item.id);
                    entry = entry.submenu(expanded, self.entries(item.children));
                }
                if item.enabled {
                    entry = entry.on_press(if item.submenu {
                        Message::ToggleSubmenu(item.id)
                    } else {
                        Message::Click(item.id)
                    });
                }
                entry
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_and_unfold() {
        let mut m = Menu::new();
        assert_eq!(m.update(Message::ToggleSubmenu(3)), Event::Unfolded(3));
        assert!(m.expanded.contains(&3));
        assert_eq!(m.update(Message::ToggleSubmenu(3)), Event::None);
        assert!(m.expanded.is_empty());
        assert_eq!(m.update(Message::Click(7)), Event::Clicked(7));
    }
}
