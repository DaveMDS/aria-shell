//! A scroll shows content taller or wider than its room:
//! `icssed::scroll(content)`. Vertical unless `.horizontal()` or
//! `.both()`.
//!
//!     icssed::scroll(icssed::column(items))
//!         .id(LIST)
//!         .on_scroll(Message::Scrolled)
//!
//! - It's a box around the content: `scroll > content`. A list grows
//!   with its items up to its `max-height`, then scrolls.
//! - A scrollbar is in the scroll's padding box, at its end unless its
//!   `justify-self` (`align-self`, a horizontal one's) says otherwise;
//!   its margins move it, out of the scroll too. The base gives the
//!   scroll a padding the bars fit in, so they cover no content.
//! - `.id(id)` names it for iced's operations (`scroll_to`, `snap_to`);
//!   `.on_scroll(f)` sends the viewport at every move.
//! - Its bars are `scrollbar.vertical` and `scrollbar.horizontal`, each
//!   with its `thumb`, boxes as any other: `:hover` under the pointer,
//!   `:active` while the thumb is dragged; their sizes may follow the
//!   scroll's states (`scroll:hover > scrollbar { width: 10px }`).

use icssed::{Element, column, row, scroll, text};

use crate::Assets;

pub const TITLE: &str = "Scroll";

#[derive(Default)]
pub struct State {
    /// How far down the list is scrolled, 0 to 1.
    offset: f32,
}

#[derive(Debug, Clone)]
pub enum Message {
    Scrolled(f32),
}

pub fn update(state: &mut State, message: Message) {
    let Message::Scrolled(offset) = message;
    state.offset = offset;
}

pub fn view<'a>(state: &'a State, _assets: &'a Assets) -> Element<'a, Message> {
    let rows = (1..=30).map(|i| text(format!("Row {i} of a list that scrolls")).into());
    let cells = (1..=20).map(|i| row![text(format!("Cell {i}"))].kind("cell").into());
    column![
        row![
            scroll(column(rows).kind("rows"))
                .class("list")
                .on_scroll(|viewport| Message::Scrolled(viewport.relative_offset().y)),
            scroll(row(cells).kind("cells"))
                .class("strip")
                .horizontal(),
        ]
        .kind("line"),
        text(format!(
            "The list scrolled {:.0}% down",
            state.offset * 100.0
        ))
        .kind("status"),
    ]
    .into()
}
