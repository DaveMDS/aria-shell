//! A scroller shows content taller or wider than its room:
//! `icssed::scroller(content)`. Vertical unless `.horizontal()` or
//! `.both()`.
//!
//!     icssed::scroller(icssed::column(items))
//!         .id(LIST)
//!         .on_scroll(Message::Scrolled)
//!
//! - It's a box around the content: `scroller > content`. A list grows
//!   with its items up to its `max-height`, then scrolls.
//! - A scrollbar is in the scroller's padding box, at its end unless its
//!   `justify-self` (`align-self`, a horizontal one's) says otherwise;
//!   its margins move it, out of the scroller too. The base gives the
//!   scroller a padding the bars fit in, so they cover no content.
//! - `.id(id)` names it for iced's operations (`scroll_to`, `snap_to`);
//!   `.on_scroll(f)` sends the viewport at every move.
//! - Its bars are `scrollbar.vertical` and `scrollbar.horizontal`, each
//!   with its `thumb`, boxes as any other: `:hover` under the pointer,
//!   `:active` while the thumb is dragged; their sizes may follow the
//!   scroller's states (`scroller:hover > scrollbar { width: 10px }`).

use icssed::{Element, column, row, scroller, text};

use crate::Assets;

pub const TITLE: &str = "Scroller";

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
            scroller(column(rows).kind("rows"))
                .class("list")
                .on_scroll(|viewport| Message::Scrolled(viewport.relative_offset().y)),
            scroller(row(cells).kind("cells"))
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
