//! Idle gadget: an icon saying whether the machine may go idle. A left
//! click holds idle (no lock, no screens off, no suspend) or lets it
//! go, like `aria-shell idle inhibit`.
//!
//! Holds no idle state: the hold comes from `ctx.idle`, the click goes
//! back as `Action::Idle`.

use iced::Element;
use iced::widget::Space;
use iced_wayland_subscriber::OutputInfo;

use crate::gadget::{Action, Context, Gadget};
use crate::idle::{Command, IdleConfig};
use crate::theme;

/// Icon size when the theme doesn't set `height` on `icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;

pub struct IdleGadget {
    /// The `[Idle]` section, shared with the daemon: only the icons are
    /// the gadget's.
    config: IdleConfig,
}

#[derive(Clone, Debug)]
pub enum Message {
    Toggle,
}

impl Gadget for IdleGadget {
    type Config = IdleConfig;
    type Message = Message;

    fn new(config: IdleConfig, _output: &OutputInfo) -> Self {
        Self { config }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Toggle => Action::Idle(Command::ToggleInhibit),
        }
    }

    fn icon_names(&self) -> Vec<String> {
        vec![self.config.icon.clone(), self.config.inhibit_icon.clone()]
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let inhibited = ctx.idle.inhibited();
        let button = ctx
            .node
            .child("button")
            .class_if("inhibited", inhibited)
            .class_if("playing", ctx.idle.held_by_player());
        let icon_node = button.child("icon");
        let style = theme.resolve(&icon_node);
        let size = style
            .height
            .or(style.width)
            .and_then(|l| match l {
                theme::Length::Px(px) => Some(px),
                _ => None,
            })
            .unwrap_or(DEFAULT_ICON_SIZE);
        let name = if inhibited {
            &self.config.inhibit_icon
        } else {
            &self.config.icon
        };
        let icon: Element<'a, Message> = match ctx.icons.get_name(name, None) {
            Some(icon) => icon.view(size, style.color),
            None => Space::new().width(size).height(size).into(),
        };
        theme.button(&button, icon).on_press(Message::Toggle).into()
    }
}
