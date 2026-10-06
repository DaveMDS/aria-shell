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
use iced::widget::{Space, column, mouse_area, row};
use iced::{Alignment, Element, Length};
use iced_wayland_subscriber::OutputInfo;

use crate::gadgets::{Action, Axis, Context, Gadget, Popup, Wheel};
use crate::process;
use crate::services::brightness::{BrightnessConfig, Command, Display, Kind, Target, WheelTarget};
use crate::ui::theme::{self, Node, Theme};

/// Icon size when the theme doesn't set `height` on an `icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;
/// Popup width when the theme doesn't size `list`.
const DEFAULT_LIST_WIDTH: f32 = 340.0;
/// The slider's rail and handle when the theme doesn't size them (the
/// theme's own defaults).
const DEFAULT_RAIL: f32 = 4.0;
const DEFAULT_HANDLE: f32 = 12.0;

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

/// A themed part of the popup and its height, built together so the
/// popup's size and its content can't disagree.
type Block<'a> = (Element<'a, Message>, f32);

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
        let theme = ctx.theme;
        let screen = self.bar_screen(&ctx);
        let button = ctx
            .node
            .child("button")
            .class_if("none", ctx.brightness.displays().is_empty());
        let mut content = row![icon(&ctx, &button.child("icon"), ICON)]
            .spacing(theme.resolve(&button).gap)
            .align_y(Alignment::Center);
        if self.config.show_percent
            && let Some(percent) = screen.and_then(Display::percent)
        {
            let t = button.child("text");
            content = content.push(theme.container(&t, theme.text(&t, format!("{percent}%"))));
        }
        let button = theme
            .button(&button, content)
            .on_press(Message::TogglePopup);
        mouse_area(self.popup.anchor(button))
            .on_right_press(Message::RunSettings)
            .on_scroll(Message::Scroll)
            .into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let list = ctx.node.child("list");
        let rows: Vec<Element<'a, Message>> = self.rows(&ctx).into_iter().map(|(e, _)| e).collect();
        ctx.theme.column(&list, rows).width(Length::Fill).into()
    }

    fn popup_size(&self, ctx: Context<'_>) -> (u32, u32) {
        let list = ctx.node.child("list");
        let s = ctx.theme.resolve(&list);
        let rows = self.rows(&ctx);
        let height = rows.iter().map(|(_, h)| h).sum::<f32>()
            + s.gap * rows.len().saturating_sub(1) as f32
            + s.padding.top
            + s.padding.bottom;
        let width = px(s.width).unwrap_or(DEFAULT_LIST_WIDTH);
        (width.ceil().max(1.0) as u32, height.ceil().max(1.0) as u32)
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

    /// Every part of the popup with its height.
    fn rows<'a>(&'a self, ctx: &Context<'a>) -> Vec<Block<'a>> {
        let theme = ctx.theme;
        let list = ctx.node.child("list");
        let mut rows: Vec<Block<'a>> = ctx
            .brightness
            .displays()
            .iter()
            .map(|d| self.screen(ctx, &list, d))
            .collect();
        if rows.is_empty() {
            let empty = list.child("empty");
            rows.push((
                theme
                    .container(
                        &empty,
                        theme.text(&empty, ctx.locale.tr("brightness.empty")),
                    )
                    .width(Length::Fill)
                    .align_x(Alignment::Center)
                    .into(),
                padded(theme, &empty, theme.line_height(&empty)),
            ));
        }
        if !self.config.settings_command.is_empty() {
            let b = list.child("button").class("settings");
            let t = b.child("text");
            let label =
                iced::widget::container(theme.text(&t, ctx.locale.tr("brightness.settings")))
                    .width(Length::Fill)
                    .align_x(Alignment::Center);
            rows.push((
                theme
                    .button(&b, label)
                    .on_press(Message::RunSettings)
                    .width(Length::Fill)
                    .into(),
                padded(theme, &b, padded(theme, &t, theme.line_height(&t))),
            ));
        }
        rows
    }

    /// A screen: its icon, its name and connector with the percent,
    /// over the slider (none while its level is unknown).
    fn screen<'a>(&'a self, ctx: &Context<'a>, list: &Node, d: &'a Display) -> Block<'a> {
        let theme = ctx.theme;
        let mut node = list
            .child("screen")
            .class(match d.kind {
                Kind::Backlight => "backlight",
                Kind::Ddc => "ddc",
            })
            .class_if("unknown", d.level.is_none());
        if let Some(output) = &d.output {
            node = node.attr("output", output.clone());
        }
        let s = theme.resolve(&node);
        let i = node.child("icon");
        let icon_name = match d.kind {
            Kind::Backlight => ICON_PANEL,
            Kind::Ddc => ICON_MONITOR,
        };
        let name_node = node.child("name");
        let output_node = node.child("output");
        let value_node = node.child("value");
        let name = match d.kind {
            Kind::Backlight => ctx.locale.tr("brightness.builtin"),
            Kind::Ddc if d.model.is_empty() => ctx.locale.tr("brightness.monitor"),
            Kind::Ddc => &d.model,
        };
        let mut head: Vec<Element<'a, Message>> = vec![
            theme
                .container(&name_node, theme.text(&name_node, name))
                .into(),
        ];
        if let Some(output) = &d.output {
            head.push(
                theme
                    .container(&output_node, theme.text(&output_node, output.as_str()))
                    .into(),
            );
        }
        head.push(Space::new().width(Length::Fill).into());
        let percent = d
            .percent()
            .map_or_else(|| "–".to_owned(), |p| format!("{p}%"));
        head.push(
            theme
                .container(&value_node, theme.text(&value_node, percent))
                .into(),
        );
        let head_height = line(theme, &name_node)
            .max(line(theme, &output_node))
            .max(line(theme, &value_node));
        let mut body: Vec<Element<'a, Message>> = vec![
            row(head)
                .spacing(s.gap)
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .into(),
        ];
        let mut body_height = head_height;
        if let Some(percent) = d.percent() {
            let slider = node.child("slider");
            let id = d.id.clone();
            body.push(
                theme
                    .slider(&slider, 0.0..=100.0, percent as f32, 1.0, move |v| {
                        Message::Set(id.clone(), v.round() as u32)
                    })
                    .into(),
            );
            body_height += s.gap + slider_height(theme, &slider);
        }
        let height = padded(theme, &i, icon_size(theme, &i)).max(body_height);
        let content = row![
            icon(ctx, &i, icon_name),
            column(body).spacing(s.gap).width(Length::Fill),
        ]
        .spacing(s.gap)
        .align_y(Alignment::Center);
        (
            theme.container(&node, content).width(Length::Fill).into(),
            padded(theme, &node, height),
        )
    }
}

