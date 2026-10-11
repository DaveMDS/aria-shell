//! A tooltip shows a tip by what the pointer rests on, after a delay:
//! `icssed::tooltip(content, text)`. It shows under it, or over it
//! where there's no room under, centred on it, and hides when the
//! pointer leaves or presses.
//!
//!     icssed::tooltip(button![..].on_press(Message::Save), "Save the file")
//!         .delay(Duration::from_millis(300))
//!
//! - `.delay(d)` is how long the pointer rests first: 600ms if not said.
//! - The tip is a `tooltip` beside the children of the element it's
//!   in: `line > tooltip` here.
//! - Its margins are its distance from what it's about.
//! - It's made anew each time it shows, so its animation (a fade in the
//!   base) runs each time.
//!
//! In a window the tip is an iced overlay, drawn over everything. A bar
//! is a surface as tall as itself, and would cut it: there the app gives
//! `.on_tip(f)`, is told the rectangle to show the tip by, and opens a
//! surface of its own with `icssed::tip(root, text)` in it.

use std::time::Duration;

use icssed::{Element, button, column, icon, row, text, tooltip};

use crate::Assets;

pub const TITLE: &str = "Tooltip";

#[derive(Default)]
pub struct State;

#[derive(Debug, Clone)]
pub enum Message {}

pub fn update(_state: &mut State, message: Message) {
    match message {}
}

pub fn view<'a>(_state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    column![row![
        tooltip(
            button![text("Rest the pointer here")],
            "A tip, after the base's 600ms",
        ),
        tooltip(button![text("A quicker one")], "Shown after 150ms")
            .delay(Duration::from_millis(150)),
        tooltip(
            icon(assets.picture.clone()).kind("avatar"),
            "Anything can have a tip: an icon too",
        ),
    ]
    .kind("line")]
    .into()
}
