//! An icon draws a picture at its node's size: `icssed::icon(node,
//! source)`. The source is a file, or a name for the app's icons to look
//! up.
//!
//!     icssed::icon(node.child("icon"), IconFile::Svg { handle, symbolic: true })
//!     icssed::icon(node.child("icon"), (icons as &dyn Icons, "network-wireless"))
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

use icssed::{Element, IconFile, Icons, Node};

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

pub fn view<'a>(_state: &'a State, node: Node<'a>, assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    let star = |class: &str| -> Element<'a, Message> {
        icssed::icon(
            line.child("icon").class_if(class, !class.is_empty()),
            assets.star.clone(),
        )
        .into()
    };
    let named: &'a dyn Icons = &assets.named;
    icssed::column(
        node,
        [
            icssed::row(
                line,
                [
                    star("small"),
                    star(""),
                    star("large"),
                    star("accent"),
                    star("turned"),
                    icssed::icon(line.child("avatar"), assets.picture.clone()).into(),
                ],
            )
            .into(),
            icssed::row(
                line,
                [
                    icssed::icon(line.child("icon"), (named, "star")).into(),
                    icssed::icon(line.child("icon"), (named, "eye")).into(),
                    icssed::text(line.child("label"), "Looked up by name").into(),
                ],
            )
            .into(),
        ],
    )
    .into()
}
