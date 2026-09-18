//! Notifications gadget: a bell with the count of the notifications
//! not looked at yet. A left click opens the history in a popup (the
//! recent notifications drawn exactly as on the desktop, with how long
//! ago they came and a ✕ each; do-not-disturb and clear buttons on
//! top), a right click toggles do-not-disturb, a middle click closes
//! every toast on screen.
//!
//! Holds no notification state: the history, the count and the
//! do-not-disturb flag come from `ctx.notifications`; what the user
//! does goes back as `Action::Notifications`.

use std::time::SystemTime;

use iced::widget::{Space, mouse_area, row, scrollable};
use iced::{Alignment, Element, Length, Subscription};
use iced_wayland_subscriber::OutputInfo;

use crate::gadget::{Action, Context, Gadget, Popup};
use crate::notifications::{Command, NotificationsConfig, toast};
use crate::theme::{self, Node};
use crate::time::aligned_ticks;

/// Icon size when the theme doesn't set `height` on `icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;
/// Popup width and height cap when the theme doesn't size `list`.
const DEFAULT_LIST_WIDTH: f32 = 380.0;
const DEFAULT_LIST_HEIGHT: f32 = 480.0;

const TITLE: &str = "Notifications";
const DND_LABEL: &str = "Do not disturb";
const CLEAR_LABEL: &str = "Clear";
const EMPTY: &str = "No notifications";

pub struct NotificationsGadget {
    /// The `[Notifications]` section, shared with the daemon: only the
    /// icons are the gadget's.
    config: NotificationsConfig,
    popup: Popup,
    /// When the ages were last computed: the popup opening, then every
    /// minute while it's open.
    now: SystemTime,
    /// The notifications not looked at when the popup opened: marked
    /// seen right then (the count on the bar clears), still marked in
    /// the list while it's open.
    unseen: Vec<u32>,
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Left click: with the ids not looked at yet (the view knows them,
    /// `update` doesn't see the daemon's state).
    TogglePopup(Vec<u32>),
    ToggleDnd,
    DismissAll,
    Clear,
    /// From a row of the popup.
    Toast(toast::Message),
    Tick,
}

impl Gadget for NotificationsGadget {
    type Config = NotificationsConfig;
    type Message = Message;

