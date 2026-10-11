//! A toggle is a value on or off: `icssed::toggle(on, f)`. A button
//! sends a message; a toggle holds a value, which the app keeps, and asks
//! for the other one: a press sends `f(!on)`. It shows what it's given,
//! as every widget of the toolkit does.
//!
//!     icssed::toggle(self.muted, Message::Mute).icon(speaker).label("Mute")
//!     icssed::toggle(self.hidden, Message::Hidden).label("Show hidden").check()
//!     icssed::toggle(self.wifi, Message::Wifi).switch()
//!
//! It looks one of three ways, picked by the call:
//!
//! - As a button that stays pressed.
//! - `.check()`: a check box, its `check > mark` first; the mark is the
//!   sheet's `content`.
//! - `.switch()`: a switch, its `switch > knob` first; the knob slides
//!   with a transform, a transition in the base.
//!
//! `.icon(..)` and `.label(..)` follow, both optional. In all three looks
//! it's `.checked` while it's on: one class for the sheet, whatever the
//! look, which its parts see: `toggle.checked > label` reaches its
//! label.
//!
//! A press anywhere on it flips it, its label too; Tab gives it the
//! focus, Space and Enter flip it; `disabled_if(true)`, and it sends
//! nothing.

use icssed::{Element, column, row, toggle};

use crate::Assets;

pub const TITLE: &str = "Toggle";

pub struct State {
    muted: bool,
    hidden: bool,
    wifi: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            muted: true,
            hidden: false,
            wifi: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Mute(bool),
    Hidden(bool),
    Wifi(bool),
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::Mute(on) => state.muted = on,
        Message::Hidden(on) => state.hidden = on,
        Message::Wifi(on) => state.wifi = on,
    }
}

pub fn view<'a>(state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    column![
        row![
            toggle(state.muted, Message::Mute)
                .icon(assets.star.clone())
                .label(if state.muted { "Muted" } else { "Mute" }),
            toggle(true, Message::Mute).label("Disabled").disabled_if(true),
        ]
        .kind("line"),
        row![
            toggle(state.hidden, Message::Hidden)
                .label("Show hidden files")
                .check(),
            toggle(true, Message::Hidden)
                .label("Disabled")
                .check()
                .disabled_if(true),
        ]
        .kind("line"),
        row![
            toggle(state.wifi, Message::Wifi).label("Wi-Fi").switch(),
            toggle(true, Message::Wifi)
                .label("Disabled")
                .switch()
                .disabled_if(true),
        ]
        .kind("line"),
    ]
    .into()
}
