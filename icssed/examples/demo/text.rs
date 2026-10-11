//! A text is a paragraph: `icssed::text(node, content)`. Its font, size,
//! weight, colour and alignment are the sheet's.
//!
//!     icssed::text(row.child("label").class("muted"), &item.name)
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
//! `icssed::mark(node)` is a text whose words are the sheet's
//! (`content`): a check, an arrow, the bullets of this demo's Info.

use std::borrow::Cow;

use icssed::{Element, Node};

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

pub fn view<'a>(_state: &'a State, node: Node<'a>, assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    let label = |class: &str, text: &'static str| -> Element<'a, Message> {
        icssed::text(line.child("label").class_if(class, !class.is_empty()), text).into()
    };
    let boxed = |class: &str, text: Cow<'static, str>| -> Element<'a, Message> {
        let boxed = line.child("box").class(class);
        icssed::row(boxed, [icssed::text(boxed.child("label"), text).into()]).into()
    };
    let pair = |untrimmed: bool, text: &'static str| -> Element<'a, Message> {
        let pair = node.child("pair").class_if("untrimmed", untrimmed);
        icssed::row(
            pair,
            [
                icssed::icon(pair.child("icon"), assets.star.clone()).into(),
                icssed::text(pair.child("label"), text).into(),
            ],
        )
        .into()
    };
    icssed::column(
        node,
        [
            icssed::row(
                line,
                [
                    label("small", "Small"),
                    label("", "Regular"),
                    label("large", "Large"),
                    label("bold", "Bold"),
                    label("italic", "Italic"),
                    label("muted", "Muted"),
                    label("caps", "Upper case"),
                ],
            )
            .into(),
            icssed::row(
                line,
                [
                    boxed("wrap", LONG.into()),
                    boxed("cut", LONG.into()),
                    boxed("center", "Centred\nin its box".into()),
                ],
            )
            .into(),
            pair(false, "Trimmed: Centred On Its Letters"),
            pair(true, "Untrimmed: Centred On Its Line Box"),
        ],
    )
    .into()
}
