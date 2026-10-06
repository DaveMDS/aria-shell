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

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, mouse_area, row};
use iced::window::Id;
use iced::{Alignment, Element, Length, Padding, Point, Rectangle, Size};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};

use crate::locale::Locale;
use crate::services::icons::{Icon, Icons};
use crate::services::notifications::{IconSource, Notification, Notifications, Position};
use crate::ui::Surfaces;
use crate::ui::theme::{self, Node, Theme};

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
pub fn age(received: SystemTime, now: SystemTime, locale: &Locale) -> String {
    let elapsed = now
        .duration_since(received)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    match elapsed {
        0..60 => locale.tr("notifications.age.now").to_owned(),
        60..3600 => locale.fmt("notifications.age.minutes", &[("n", &(elapsed / 60))]),
        3600..86400 => locale.fmt("notifications.age.hours", &[("n", &(elapsed / 3600))]),
        86400..172800 => locale.tr("notifications.age.yesterday").to_owned(),
        _ => locale.date(&chrono::DateTime::<chrono::Local>::from(received), "%d %b"),
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

/// The toasts on screen: one surface per notification shown, on the
/// focused output when it appears, sized to its content and stacked
/// from the configured corner (newest nearest to it) with the
/// `notifications` root's `padding` from the edges and `gap` between
/// them. The daemon keeps one and syncs it with the notifications
/// after every change.
#[derive(Default)]
pub struct Toasts {
    toasts: Vec<Toast>,
}

/// A notification's surface.
struct Toast {
    window: Id,
    /// The notification's id.
    id: u32,
    output: OutputId,
    size: (u32, u32),
    /// (top, right, bottom, left)
    margin: (i32, i32, i32, i32),
}

impl Toasts {
    /// Make the toasts match the notifications: the surfaces of those
    /// gone go, the new ones get one on output `focused`, sizes and
    /// margins are updated in place.
    pub fn sync(
        &mut self,
        notifications: &Notifications,
        outputs: &BTreeMap<OutputId, OutputInfo>,
        focused: Option<&OutputInfo>,
        theme: &Theme,
        icons: &Icons,
    ) -> Surfaces {
        let mut surfaces = Surfaces::default();
        // Notifications gone: their surfaces go.
        self.toasts.retain(|t| {
            let keep = notifications.get(t.id).is_some();
            if !keep {
                surfaces.close.push(t.window);
            }
            keep
        });
        // New notifications: a surface each, on the focused output.
        let mut new = Vec::new();
        for n in &notifications.items {
            if self.toasts.iter().any(|t| t.id == n.id) {
                continue;
            }
            let Some(output) = focused else {
                log::warn!("no output to show notification {} on", n.id);
                continue;
            };
            let window = Id::unique();
            new.push(window);
            self.toasts.push(Toast {
                window,
                id: n.id,
                output: OutputId::from(output),
                size: (0, 0),
                margin: (0, 0, 0, 0),
            });
        }
        // Sizes and places, per output, in notification order.
        let position = notifications.config().position;
        let stack = theme.resolve(&Node::root("notifications"));
        let (edge, gap) = (stack.padding, stack.gap as i32);
        let mut offsets: BTreeMap<OutputId, i32> = BTreeMap::new();
        for n in &notifications.items {
            let Some(toast) = self.toasts.iter_mut().find(|t| t.id == n.id) else {
                continue;
            };
            let info = outputs.get(&toast.output);
            let output_name = info.and_then(|o| o.name.as_deref()).unwrap_or("?");
            let node = node(n, output_name);
            let content = size(theme, &node, n, notifications, icons, &Extras::default());
            let room = theme.shadow_room(&node);
            let offset = offsets.entry(toast.output).or_insert(0);
            let along = *offset;
            *offset += content.1 as i32 + gap;
            let (size, margin) = placement(position, content, along, edge, room);
            if new.contains(&toast.window) {
                toast.size = size;
                toast.margin = margin;
                log::debug!(
                    "notification {}: surface {:?} on {output_name}, {}x{} at margin {margin:?}",
                    n.id,
                    toast.window,
                    size.0,
                    size.1
                );
                let settings = NewLayerShellSettings {
                    anchor: anchor(position),
                    size: LayerSize::px(size.0, size.1),
                    layer: Layer::Overlay,
                    exclusive_zone: None,
                    margin: Some(margin),
                    keyboard_interactivity: KeyboardInteractivity::None,
                    output_option: OutputOption::GlobalName(info.map(|o| o.id).unwrap_or_default()),
                    namespace: Some("aria-notification".to_owned()),
                    ..Default::default()
                };
                surfaces.open.push((toast.window, settings));
                continue;
            }
            if toast.size != size {
                toast.size = size;
                let layout = LayerSize::px(size.0, size.1);
                surfaces
                    .resize
                    .push((toast.window, anchor(position), layout));
            }
            if toast.margin != margin {
                toast.margin = margin;
                surfaces.margin.push((toast.window, margin));
            }
        }
        surfaces
    }

    /// Close every toast (the corner or the theme may have changed): the
    /// next [`Toasts::sync`] opens them again.
    pub fn close(&mut self) -> Surfaces {
        Surfaces {
            close: self.toasts.drain(..).map(|t| t.window).collect(),
            ..Surfaces::default()
        }
    }

    /// Output `output` went away: its toasts go, the next
    /// [`Toasts::sync`] shows them on another.
    pub fn output_removed(&mut self, output: OutputId) -> Surfaces {
        let mut surfaces = Surfaces::default();
        self.toasts.retain(|t| {
            let keep = t.output != output;
            if !keep {
                surfaces.close.push(t.window);
            }
            keep
        });
        surfaces
    }

    /// Surface `window` was closed, with its output or on our request:
    /// whether it was a toast. If its notification is still there, the
    /// next [`Toasts::sync`] gives it a new one.
    pub fn closed(&mut self, window: Id) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|t| t.window != window);
        self.toasts.len() != before
    }

    /// The surfaces: window, notification id, output, surface size and
    /// margins, for `debug surfaces` (with [`rect`]).
    pub fn placed(
        &self,
    ) -> impl Iterator<Item = (Id, u32, OutputId, (u32, u32), (i32, i32, i32, i32))> + '_ {
        self.toasts
            .iter()
            .map(|t| (t.window, t.id, t.output, t.size, t.margin))
    }

    pub fn output_of(&self, window: Id) -> Option<OutputId> {
        self.toasts
            .iter()
            .find(|t| t.window == window)
            .map(|t| t.output)
    }

    /// Surface `window`'s notification, on output `output` (its name).
    pub fn view<'a>(
        &self,
        window: Id,
        theme: &'a Theme,
        notifications: &'a Notifications,
        icons: &'a Icons,
        output: &str,
    ) -> Element<'a, Message> {
        let Some(n) = self
            .toasts
            .iter()
            .find(|t| t.window == window)
            .and_then(|t| notifications.get(t.id))
        else {
            return Space::new().into();
        };
        let node = node(n, output);
        let content = view(theme, &node, n, notifications, icons, Extras::default());
        let root = theme
            .container(&node, content)
            .width(Length::Fill)
            .height(Length::Fill);
        theme.surface(&node, root).into()
    }
}

