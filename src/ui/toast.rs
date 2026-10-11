//! One notification as drawn: its layer surface on the desktop (a
//! toast), and its row in the history popup (the same code, with
//! [`Extras`] for what only the popup shows); the toasts' surfaces,
//! sized by their content and stacked from a corner.
//!
//! ```text
//! notification            the box
//! ├─ head                 a row
//! │  ├─ icon              image-data, image-path or app_icon
//! │  ╰─ texts             a column
//! │     ├─ line           a row
//! │     │  ├─ summary
//! │     │  ├─ time        how long ago (popup only)
//! │     │  ╰─ button.close.flat > cross   ✕ (popup only)
//! │     ╰─ body
//! ╰─ actions              a row of buttons, one per action but `default`
//!    ╰─ button > label
//! ```

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use iced::window::Id;
use iced::{Padding, Point, Rectangle, Size};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};
use icssed::{Element, Node};

use iced::widget::mouse_area;

use crate::locale::Locale;
use crate::services::icons::{Icon, Icons};
use crate::services::notifications::{IconSource, Notification, Notifications, Position};
use crate::ui::theme::Theme;
use crate::ui::{Measuring, Surfaces, popup};

#[derive(Debug, Clone)]
pub enum Message {
    /// A left click on the notification itself.
    Activate(u32),
    /// A right click: close without invoking anything.
    Dismiss(u32),
    /// A click on an action button.
    Invoke(u32, String),
}

/// The root node of notification `n`'s surface, from `root` (a
/// `notification` root, at its output's scale): its urgency as a
/// class, its id, the app and the output as attributes (the id isn't
/// an `#id`: a selector's can't start with a digit).
pub fn node<'a>(root: Node<'a>, n: &Notification, output: &str) -> Node<'a> {
    classify(root, n).attr("output", output)
}

/// The node of notification `n` shown under `parent` (a popup list).
pub fn node_under<'a>(parent: Node<'a>, n: &Notification) -> Node<'a> {
    classify(parent.child("notification"), n)
}

fn classify<'a>(node: Node<'a>, n: &Notification) -> Node<'a> {
    node.class(n.urgency.name())
        .attr("id", &n.id.to_string())
        .attr("app", &n.app_name)
}

/// What the popup shows on top of the notification itself.
#[derive(Debug, Clone, Default)]
pub struct Extras {
    /// How long ago it came, next to the summary.
    pub age: Option<String>,
    /// A ✕ button that dismisses it.
    pub close: bool,
}

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

/// `n` in its box, `node`. A left click anywhere but on a button
/// activates it, a right click dismisses it.
pub fn view<'a>(
    node: Node<'a>,
    n: &'a Notification,
    notifications: &'a Notifications,
    icons: &'a Icons,
    extras: Extras,
) -> Element<'a, Message> {
    let head = node.child("head");
    let texts = head.child("texts");
    let line = texts.child("line");
    let mut first: Vec<Element<'a, Message>> =
        vec![icssed::text(line.child("summary"), n.summary.as_str()).into()];
    if let Some(age) = extras.age {
        first.push(icssed::text(line.child("time"), age).into());
    }
    if extras.close {
        let close = line.child("button").class("close").class("flat");
        first.push(
            icssed::button(close, [icssed::mark(close.child("cross"))])
                .on_press(Message::Dismiss(n.id))
                .into(),
        );
    }
    let mut lines: Vec<Element<'a, Message>> = vec![icssed::row(line, first).into()];
    if !n.body.is_empty() {
        lines.push(icssed::text(texts.child("body"), n.body.as_str()).into());
    }
    let mut top: Vec<Element<'a, Message>> = Vec::new();
    if let Some(icon) = icon(n, notifications, icons) {
        top.push(icssed::icon(head.child("icon"), icon.clone()).into());
    }
    top.push(icssed::column(texts, lines).into());
    let mut content: Vec<Element<'a, Message>> = vec![icssed::row(head, top).into()];
    if n.buttons().next().is_some() {
        let actions = node.child("actions");
        let count = n.buttons().count();
        let buttons = n.buttons().enumerate().map(|(i, (key, label))| {
            let b = actions.child_at("button", i, count);
            icssed::button(b, [icssed::text(b.child("label"), label.as_str()).into()])
                .on_press(Message::Invoke(n.id, key.clone()))
                .into()
        });
        content.push(icssed::row(actions, buttons).into());
    }
    mouse_area(icssed::column(node, content))
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
    /// gone go, the new ones get one on output `focused`, sizes (each
    /// measured at its output's scale) and margins are updated in
    /// place.
    pub fn sync(
        &mut self,
        outputs: &BTreeMap<OutputId, OutputInfo>,
        focused: Option<&OutputInfo>,
        m: Measuring<'_>,
    ) -> Surfaces {
        let (notifications, theme, icons) =
            (m.shared.notifications, m.shared.theme, m.shared.icons);
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
        let mut offsets: BTreeMap<OutputId, i32> = BTreeMap::new();
        for n in &notifications.items {
            let Some(toast) = self.toasts.iter_mut().find(|t| t.id == n.id) else {
                continue;
            };
            let info = outputs.get(&toast.output);
            let output_name = info.and_then(|o| o.name.as_deref()).unwrap_or("?");
            let scale = m.scales.of(toast.output);
            // The stack: its padding from the edges, its gap between
            // the boxes.
            let stack = theme.root("notifications", scale).base();
            let edge = popup::padding(stack.padding);
            let gap = stack.row_gap as i32;
            let node = node(theme.root("notification", scale), n, output_name);
            let room = popup::room(node);
            let size = crate::ui::size(
                &mut m.measure.borrow_mut(),
                toast.window,
                surface(node, n, notifications, icons),
                crate::ui::room_on(info),
            );
            let offset = offsets.entry(toast.output).or_insert(0);
            let along = *offset;
            *offset += (size.1 as f32 - room.top - room.bottom) as i32 + gap;
            let margin = placement(position, along, edge, room);
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

    /// Surface `window`'s notification, on output `output` (its name),
    /// at `scale`.
    pub fn view<'a>(
        &self,
        window: Id,
        theme: &'a Theme,
        notifications: &'a Notifications,
        icons: &'a Icons,
        output: &str,
        scale: f32,
    ) -> Element<'a, Message> {
        let root = theme.root("notification", scale);
        let Some(n) = self
            .toasts
            .iter()
            .find(|t| t.window == window)
            .and_then(|t| notifications.get(t.id))
        else {
            return icssed::column(root, []).into();
        };
        surface(node(root, n, output), n, notifications, icons)
    }
}

/// The content of notification `n`'s own surface: its view in its box
/// `node` (its [`node`]), with its shadow's room around.
pub fn surface<'a>(
    node: Node<'a>,
    n: &'a Notification,
    notifications: &'a Notifications,
    icons: &'a Icons,
) -> Element<'a, Message> {
    icssed::surface(node, view(node, n, notifications, icons, Extras::default())).into()
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

/// A toast's margins (top, right, bottom, left) in the stack from
/// corner `position`: `along` is how far the boxes before it on its
/// output reach from the edge, `edge` the `notifications` root's
/// padding, `room` its box's shadow's, around the box in its surface.
/// The boxes are stacked, their shadows overlap the gaps.
pub fn placement(
    position: Position,
    along: i32,
    edge: Padding,
    room: Padding,
) -> (i32, i32, i32, i32) {
    let (top, right, bottom, left) = (
        room.top as i32,
        room.right as i32,
        room.bottom as i32,
        room.left as i32,
    );
    if position.is_top() {
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
    }
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
