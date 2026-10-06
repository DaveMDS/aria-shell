//! Power gadget: on the bar the battery (UPower's icon, the percent),
//! the power profile's icon and the idle eye, each one optional and the
//! first two only where there is a battery / a profiles daemon. A left
//! click on the battery or the profile opens the popup: the battery's
//! charge, state, time and figures, the peripherals' charge, the
//! profile picker, the "keep awake" toggle, a Settings button. A left
//! click on the eye, or a middle click anywhere, holds idle or lets it
//! go; a right click runs `settings_command`.
//!
//! Holds no power state: it comes from `ctx.power` and `ctx.idle`;
//! what the user does goes back as `Action::Power` / `Action::Idle`.

use iced::widget::{Space, column, mouse_area, row, text};
use iced::{Alignment, Element, Length};
use iced_wayland_subscriber::OutputInfo;

use crate::gadgets::{Action, Context, Gadget, Popup};
use crate::locale::Locale;
use crate::process;
use crate::services::idle;
use crate::services::power::{Battery, Command, PowerConfig, State, Warning};
use crate::theme::{self, Node};

/// Icon size when the theme doesn't set `height` on an `icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;
/// Popup width when the theme doesn't size `list`.
const DEFAULT_LIST_WIDTH: f32 = 320.0;

/// When UPower names no icon.
const ICON_BATTERY: &str = "battery-missing-symbolic";

pub struct PowerGadget {
    config: PowerConfig,
    popup: Popup,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    /// Hold idle or let it go.
    ToggleIdle,
    SetIdle(bool),
    SetProfile(String),
    RunSettings,
}

/// A themed part of the popup and its height, built together so the
/// popup's size and its content can't disagree.
type Block<'a> = (Element<'a, Message>, f32);

impl Gadget for PowerGadget {
    type Config = PowerConfig;
    type Message = Message;

    fn new(config: PowerConfig, _output: &OutputInfo) -> Self {
        Self {
            config,
            popup: Popup::new(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::TogglePopup => self.popup.toggle(),
            Message::ToggleIdle => Action::Idle(idle::Command::ToggleInhibit),
            Message::SetIdle(on) => Action::Idle(idle::Command::SetInhibit(on)),
            Message::SetProfile(profile) => Action::Power(Command::SetProfile(profile)),
            Message::RunSettings => {
                process::run(&self.config.settings_command);
                self.popup.close()
            }
        }
    }

    fn icon_names(&self) -> Vec<String> {
        // The battery's names come from UPower: the daemon resolves
        // them from `ctx.power` (`Power::icon_names`).
        let mut names = vec![
            self.config.idle_icon.clone(),
            self.config.inhibit_icon.clone(),
            ICON_BATTERY.to_owned(),
        ];
        names.extend(PROFILES.iter().map(|p| profile_icon(p)));
        names
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        let battery = ctx.power.battery().filter(|_| self.config.show_battery);
        let profile = ctx
            .power
            .profiles()
            .filter(|_| self.config.show_profile)
            .map(|p| p.active.as_str());
        if battery.is_some() || profile.is_some() {
            let mut button = ctx.node.child("button").class("status");
            if let Some(b) = battery {
                button = button.class(b.state.name()).class_if(
                    match b.warning {
                        Warning::Critical => "critical",
                        _ => "low",
                    },
                    b.warning != Warning::None,
                );
            }
            if let Some(p) = profile {
                button = button.class(profile_class(p));
            }
            let mut content: Vec<Element<'a, Message>> = Vec::new();
            if let Some(b) = battery {
                content.push(icon(
                    &ctx,
                    &button.child("icon").class("battery"),
                    battery_icon(b),
                ));
                if self.config.show_percent {
                    let t = button.child("text");
                    content.push(
                        theme
                            .container(&t, theme.text(&t, format!("{:.0}%", b.percentage)))
                            .into(),
                    );
                }
            }
            if let Some(p) = profile {
                content.push(icon(
                    &ctx,
                    &button.child("icon").class("profile"),
                    &profile_icon(p),
                ));
            }
            let content = row(content)
                .spacing(theme.resolve(&button).gap)
                .align_y(Alignment::Center);
            parts.push(
                self.popup.anchor(
                    theme
                        .button(&button, content)
                        .on_press(Message::TogglePopup),
                ),
            );
        }
        if self.config.show_idle {
            let inhibited = ctx.idle.inhibited();
            let button = ctx
                .node
                .child("button")
                .class("idle")
                .class_if("inhibited", inhibited)
                .class_if("playing", ctx.idle.held_by_player());
            let name = if inhibited {
                &self.config.inhibit_icon
            } else {
                &self.config.idle_icon
            };
            parts.push(
                theme
                    .button(&button, icon(&ctx, &button.child("icon"), name))
                    .on_press(Message::ToggleIdle)
                    .into(),
            );
        }
        let gap = theme.resolve(&ctx.node).gap;
        mouse_area(row(parts).spacing(gap).align_y(Alignment::Center))
            .on_middle_press(Message::ToggleIdle)
            .on_right_press(Message::RunSettings)
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
        (
            list_width(&ctx).ceil().max(1.0) as u32,
            height.ceil().max(1.0) as u32,
        )
    }
}

/// The profiles power-profiles-daemon knows, in the picker's order.
const PROFILES: [&str; 3] = ["power-saver", "balanced", "performance"];

pub(crate) fn profile_icon(profile: &str) -> String {
    format!("power-profile-{profile}-symbolic")
}

/// The class for a profile: its name when it's a known one.
fn profile_class(profile: &str) -> &'static str {
    PROFILES
        .iter()
        .find(|p| **p == profile)
        .copied()
        .unwrap_or("other")
}

