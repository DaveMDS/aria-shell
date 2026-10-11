//! Every element is a box, as in CSS: its content, then `padding`, then
//! `border`, then `margin` around it. `icssed::row` and `icssed::column`
//! are the boxes an app builds with; their look is all in the sheet.
//!
//!     row![..].kind("card")
//!
//! - `width` and `height` are lengths, `fill` (the room left) or the
//!   content's when not set; `min-` and `max-` bound them, and
//!   `min-height` beats `height`, as in CSS.
//! - `border` may differ by side, in width and colour, with or without
//!   `border-radius`. Borders snap to whole device pixels, at least one,
//!   at any scale.
//! - `box-shadow`: an outer shadow and an inset one at most, no spread.
//! - `outline` is drawn around the border box, over the children,
//!   taking no room; `outline-offset` moves it out.
//!
//! The second row is painted only: `transform` (translate, a uniform
//! scale) and `opacity` leave the layout untouched, and the pointer
//! follows what's drawn. `opacity` isn't a group, unlike CSS: where
//! children overlap in a faded box, the lower shows through the upper.
//! A box doesn't turn: iced can't rotate one (`rotate` turns icons and
//! images).

use icssed::{Element, column, icon, row, text};

use crate::Assets;

pub const TITLE: &str = "Boxes";

#[derive(Default)]
pub struct State;

#[derive(Debug, Clone)]
pub enum Message {}

pub fn update(_state: &mut State, message: Message) {
    match message {}
}

pub fn view<'a>(_state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let boxed = |class: &str, content: &'static str| row![text(content)].kind("box").class(class);
    column![
        row![
            boxed("plain", "Background"),
            boxed("bordered", "Border"),
            boxed("rounded", "Radius"),
            boxed("shadow", "Shadow"),
            boxed("inset", "Inset"),
            boxed("sides", "Sides"),
        ]
        .kind("line"),
        row![
            boxed("tab", "A tab"),
            boxed("four", "Four sides"),
            boxed("ring", "Ring"),
            boxed("outlined", "Outline"),
        ]
        .kind("line"),
        row![
            boxed("scaled", "Scaled 80%"),
            boxed("moved", "Moved"),
            row![icon(assets.star.clone()), text("Opacity 50%")]
                .kind("box")
                .class("faded"),
        ]
        .kind("line"),
    ]
    .into()
}