fn px(l: Option<theme::Length>) -> Option<f32> {
    match l {
        Some(theme::Length::Px(px)) => Some(px),
        _ => None,
    }
}

fn padded(theme: &Theme, node: &Node, content: f32) -> f32 {
    let s = theme.resolve(node);
    content + s.padding.top + s.padding.bottom
}

/// A text line's height in `node`, its padding included.
fn line(theme: &Theme, node: &Node) -> f32 {
    padded(theme, node, theme.line_height(node))
}

fn icon_size(theme: &Theme, node: &Node) -> f32 {
    let style = theme.resolve(node);
    px(style.height.or(style.width)).unwrap_or(DEFAULT_ICON_SIZE)
}

fn slider_height(theme: &Theme, node: &Node) -> f32 {
    let rail = px(theme.resolve(node).height).unwrap_or(DEFAULT_RAIL);
    let handle = px(theme.resolve(&node.child("handle")).width).unwrap_or(DEFAULT_HANDLE);
    rail.max(handle)
}

/// A themed icon by name, at the node's `height` (its `width` if
/// there's no height), or a blank of that size.
fn icon<'a>(ctx: &Context<'a>, node: &Node, name: &str) -> Element<'a, Message> {
    let style = ctx.theme.resolve(node);
    let size = icon_size(ctx.theme, node);
    let icon = match ctx.icons.get_name(name, None) {
        Some(icon) => icon.view(size, style.color),
        None => Space::new().width(size).height(size).into(),
    };
    ctx.theme.container(node, icon).into()
}
