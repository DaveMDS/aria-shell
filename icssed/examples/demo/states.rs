//! States are the widget's, classes the view's. A widget follows the
//! pointer and the keyboard itself, and the sheet sees it as
//! pseudo-classes: `:hover`, `:active` (pressed), `:focus`,
//! `:focus-visible`. `:disabled` is the view's to say, with
//! `disabled_if`.
//!
//!     tile:hover { background: var(--border); }
//!     tile:active { transform: scale(0.95); }
//!     tile:focus-visible { outline: 2px solid var(--accent); }
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
//! - As in CSS, a click gives `:focus` too; `:focus-visible` is the
//!   keyboard's (Tab, the arrows) and the app's: where a focus ring
//!   goes. Click the focus tile, then Tab to it.
//! - States are tracked only on nodes some rule can see them on: a
//!   pointer moving over UI no rule cares about asks for no frame.
//!
//! State a widget holds at runtime shows the same way: an open select is
//! `:active`, an input whose text has the focus is `:focus` (and
//! `:focus-visible`, always).

use icssed::{Element, button, column, icon, row, text};

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

pub fn view<'a>(state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let tile = |class: &str, content: &'static str| {
        button![icon(assets.star.clone()), text(content)]
            .kind("tile")
            .class(class)
            .on_press(Message::Press)
    };
    column![
        row![
            tile("hover", "Hover me"),
            tile("press", "Press me"),
            tile("focus", "Click or Tab to me"),
            tile("grow", "I grow"),
            tile("hover", "Disabled").disabled_if(true),
        ]
        .kind("line"),
        text(format!("Pressed {} times", state.presses)).kind("status"),
    ]
    .into()
}
