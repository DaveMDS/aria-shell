//! Custom gadget: a user-defined button. An icon and/or a label, a
//! program per mouse button and wheel direction, and optionally a
//! program (`exec`) whose output is the label, run every `interval`
//! seconds and again after every click. The output is plain text or,
//! as waybar's custom modules do, JSON with `text`, `icon` and `class`
//! (CSS classes on the button).
//!
//! Programs are run as written, without a shell (see
//! [`crate::process`]); `[Custom:id]` sections give several of them.
//!
//! Holds no output: `exec` is described to the daemon as a
//! [`scripts::Spec`] (run once for every panel showing it) and its last
//! output read from `ctx.scripts`.

use iced::Element;
use iced::mouse::ScrollDelta;
use iced::widget::{Space, mouse_area, row};
use iced_wayland_subscriber::OutputInfo;

use crate::config::{RawSection, Section};
use crate::gadget::{Action, Axis, Context, Gadget, Wheel};
use crate::process;
use crate::scripts::{self, Output, ReturnType, Spec};
use crate::theme;

/// `[Custom]` section.
#[derive(Debug, Clone)]
pub struct CustomConfig {
    /// Fixed text, and an icon name from the icon theme.
    pub label: String,
    pub icon: String,
    /// Command lines run on the left, right and middle button, and on
    /// the wheel.
    pub command: String,
    pub command_right: String,
    pub command_middle: String,
    pub command_wheel_up: String,
    pub command_wheel_down: String,
    /// Command line whose standard output feeds the label; empty for a
    /// static gadget.
    pub exec: String,
    /// Seconds between runs of `exec`; 0 runs it once (and after clicks).
    pub interval: u64,
    /// The label: `{}` is `exec`'s output (or `label` without `exec`),
    /// `{label}` the `label` key.
    pub format: String,
    /// How `exec`'s output is read.
    pub return_type: ReturnType,
    /// With `exec`: show nothing while the output is empty (or the
    /// program failed).
    pub hide_empty: bool,
}

impl Section for CustomConfig {
    const NAME: &'static str = "Custom";

    fn from_raw(raw: &RawSection) -> Self {
        let return_type = match raw.get("return_type") {
            None | Some("text") => ReturnType::Text,
            Some("json") => ReturnType::Json,
            Some(other) => {
                log::warn!("invalid return_type {other:?}, using text");
                ReturnType::Text
            }
        };
        Self {
            label: raw.str_or("label", ""),
            icon: raw.str_or("icon", ""),
            command: raw.str_or("command", ""),
            command_right: raw.str_or("command_right", ""),
            command_middle: raw.str_or("command_middle", ""),
            command_wheel_up: raw.str_or("command_wheel_up", ""),
            command_wheel_down: raw.str_or("command_wheel_down", ""),
            exec: raw.str_or("exec", ""),
            interval: raw.u64_or("interval", 0),
            format: raw.str_or("format", "{}"),
            return_type,
            hide_empty: raw.bool_or("hide_empty", true),
        }
    }
}

/// Icon size when the theme doesn't set `height` on `icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;

pub struct Custom {
    config: CustomConfig,
    /// `exec`, for the daemon; `None` for a static gadget.
    script: Option<Spec>,
    wheel: Wheel,
}

#[derive(Clone, Debug)]
pub enum Message {
    Left,
    Right,
    Middle,
    Scroll(ScrollDelta),
}

impl Gadget for Custom {
    type Config = CustomConfig;
    type Message = Message;

    fn new(config: CustomConfig, _output: &OutputInfo) -> Self {
        Self::with_config(config)
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        let line = match message {
            Message::Left => &self.config.command,
            Message::Right => &self.config.command_right,
            Message::Middle => &self.config.command_middle,
            Message::Scroll(delta) => match self.wheel.clicks(delta) {
                Some((clicks, Axis::Vertical)) if clicks > 0 => &self.config.command_wheel_up,
                Some((_, Axis::Vertical)) => &self.config.command_wheel_down,
                _ => return Action::None,
            },
        };
        if line.is_empty() {
            return Action::None;
        }
        process::run(line);
        // What the command did may show in the output.
        match &self.script {
            Some(spec) => Action::Script(scripts::Command::Refresh(spec.clone())),
            None => Action::None,
        }
    }