    fn new(config: NotificationsConfig, _output: &OutputInfo) -> Self {
        Self {
            config,
            popup: Popup::new(),
            now: SystemTime::now(),
            unseen: Vec::new(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::TogglePopup(unseen) => {
                if self.popup.is_open() {
                    return self.popup.close();
                }
                self.now = SystemTime::now();
                self.unseen = unseen;
                Action::Many(vec![
                    Action::Notifications(Command::MarkSeen),
                    self.popup.toggle(),
                ])
            }
            Message::ToggleDnd => Action::Notifications(Command::ToggleDnd),
            Message::DismissAll => Action::Notifications(Command::DismissAll),
            Message::Clear => Action::Notifications(Command::Clear),
            Message::Toast(m) => Action::Notifications(Command::from(m)),
            Message::Tick => {
                self.now = SystemTime::now();
                Action::None
            }
        }
    }

    fn icon_names(&self) -> Vec<String> {
        vec![self.config.icon.clone(), self.config.dnd_icon.clone()]
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let unseen = ctx.notifications.unseen();
        let button = ctx
            .node
            .child("button")
            .class_if("dnd", ctx.notifications.dnd())
            .class_if("new", unseen > 0);
        let icon = self.icon(&ctx, &button);
        let mut content = row![icon]
            .spacing(theme.resolve(&button).gap)
            .align_y(Alignment::Center);
        if unseen > 0 {
            let count = button.child("text");
            content = content.push(theme.container(&count, theme.text(&count, unseen.to_string())));
        }
        let unseen_ids = ctx
            .notifications
            .history()
            .iter()
            .filter(|e| !e.seen)
            .map(|e| e.notification.id)
            .collect();
        let button = theme
            .button(&button, content)
            .on_press(Message::TogglePopup(unseen_ids));
        mouse_area(self.popup.anchor(button))
            .on_right_press(Message::ToggleDnd)
            .on_middle_press(Message::DismissAll)
            .into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_closed(&mut self) {
        self.unseen.clear();
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let list = ctx.node.child("list");
        let list_style = theme.resolve(&list);
        let width = self.list_width(&ctx);
        let (header, _) = self.header(&ctx, &list);
        let mut rows: Vec<Element<'a, Message>> = vec![header];
        let entries = ctx.notifications.history();
        if entries.is_empty() {
            let empty = list.child("empty");
            rows.push(
                theme
                    .container(&empty, theme.text(&empty, EMPTY))
                    .width(Length::Fill)
                    .align_x(Alignment::Center)
                    .into(),
            );
        }
        for entry in entries {
            let n = &entry.notification;
            let node = toast::node_under(&list, n).class_if("unseen", self.unseen.contains(&n.id));
            let extras = toast::Extras {
                width: Some(width - list_style.padding.left - list_style.padding.right),
                age: Some(toast::age(entry.received, self.now)),
                close: true,
            };
            let content = toast::view(theme, &node, n, ctx.notifications, ctx.icons, extras)
                .map(Message::Toast);
            rows.push(theme.container(&node, content).width(Length::Fill).into());
        }
        let content = theme.column(&list, rows).width(Length::Fill);
        let (_, capped) = self.list_height(&ctx);
        if capped {
            scrollable(content).height(Length::Fill).into()
        } else {
            content.into()
        }
    }

    fn popup_size(&self, ctx: Context<'_>) -> (u32, u32) {
        let width = self.list_width(&ctx);
        let (height, _) = self.list_height(&ctx);
        (width.ceil().max(1.0) as u32, height.ceil().max(1.0) as u32)
    }

    fn subscription(&self) -> Subscription<Message> {
        if !self.popup.is_open() {
            return Subscription::none();
        }
        Subscription::run_with(60u32, |step| aligned_ticks(*step)).map(|_| Message::Tick)
    }
}

impl NotificationsGadget {
    /// The bell, or the quiet icon.
    fn icon<'a>(&'a self, ctx: &Context<'a>, button: &Node) -> Element<'a, Message> {
        let icon_node = button.child("icon");
        let style = ctx.theme.resolve(&icon_node);
        let size = px(style.height.or(style.width)).unwrap_or(DEFAULT_ICON_SIZE);
        let name = if ctx.notifications.dnd() {
            &self.config.dnd_icon
        } else {
            &self.config.icon
        };
        match ctx.icons.get_name(name, None) {
            Some(icon) => icon.view(size, style.color),
            None => Space::new().width(size).height(size).into(),
        }
    }

    fn list_width(&self, ctx: &Context<'_>) -> f32 {
        px(ctx.theme.resolve(&ctx.node.child("list")).width).unwrap_or(DEFAULT_LIST_WIDTH)
    }

    /// The header row and its height.
    fn header<'a>(&'a self, ctx: &Context<'a>, list: &Node) -> (Element<'a, Message>, f32) {
        let theme = ctx.theme;
        let header = list.child("header");
        let title = header.child("title");
        let dnd = header
            .child("button")
            .class("dnd")
            .class_if("on", ctx.notifications.dnd());
        let clear = header.child("button").class("clear");
        let s = theme.resolve(&header);
        let button_height = |b: &Node, label: &str| {
            let bs = theme.resolve(b);
            let t = b.child("text");
            theme.measure(&t, label).height.max(theme.line_height(&t))
                + bs.padding.top
                + bs.padding.bottom
        };
        let height = theme
            .measure(&title, TITLE)
            .height
            .max(theme.line_height(&title))
            .max(button_height(&dnd, DND_LABEL))
            .max(button_height(&clear, CLEAR_LABEL))
            + s.padding.top
            + s.padding.bottom;
        let row = theme
            .row(
                &header,
                [
                    theme.text(&title, TITLE).width(Length::Fill).into(),
                    theme
                        .button(&dnd, theme.text(&dnd.child("text"), DND_LABEL))
                        .on_press(Message::ToggleDnd)
                        .into(),
                    theme
                        .button(&clear, theme.text(&clear.child("text"), CLEAR_LABEL))
                        .on_press(Message::Clear)
                        .into(),
                ],
            )
            .align_y(Alignment::Center)
            .width(Length::Fill);
        (row.into(), height)
    }

    /// The popup height: the header, the rows (or the empty text) and
    /// the gaps, within the list's padding, capped by its `height`;
    /// whether it was capped (then the content scrolls).
    fn list_height(&self, ctx: &Context<'_>) -> (f32, bool) {
        let theme = ctx.theme;
        let list = ctx.node.child("list");
        let s = theme.resolve(&list);
        let width = self.list_width(ctx) - s.padding.left - s.padding.right;
        let (_, header) = self.header(ctx, &list);
        let mut height = header;
        let entries = ctx.notifications.history();
        if entries.is_empty() {
            let empty = list.child("empty");
            let es = theme.resolve(&empty);
            height += s.gap
                + theme
                    .measure(&empty, EMPTY)
                    .height
                    .max(theme.line_height(&empty))
                + es.padding.top
                + es.padding.bottom;
        }
        for entry in entries {
            let n = &entry.notification;
            let node = toast::node_under(&list, n).class_if("unseen", self.unseen.contains(&n.id));
            let extras = toast::Extras {
                width: Some(width),
                age: Some(toast::age(entry.received, self.now)),
                close: true,
            };
            // `toast::size` includes the node's padding.
            let (_, h) = toast::size(theme, &node, n, ctx.notifications, ctx.icons, &extras);
            height += s.gap + h as f32;
        }
        height += s.padding.top + s.padding.bottom;
        let cap = px(s.height).unwrap_or(DEFAULT_LIST_HEIGHT);
        if height > cap {
            (cap, true)
        } else {
            (height, false)
        }
    }
}

fn px(l: Option<theme::Length>) -> Option<f32> {
    match l {
        Some(theme::Length::Px(px)) => Some(px),
        _ => None,
    }
}

impl From<toast::Message> for Command {
    fn from(m: toast::Message) -> Self {
        match m {
            toast::Message::Activate(id) => Command::Activate(id),
            toast::Message::Invoke(id, key) => Command::Invoke(id, key),
            toast::Message::Dismiss(id) => Command::Dismiss(id),
        }
    }
}
