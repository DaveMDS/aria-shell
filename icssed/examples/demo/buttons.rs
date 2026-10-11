//! A button is a box that sends a message when pressed: `icssed::button`
//! takes its node and its children, `.on_press(message)` makes it a
//! button. Without `on_press` it's a plain box.
//!
//!     let node = row.child("button").class("primary");
//!     icssed::button(node, [
//!         icssed::icon(node.child("icon"), star).into(),
//!         icssed::text(node.child("label"), "Save").into(),
//!     ])
//!     .on_press(Message::Save)
//!
//! Everything it looks like comes from the sheet. The toolkit's base
//! styles the `button` kind and its classes `.flat`, `.primary` and
//! `.danger`; an app adds its own classes and rules.
//!
//! - `:hover`, `:active` and `:focus` follow the pointer and the
//!   keyboard: Tab moves the focus, Enter and Space press.
//! - `disabled_if(true)` on the node makes it `:disabled`: it sends
//!   nothing and takes no focus.
//! - What a state changes, the children inherit: `button:hover { color }`
//!   reaches its label and its icon.
//! - A state may change a box's own size (the last button grows), never
//!   its children's: those follow only their own states.
//!
//! Any box can follow the pointer: the card below is a `row` whose
//! `:hover` rule changes its background and its children's colour, with
//! a transition.

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Buttons";

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

pub fn view<'a>(state: &'a State, node: Node<'a>, assets: &'a Assets) -> Element<'a, Message> {
    let row = node.child("row");
    let button = |class: &str, label: &'static str, disabled: bool| -> Element<'a, Message> {
        let button = row
            .child("button")
            .class_if(class, !class.is_empty())
            .disabled_if(disabled);
        icssed::button(
            button,
            [
                icssed::icon(button.child("icon"), assets.star.clone()).into(),
                icssed::text(button.child("label"), label).into(),
            ],
        )
        .on_press(Message::Pressed(label))
        .into()
    };
    let pressed = match state.pressed {
        None => "Nothing pressed yet: click, or Tab then Enter".to_string(),
        Some((name, 1)) => format!("Pressed: {name}"),
        Some((name, n)) => format!("Pressed: {name}, {n} times"),
    };
    let card = node.child("card");
    icssed::column(
        node,
        [
            icssed::row(
                row,
                [
                    button("", "Default", false),
                    button("flat", "Flat", false),
                    button("primary", "Primary", false),
                    button("danger", "Danger", false),
                    button("", "Disabled", true),
                    button("grow", "Grows", false),
                ],
            )
            .into(),
            icssed::text(node.child("status"), pressed).into(),
            icssed::row(
                card,
                [
                    icssed::icon(card.child("icon"), assets.star.clone()).into(),
                    icssed::text(card.child("label"), "Hover the card").into(),
                ],
            )
            .into(),
        ],
    )
    .into()
}
