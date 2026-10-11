//! A view builds nodes top-down from its surface's root, and hands each
//! widget its node: the node is the element the sheet's selectors match.
//!
//!     let root = sheet.root("popup", scale);
//!     let list = root.child("list");
//!     let row = list.child_at("entry", i, n).class_if("active", i == shown);
//!
//! - `child(kind)` is a child element of that kind: `list > entry`.
//! - `class(c)`, `class_if(c, on)`, `id(i)`, `attr(name, value)` are
//!   what `.c`, `#i` and `[name="value"]` match.
//! - `child_at(kind, i, n)` is the i-th of n: what `:first-child`,
//!   `:last-child` and `:nth-child(an+b)` match. In a sheet without them
//!   it's just `child`.
//! - `disabled_if(off)` makes it `:disabled`.
//!
//! A node is an interned path: each step is a hash lookup, never an
//! allocation after the first time, and its style is computed once and
//! shared by every element on the same path in the same states. 500
//! rows on one path cost one style. Positional pseudo-classes split them
//! a little (first, last, odd, even), each resolved once.
//!
//! Element types are global, as in CSS: an app's own `track`, `label`,
//! `item` gets the base's rules for the toolkit's parts of that name.
//! Scope rules (`list > item`), or pick names the base doesn't use.

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Nodes and selectors";

/// The rows: a name, and whether it's a folder.
const FILES: [(&str, bool); 6] = [
    ("Documents", true),
    ("Pictures", true),
    ("notes.txt", false),
    ("Music", true),
    ("todo.md", false),
    ("report.pdf", false),
];

#[derive(Default)]
pub struct State {
    /// The row picked, an index in `FILES`.
    picked: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Pick(usize),
}

pub fn update(state: &mut State, message: Message) {
    let Message::Pick(index) = message;
    state.picked = Some(index);
}

pub fn view<'a>(state: &'a State, node: Node<'a>, assets: &'a Assets) -> Element<'a, Message> {
    let list = node.child("list");
    let rows = FILES.iter().enumerate().map(|(i, (name, folder))| {
        let entry = list
            .child_at("entry", i, FILES.len())
            .class_if("picked", state.picked == Some(i))
            .attr("kind", if *folder { "folder" } else { "file" });
        icssed::button(
            entry,
            [
                icssed::icon(entry.child("icon"), assets.star.clone()).into(),
                icssed::text(entry.child("label"), *name).into(),
            ],
        )
        .on_press(Message::Pick(i))
        .into()
    });
    let picked = match state.picked {
        Some(i) => format!(
            "list > entry:nth-child({}).picked[kind=\"{}\"]",
            i + 1,
            if FILES[i].1 { "folder" } else { "file" }
        ),
        None => "Pick a row".into(),
    };
    icssed::column(
        node,
        [
            icssed::column(list, rows).into(),
            icssed::text(node.child("status"), picked).into(),
        ],
    )
    .into()
}
