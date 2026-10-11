//! Tabs are a row of buttons, one per page:
//! `icssed::tabs(node, [Tab::new(label, message).active(shown)])`. The
//! page shown is the app's: the tabs mark it `.active` and send a
//! message when another is pressed.
//!
//!     icssed::tabs(node.child("tabs"), PAGES.iter().enumerate().map(|(i, page)| {
//!         Tab::new(page.title, Message::Tab(i)).active(i == self.tab)
//!     }))
//!
//! - The row draws the line under them; a tab its own mark, the active
//!   one's in the base.
//! - Left and Right move the focus among them once one has it.
//! - `.class(c)` styles one tab.
//!
//! The pages are the app's too: the tabs show no content, the view puts
//! the page shown under them. This demo's own Info, Source and Style are
//! tabs.

use icssed::{Element, Node, Tab};

use crate::Assets;

pub const TITLE: &str = "Tabs";

const PAGES: [(&str, &str); 3] = [
    (
        "Overview",
        "The first page: Left and Right move among the tabs once one has the focus.",
    ),
    ("Details", "The second page, its tab marked by the theme."),
    ("Settings", "The third page."),
];

#[derive(Default)]
pub struct State {
    /// The page shown, an index in `PAGES`.
    tab: usize,
}

#[derive(Debug, Clone)]
pub enum Message {
    Tab(usize),
}

pub fn update(state: &mut State, message: Message) {
    let Message::Tab(tab) = message;
    state.tab = tab;
}

pub fn view<'a>(state: &'a State, node: Node<'a>, _assets: &'a Assets) -> Element<'a, Message> {
    icssed::column(
        node,
        [
            icssed::tabs(
                node.child("tabs"),
                PAGES.iter().enumerate().map(|(i, (title, _))| {
                    Tab::new(*title, Message::Tab(i)).active(i == state.tab)
                }),
            ),
            icssed::text(node.child("page"), PAGES[state.tab].1).into(),
        ],
    )
    .into()
}
