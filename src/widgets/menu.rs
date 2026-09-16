//! A menu: a column of rows (plain, checkable, submenus that unfold in
//! place, separators), for a popup. A reusable component, not a gadget:
//! the host keeps a [`Menu`] (which submenus are unfolded), builds the
//! [`Item`]s from its own state, calls [`Menu::update`] with the menu's
//! [`Message`], `.map()`s [`Menu::view`] and sizes its popup with
//! [`Menu::size`]. The two walk the same rows so the surface fits the
//! content exactly.
//!
//! Element tree (see `assets/base.css`): `menu > item[.checked][.submenu]
//! [.expanded] > (check | label | arrow)`, `menu > separator`, `menu >
//! submenu > item …` for an unfolded submenu.

use std::collections::BTreeSet;

use iced::widget::{Column, Space};
use iced::{Element, Length, Size};

use crate::theme::{self, Node, Theme};

const CHECK_ON: &str = "✓";
const RADIO_ON: &str = "●";
const RADIO_OFF: &str = "○";
const ARROW_CLOSED: &str = "▸";
const ARROW_OPEN: &str = "▾";

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

/// What each row of a level shows, shared by the view and its
/// measurement so the two agree.
struct Row {
    item: Item,
    node: Node,
    /// The check column glyph, when this level has toggles.
    check: Option<&'static str>,
    arrow: Option<&'static str>,
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

    /// The menu as a column under `node` (the `menu` element). Items
    /// are taken by value: hosts build them from their state in `view`.
    pub fn view<'a>(
        &'a self,
        theme: &'a Theme,
        node: &Node,
        items: Vec<Item>,
    ) -> Element<'a, Message> {
        self.level(theme, node, items).into()
    }

    /// The size [`Menu::view`] takes, including `node`'s padding.
    pub fn size(&self, theme: &Theme, node: &Node, items: &[Item]) -> Size {
        let inner = self.measure_level(theme, node, items);
        let pad = theme.resolve(node).padding;
        Size::new(
            (inner.width + pad.left + pad.right).ceil(),
            (inner.height + pad.top + pad.bottom).ceil(),
        )
    }

    fn rows(&self, node: &Node, items: Vec<Item>) -> Vec<Row> {
        let toggles = items.iter().any(|i| i.toggle.is_some());
        let count = items.len();
        items
            .into_iter()
            .enumerate()
            .map(|(i, item)| {
                let expanded = self.expanded.contains(&item.id);
                let node = if item.separator {
                    node.child("separator").nth(i, count)
                } else {
                    node.child("item")
                        .class_if("checked", item.checked == Some(true))
                        .class_if("submenu", item.submenu)
                        .class_if("expanded", expanded)
                        .nth(i, count)
                };
                let check = toggles.then_some(match (item.toggle, item.checked) {
                    (Some(Toggle::Check), Some(true)) => CHECK_ON,
                    (Some(Toggle::Radio), Some(true)) => RADIO_ON,
                    (Some(Toggle::Radio), _) => RADIO_OFF,
                    _ => " ",
                });
                let arrow =
                    item.submenu
                        .then_some(if expanded { ARROW_OPEN } else { ARROW_CLOSED });
                Row {
                    item,
                    node,
                    check,
                    arrow,
                }
            })
            .collect()
    }

    /// One level: a column of rows, unfolded submenus nested under
    /// their row.
    fn level<'a>(&'a self, theme: &'a Theme, node: &Node, items: Vec<Item>) -> Column<'a, Message> {
        let sub_node = node.child("submenu");
        let rows = self.rows(node, items).into_iter().flat_map(move |row| {
            let item = row.item;
            if item.separator {
                let sep: Element<'a, Message> = theme
                    .container(&row.node, Space::new().width(Length::Fill))
                    .width(Length::Fill)
                    .into();
                return vec![sep];
            }
            let mut parts: Vec<Element<'a, Message>> = Vec::new();
            if let Some(glyph) = row.check {
                parts.push(theme.text(&row.node.child("check"), glyph).into());
            }
            parts.push(
                theme
                    .text(&row.node.child("label"), item.label)
                    .width(Length::Fill)
                    .into(),
            );
            if let Some(arrow) = row.arrow {
                parts.push(theme.text(&row.node.child("arrow"), arrow).into());
            }
            // The button already has the node's padding: only its gap.
            let content = iced::widget::row(parts)
                .spacing(theme.resolve(&row.node).gap)
                .align_y(iced::Alignment::Center)
                .width(Length::Fill);
            let on_press = item.enabled.then_some(if item.submenu {
                Message::ToggleSubmenu(item.id)
            } else {
                Message::Click(item.id)
            });
            let button: Element<'a, Message> = theme
                .button(&row.node, content)
                .width(Length::Fill)
                .on_press_maybe(on_press)
                .into();
            let mut out = vec![button];
            if item.submenu && self.expanded.contains(&item.id) {
                out.push(
                    self.level(theme, &sub_node, item.children)
                        .width(Length::Fill)
                        .into(),
                );
            }
            out
        });
        theme.column(node, rows).width(Length::Fill)
    }

    /// The size [`Menu::level`] takes, without `node`'s padding.
    fn measure_level(&self, theme: &Theme, node: &Node, items: &[Item]) -> Size {
        let mut width: f32 = 0.0;
        let mut height: f32 = 0.0;
        let mut rows: usize = 0;
        for row in self.rows(node, items.to_vec()) {
            let item = &row.item;
            let s = theme.resolve(&row.node);
            let pad = s.padding;
            if item.separator {
                let h = match s.height {
                    Some(theme::Length::Px(px)) => px,
                    _ => 1.0,
                };
                height += h + pad.top + pad.bottom;
                rows += 1;
                continue;
            }
            let mut w = pad.left + pad.right + 2.0 * s.border_width;
            let mut parts = 0;
            if let Some(glyph) = row.check {
                w += theme.measure(&row.node.child("check"), glyph).width;
                parts += 1;
            }
            let label = theme.measure(&row.node.child("label"), &item.label);
            w += label.width;
            parts += 1;
            if let Some(arrow) = row.arrow {
                w += theme.measure(&row.node.child("arrow"), arrow).width;
                parts += 1;
            }
            w += s.gap * (parts - 1) as f32;
            width = width.max(w);
            height += label
                .height
                .max(theme.line_height(&row.node.child("label")))
                + pad.top
                + pad.bottom
                + 2.0 * s.border_width;
            rows += 1;
            if item.submenu && self.expanded.contains(&item.id) {
                let sub_node = node.child("submenu");
                let sub = self.measure_level(theme, &sub_node, &item.children);
                let sub_pad = theme.resolve(&sub_node).padding;
                width = width.max(sub.width + sub_pad.left + sub_pad.right);
                height += sub.height + sub_pad.top + sub_pad.bottom;
                rows += 1;
            }
        }
        let gap = theme.resolve(node).gap;
        height += gap * rows.saturating_sub(1) as f32;
        Size::new(width.ceil(), height.ceil())
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

    #[test]
    fn check_column_only_with_toggles() {
        let m = Menu::new();
        let node = Node::root("popup").child("menu");
        let plain = vec![Item::new(1, "a"), Item::separator(2)];
        assert!(m.rows(&node, plain).iter().all(|r| r.check.is_none()));
        let toggles = vec![Item::new(1, "a"), Item::new(2, "b").radio(true)];
        let rows = m.rows(&node, toggles);
        assert_eq!(rows[0].check, Some(" "));
        assert_eq!(rows[1].check, Some(RADIO_ON));
        assert!(rows[1].node.has_class("checked"));
    }
}
