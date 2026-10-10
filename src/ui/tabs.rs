//! Tabs: one button per page, the open one `.active`, in a row whose
//! box draws the line under them (the active tab's mark is its own,
//! over that line).
//!
//! ```text
//! tabs                              a row (`gap`)
//! ╰─ button.tab[.<name>][.active]
//!    ╰─ text
//! ```

use iced::widget::row;
use iced::{Alignment, Element, Length};

use crate::ui::theme::{Node, Theme};

pub struct Tab<'a, M> {
    /// The page's class, when it has one: `button.tab.<name>`.
    pub name: Option<&'static str>,
    pub label: &'a str,
    pub active: bool,
    pub on_press: M,
}

/// The tabs, styled as `node` (a `tabs`), `width` wide: the line runs
/// under all of it.
pub fn view<'a, M: Clone + 'a>(
    theme: &'a Theme,
    node: &Node,
    tabs: impl IntoIterator<Item = Tab<'a, M>>,
    width: Length,
) -> Element<'a, M> {
    let tabs: Vec<Tab<'a, M>> = tabs.into_iter().collect();
    let count = tabs.len();
    let buttons = tabs.into_iter().enumerate().map(|(i, t)| {
        let mut n = node.child("button").class("tab").nth(i, count);
        if let Some(name) = t.name {
            n = n.class(name);
        }
        let n = n.class_if("active", t.active);
        theme
            .button(&n, theme.text(&n.child("text"), t.label))
            .on_press(t.on_press)
            .into()
    });
    // The box already has the node's padding: only its gap.
    let buttons = row(buttons)
        .spacing(theme.resolve(node).gap)
        .align_y(Alignment::Center);
    theme.container(node, buttons).width(width).into()
}
