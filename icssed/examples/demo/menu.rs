//! A menu is a list of items to pick from: `icssed::menu(items)`,
//! each item a `MenuItem`. It's a widget like any other, drawn where the
//! view puts it: a popup is the app's surface with a menu inside.
//!
//!     icssed::menu([
//!         MenuItem::new("Open").icon(star).shortcut("Ctrl+O")
//!             .on_press(Message::Open),
//!         MenuItem::separator(),
//!         MenuItem::new("Show hidden").check(hidden)
//!             .on_press(Message::Hidden),
//!     ])
//!
//! - An item without `on_press` is disabled.
//! - `.check(on)` and `.radio(on)` make it a check or a radio item; the
//!   app holds the value and flips it on the message, as with any
//!   controlled widget.
//! - `.submenu(open, items)` nests items under it, unfolded while `open`
//!   is true: the app holds that too.
//! - `.icon(s)` puts an icon before the label, in the column of the
//!   checks and the radios: an item has one or the other, and the
//!   labels line up. Only where an item has both are they two columns.
//! - `.class(c)` styles one item (`.danger` below).
//! - Up and Down move the focus round the items once one has it; Enter
//!   picks.
//!
//! A check item's box and a radio item's circle are a toggle's, drawn by
//! the same rules: `item.check > check > mark`, `item.radio > radio >
//! dot`. The submenu's arrow is the sheet's: `content` on `item > arrow`.

use icssed::{Element, MenuItem, column, menu, text};

use crate::Assets;

pub const TITLE: &str = "Menu";

#[derive(Default)]
pub struct State {
    /// The last item picked.
    picked: Option<&'static str>,
    hidden: bool,
    by_date: bool,
    /// The submenu, unfolded.
    more: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Picked(&'static str),
    Hidden,
    ByDate(bool),
    More,
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::Picked(label) => state.picked = Some(label),
        Message::Hidden => state.hidden = !state.hidden,
        Message::ByDate(by_date) => state.by_date = by_date,
        Message::More => state.more = !state.more,
    }
}

pub fn view<'a>(state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let picked = match state.picked {
        Some(label) => format!("Picked: {label}"),
        None => "Nothing picked yet: click, or Tab to an item then Up, Down, Enter".into(),
    };
    let item = |label: &'static str| MenuItem::new(label).on_press(Message::Picked(label));
    column![
        menu([
            item("Open").icon(assets.star.clone()).shortcut("Ctrl+O"),
            item("A label far too long for its menu, cut short").shortcut("Ctrl+L"),
            MenuItem::new("Paste, disabled").shortcut("Ctrl+V"),
            MenuItem::separator(),
            MenuItem::new("Show hidden files")
                .check(state.hidden)
                .on_press(Message::Hidden),
            MenuItem::new("Sort by name")
                .radio(!state.by_date)
                .on_press(Message::ByDate(false)),
            MenuItem::new("Sort by date")
                .radio(state.by_date)
                .on_press(Message::ByDate(true)),
            MenuItem::separator(),
            MenuItem::new("More").on_press(Message::More).submenu(
                state.more,
                [
                    item("Rename").icon(assets.star.clone()),
                    item("Delete").class("danger"),
                ],
            ),
        ]),
        text(picked).kind("status"),
    ]
    .into()
}
