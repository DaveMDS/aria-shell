//! Pictures come two ways: as an element, `icssed::image(handle)`,
//! or as a box's background, from the sheet.
//!
//!     icssed::image(self.photo.clone()).class("cover")
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

use icssed::{Element, column, image, row, text};

use crate::Assets;

pub const TITLE: &str = "Images";

#[derive(Default)]
pub struct State;

#[derive(Debug, Clone)]
pub enum Message {}

pub fn update(_state: &mut State, message: Message) {
    match message {}
}

pub fn view<'a>(_state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let fitted = |class: &str| {
        column![
            image(assets.photo.clone()).kind("picture").class(class),
            text(format!("object-fit: {class}")),
        ]
        .kind("fitted")
    };
    let boxed = |class: &str, content: &'static str| row![text(content)].kind("box").class(class);
    column![
        row![fitted("fill"), fitted("contain"), fitted("cover")].kind("line"),
        row![
            boxed("tiled", "Tiled"),
            boxed("framed", "A 9-patch"),
            boxed("scenery", "Cover"),
        ]
        .kind("line"),
    ]
    .into()
}
