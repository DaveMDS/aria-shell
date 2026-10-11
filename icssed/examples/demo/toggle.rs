//! A toggle is a value on or off: `icssed::toggle(on, f)`. A button
//! sends a message; a toggle holds a value, which the app keeps, and asks
//! for the other one: a press sends `f(!on)`. It shows what it's given,
//! as every widget of the toolkit does.
//!
//!     icssed::toggle(self.muted, Message::Mute).icon(speaker).label("Mute")
//!     icssed::toggle(self.hidden, Message::Hidden).label("Show hidden").check()
//!     icssed::toggle(self.wifi, Message::Wifi).switch()
//!     icssed::toggle(self.sort == By::Name, |_| Message::Sort(By::Name)).radio()
//!
//! It looks one of four ways, picked by the call:
//!
//! - As a button that stays pressed.
//! - `.check()`: a check box, its `check > mark` first; the mark is the
//!   sheet's `content`.
//! - `.radio()`: a radio, its `radio > dot` first.
//! - `.switch()`: a switch, its `switch > knob` first; the knob slides
//!   with a transform, a transition in the base.
//!
//! `.icon(..)` and `.label(..)` follow, both optional. In all four looks
//! it's `.checked` while it's on: one class for the sheet, whatever the
//! look, which its parts see: `toggle.checked > label` reaches its
//! label.
//!
//! A press anywhere on it flips it, its label too; Tab gives it the
//! focus, Space and Enter flip it; `disabled_if(true)`, and it sends
//! nothing.
//!
//! A radio is one chosen among a few: each a toggle, on when it's the
//! one chosen, its press asking for it whatever its value (`|_|`), so a
//! press on the chosen one changes nothing. The pressed buttons choose
//! the same way, side by side.

use icssed::{Element, Toggle, column, row, toggle};

use crate::Assets;

pub const TITLE: &str = "Toggle";

const SORTS: [&str; 3] = ["By name", "By date", "By size"];

pub struct State {
    muted: bool,
    hidden: bool,
    wifi: bool,
    sort: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            muted: true,
            hidden: false,
            wifi: true,
            sort: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Mute(bool),
    Hidden(bool),
    Wifi(bool),
    Sort(usize),
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::Mute(on) => state.muted = on,
        Message::Hidden(on) => state.hidden = on,
        Message::Wifi(on) => state.wifi = on,
        Message::Sort(i) => state.sort = i,
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
        sorts(state, |toggle| toggle.radio()),
        sorts(state, |toggle| toggle),
    ]
    .into()
}

/// The ways to sort: a radio per way, or a pressed button.
fn sorts<'a>(
    state: &State,
    look: impl Fn(Toggle<'a, Message>) -> Toggle<'a, Message>,
) -> Element<'a, Message> {
    let toggles = SORTS.iter().enumerate().map(|(i, &by)| {
        look(toggle(i == state.sort, move |_| Message::Sort(i)))
            .label(by)
            .into()
    });
    row(toggles).kind("line").into()
}
