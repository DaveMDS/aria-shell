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

use iced::Subscription;
use iced::widget::mouse_area;
use iced_wayland_subscriber::OutputInfo;
use icssed::Element;

use crate::gadgets::{Action, Context, Gadget, Popup};
use crate::services::notifications::{Command, NotificationsConfig};
use crate::time::aligned_ticks;
use crate::ui::toast;

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
        let unseen = ctx.notifications.unseen();
        let button = ctx
            .node
            .child("button")
            .class_if("dnd", ctx.notifications.dnd())
            .class_if("new", unseen > 0);
        let name = if ctx.notifications.dnd() {
            &self.config.dnd_icon
        } else {
            &self.config.icon
        };
        let mut content = vec![ctx.icon(button.child("icon"), name.as_str())];
        if unseen > 0 {
            content.push(icssed::text(button.child("label"), unseen.to_string()).into());
        }
        let unseen_ids = ctx
            .notifications
            .history()
            .iter()
            .filter(|e| !e.seen)
            .map(|e| e.notification.id)
            .collect();
        let button = icssed::button(button, content)
            .on_press(Message::TogglePopup(unseen_ids))
            .id(self.popup.anchor());
        let button = mouse_area(button)
            .on_right_press(Message::ToggleDnd)
            .on_middle_press(Message::DismissAll);
        icssed::row(ctx.node, [button.into()]).into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let scroll = ctx.node.child("scroll");
        let list = scroll.child("list");
        let header = list.child("header");
        let dnd = header
            .child("button")
            .class("dnd")
            .class_if("active", ctx.notifications.dnd());
        let clear = header.child("button").class("clear");
        let header = icssed::row(
            header,
            [
                icssed::text(header.child("title"), ctx.locale.tr("notifications.title")).into(),
                icssed::button(
                    dnd,
                    [icssed::text(dnd.child("label"), ctx.locale.tr("notifications.dnd")).into()],
                )
                .on_press(Message::ToggleDnd)
                .into(),
                icssed::button(
                    clear,
                    [
                        icssed::text(clear.child("label"), ctx.locale.tr("notifications.clear"))
                            .into(),
                    ],
                )
                .on_press(Message::Clear)
                .into(),
            ],
        );
        let mut rows: Vec<Element<'a, Message>> = vec![header.into()];
        let entries = ctx.notifications.history();
        if entries.is_empty() {
            let text = ctx.locale.tr("notifications.empty");
            rows.push(super::note(list.child("empty"), text));
        }
        for entry in entries {
            let n = &entry.notification;
            let node = toast::node_under(list, n).class_if("unseen", self.unseen.contains(&n.id));
            let extras = toast::Extras {
                age: Some(toast::age(entry.received, self.now, ctx.locale)),
                close: true,
            };
            rows.push(
                toast::view(node, n, ctx.notifications, ctx.icons, extras).map(Message::Toast),
            );
        }
        icssed::scroll(scroll, icssed::column(list, rows)).into()
    }

    fn popup_closed(&mut self) {
        self.unseen.clear();
    }

    fn subscription(&self) -> Subscription<Message> {
        if !self.popup.is_open() {
            return Subscription::none();
        }
        Subscription::run_with(60u32, |step| aligned_ticks(*step)).map(|_| Message::Tick)
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
