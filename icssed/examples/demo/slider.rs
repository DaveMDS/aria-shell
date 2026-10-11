//! A slider picks a value in a range:
//! `icssed::slider(0.0..=100.0, value, Message::Volume)`. Drag the
//! knob, click the track, turn the wheel over it, or use the keys once
//! it has the focus: the arrows, Page Up and Down, Home, End.
//!
//!     icssed::slider(0.0..=100.0, self.volume, Message::Volume)
//!         .step(1.0)
//!         .on_release(Message::Commit)
//!
//! - `.step(s)` rounds the value to steps; without it, any value.
//! - `.on_release(m)` sends `m` when a drag ends: for what's costly to
//!   do at every move (setting a device's volume).
//! - It's `:active` while dragged.
//!
//! Its parts: the `track`, a meter filled up to the value (`track >
//! fill`), and the `knob`, centred on the value. The slider's padding is
//! the room the knob overhangs into at both ends: half the knob's width.

use icssed::{Element, column, row, slider, text};

use crate::Assets;

pub const TITLE: &str = "Slider";

pub struct State {
    /// 0 to 100.
    volume: f32,
    /// Where the last drag ended.
    released: Option<f32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            volume: 40.0,
            released: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Volume(f32),
    Released,
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::Volume(volume) => state.volume = volume,
        Message::Released => state.released = Some(state.volume),
    }
}

pub fn view<'a>(state: &'a State, _assets: &'a Assets) -> Element<'a, Message> {
    let released = match state.released {
        Some(volume) => format!("Released at {volume:.0}%"),
        None => "Not released yet".into(),
    };
    column![
        row![
            slider(0.0..=100.0, state.volume, Message::Volume)
                .step(1.0)
                .on_release(Message::Released),
            text(format!("Volume {:.0}%", state.volume)),
        ]
        .kind("line"),
        row![
            slider(0.0..=1.0, 0.3, Message::Volume).disabled_if(true),
            text("Disabled"),
        ]
        .kind("line"),
        text(released).kind("status"),
    ]
    .into()
}
