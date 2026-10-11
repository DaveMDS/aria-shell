//! Every widget is an element of the sheet's tree, as in a document: a
//! type, classes, an id, attributes. The view says them on the widget;
//! where the element is, under which parent, the toolkit finds: the
//! container it's put in.
//!
//!     column(rows).kind("list")
//!     button![icon(star), text(name)]
//!         .kind("entry")
//!         .class_if("picked", picked)
//!         .attr("kind", "folder")
//!
//! - Each widget has a type of its own (`text` is a `label`, `icon` an
//!   `icon`, `row` a `row`); `.kind(k)` gives another: `list > entry`.
//! - `.class(c)`, `.class_if(c, on)`, `.named(i)`, `.attr(name, value)`
//!   are what `.c`, `#i` and `[name="value"]` match. Said before the
//!   children or after, the children see them all the same.
//! - Its place among its siblings is known: `:first-child`,
//!   `:last-child` and `:nth-child(an+b)` match with nothing said.
//! - `.disabled_if(off)` makes it `:disabled`.
//! - The top of a view is given its place: `.at(root)`, a surface's root
//!   (`sheet.root("popup", scale)`).
//! - `icssed::slot(view)` says what a view is, from where it's put: this
//!   page is `slot(page).kind("demo").named("elements")`, a bar's gadget
//!   would be `slot(view).kind("gadget").class("clock")`.
//!
//! An element's path is interned: each step is a hash lookup, never an
//! allocation after the first time, and its style is computed once and
//! shared by every element on the same path in the same states. 500
//! rows on one path cost one style. Positional pseudo-classes split them
//! a little (first, last, odd, even), each resolved once.
//!
//! Element types are global, as in CSS: an app's own `track`, `label`,
//! `item` gets the base's rules for the toolkit's parts of that name.
//! Scope rules (`list > item`), or pick names the base doesn't use.

use icssed::{Element, button, column, icon, text};

use crate::Assets;

pub const TITLE: &str = "Elements and selectors";

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

pub fn view<'a>(state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let rows = FILES.iter().enumerate().map(|(i, (name, folder))| {
        button![icon(assets.star.clone()), text(*name)]
            .kind("entry")
            .class_if("picked", state.picked == Some(i))
            .attr("kind", if *folder { "folder" } else { "file" })
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
    column![column(rows).kind("list"), text(picked).kind("status")].into()
}
