//! States are the widget's, classes the view's. A widget follows the
//! pointer and the keyboard itself, and the sheet sees it as
//! pseudo-classes: `:hover`, `:active` (pressed), `:focus`. `:disabled`
//! is the view's to say, with `disabled_if`.
//!
//!     tile:hover { background: var(--border); }
//!     tile:active { transform: scale(0.95); }
//!     tile:focus { outline: 2px solid var(--accent); }
//!     tile:hover > label { color: var(--accent); }
//!
//! - Any box follows them: the tiles below are buttons of the demo's
//!   own kind, `tile`, styled by this demo's sheet alone.
//! - What a state changes, the descendants inherit, and a rule may name
//!   an ancestor's state: `tile:hover > label`.
//! - A state may change its own element's size (padding, width), never
//!   its descendants': layout under an ancestor's state isn't applied.
//!   Paint (colours, shadows, transforms, opacity) follows any state.
//! - Unlike CSS, a disabled element is never `:hover` or `:active`.
//! - States are tracked only on nodes some rule can see them on: a
//!   pointer moving over UI no rule cares about asks for no frame.
//!
//! State a widget holds at runtime shows the same way: an open select is
//! `:active`, an input whose text has the focus is `:focus`.

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "States";

#[derive(Default)]
pub struct State {
    presses: u32,
}

#[derive(Debug, Clone)]
pub enum Message {
    Press,
}

pub fn update(state: &mut State, message: Message) {
    let Message::Press = message;
    state.presses += 1;
}

pub fn view<'a>(state: &'a State, node: Node<'a>, assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    let tile = |class: &str, text: &'static str, disabled: bool| -> Element<'a, Message> {
        let tile = line.child("tile").class(class).disabled_if(disabled);
        icssed::button(
            tile,
            [
                icssed::icon(tile.child("icon"), assets.star.clone()).into(),
                icssed::text(tile.child("label"), text).into(),
            ],
        )
        .on_press(Message::Press)
        .into()
    };
    icssed::column(
        node,
        [
            icssed::row(
                line,
                [
                    tile("hover", "Hover me", false),
                    tile("press", "Press me", false),
                    tile("focus", "Tab to me", false),
                    tile("grow", "I grow", false),
                    tile("hover", "Disabled", true),
                ],
            )
            .into(),
            icssed::text(
                node.child("status"),
                format!("Pressed {} times", state.presses),
            )
            .into(),
        ],
    )
    .into()
}
