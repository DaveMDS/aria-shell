//! A select is one of a list of options:
//! `icssed::select(node, options, selected, Message::Choose)`, the
//! choice an index the app holds. Pressed, it opens its options under it
//! as a menu (over it where there's no room), the one chosen checked.
//!
//!     icssed::select(row.child("select"), ["Light", "Dark"], self.scheme, Message::Scheme)
//!         .placeholder("Pick a scheme")
//!
//! - `.placeholder(t)` is shown while nothing is chosen (`None`).
//! - It's `:active` while its options are open.
//! - Focused and closed, Up and Down choose the option before or after.
//! - The label is cut with an ellipsis when the choice is too long.
//!
//! In a window the options are an iced overlay, drawn over everything.
//! An app whose surfaces are small (a bar) gives it `.on_open(open, f)`:
//! it's told where the options should show, opens a surface of its own
//! with an `icssed::menu` there, and tells the select whether they're
//! open.

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Select";

const SCHEMES: [&str; 4] = [
    "Follow the system",
    "Light",
    "Dark",
    "A choice far too long for its box",
];

#[derive(Default)]
pub struct State {
    /// An index in `SCHEMES`.
    choice: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Choose(usize),
}

pub fn update(state: &mut State, message: Message) {
    let Message::Choose(choice) = message;
    state.choice = Some(choice);
}

pub fn view<'a>(state: &'a State, node: Node<'a>, _assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    let chosen = match state.choice {
        Some(i) => format!("Chosen: {}", SCHEMES[i]),
        None => "Nothing chosen yet".into(),
    };
    icssed::column(
        node,
        [
            icssed::row(
                line,
                [
                    icssed::select(line.child("select"), SCHEMES, state.choice, Message::Choose)
                        .placeholder("Pick a scheme")
                        .into(),
                    icssed::select(
                        line.child("select").disabled_if(true),
                        ["Disabled"],
                        Some(0),
                        Message::Choose,
                    )
                    .into(),
                ],
            )
            .into(),
            icssed::text(node.child("status"), chosen).into(),
        ],
    )
    .into()
}
