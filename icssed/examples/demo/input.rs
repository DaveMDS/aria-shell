//! An input is a line of text to edit:
//! `icssed::input(placeholder, value)`, iced's text input in a box
//! of the toolkit. The app holds the text and gets every change.
//!
//!     icssed::input("Search", &self.search)
//!         .on_input(Message::Search)
//!         .on_submit(Message::Go)
//!         .leading(icssed::icon(search_icon))
//!
//! - Without `on_input` it's read-only.
//! - `.on_submit(m)` is sent on Enter.
//! - `.secure(true)` hides the text: a password.
//! - `.leading(e)` and `.trailing(e)` put the app's children before and
//!   after the text: an icon, a button to peek at a password.
//! - `.id(id)` names it for iced's focus operations: an app focuses it
//!   when its surface opens.
//!
//! The box is `:focus` while its text has the focus (CSS's
//! `:focus-within`), and a press anywhere on it puts the focus there.
//! The text itself is `input > field`, with its `placeholder` and its
//! `selection`: iced draws them, so their colours don't transition.

use icssed::{Element, button, column, icon, input, row, text};

use crate::Assets;

pub const TITLE: &str = "Input";

pub struct State {
    search: String,
    password: String,
    /// The password shown.
    peek: bool,
    /// What Enter last sent.
    submitted: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            search: String::new(),
            password: "hunter2".into(),
            peek: false,
            submitted: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Search(String),
    Submit,
    Password(String),
    Peek,
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::Search(text) => state.search = text,
        Message::Submit => state.submitted = Some(state.search.clone()),
        Message::Password(text) => state.password = text,
        Message::Peek => state.peek = !state.peek,
    }
}

pub fn view<'a>(state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let submitted = match &state.submitted {
        Some(text) => format!("Submitted: {text:?}"),
        None => "Type, then Enter".into(),
    };
    column![
        row![
            input("Search", &state.search)
                .on_input(Message::Search)
                .on_submit(Message::Submit)
                .leading(icon(assets.star.clone())),
            input("Password", &state.password)
                .on_input(Message::Password)
                .secure(!state.peek)
                .trailing(
                    button![icon(assets.eye.clone())]
                        .class("peek")
                        .class_if("active", state.peek)
                        .on_press(Message::Peek),
                ),
            input("Disabled", "")
                .disabled_if(true)
                .on_input(Message::Search),
        ]
        .kind("line"),
        text(submitted).kind("status"),
    ]
    .into()
}
