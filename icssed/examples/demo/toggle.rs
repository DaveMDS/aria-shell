//! A toggle is a value on or off: `icssed::toggle(node, on, f)`. A button
//! sends a message; a toggle holds a value, which the app keeps, and asks
//! for the other one: a press sends `f(!on)`. It shows what it's given,
//! as every widget of the toolkit does.
//!
//!     icssed::toggle(node, self.muted, Message::Mute).icon(speaker).label("Mute")
//!     icssed::toggle(node, self.hidden, Message::Hidden).label("Show hidden").check()
//!     icssed::toggle(node, self.wifi, Message::Wifi).switch()
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
//! look. It makes all its nodes itself, as a menu's items do, so
//! `toggle.checked > label` reaches its label.
//!
//! A press anywhere on it flips it, its label too; Tab gives it the
//! focus, Space and Enter flip it; `disabled_if(true)` on its node, and
//! it sends nothing.

use icssed::{Element, Node};

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

pub fn view<'a>(state: &'a State, node: Node<'a>, assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    let toggle = line.child("toggle");
    let disabled = toggle.disabled_if(true);
    icssed::column(
        node,
        [
            icssed::row(
                line,
                [
                    icssed::toggle(toggle, state.muted, Message::Mute)
                        .icon(assets.star.clone())
                        .label(if state.muted { "Muted" } else { "Mute" })
                        .into(),
                    icssed::toggle(disabled, true, Message::Mute)
                        .label("Disabled")
                        .into(),
                ],
            )
            .into(),
            icssed::row(
                line,
                [
                    icssed::toggle(toggle, state.hidden, Message::Hidden)
                        .label("Show hidden files")
                        .check()
                        .into(),
                    icssed::toggle(disabled, true, Message::Hidden)
                        .label("Disabled")
                        .check()
                        .into(),
                ],
            )
            .into(),
            icssed::row(
                line,
                [
                    icssed::toggle(toggle, state.wifi, Message::Wifi)
                        .label("Wi-Fi")
                        .switch()
                        .into(),
                    icssed::toggle(disabled, true, Message::Wifi)
                        .label("Disabled")
                        .switch()
                        .into(),
                ],
            )
            .into(),
        ],
    )
    .into()
}
