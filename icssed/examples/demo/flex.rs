//! Every box lays its children out as CSS's flexbox does, a subset of
//! it: `icssed::row` along the width, `icssed::column` along the height
//! (`flex-direction` in the sheet changes it).
//!
//! - `gap` puts room between the children, `padding` around them.
//! - `justify-content` (start, center, end) places them along the box
//!   when there's room left.
//! - `align-items` (stretch, start, center, end) places each one across
//!   it.
//! - `width: fill` (or `height: fill` in a column) makes a child take the
//!   room left, as `flex-grow: 1` does; children that fill share it. It's
//!   how a row puts something at its end: never a spacer.
//! - Children shrink down to their content when the room runs out, and
//!   `min-width` keeps one from it.
//!
//! The controls below change the lane's classes; the sheet has a rule for
//! each. Change `flex.css` while the demo runs: it's read again on save.

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Flex";

const JUSTIFY: [&str; 3] = ["start", "center", "end"];
const ALIGN: [&str; 4] = ["stretch", "start", "center", "end"];

#[derive(Default)]
pub struct State {
    /// An index in `JUSTIFY`.
    justify: usize,
    /// An index in `ALIGN`.
    align: usize,
    /// The first cell takes the room left.
    fill: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Justify(usize),
    Align(usize),
    Fill(bool),
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::Justify(index) => state.justify = index,
        Message::Align(index) => state.align = index,
        Message::Fill(fill) => state.fill = fill,
    }
}

pub fn view<'a>(state: &'a State, node: Node<'a>, _assets: &'a Assets) -> Element<'a, Message> {
    let controls = node.child("controls");
    let label = |text: &'static str| -> Element<'a, Message> {
        icssed::text(controls.child("label"), text).into()
    };
    let justify = format!("justify-{}", JUSTIFY[state.justify]);
    let align = format!("align-{}", ALIGN[state.align]);
    let lane = node.child("lane").class(&justify).class(&align);
    let cell = |class: &str, text: &'static str| -> Element<'a, Message> {
        let cell = lane.child("cell").class(class);
        icssed::row(cell, [icssed::text(cell.child("label"), text).into()]).into()
    };
    icssed::column(
        node,
        [
            icssed::row(
                controls,
                [
                    label("justify-content"),
                    icssed::select(
                        controls.child("select"),
                        JUSTIFY,
                        Some(state.justify),
                        Message::Justify,
                    )
                    .into(),
                    label("align-items"),
                    icssed::select(
                        controls.child("select"),
                        ALIGN,
                        Some(state.align),
                        Message::Align,
                    )
                    .into(),
                    label("The first fills"),
                    icssed::toggle(controls.child("toggle"), state.fill, Message::Fill).into(),
                ],
            )
            .into(),
            icssed::row(
                lane,
                [
                    cell(if state.fill { "fill" } else { "short" }, "One"),
                    cell("tall", "Two"),
                    cell("mid", "Three"),
                ],
            )
            .into(),
            icssed::text(
                node.child("status"),
                format!("lane.{justify}.{align}"),
            )
            .into(),
        ],
    )
    .into()
}