pub(crate) fn profile_label(locale: &Locale, profile: &str) -> String {
    match profile {
        "power-saver" => locale.tr("power.profile.power_saver").to_owned(),
        "balanced" => locale.tr("power.profile.balanced").to_owned(),
        "performance" => locale.tr("power.profile.performance").to_owned(),
        other => other.to_owned(),
    }
}

pub(crate) fn battery_icon(b: &Battery) -> &str {
    if b.icon.is_empty() {
        ICON_BATTERY
    } else {
        &b.icon
    }
}

/// `1 h 20 min`, `45 min`.
pub fn duration(locale: &Locale, secs: u64) -> String {
    let minutes = secs.div_ceil(60);
    let (h, m) = (minutes / 60, minutes % 60);
    if h == 0 {
        locale.fmt("power.minutes", &[("m", &m)])
    } else {
        locale.fmt("power.hours_minutes", &[("h", &h), ("m", &m)])
    }
}

/// What the battery is doing, in words.
fn battery_status(locale: &Locale, b: &Battery) -> String {
    match b.state {
        State::Charging if b.time_to_full > 0 => locale.fmt(
            "power.charging_time",
            &[("time", &duration(locale, b.time_to_full))],
        ),
        State::Charging => locale.tr("power.charging").to_owned(),
        State::Discharging if b.time_to_empty > 0 => locale.fmt(
            "power.time_left",
            &[("time", &duration(locale, b.time_to_empty))],
        ),
        State::Discharging => locale.tr("power.discharging").to_owned(),
        State::Full => locale.tr("power.full").to_owned(),
        State::NotCharging => locale.tr("power.not_charging").to_owned(),
        State::Empty => locale.tr("power.empty").to_owned(),
        State::Unknown => String::new(),
    }
}

fn px(l: Option<theme::Length>) -> Option<f32> {
    match l {
        Some(theme::Length::Px(px)) => Some(px),
        _ => None,
    }
}

fn padded(theme: &theme::Theme, node: &Node, content: f32) -> f32 {
    let s = theme.resolve(node);
    content + s.padding.top + s.padding.bottom
}

