//! An icon draws a picture at its element's size: `icssed::icon(source)`.
//! The source is a file, or a name for the app's icons to look up.
//!
//!     icssed::icon(IconFile::Svg { handle, symbolic: true })
//!     icssed::icon((icons as &dyn Icons, "network-wireless"))
//!
//! - `IconFile::Svg { handle, symbolic }`: a symbolic one is recoloured
//!   by the element's `color`, so it follows the text around it, its
//!   states and its transitions.
//! - `IconFile::Image(handle)`: a picture, drawn as it is; `border-radius`
//!   rounds it (an avatar).
//! - `(icons, name)`: the app's `Icons` (a freedesktop theme, its own
//!   files) answers `lookup(name, size, scale)` at draw, for the size and
//!   the scale the icon is drawn at: a png theme's `@2` directory serves
//!   a scale-2 output. It's asked at every draw: answer from a cache.
//!
//! Its size is `height` (and `width`, square if not said) in the sheet;
//! `rotate` turns it. The icons below are the same star, sized and
//! coloured by classes, and two looked up by name.

use icssed::{Element, IconFile, Icons, column, icon, row, text};

use crate::Assets;

pub const TITLE: &str = "Icon";

#[derive(Default)]
pub struct State;

#[derive(Debug, Clone)]
pub enum Message {}

pub fn update(_state: &mut State, message: Message) {
    match message {}
}

/// The demo's icons by name, made once with the assets: an app's would
/// be a theme's, with a cache.
pub struct Named {
    pub star: IconFile,
    pub eye: IconFile,
}

impl Icons for Named {
    fn lookup(&self, name: &str, _size: f32, _scale: f32) -> Option<IconFile> {
        match name {
            "star" => Some(self.star.clone()),
            "eye" => Some(self.eye.clone()),
            _ => None,
        }
    }
}

pub fn view<'a>(_state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let star = || icon(assets.star.clone());
    let named: &'a dyn Icons = &assets.named;
    column![
        row![
            star().class("small"),
            star(),
            star().class("large"),
            star().class("accent"),
            star().class("turned"),
            icon(assets.picture.clone()).kind("avatar"),
        ]
        .kind("line"),
        row![
            icon((named, "star")),
            icon((named, "eye")),
            text("Looked up by name"),
        ]
        .kind("line"),
    ]
    .into()
}