    fn icon_names(&self) -> Vec<String> {
        if self.config.icon.is_empty() {
            Vec::new()
        } else {
            vec![self.config.icon.clone()]
        }
    }

    fn script(&self) -> Option<Spec> {
        self.script.clone()
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let output = self.script.as_ref().and_then(|s| ctx.scripts.output(s));
        if self.script.is_some()
            && self.config.hide_empty
            && output.is_none_or(|o| o.text.is_empty())
        {
            return Space::new().into();
        }
        let mut button = ctx.node.child("button");
        for class in output.map(|o| o.classes.as_slice()).unwrap_or_default() {
            button = button.class(class.clone());
        }
        let style = theme.resolve(&button);

        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        let icon = output
            .and_then(|o| o.icon.as_deref())
            .unwrap_or(&self.config.icon);
        if !icon.is_empty() {
            let icon_node = button.child("icon");
            let icon_style = theme.resolve(&icon_node);
            let size = icon_style
                .height
                .or(icon_style.width)
                .and_then(|l| match l {
                    theme::Length::Px(px) => Some(px),
                    _ => None,
                })
                .unwrap_or(DEFAULT_ICON_SIZE);
            let icon: Element<'a, Message> = match ctx.icons.get_name(icon, None) {
                Some(icon) => icon.view(size, icon_style.color),
                None => Space::new().width(size).height(size).into(),
            };
            // In containers tagged with their node, so `debug widgets`
            // sees them and the theme can pad them.
            parts.push(theme.container(&icon_node, icon).into());
        }
        let text = self.text(output);
        if !text.is_empty() {
            let text_node = button.child("text");
            let text = theme.text(&text_node, text);
            parts.push(theme.container(&text_node, text).into());
        }
        let content = row(parts)
            .spacing(style.gap)
            .align_y(iced::Alignment::Center);
        mouse_area(theme.button(&button, content).on_press(Message::Left))
            .on_right_press(Message::Right)
            .on_middle_press(Message::Middle)
            .on_scroll(Message::Scroll)
            .into()
    }
}

impl Custom {
    fn with_config(config: CustomConfig) -> Self {
        let argv = process::split_words(&config.exec);
        let script = (!argv.is_empty()).then_some(Spec {
            argv,
            interval: config.interval,
            return_type: config.return_type,
        });
        Self {
            config,
            script,
            wheel: Wheel::default(),
        }
    }

    /// The label for `output` (the last one, or none yet).
    fn text(&self, output: Option<&Output>) -> String {
        let value = match output {
            Some(o) => o.text.as_str(),
            None => &self.config.label,
        };
        self.config
            .format
            .replace("{label}", &self.config.label)
            .replace("{}", value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gadget(keys: &[(&str, &str)]) -> Custom {
        let mut text = String::from("[Custom]\n");
        for (k, v) in keys {
            text.push_str(&format!("{k} = {v}\n"));
        }
        Custom::with_config(crate::config::Config::parse(&text).section(None))
    }

    #[test]
    fn label_from_format() {
        let g = gadget(&[("label", "Aria")]);
        assert_eq!(g.text(None), "Aria", "no exec: {{}} is the label");
        let g = gadget(&[("label", "up"), ("exec", "true"), ("format", "{label}: {}")]);
        assert_eq!(g.text(None), "up: up", "before the first run, the label");
        let out = Output {
            text: "3".into(),
            ..Output::default()
        };
        assert_eq!(g.text(Some(&out)), "up: 3");
    }

    #[test]
    fn commands_refresh_the_script() {
        let mut g = gadget(&[("exec", "true"), ("command", "")]);
        assert!(
            matches!(g.update(Message::Left), Action::None),
            "no command"
        );
        let mut g = gadget(&[("exec", "true"), ("command", "true")]);
        assert!(matches!(g.update(Message::Left), Action::Script(_)));
        assert!(
            matches!(
                g.update(Message::Scroll(ScrollDelta::Lines { x: 0.0, y: 1.0 })),
                Action::None
            ),
            "no command_wheel_up"
        );
        let mut g = gadget(&[("command", "true")]);
        assert!(
            matches!(g.update(Message::Left), Action::None),
            "static: nothing to refresh"
        );
    }
}
