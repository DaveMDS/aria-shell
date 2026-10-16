//! A toggle is a value on or off: `icssed::toggle(on).on_toggle(f)`. A button
//! sends a message; a toggle holds a value, which the app keeps, and asks
//! for the other one: a press sends `f(!on)`. It shows what it's given,
//! as every widget of the toolkit does.
//!
//!     icssed::toggle(self.muted).on_toggle(Message::Mute).icon(speaker).label("Mute")
//!     icssed::toggle(self.hidden).on_toggle(Message::Hidden).label("Show hidden").checkbox()
//!     icssed::toggle(self.wifi).on_toggle(Message::Wifi).switch()
//!     icssed::toggle(self.sort == By::Name).on_toggle(|_| Message::Sort(By::Name)).radio()
//!
//! It looks one of four ways, picked by the call, each its own name for
//! the sheet: a `toggle` (a button that stays pressed), a `checkbox`
//! (`.checkbox()`), a `radio` (`.radio()`), a `switch` (`.switch()`).
//! The four are the same tree, `indicator > mark` first, then `.icon(..)`
//! and `.label(..)`, both optional: the indicator is the box, the circle,
//! the switch's track; the mark the tick, the dot, the knob. The base
//! hides a toggle's indicator (`display: none`) and slides a switch's
//! knob with a transform, a transition.
//!
//! It's `:checked` while it's on, whatever the look, which its parts
//! see: `checkbox:checked > indicator > mark` is a checked box's tick,
//! `toggle:checked > label` a pressed button's label.
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
            toggle(state.muted).on_toggle(Message::Mute)
                .icon(assets.star.clone())
                .label(if state.muted { "Muted" } else { "Mute" }),
            toggle(true).on_toggle(Message::Mute).label("Disabled").disabled_if(true),
        ]
        .kind("line"),
        row![
            toggle(state.hidden).on_toggle(Message::Hidden)
                .label("Show hidden files")
                .checkbox(),
            toggle(true).on_toggle(Message::Hidden)
                .label("Disabled")
                .checkbox()
                .disabled_if(true),
        ]
        .kind("line"),
        row![
            toggle(state.wifi).on_toggle(Message::Wifi).label("Wi-Fi").switch(),
            toggle(true).on_toggle(Message::Wifi)
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
        look(toggle(i == state.sort).on_toggle(move |_| Message::Sort(i)))
            .label(by)
            .into()
    });
    row(toggles).kind("line").into()
}
