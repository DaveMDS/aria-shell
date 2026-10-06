//! A popup: a gadget's content hanging off its widget on a bar, on an
//! xdg popup surface child of the bar's. Centred on the widget, on the
//! side away from the screen edge; the surface is the content plus the
//! `popup` root's padding and border plus the room for its shadow, and
//! the box, not the surface, meets the widget. The panel that owns the
//! popups places them with these; a click on our surfaces outside them
//! closes them ([`presses_outside`]).

use iced::window;
use iced::{Padding, Rectangle, Subscription};
use iced_exwlshell::actions::IcedNewPopupSettings;
use iced_exwlshell::reexport::{PixelSize, PopupAnchor, PopupGravity};

use crate::ui::theme::{Node, Theme};

/// The side of its widget a popup hangs on: below a top bar's, above a
/// bottom bar's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Below,
    Above,
}

/// The surface size for content of size `content`: plus the `popup`
/// root's padding and border, plus the room for its shadow.
pub fn surface_size(theme: &Theme, content: (u32, u32)) -> (u32, u32) {
    let chrome = theme.resolve(&Node::root("popup"));
    let pad = chrome.padding;
    let room = room(theme);
    let extra = 2.0 * chrome.border_width;
    (
        content.0 + (pad.left + pad.right + extra + room.left + room.right) as u32,
        content.1 + (pad.top + pad.bottom + extra + room.top + room.bottom) as u32,
    )
}

/// The room around a popup's box for its shadow.
pub fn room(theme: &Theme) -> Padding {
    theme.shadow_room(&Node::root("popup"))
}

/// Placement of a popup of surface size `size` hanging off a widget
/// with `anchor` bounds in the surface `parent`, on `side`. `room` is
/// the part of the surface around the popup's box kept for its shadow:
/// the box, not the surface, meets the widget.
pub fn settings(
    parent: window::Id,
    side: Side,
    anchor: Rectangle,
    size: (u32, u32),
    room: Padding,
) -> IcedNewPopupSettings {
    let anchor = shadow_anchor(side, anchor, room);
    let (edge, gravity) = match side {
        Side::Below => (PopupAnchor::Bottom, PopupGravity::Bottom),
        Side::Above => (PopupAnchor::Top, PopupGravity::Top),
    };
    IcedNewPopupSettings::new(
        parent,
        PixelSize::px(size.0.max(1), size.1.max(1)),
        (anchor.x as i32, anchor.y as i32),
        PixelSize::px((anchor.width as u32).max(1), (anchor.height as u32).max(1)),
    )
    .anchor(edge)
    .gravity(gravity)
}

/// Where [`settings`] asks the popup to go, relative to the parent
/// surface, before the compositor slides it on screen: for `debug
/// surfaces`.
pub fn estimate(side: Side, anchor: Rectangle, size: (u32, u32), room: Padding) -> Rectangle {
    let anchor = shadow_anchor(side, anchor, room);
    let (w, h) = (size.0 as f32, size.1 as f32);
    let x = anchor.x + (anchor.width - w) / 2.0;
    let y = match side {
        Side::Below => anchor.y + anchor.height,
        Side::Above => anchor.y - h,
    };
    Rectangle::new(iced::Point::new(x, y), iced::Size::new(w, h))
}

/// The anchor rectangle that puts a popup's box, not its surface, on
/// the widget: shortened on the side the popup hangs from by the room
/// there (xdg-shell has no offset; a shorter rectangle stays inside the
/// bar, a moved one might not), moved sideways when the shadow isn't
/// centred.
fn shadow_anchor(side: Side, anchor: Rectangle, room: Padding) -> Rectangle {
    let x = anchor.x + (room.right - room.left) / 2.0;
    match side {
        Side::Below => Rectangle {
            x,
            height: (anchor.height - room.top).max(1.0),
            ..anchor
        },
        Side::Above => {
            let cut = room.bottom.min(anchor.height - 1.0).max(0.0);
            Rectangle {
                x,
                y: anchor.y + cut,
                height: anchor.height - cut,
                ..anchor
            }
        }
    }
}

/// Mouse buttons pressed on any of our windows that no widget took (a
/// click on a bar beside its gadgets): the daemon closes the popups. A
/// popup's grab makes the compositor dismiss it on a click outside, but
/// wlroots (Sway) delivers a click on the same client's other surfaces
/// instead, so that one is ours to act on.
pub fn presses_outside() -> Subscription<window::Id> {
    iced::event::listen_with(|event, status, window| match (event, status) {
        (
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_)),
            iced::event::Status::Ignored,
        ) => Some(window),
        _ => None,
    })
}