fn icon_size(ctx: &Context<'_>, node: &Node) -> f32 {
    let style = ctx.theme.resolve(node);
    px(style.height.or(style.width)).unwrap_or(DEFAULT_ICON_SIZE)
}

/// A themed icon by name, at the node's `height` (its `width` if
/// there's no height), or a blank of that size.
fn icon<'a>(ctx: &Context<'a>, node: &Node, name: &str) -> Element<'a, Message> {
    let style = ctx.theme.resolve(node);
    let size = icon_size(ctx, node);
    let icon = match ctx.icons.get_name(name, None) {
        Some(icon) => icon.view(size, style.color),
        None => Space::new().width(size).height(size).into(),
    };
    ctx.theme.container(node, icon).into()
}

/// A text line's height in `node`, its padding included.
fn line(ctx: &Context<'_>, node: &Node) -> f32 {
    padded(ctx.theme, node, ctx.theme.line_height(node))
}

fn list_width(ctx: &Context<'_>) -> f32 {
    px(ctx.theme.resolve(&ctx.node.child("list")).width).unwrap_or(DEFAULT_LIST_WIDTH)
}

impl PowerGadget {
    /// Every part of the popup with its height.
    fn rows<'a>(&'a self, ctx: &Context<'a>) -> Vec<Block<'a>> {
        let list = ctx.node.child("list");
        let mut rows = Vec::new();
        if let Some(b) = ctx.power.battery() {
            rows.push(self.battery(ctx, &list, b));
            let mut lines = Vec::new();
            if b.energy_rate > 0.0 && b.state != State::Full {
                lines.push(ctx.locale.fmt(
                    "power.rate",
                    &[("w", &ctx.locale.decimal(b.energy_rate, 1))],
                ));
            }
            if let Some(c) = b.capacity {
                lines.push(ctx.locale.fmt("power.health", &[("n", &format!("{c:.0}"))]));
            }
            if !lines.is_empty() {
                rows.push(self.details(ctx, &list, lines));
            }
        }
        if !ctx.power.devices().is_empty() {
            rows.push(self.header(ctx, &list, "devices", ctx.locale.tr("power.devices")));
            for d in ctx.power.devices() {
                let name = if d.model.is_empty() {
                    ctx.locale.tr("power.device").to_owned()
                } else {
                    d.model.clone()
                };
                let icon_name = if d.icon.is_empty() {
                    ICON_BATTERY
                } else {
                    &d.icon
                };
                rows.push(self.device(ctx, &list, icon_name, name, d.percentage));
            }
        }
        if let Some(p) = ctx.power.profiles() {
            rows.push(self.header(ctx, &list, "profile", ctx.locale.tr("power.profile")));
            rows.push(self.profiles(ctx, &list, &p.available, &p.active));
            if !p.degraded.is_empty() {
                let reason = match p.degraded.as_str() {
                    "lap-detected" => ctx.locale.tr("power.degraded.lap"),
                    "high-operating-temperature" => ctx.locale.tr("power.degraded.heat"),
                    other => other,
                };
                rows.push(self.message(
                    ctx,
                    &list,
                    ctx.locale.fmt("power.degraded", &[("reason", &reason)]),
                ));
            }
        }
        rows.push(self.idle(ctx, &list));
        if ctx.idle.held_by_player() && !ctx.idle.inhibited() {
            rows.push(self.message(ctx, &list, ctx.locale.tr("power.held_by_player").to_owned()));
        }
        if !self.config.settings_command.is_empty() {
            let b = list.child("button").class("settings");
            let t = b.child("text");
            let label =
                iced::widget::container(ctx.theme.text(&t, ctx.locale.tr("power.settings")))
                    .width(Length::Fill)
                    .align_x(Alignment::Center);
            rows.push((
                ctx.theme
                    .button(&b, label)
                    .on_press(Message::RunSettings)
                    .width(Length::Fill)
                    .into(),
                padded(ctx.theme, &b, line(ctx, &t)),
            ));
        }
        rows
    }

