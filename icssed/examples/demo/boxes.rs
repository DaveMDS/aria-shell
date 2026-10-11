//! Every element is a box, as in CSS: its content, then `padding`, then
//! `border`, then `margin` around it. `icssed::row` and `icssed::column`
//! are the boxes an app builds with; their look is all in the sheet.
//!
//!     icssed::row(node.child("card"), [..])
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

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Boxes";

#[derive(Default)]
pub struct State;

#[derive(Debug, Clone)]
pub enum Message {}

pub fn update(_state: &mut State, message: Message) {
    match message {}
}

pub fn view<'a>(_state: &'a State, node: Node<'a>, assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    let boxed = |class: &str, text: &'static str| -> Element<'a, Message> {
        let boxed = line.child("box").class(class);
        icssed::row(boxed, [icssed::text(boxed.child("label"), text).into()]).into()
    };
    let faded = line.child("box").class("faded");
    icssed::column(
        node,
        [
            icssed::row(
                line,
                [
                    boxed("plain", "Background"),
                    boxed("bordered", "Border"),
                    boxed("rounded", "Radius"),
                    boxed("shadow", "Shadow"),
                    boxed("inset", "Inset"),
                    boxed("sides", "Sides"),
                ],
            )
            .into(),
            icssed::row(
                line,
                [
                    boxed("tab", "A tab"),
                    boxed("four", "Four sides"),
                    boxed("ring", "Ring"),
                    boxed("outlined", "Outline"),
                ],
            )
            .into(),
            icssed::row(
                line,
                [
                    boxed("scaled", "Scaled 80%"),
                    boxed("moved", "Moved"),
                    icssed::row(
                        faded,
                        [
                            icssed::icon(faded.child("icon"), assets.star.clone()).into(),
                            icssed::text(faded.child("label"), "Opacity 50%").into(),
                        ],
                    )
                    .into(),
                ],
            )
            .into(),
        ],
    )
    .into()
}
