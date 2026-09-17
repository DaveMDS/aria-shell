//! One notification as drawn: the content of its layer surface on the
//! desktop, and of its row in the history popup (the same code, with
//! [`Extras`] for what only the popup shows), and its size, which a
//! surface must be given before anything is laid out (measured the way
//! the view lays it out).
//!
//! ```text
//! notification            the container the caller applies
//! ├─ icon                 image-data, image-path or app_icon
//! ├─ summary
//! ├─ time                 how long ago (popup only)
//! ├─ button.close         ✕ (popup only)
//! ├─ body
//! ╰─ actions              a row of buttons, one per action but `default`
//!    ╰─ button
//!       ╰─ text
//! ```

use std::time::{Duration, SystemTime};

use iced::widget::text::Wrapping;
use iced::widget::{column, mouse_area, row};
use iced::{Alignment, Element, Length, Size};

use super::{IconSource, Notification, Notifications};
use crate::icons::{Icon, Icons};
use crate::theme::{self, Node, Theme};

/// Width when the theme doesn't set one on `notification`.
const DEFAULT_WIDTH: f32 = 360.0;
/// Icon size when the theme doesn't set `height` on `notification icon`.
const DEFAULT_ICON: f32 = 32.0;

#[derive(Debug, Clone)]
pub enum Message {
    /// A left click on the notification itself.
    Activate(u32),
    /// A right click: close without invoking anything.
    Dismiss(u32),
    /// A click on an action button.
    Invoke(u32, String),
}

/// The root node of notification `n`'s surface: its urgency as a
/// class, its id, the app as an attribute.
pub fn node(n: &Notification, output: &str) -> Node {
    classify(Node::root("notification"), n).attr("output", output.to_owned())
}

/// The node of notification `n` shown under `parent` (a popup list).
pub fn node_under(parent: &Node, n: &Notification) -> Node {
    classify(parent.child("notification"), n)
}

fn classify(node: Node, n: &Notification) -> Node {
    node.class(n.urgency.name())
        .id(n.id.to_string())
        .attr("app", n.app_name.clone())
}

/// What the popup shows on top of the notification itself.
#[derive(Debug, Clone, Default)]
pub struct Extras {
    /// The width to lay out in, instead of the node's `width`.
    pub width: Option<f32>,
    /// How long ago it came, next to the summary.
    pub age: Option<String>,
    /// A ✕ button that dismisses it.
    pub close: bool,
}

const CLOSE_GLYPH: &str = "✕";

/// "now", "5 min", "2 h", "yesterday", or the date, for the popup.
pub fn age(received: SystemTime, now: SystemTime) -> String {
    let elapsed = now
        .duration_since(received)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    match elapsed {
        0..60 => "now".to_owned(),
        60..3600 => format!("{} min", elapsed / 60),
        3600..86400 => format!("{} h", elapsed / 3600),
        86400..172800 => "yesterday".to_owned(),
        _ => {
            let date = chrono::DateTime::<chrono::Local>::from(received);
            date.format("%d %b").to_string()
        }
    }
}

/// The surface width the theme asks for, and the layout inside it.
struct Layout {
    width: f32,
    /// The root's padding, around everything (iced draws borders over
    /// the padding, they take no room).
    frame: (f32, f32, f32, f32),
    icon: Option<f32>,
    /// Space between the icon and the texts, and between the texts row
    /// and the actions.
    gap: f32,
    text_width: f32,
}

fn px(l: Option<theme::Length>) -> Option<f32> {
    match l {
        Some(theme::Length::Px(px)) => Some(px),
        _ => None,
    }
}

fn layout(theme: &Theme, node: &Node, has_icon: bool, extras: &Extras) -> Layout {
    let root = theme.resolve(node);
    let width = extras
        .width
        .or(px(root.width))
        .unwrap_or(DEFAULT_WIDTH)
        .max(1.0);
    let pad = root.padding;
    let frame = (pad.top, pad.right, pad.bottom, pad.left);
    let icon = has_icon.then(|| {
        let s = theme.resolve(&node.child("icon"));
        px(s.height).or(px(s.width)).unwrap_or(DEFAULT_ICON)
    });
    let gap = root.gap;
    let text_width = width - frame.1 - frame.3 - icon.map_or(0.0, |i| i + gap);
    Layout {
        width,
        frame,
        icon,
        gap,
        text_width: text_width.max(1.0),
    }
}

/// The icon to draw for `n`, if any: the image the app sent, else a
/// file it named, else the theme icon it named.
fn icon<'a>(
    n: &'a Notification,
    notifications: &'a Notifications,
    icons: &'a Icons,
) -> Option<&'a Icon> {
    n.image.as_ref().or_else(|| match &n.icon {
        Some(IconSource::Path(p)) => notifications.file_icon(p),
        Some(IconSource::Name(name)) => icons.get_name(name, None),
        None => None,
    })
}

