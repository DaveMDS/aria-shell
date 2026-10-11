//! A toggle is a switch: `icssed::toggle(node, on, Message::Wifi)`. It
//! sends the new value, and the app holds it: the toggle shows what it's
//! given, as every widget of the toolkit does.
//!
//!     icssed::toggle(row.child("toggle"), self.wifi, Message::Wifi)
//!
//! - It's a button holding its `knob`, `.on` when it is: the knob slides
//!   with a transform, a transition in the base.
//! - The knob's travel is the toggle's width less its padding and the
//!   knob's own width: a theme that resizes one resizes the travel too.
//! - Tab gives it the focus, Space and Enter flip it.
//! - `disabled_if(true)` on its node: it sends nothing.
//!
//! Its label is the app's, beside it in a row: a toggle has no text of
//! its own.

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Toggle";

pub struct State {
    wifi: bool,
    airplane: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            wifi: true,
            airplane: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Wifi(bool),
    Airplane(bool),
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::Wifi(on) => state.wifi = on,
        Message::Airplane(on) => state.airplane = on,
    }
}

pub fn view<'a>(state: &'a State, node: Node<'a>, _assets: &'a Assets) -> Element<'a, Message> {
    let line = |toggle: Element<'a, Message>, text: &'static str| -> Element<'a, Message> {
        let line = node.child("line");
        icssed::row(line, [toggle, icssed::text(line.child("label"), text).into()]).into()
    };
    let toggle = node.child("line").child("toggle");
    icssed::column(
        node,
        [
            line(
                icssed::toggle(toggle, state.wifi, Message::Wifi).into(),
                if state.wifi { "Wi-Fi: on" } else { "Wi-Fi: off" },
            ),
            line(
                icssed::toggle(toggle, state.airplane, Message::Airplane).into(),
                "Airplane mode",
            ),
            line(
                icssed::toggle(toggle.disabled_if(true), true, Message::Wifi).into(),
                "Disabled",
            ),
        ],
    )
    .into()
}
