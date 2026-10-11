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

use icssed::{Element, button, column, icon, row, text, toggle};

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

pub fn view<'a>(state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let (label, press) = match state.toast {
        Some(_) => ("Dismiss", Message::Dismiss),
        None => ("Toast", Message::Toast),
    };
    let mut toasts: Vec<Element<'a, Message>> = vec![button![text(label)].on_press(press).into()];
    if let Some(leaving) = state.toast {
        toasts.push(
            row![
                icon(assets.star.clone()),
                text("A toast: in, and out when dismissed"),
            ]
            .kind("toast")
            .class_if("leaving", leaving)
            .on_animation_end(Message::Toasted)
            .into(),
        );
    }
    column![
        row![
            icon(assets.star.clone()).kind("spinner"),
            row![text("Pulse")].kind("pulse"),
            row![text("Shaken on demand")]
                .kind("box")
                .class("shake")
                .class_if("shaking", state.shaking)
                .on_animation_end(Message::Shaken),
            button![text("Shake")].on_press(Message::Shake),
        ]
        .kind("line"),
        row![
            toggle(state.lit, Message::Lit).switch(),
            row![text("A transition: colour, shadow, scale")]
                .kind("lamp")
                .class_if("lit", state.lit),
        ]
        .kind("line"),
        row(toasts).kind("line"),
    ]
    .into()
}
