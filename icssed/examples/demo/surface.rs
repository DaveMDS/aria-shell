//! A popup, a toast, a dialog are widgets: a box with a shadow and its
//! content. The surface they're shown in (a window, a layer surface) is
//! the app's to open: the toolkit knows no Wayland.
//!
//! `icssed::surface(content).at(root)` is a surface's root: its content
//! is the root's box, and it adds the room the box's shadow needs around
//! it, so the shadow isn't cut where the surface ends.
//! `Surface::room(root)` is that room: what an app takes off when it
//! places the surface, so the box, not the shadow, lands where it should
//! (a popup on its bar).
//!
//!     let root = sheet.root("popup", scale);
//!     let content = icssed::surface(column![..]).at(root);
//!     let size = measure.size(POPUP, content, max);
//!
//! A surface's size is its content's, measured: `icssed::Measure` lays an
//! element out with no surface, on a headless renderer, with the real
//! widgets. Keep one `Measure`, and one key per surface, so text is
//! shaped once; `forget(key)` when the surface closes. Build what's
//! measured at the scale it will be shown at, from the same root.
//!
//! Below, the popup is drawn in place: the line around it would be its
//! surface's edge, a `popup` box here rather than a root. The toggle
//! changes its shadow, and the room with it.

use icssed::{Element, column, icon, row, surface, text, toggle};

use crate::Assets;

pub const TITLE: &str = "Surface";

#[derive(Default)]
pub struct State {
    /// The popup's bigger shadow.
    deep: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Deep(bool),
}

pub fn update(state: &mut State, message: Message) {
    let Message::Deep(deep) = message;
    state.deep = deep;
}

pub fn view<'a>(state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let item = |content: &'static str| row![icon(assets.star.clone()), text(content)].kind("entry");
    column![
        row![
            toggle(state.deep, Message::Deep)
                .label("A deeper shadow")
                .checkbox()
        ]
        .kind("controls"),
        row![
            surface(column![item("A popup's content"), item("Its shadow fits")])
                .kind("popup")
                .class_if("deep", state.deep)
        ]
        .kind("edge"),
    ]
    .into()
}
