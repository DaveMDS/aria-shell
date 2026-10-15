//! A button is a box that sends a message when pressed: `button![..]`
//! takes its children, `.on_press(message)` makes it a button. Without
//! `on_press` it's a plain box.
//!
//!     button![icon(star), text("Save")]
//!         .class("primary")
//!         .on_press(Message::Save)
//!
//! Everything it looks like comes from the sheet. The toolkit's base
//! styles the `button` kind and its classes `.flat`, `.primary` and
//! `.danger`; an app adds its own classes and rules.
//!
//! - `:hover`, `:active` and `:focus` follow the pointer and the
//!   keyboard: a click or Tab gives the focus, Enter and Space press;
//!   the base's ring is on `:focus-visible`, the keyboard's only.
//! - A grid of buttons in `focus_scope(grid).arrows()` takes the four
//!   arrows instead, once one of them has the focus: each moves it to
//!   the nearest button that way, round to the other end of the row or
//!   the column (Tab into the grid below, then the arrows).
//! - `disabled_if(true)` makes it `:disabled`: it sends
//!   nothing and takes no focus.
//! - What a state changes, the children inherit: `button:hover { color }`
//!   reaches its label and its icon.
//! - A state may change a box's own size (the last button grows), never
//!   its children's: those follow only their own states.
//!
//! Any box can follow the pointer: the card below is a `row` whose
//! `:hover` rule changes its background and its children's colour, with
//! a transition.

use icssed::{Element, button, column, focus_scope, icon, row, text};

use crate::Assets;

pub const TITLE: &str = "Buttons";

/// The grid's buttons, by row.
const GRID: [[&str; 3]; 2] = [["1", "2", "3"], ["4", "5", "6"]];

#[derive(Default)]
pub struct State {
    /// The last button pressed, and how many times in a row.
    pressed: Option<(&'static str, u32)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Pressed(&'static str),
}

pub fn update(state: &mut State, message: Message) {
    let Message::Pressed(name) = message;
    state.pressed = match state.pressed {
        Some((last, n)) if last == name => Some((name, n + 1)),
        _ => Some((name, 1)),
    };
}

pub fn view<'a>(state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let button = |label: &'static str| {
        button![icon(assets.star.clone()), text(label)].on_press(Message::Pressed(label))
    };
    let pressed = match state.pressed {
        None => "Nothing pressed yet: click, or Tab then Enter".to_string(),
        Some((name, 1)) => format!("Pressed: {name}"),
        Some((name, n)) => format!("Pressed: {name}, {n} times"),
    };
    column![
        row![
            button("Default"),
            button("Flat").class("flat"),
            button("Primary").class("primary"),
            button("Danger").class("danger"),
            button("Disabled").disabled_if(true),
            button("Grows").class("grow"),
        ],
        text(pressed).kind("status"),
        focus_scope(
            column(GRID.map(|line| {
                row(line.map(|name| button![text(name)].on_press(Message::Pressed(name)).into()))
                    .into()
            }))
            .kind("grid"),
        )
        .arrows(),
        row![icon(assets.star.clone()), text("Hover the card")].kind("card"),
    ]
    .into()
}