/// The size of notification `n`'s surface: the theme's `width`, and
/// the height its content takes at that width.
pub fn size(
    theme: &Theme,
    node: &Node,
    n: &Notification,
    notifications: &Notifications,
    icons: &Icons,
    extras: &Extras,
) -> (u32, u32) {
    let l = layout(theme, node, icon(n, notifications, icons).is_some(), extras);
    // The summary shares its line with the age and the ✕.
    let (side_width, side_height) = side_size(theme, node, extras);
    let text_height = |kind: &'static str, content: &str, taken: f32| {
        let node = node.child(kind);
        let s = theme.resolve(&node);
        let pad = s.padding;
        let inner = (l.text_width - taken - pad.left - pad.right).max(1.0);
        theme
            .measure_in(&node, content, inner)
            .height
            .max(theme.line_height(&node))
            + pad.top
            + pad.bottom
    };
    let mut texts = text_height("summary", &n.summary, side_width).max(side_height);
    if !n.body.is_empty() {
        texts += text_height("body", &n.body, 0.0);
    }
    let mut height = texts.max(l.icon.unwrap_or(0.0));
    if n.buttons().next().is_some() {
        let actions = node.child("actions");
        let count = n.buttons().count();
        let buttons = n
            .buttons()
            .enumerate()
            .map(|(i, (_, label))| {
                let b = actions.child("button").nth(i, count);
                let s = theme.resolve(&b);
                let text = theme.measure(&b.child("text"), label);
                text.height.max(theme.line_height(&b.child("text")))
                    + s.padding.top
                    + s.padding.bottom
            })
            .fold(0.0_f32, f32::max);
        let s = theme.resolve(&actions);
        height += l.gap + buttons + s.padding.top + s.padding.bottom;
    }
    let size = Size::new(l.width, height + l.frame.0 + l.frame.2);
    (size.width.ceil() as u32, size.height.ceil() as u32)
}

/// The room the age and the ✕ take on the summary's line: their
/// width (with the gaps before them) and their height.
fn side_size(theme: &Theme, node: &Node, extras: &Extras) -> (f32, f32) {
    let gap = theme.resolve(node).gap;
    let mut width = 0.0_f32;
    let mut height = 0.0_f32;
    if let Some(age) = &extras.age {
        let t = node.child("time");
        let s = theme.resolve(&t);
        let m = theme.measure(&t, age);
        width += gap + m.width + s.padding.left + s.padding.right;
        height = height.max(m.height.max(theme.line_height(&t)) + s.padding.top + s.padding.bottom);
    }
    if extras.close {
        let b = node.child("button").class("close");
        let s = theme.resolve(&b);
        let m = theme.measure(&b.child("text"), CLOSE_GLYPH);
        width += gap + m.width + s.padding.left + s.padding.right;
        height = height.max(
            m.height.max(theme.line_height(&b.child("text"))) + s.padding.top + s.padding.bottom,
        );
    }
    (width, height)
}

/// The content of `n`, inside the `notification` container the caller
/// applies. A left click anywhere but on a button activates it, a
/// right click dismisses it.
pub fn view<'a>(
    theme: &'a Theme,
    node: &Node,
    n: &'a Notification,
    notifications: &'a Notifications,
    icons: &'a Icons,
    extras: Extras,
) -> Element<'a, Message> {
    let icon = icon(n, notifications, icons);
    let l = layout(theme, node, icon.is_some(), &extras);
    let summary: Element<'a, Message> = theme
        .container(
            &node.child("summary"),
            theme
                .text(&node.child("summary"), &n.summary)
                .wrapping(Wrapping::Word),
        )
        .width(Length::Fill)
        .into();
    let mut first = row![summary].spacing(l.gap).align_y(Alignment::Start);
    if let Some(age) = extras.age {
        let t = node.child("time");
        first = first.push(theme.container(&t, theme.text(&t, age)));
    }
    if extras.close {
        let b = node.child("button").class("close");
        first = first.push(
            theme
                .button(&b, theme.text(&b.child("text"), CLOSE_GLYPH))
                .on_press(Message::Dismiss(n.id)),
        );
    }
    let mut texts = column![first.width(Length::Fill)];
    if !n.body.is_empty() {
        texts = texts.push(
            theme
                .container(
                    &node.child("body"),
                    theme
                        .text(&node.child("body"), &n.body)
                        .wrapping(Wrapping::Word),
                )
                .width(Length::Fill),
        );
    }
    let mut top = row![].spacing(l.gap).align_y(Alignment::Start);
    if let (Some(icon), Some(size)) = (icon, l.icon) {
        let icon_node = node.child("icon");
        let color = theme.resolve(&icon_node).color;
        top = top.push(theme.container(&icon_node, icon.view(size, color)));
    }
    top = top.push(texts.width(Length::Fill));
    let mut content = column![top.width(Length::Fill)].spacing(l.gap);
    if n.buttons().next().is_some() {
        let actions = node.child("actions");
        let count = n.buttons().count();
        let buttons = n.buttons().enumerate().map(|(i, (key, label))| {
            let b = actions.child("button").nth(i, count);
            theme
                .button(&b, theme.text(&b.child("text"), label))
                .on_press(Message::Invoke(n.id, key.clone()))
                .into()
        });
        let gap = theme.resolve(&actions).gap;
        content = content.push(
            theme
                .container(
                    &actions,
                    row(buttons).spacing(gap).align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .align_x(Alignment::End),
        );
    }
    mouse_area(content.width(Length::Fill))
        .on_press(Message::Activate(n.id))
        .on_right_press(Message::Dismiss(n.id))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let at = |secs: u64| age(t0, t0 + Duration::from_secs(secs));
        assert_eq!(at(0), "now");
        assert_eq!(at(59), "now");
        assert_eq!(at(60), "1 min");
        assert_eq!(at(3599), "59 min");
        assert_eq!(at(3600), "1 h");
        assert_eq!(at(86399), "23 h");
        assert_eq!(at(86400), "yesterday");
        assert!(at(200_000).contains(' '), "a date: {}", at(200_000));
        // A clock that went back: still "now".
        assert_eq!(age(t0 + Duration::from_secs(10), t0), "now");
    }
}
