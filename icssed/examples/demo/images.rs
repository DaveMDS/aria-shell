//! Pictures come two ways: as an element, `icssed::image(node, handle)`,
//! or as a box's background, from the sheet.
//!
//!     icssed::image(node.child("picture").class("cover"), self.photo.clone())
//!
//! - `object-fit` says how an image fills its box: `fill` stretches it,
//!   `contain` shows it whole, `cover` fills the box and cuts the rest;
//!   `border-radius` rounds it.
//! - The handle is the app's: decoded its own way (here a png, read once
//!   at start), or made in memory.
//!
//! A box's `background` takes `url()`, found from its sheet's directory:
//! tiled (`background-repeat`), placed (`background-position`), sized
//! (`background-size: cover | contain | <length>`). `border-image` draws a
//! 9-patch in place of the border, its middle with `fill`: a frame that
//! keeps its corners at any size. The three boxes below are rows with a
//! label, all of it in `images.css`.

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Images";

#[derive(Default)]
pub struct State;

#[derive(Debug, Clone)]
pub enum Message {}

pub fn update(_state: &mut State, message: Message) {
    match message {}
}

pub fn view<'a>(_state: &'a State, node: Node<'a>, assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    let fitted = |class: &str| -> Element<'a, Message> {
        let cell = line.child("fitted");
        icssed::column(
            cell,
            [
                icssed::image(cell.child("picture").class(class), assets.photo.clone()).into(),
                icssed::text(cell.child("label"), format!("object-fit: {class}")).into(),
            ],
        )
        .into()
    };
    let boxed = |class: &str, text: &'static str| -> Element<'a, Message> {
        let boxed = line.child("box").class(class);
        icssed::row(boxed, [icssed::text(boxed.child("label"), text).into()]).into()
    };
    icssed::column(
        node,
        [
            icssed::row(line, [fitted("fill"), fitted("contain"), fitted("cover")]).into(),
            icssed::row(
                line,
                [
                    boxed("tiled", "Tiled"),
                    boxed("framed", "A 9-patch"),
                    boxed("scenery", "Cover"),
                ],
            )
            .into(),
        ],
    )
    .into()
}
