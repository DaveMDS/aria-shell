//! A meter is a bar filled to a fraction:
//! `icssed::meter(node, fraction)`, 0 to 1. A battery's charge, a disk's
//! use; with `.label(text)` over it, a gauge.
//!
//!     icssed::meter(row.child("meter").class_if("warning", low), charge)
//!         .label(format!("{:.0}%", charge * 100.0))
//!
//! - Its `fill` is the fraction of its content box wide, as tall as it.
//! - The label is placed as in a row: `justify-content` and
//!   `align-items` on the meter place it.
//! - Its states are the app's to give, as classes: `.warning` below is
//!   this demo's, coloured with the palette's `--warning`.
//!
//! The slider moves the first one: the meter itself takes no input.

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Meter";

pub struct State {
    /// 0 to 1.
    level: f32,
}

impl Default for State {
    fn default() -> Self {
        Self { level: 0.4 }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Level(f32),
}

pub fn update(state: &mut State, message: Message) {
    let Message::Level(level) = message;
    state.level = level;
}

pub fn view<'a>(state: &'a State, node: Node<'a>, _assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    let warning = state.level > 0.8;
    icssed::column(
        node,
        [
            icssed::row(
                line,
                [
                    icssed::meter(line.child("meter").class_if("warning", warning), state.level)
                        .into(),
                    icssed::slider(line.child("slider"), 0.0..=1.0, state.level, Message::Level)
                        .step(0.01)
                        .into(),
                    icssed::text(
                        line.child("label"),
                        if warning { "Over 80%: .warning" } else { "Up to 80%" },
                    )
                    .into(),
                ],
            )
            .into(),
            icssed::row(
                line,
                [
                    icssed::meter(line.child("meter").class("gauge"), 0.42)
                        .label("CPU 42%")
                        .into(),
                    icssed::meter(line.child("meter").class("warning"), 0.85).into(),
                ],
            )
            .into(),
        ],
    )
    .into()
}
