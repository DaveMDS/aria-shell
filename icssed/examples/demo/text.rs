//! A text is a paragraph: `icssed::text(content)`, a `label`. Its font, size,
//! weight, colour and alignment are the sheet's.
//!
//!     icssed::text(&item.name).class("muted")
//!     icssed::text(&item.size).kind("size")
//!
//! - It wraps at words by its box's width; `white-space: nowrap` keeps
//!   it on one line, and `text-overflow: ellipsis` cuts it with an
//!   ellipsis where it doesn't fit.
//! - A text has no padding, margin or border: put it in a box for those.
//! - It's centred on its letters: every element's `text-box` is
//!   `trim-both cap alphabetic`, so a label's box runs from its
//!   capitals' top to its baseline, and an icon beside it lines up with
//!   what's seen, not with the line box. `text-box: normal` gives the
//!   line box back (the last row below).
//! - `text-transform` changes the case, as the theme likes.
//! - It's shaped once and kept: a text that doesn't change costs nothing
//!   at the next view.
//!
//! `icssed::mark()` is a text whose words are the sheet's
//! (`content`): a check, an arrow, the bullets of this demo's Info.

use std::borrow::Cow;

use icssed::{Element, column, icon, row, text};

use crate::Assets;

pub const TITLE: &str = "Text";

/// Long enough to wrap, or to be cut.
const LONG: &str = "A text long enough to wrap at its box's width, words kept whole, \
    lines a whole number of device pixels apart.";

#[derive(Default)]
pub struct State;

#[derive(Debug, Clone)]
pub enum Message {}

pub fn update(_state: &mut State, message: Message) {
    match message {}
}

pub fn view<'a>(_state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let boxed = |class: &str, content: Cow<'static, str>| row![text(content)].kind("box").class(class);
    let pair = |untrimmed: bool, content: &'static str| {
        row![icon(assets.star.clone()), text(content)]
            .kind("pair")
            .class_if("untrimmed", untrimmed)
    };
    column![
        row![
            text("Small").class("small"),
            text("Regular"),
            text("Large").class("large"),
            text("Bold").class("bold"),
            text("Italic").class("italic"),
            text("Muted").class("muted"),
            text("Upper case").class("caps"),
        ]
        .kind("line"),
        row![
            boxed("wrap", LONG.into()),
            boxed("cut", LONG.into()),
            boxed("center", "Centred\nin its box".into()),
        ]
        .kind("line"),
        pair(false, "Trimmed: Centred On Its Letters"),
        pair(true, "Untrimmed: Centred On Its Line Box"),
    ]
    .into()
}
