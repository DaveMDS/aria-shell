//! Brightness gadget: an icon on the bar (and the percent of the bar's
//! screen, with `show_percent`); the wheel changes every screen, or
//! only the bar's (`wheel = output`), by `step`; a left click opens the
//! popup: a slider per screen (the laptop's panel, each monitor
//! answering DDC/CI) and a Settings button; a right click runs
//! `settings_command`. The popup opening reads the monitors again:
//! their own buttons go unseen otherwise.
//!
//! Holds no brightness state: the screens come from `ctx.brightness`;
//! what the user does goes back as `Action::Brightness`.

use iced::mouse::ScrollDelta;
use iced::widget::mouse_area;
use iced_wayland_subscriber::OutputInfo;
use icssed::{Element, Node};

use crate::gadgets::{Action, Axis, Context, Gadget, Popup, Wheel};
use crate::process;
use crate::services::brightness::{BrightnessConfig, Command, Display, Kind, Target, WheelTarget};

const ICON: &str = "display-brightness-symbolic";
const ICON_PANEL: &str = "computer-laptop-symbolic";
const ICON_MONITOR: &str = "video-display-symbolic";

pub struct BrightnessGadget {
    config: BrightnessConfig,
    /// The connector of the bar's output.
    output: Option<String>,
    popup: Popup,
    wheel: Wheel,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    Scroll(ScrollDelta),
    /// A screen's slider: its id, the percent.
    Set(String, u32),
    RunSettings,
}

impl Gadget for BrightnessGadget {
    type Config = BrightnessConfig;
    type Message = Message;

    fn new(config: BrightnessConfig, output: &OutputInfo) -> Self {
        Self {
            config,
            output: output.name.clone(),
            popup: Popup::new(),
            wheel: Wheel::default(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::TogglePopup => {
                let opening = !self.popup.is_open();
                let toggle = self.popup.toggle();
                if opening {
                    Action::Many(vec![toggle, Action::Brightness(Command::Refresh)])
                } else {
                    toggle
                }
            }
            Message::Scroll(delta) => match self.wheel.clicks(delta) {
                Some((clicks, Axis::Vertical)) => {
                    let target = match (self.config.wheel, &self.output) {
                        (WheelTarget::Output, Some(output)) => Target::Output(output.clone()),
                        _ => Target::All,
                    };
                    Action::Brightness(Command::Step {
                        target,
                        up: clicks > 0,
                        by: Some(self.config.step * clicks.unsigned_abs()),
                    })
                }
                _ => Action::None,
            },
            Message::Set(id, percent) => {
                Action::Brightness(Command::Set(Target::Display(id), percent))
            }
            Message::RunSettings => {
                if !self.config.settings_command.is_empty() {
                    process::run(&self.config.settings_command);
                }
                self.popup.close()
            }
        }
    }

    fn icon_names(&self) -> Vec<String> {
        [ICON, ICON_PANEL, ICON_MONITOR]
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let screen = self.bar_screen(&ctx);
        let button = ctx
            .node
            .child("button")
            .class_if("none", ctx.brightness.displays().is_empty());
        let mut content = vec![ctx.icon(button.child("icon"), ICON)];
        if self.config.show_percent
            && let Some(percent) = screen.and_then(Display::percent)
        {
            content.push(icssed::text(button.child("label"), format!("{percent}%")).into());
        }
        let button = icssed::button(button, content)
            .on_press(Message::TogglePopup)
            .id(self.popup.anchor());
        let button = mouse_area(button)
            .on_right_press(Message::RunSettings)
            .on_scroll(Message::Scroll);
        icssed::row(ctx.node, [button.into()]).into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let list = ctx.node.child("list");
        let mut rows: Vec<Element<'a, Message>> = ctx
            .brightness
            .displays()
            .iter()
            .map(|d| screen(ctx, list, d))
            .collect();
        if rows.is_empty() {
            let text = ctx.locale.tr("brightness.empty");
            rows.push(super::note(list.child("empty"), text));
        }
        if !self.config.settings_command.is_empty() {
            let b = list.child("button").class("settings");
            let label = icssed::text(b.child("label"), ctx.locale.tr("brightness.settings"));
            rows.push(
                icssed::button(b, [label.into()])
                    .on_press(Message::RunSettings)
                    .into(),
            );
        }
        icssed::column(list, rows).into()
    }
}

impl BrightnessGadget {
    /// The screen the bar is on, else the first one: what the bar's
    /// percent tells.
    fn bar_screen<'a>(&self, ctx: &Context<'a>) -> Option<&'a Display> {
        let b = ctx.brightness;
        self.output
            .as_deref()
            .and_then(|o| b.on_output(o))
            .or_else(|| b.displays().first())
    }
}

/// A screen: its icon, its name and connector with the percent, over
/// the slider (none while its level is unknown).
fn screen<'a>(ctx: Context<'a>, list: Node<'a>, d: &'a Display) -> Element<'a, Message> {
    let mut node = list
        .child("screen")
        .class(match d.kind {
            Kind::Backlight => "backlight",
            Kind::Ddc => "ddc",
        })
        .class_if("unknown", d.level.is_none());
    if let Some(output) = &d.output {
        node = node.attr("output", output);
    }
    let icon_name = match d.kind {
        Kind::Backlight => ICON_PANEL,
        Kind::Ddc => ICON_MONITOR,
    };
    let name = match d.kind {
        Kind::Backlight => ctx.locale.tr("brightness.builtin"),
        Kind::Ddc if d.model.is_empty() => ctx.locale.tr("brightness.monitor"),
        Kind::Ddc => &d.model,
    };
    let info = node.child("info");
    let head = info.child("head");
    let mut parts: Vec<Element<'a, Message>> = vec![icssed::text(head.child("name"), name).into()];
    if let Some(output) = &d.output {
        parts.push(icssed::text(head.child("output"), output.as_str()).into());
    }
    let percent = d
        .percent()
        .map_or_else(|| "–".to_owned(), |p| format!("{p}%"));
    parts.push(icssed::text(head.child("value"), percent).into());
    let mut body: Vec<Element<'a, Message>> = vec![icssed::row(head, parts).into()];
    if let Some(percent) = d.percent() {
        let id = d.id.clone();
        body.push(
            icssed::slider(
                info.child("slider"),
                0.0..=100.0,
                percent as f32,
                move |v| Message::Set(id.clone(), v.round() as u32),
            )
            .step(1.0)
            .into(),
        );
    }
    icssed::row(
        node,
        [
            ctx.icon(node.child("icon"), icon_name),
            icssed::column(info, body).into(),
        ],
    )
    .into()
}
