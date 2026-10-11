//! A sheet is compiled from sources, in order: the toolkit's base
//! always first, then the app's stylesheet, then a user's theme.
//!
//!     let (sheet, warnings) = icssed::Sheet::load(
//!         &[
//!             icssed::Source { name: "app.css", text: APP, dir: Some(app_dir) },
//!             icssed::Source { name: &theme_name, text: &theme, dir: theme_dir },
//!         ],
//!         icssed::Scheme::Dark,
//!     )?;
//!
//! - The base is CSS's user-agent origin: any app or theme rule beats it,
//!   whatever the specificity. Then specificity and source order decide,
//!   as in CSS.
//! - A syntax error (a bracket never closed) rejects the whole file: keep
//!   the last good sheet. Anything else that can't be used is a warning
//!   with its file, line and column, and is skipped: log the warnings.
//! - `url()`s are found from a source's `dir`.
//!
//! A sheet is for one colour scheme. Variables live in `:root` and in
//! `:root.light` / `:root.dark`, which hold one scheme's values over the
//! plain ones; every root carries the scheme as a class, so `menu.dark`
//! rules work too. Unlike CSS, variables are global: set in those three
//! only. The swatches below are the base's palette: flip the scheme with
//! the switch at the top, and they follow. A theme that sets only these
//! variables restyles every widget.

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Sheets and palette";

/// The palette's variables, as classes of the swatches.
const VARIABLES: [&str; 9] = [
    "bg",
    "fg",
    "muted",
    "border",
    "accent",
    "on-accent",
    "accent-soft",
    "danger",
    "warning",
];

#[derive(Default)]
pub struct State;

#[derive(Debug, Clone)]
pub enum Message {}

pub fn update(_state: &mut State, message: Message) {
    match message {}
}

pub fn view<'a>(_state: &'a State, node: Node<'a>, _assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    icssed::row(
        line,
        VARIABLES.map(|variable| {
            let cell = line.child("cell");
            icssed::column(
                cell,
                [
                    icssed::row(cell.child("swatch").class(variable), []).into(),
                    icssed::text(cell.child("label"), format!("--{variable}")).into(),
                ],
            )
            .into()
        }),
    )
    .into()
}