    /// The battery: its icon, the percent, what it's doing.
    fn battery<'a>(&'a self, ctx: &Context<'a>, list: &Node, b: &'a Battery) -> Block<'a> {
        let theme = ctx.theme;
        let node = list
            .child("battery")
            .class(b.state.name())
            .class_if("low", b.warning == Warning::Low)
            .class_if("critical", b.warning == Warning::Critical);
        let i = node.child("icon");
        let pct = node.child("percent");
        let st = node.child("status");
        let height = padded(theme, &i, icon_size(ctx, &i))
            .max(line(ctx, &pct))
            .max(line(ctx, &st));
        let content = row![
            icon(ctx, &i, battery_icon(b)),
            theme.container(&pct, theme.text(&pct, format!("{:.0}%", b.percentage))),
            Space::new().width(Length::Fill),
            theme.container(&st, theme.text(&st, battery_status(ctx.locale, b))),
        ]
        .spacing(theme.resolve(&node).gap)
        .align_y(Alignment::Center);
        (
            theme.container(&node, content).width(Length::Fill).into(),
            padded(theme, &node, height),
        )
    }

    /// Facts, one line each.
    fn details<'a>(&'a self, ctx: &Context<'a>, list: &Node, lines: Vec<String>) -> Block<'a> {
        let theme = ctx.theme;
        let details = list.child("details");
        let l = details.child("line");
        let s = theme.resolve(&details);
        let n = lines.len();
        let rows: Vec<Element<'a, Message>> = lines
            .into_iter()
            .map(|t| {
                theme
                    .container(&l, theme.text(&l, t).wrapping(text::Wrapping::None))
                    .width(Length::Fill)
                    .into()
            })
            .collect();
        (
            theme
                .container(&details, column(rows).spacing(s.gap).width(Length::Fill))
                .width(Length::Fill)
                .into(),
            padded(
                theme,
                &details,
                line(ctx, &l) * n as f32 + s.gap * n.saturating_sub(1) as f32,
            ),
        )
    }

    /// A section title.
    fn header<'a>(
        &'a self,
        ctx: &Context<'a>,
        list: &Node,
        class: &'static str,
        title: &'a str,
    ) -> Block<'a> {
        let theme = ctx.theme;
        let header = list.child("header").class(class);
        let t = header.child("title");
        (
            theme
                .container(
                    &header,
                    theme
                        .container(&t, theme.text(&t, title))
                        .width(Length::Fill),
                )
                .width(Length::Fill)
                .into(),
            padded(theme, &header, line(ctx, &t)),
        )
    }

    /// A peripheral: its icon, its name, its charge.
    fn device<'a>(
        &'a self,
        ctx: &Context<'a>,
        list: &Node,
        icon_name: &str,
        name: String,
        percentage: f64,
    ) -> Block<'a> {
        let theme = ctx.theme;
        let node = list.child("device");
        let i = node.child("icon");
        let n = node.child("name");
        let pct = node.child("percent");
        let height = padded(theme, &i, icon_size(ctx, &i))
            .max(line(ctx, &n))
            .max(line(ctx, &pct));
        let content = row![
            icon(ctx, &i, icon_name),
            theme.container(&n, theme.text(&n, name).wrapping(text::Wrapping::None)),
            Space::new().width(Length::Fill),
            theme.container(&pct, theme.text(&pct, format!("{percentage:.0}%"))),
        ]
        .spacing(theme.resolve(&node).gap)
        .align_y(Alignment::Center);
        (
            theme.container(&node, content).width(Length::Fill).into(),
            padded(theme, &node, height),
        )
    }

    /// The profile picker: a button per profile, the active one marked.
    fn profiles<'a>(
        &'a self,
        ctx: &Context<'a>,
        list: &Node,
        available: &[String],
        active: &str,
    ) -> Block<'a> {
        let theme = ctx.theme;
        let node = list.child("profiles");
        let mut height: f32 = 0.0;
        // Known ones in their order, then whatever else the daemon has.
        let ordered: Vec<&String> = PROFILES
            .iter()
            .filter_map(|p| available.iter().find(|a| a == p))
            .chain(available.iter().filter(|a| !PROFILES.contains(&a.as_str())))
            .collect();
        let buttons: Vec<Element<'a, Message>> = ordered
            .into_iter()
            .map(|p| {
                let b = node
                    .child("button")
                    .class(profile_class(p))
                    .class_if("active", p == active);
                let i = b.child("icon");
                let t = b.child("text");
                let gap = theme.resolve(&b).gap;
                height = height.max(padded(
                    theme,
                    &b,
                    padded(theme, &i, icon_size(ctx, &i)) + gap + line(ctx, &t),
                ));
                // The icon over the label: three side by side fit the
                // popup's width.
                let content = column![
                    icon(ctx, &i, &profile_icon(p)),
                    theme.container(
                        &t,
                        theme
                            .text(&t, profile_label(ctx.locale, p))
                            .wrapping(text::Wrapping::None)
                    ),
                ]
                .spacing(gap)
                .align_x(Alignment::Center);
                theme
                    .button(
                        &b,
                        iced::widget::container(content)
                            .width(Length::Fill)
                            .align_x(Alignment::Center),
                    )
                    .on_press(Message::SetProfile(p.clone()))
                    .width(Length::Fill)
                    .into()
            })
            .collect();
        (
            theme
                .container(&node, row(buttons).spacing(theme.resolve(&node).gap))
                .width(Length::Fill)
                .into(),
            padded(theme, &node, height),
        )
    }

    /// "Keep awake" and its toggle.
    fn idle<'a>(&'a self, ctx: &Context<'a>, list: &Node) -> Block<'a> {
        let theme = ctx.theme;
        let inhibited = ctx.idle.inhibited();
        let node = list
            .child("idle")
            .class_if("inhibited", inhibited)
            .class_if("playing", ctx.idle.held_by_player());
        let i = node.child("icon");
        let t = node.child("title");
        let toggle = node.child("toggle");
        let name = if inhibited {
            &self.config.inhibit_icon
        } else {
            &self.config.idle_icon
        };
        let height = padded(theme, &i, icon_size(ctx, &i))
            .max(line(ctx, &t))
            .max(px(theme.resolve(&toggle).height).unwrap_or(16.0));
        let content = row![
            icon(ctx, &i, name),
            theme
                .container(&t, theme.text(&t, ctx.locale.tr("power.keep_awake")))
                .width(Length::Fill),
            theme.toggler(&toggle, inhibited, Message::SetIdle),
        ]
        .spacing(theme.resolve(&node).gap)
        .align_y(Alignment::Center);
        (
            theme.container(&node, content).width(Length::Fill).into(),
            padded(theme, &node, height),
        )
    }

    /// A line of text: a hint, a warning.
    fn message<'a>(&'a self, ctx: &Context<'a>, list: &Node, text: String) -> Block<'a> {
        let theme = ctx.theme;
        let m = list.child("message");
        let s = theme.resolve(&m);
        let width = list_width(ctx) - s.padding.left - s.padding.right;
        let height = theme
            .measure_in(&m, &text, width)
            .height
            .max(theme.line_height(&m));
        (
            theme
                .container(&m, theme.text(&m, text))
                .width(Length::Fill)
                .into(),
            padded(theme, &m, height),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        let en = Locale::new("en");
        assert_eq!(duration(&en, 45 * 60), "45 min");
        assert_eq!(duration(&en, 80 * 60), "1 h 20 min");
        assert_eq!(duration(&en, 61), "2 min", "rounded up");
    }

    #[test]
    fn profiles() {
        assert_eq!(profile_class("power-saver"), "power-saver");
        assert_eq!(profile_class("quiet"), "other");
        assert_eq!(profile_icon("balanced"), "power-profile-balanced-symbolic");
    }
}
