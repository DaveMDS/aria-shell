//! Two ways to move, both CSS's. A `transition` animates a change: when a
//! state, a class or an ancestor's state changes a colour, an opacity, a
//! shadow, a transform, it eases from the old value to the new one. An
//! `animation` runs `@keyframes`: when the element gets it (its first
//! frame, a class added), as many times as it says.
//!
//!     toast { animation: slide-in 250ms ease-out; }
//!     toast.leaving { animation: fade-out 200ms ease-in forwards; }
//!
//! - What moves: colours, opacity, box-shadow, transform, rotate. Layout
//!   never does: a padding changes at once.
//! - A transition reversed half-way takes half the time back, as in CSS.
//! - An animation asks for frames only while it runs; an idle UI costs
//!   nothing.
//! - `.on_animation_end(m)` on a box sends `m` when its animation ends:
//!   how a view knows an exit animation is done.
//!
//! An exit animation: the app adds a class (`.leaving`), the sheet's
//! animation runs, and on `on_animation_end` the app removes the element,
//! or closes its surface. The toast below leaves that way; the box shakes
//! on demand, its class taken off when the shake ends so it can shake
//! again.

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Animation";

#[derive(Default)]
pub struct State {
    /// The box shakes until its animation ends.
    shaking: bool,
    /// The toast, shown: `true` while it leaves.
    toast: Option<bool>,
    /// The switch's lamp, lit.
    lit: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Shake,
    Shaken,
    Toast,
    Dismiss,
    /// The toast's animation ended: in, or out.
    Toasted,
    Lit(bool),
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::Shake => state.shaking = true,
        Message::Shaken => state.shaking = false,
        Message::Toast => state.toast = Some(false),
        Message::Dismiss => state.toast = state.toast.map(|_| true),
        Message::Toasted => {
            if state.toast == Some(true) {
                state.toast = None;
            }
        }
        Message::Lit(lit) => state.lit = lit,
    }
}

pub fn view<'a>(state: &'a State, node: Node<'a>, assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    let labelled = |node: Node<'a>, text: &'static str| -> Element<'a, Message> {
        icssed::text(node.child("label"), text).into()
    };
    let pulse = line.child("pulse");
    let shaken = line
        .child("box")
        .class("shake")
        .class_if("shaking", state.shaking);
    let shake = line.child("button");
    let lamp = line.child("lamp").class_if("lit", state.lit);
    let toggle = node.child("line").child("button");
    let mut toasts: Vec<Element<'a, Message>> = vec![
        icssed::button(
            toggle,
            [labelled(
                toggle,
                if state.toast.is_some() {
                    "Dismiss"
                } else {
                    "Toast"
                },
            )],
        )
        .on_press(if state.toast.is_some() {
            Message::Dismiss
        } else {
            Message::Toast
        })
        .into(),
    ];
    if let Some(leaving) = state.toast {
        let toast = line.child("toast").class_if("leaving", leaving);
        toasts.push(
            icssed::row(
                toast,
                [
                    icssed::icon(toast.child("icon"), assets.star.clone()).into(),
                    labelled(toast, "A toast: in, and out when dismissed"),
                ],
            )
            .on_animation_end(Message::Toasted)
            .into(),
        );
    }
    icssed::column(
        node,
        [
            icssed::row(
                line,
                [
                    icssed::icon(line.child("spinner"), assets.star.clone()).into(),
                    icssed::row(pulse, [labelled(pulse, "Pulse")]).into(),
                    icssed::row(shaken, [labelled(shaken, "Shaken on demand")])
                        .on_animation_end(Message::Shaken)
                        .into(),
                    icssed::button(shake, [labelled(shake, "Shake")])
                        .on_press(Message::Shake)
                        .into(),
                ],
            )
            .into(),
            icssed::row(
                line,
                [
                    icssed::toggle(line.child("toggle"), state.lit, Message::Lit)
                        .switch()
                        .into(),
                    icssed::row(lamp, [labelled(lamp, "A transition: colour, shadow, scale")])
                        .into(),
                ],
            )
            .into(),
            icssed::row(line, toasts).into(),
        ],
    )
    .into()
}