/// The layer-shell anchor of the toasts in corner `position`.
pub fn anchor(position: Position) -> Anchor {
    use Position::*;
    match position {
        TopLeft => Anchor::Top | Anchor::Left,
        TopRight => Anchor::Top | Anchor::Right,
        TopCenter => Anchor::Top,
        BottomLeft => Anchor::Bottom | Anchor::Left,
        BottomRight => Anchor::Bottom | Anchor::Right,
        BottomCenter => Anchor::Bottom,
    }
}

/// A toast's surface size and margins (top, right, bottom, left) in
/// the stack from corner `position`: `size` is its box's ([`size`]),
/// `along` how far the boxes before it on its output reach from the
/// edge, `edge` the `notifications` root's padding, `room` the box's
/// shadow's. The surface is the box plus the room; the boxes are
/// stacked, their shadows overlap the gaps.
pub fn placement(
    position: Position,
    size: (u32, u32),
    along: i32,
    edge: Padding,
    room: Padding,
) -> ((u32, u32), (i32, i32, i32, i32)) {
    let surface = (
        size.0 + (room.left + room.right) as u32,
        size.1 + (room.top + room.bottom) as u32,
    );
    let (top, right, bottom, left) = (
        room.top as i32,
        room.right as i32,
        room.bottom as i32,
        room.left as i32,
    );
    let margin = if position.is_top() {
        (
            edge.top as i32 + along - top,
            edge.right as i32 - right,
            0,
            edge.left as i32 - left,
        )
    } else {
        (
            0,
            edge.right as i32 - right,
            edge.bottom as i32 + along - bottom,
            edge.left as i32 - left,
        )
    };
    (surface, margin)
}

/// Where a toast's surface is, as asked: from corner `position` of an
/// output with rectangle `output`, past the bars' exclusive zones on
/// that edge (`bars`: the top one's height, the bottom one's), by its
/// margin.
pub fn rect(
    position: Position,
    output: Rectangle,
    bars: (f32, f32),
    size: (u32, u32),
    margin: (i32, i32, i32, i32),
) -> Rectangle {
    use Position::*;
    let (w, h) = (size.0 as f32, size.1 as f32);
    let (top, right, bottom, left) = margin;
    let x = match position {
        TopLeft | BottomLeft => output.x + left as f32,
        TopRight | BottomRight => output.x + output.width - right as f32 - w,
        TopCenter | BottomCenter => output.x + (output.width - w) / 2.0,
    };
    let y = if position.is_top() {
        output.y + bars.0 + top as f32
    } else {
        output.y + output.height - bars.1 - bottom as f32 - h
    };
    Rectangle::new(Point::new(x, y), Size::new(w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let en = Locale::new("en");
        let at = |secs: u64| age(t0, t0 + Duration::from_secs(secs), &en);
        assert_eq!(at(0), "now");
        assert_eq!(at(59), "now");
        assert_eq!(at(60), "1 min");
        assert_eq!(at(3599), "59 min");
        assert_eq!(at(3600), "1 h");
        assert_eq!(at(86399), "23 h");
        assert_eq!(at(86400), "yesterday");
        assert!(at(200_000).contains(' '), "a date: {}", at(200_000));
        // A clock that went back: still "now".
        assert_eq!(age(t0 + Duration::from_secs(10), t0, &en), "now");
    }
}
