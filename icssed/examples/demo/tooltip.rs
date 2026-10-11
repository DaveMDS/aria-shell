//! A tooltip shows a tip by what the pointer rests on, after a delay:
//! `icssed::tooltip(content, node, text)`. It shows under it, or over it
//! where there's no room under, centred on it, and hides when the
//! pointer leaves or presses.
//!
//!     icssed::tooltip(
//!         icssed::button(node, [..]).on_press(Message::Save),
//!         row.child("tooltip"),
//!         "Save the file",
//!     )
//!     .delay(Duration::from_millis(300))
//!
//! - `.delay(d)` is how long the pointer rests first: 600ms if not said.
//! - Its margins are its distance from what it's about.
//! - It's made anew each time it shows, so its animation (a fade in the
//!   base) runs each time.
//!
//! In a window the tip is an iced overlay, drawn over everything. A bar
//! is a surface as tall as itself, and would cut it: there the app gives
//! `.on_tip(f)`, is told the rectangle to show the tip by, and opens a
//! surface of its own with `icssed::tip(node, text)` in it.

use std::time::Duration;

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Tooltip";

#[derive(Default)]
pub struct State;

#[derive(Debug, Clone)]
pub enum Message {}

pub fn update(_state: &mut State, message: Message) {
    match message {}
}

pub fn view<'a>(_state: &'a State, node: Node<'a>, assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    let button = |text: &'static str| -> Element<'a, Message> {
        let button = line.child("button");
        icssed::button(button, [icssed::text(button.child("label"), text).into()]).into()
    };
    icssed::row(
        line,
        [
            icssed::tooltip(
                button("Rest the pointer here"),
                line.child("tooltip"),
                "A tip, after the base's 600ms",
            )
            .into(),
            icssed::tooltip(
                button("A quicker one"),
                line.child("tooltip"),
                "Shown after 150ms",
            )
            .delay(Duration::from_millis(150))
            .into(),
            icssed::tooltip(
                icssed::icon(line.child("avatar"), assets.picture.clone()),
                line.child("tooltip"),
                "Anything can have a tip: an icon too",
            )
            .into(),
        ],
    )
    .into()
}
