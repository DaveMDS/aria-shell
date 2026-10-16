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

use iced::widget::mouse_area;
use iced_wayland_subscriber::OutputInfo;
use icssed::Element;

use crate::gadgets::{Action, Context, Gadget, Popup};
use crate::locale::Locale;
use crate::process;
use crate::services::idle;
use crate::services::power::{
    Battery, Command, ICON_BATTERY, PowerConfig, State, Warning, duration, profile_icon,
    profile_label,
};

pub struct PowerGadget {
    config: PowerConfig,
    popup: Popup,
}

#[derive(Clone, Debug)]
pub enum Message {
    /// From the battery (anchor 0) or the profile (1).
    TogglePopup(usize),
    /// Hold idle or let it go.
    ToggleIdle,
    SetIdle(bool),
    SetProfile(String),
    RunSettings,
}

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
            Message::TogglePopup(n) => self.popup.toggle_nth(n),
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
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        let battery = ctx.power.battery().filter(|_| self.config.show_battery);
        let profile = ctx
            .power
            .profiles()
            .filter(|_| self.config.show_profile)
            .map(|p| p.active.as_str());
        if let Some(b) = battery {
            let mut content: Vec<Element<'a, Message>> = vec![ctx.icon(b.icon_name()).into()];
            if self.config.show_percent {
                content.push(icssed::text(format!("{:.0}%", b.percentage)).into());
            }
            parts.push(
                icssed::button(content)
                    .kind("indicator")
                    .class("battery")
                    .class_if("warning", b.warning == Warning::Low)
                    .class_if("danger", b.warning == Warning::Critical)
                    .on_press(Message::TogglePopup(0))
                    .id(self.popup.anchor_nth(0))
                    .into(),
            );
        }
        if let Some(p) = profile {
            parts.push(
                icssed::button![ctx.icon(profile_icon(p))]
                    .kind("indicator")
                    .class("profile")
                    .on_press(Message::TogglePopup(1))
                    .id(self.popup.anchor_nth(1))
                    .into(),
            );
        }
        if self.config.show_idle {
            let inhibited = ctx.idle.inhibited();
            let name = if inhibited {
                &self.config.inhibit_icon
            } else {
                &self.config.idle_icon
            };
            parts.push(
                icssed::button![ctx.icon(name.as_str())]
                    .kind("indicator")
                    .class("idle")
                    .class_if("active", inhibited)
                    .on_press(Message::ToggleIdle)
                    .into(),
            );
        }
        mouse_area(icssed::row(parts))
            .on_middle_press(Message::ToggleIdle)
            .on_right_press(Message::RunSettings)
            .into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        icssed::column(self.rows(ctx)).kind("list").into()
    }
}

/// The profiles power-profiles-daemon knows, in the picker's order.
const PROFILES: [&str; 3] = ["power-saver", "balanced", "performance"];

/// The class for a profile: its name when it's a known one.
fn profile_class(profile: &str) -> &'static str {
    PROFILES
        .iter()
        .find(|p| **p == profile)
        .copied()
        .unwrap_or("other")
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

impl PowerGadget {
    /// Every part of the popup.
    fn rows<'a>(&'a self, ctx: Context<'a>) -> Vec<Element<'a, Message>> {
        let mut rows = Vec::new();
        if let Some(b) = ctx.power.battery() {
            rows.push(battery(ctx, b));
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
                let lines = lines
                    .into_iter()
                    .map(|l| icssed::text(l).kind("line").into());
                rows.push(icssed::column(lines).kind("details").into());
            }
        }
        if !ctx.power.devices().is_empty() {
            rows.push(header("devices", ctx.locale.tr("power.devices")));
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
                rows.push(
                    icssed::row![
                        ctx.icon(icon_name),
                        icssed::text(name).kind("name"),
                        icssed::text(format!("{:.0}%", d.percentage)).kind("percent"),
                    ]
                    .kind("device")
                    .into(),
                );
            }
        }
        if let Some(p) = ctx.power.profiles() {
            rows.push(header("profile", ctx.locale.tr("power.profile")));
            rows.push(profiles(ctx, &p.available, &p.active));
            if !p.degraded.is_empty() {
                let reason = match p.degraded.as_str() {
                    "lap-detected" => ctx.locale.tr("power.degraded.lap"),
                    "high-operating-temperature" => ctx.locale.tr("power.degraded.heat"),
                    other => other,
                };
                let text = ctx.locale.fmt("power.degraded", &[("reason", &reason)]);
                rows.push(super::note(text).kind("message").into());
            }
        }
        rows.push(self.idle(ctx));
        if ctx.idle.held_by_player() && !ctx.idle.inhibited() {
            let text = ctx.locale.tr("power.held_by_player");
            rows.push(super::note(text).kind("message").into());
        }
        if !self.config.settings_command.is_empty() {
            rows.push(
                icssed::button![icssed::text(ctx.locale.tr("power.settings"))]
                    .class("settings")
                    .on_press(Message::RunSettings)
                    .into(),
            );
        }
        rows
    }

    /// "Keep awake" and its toggle.
    fn idle<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let inhibited = ctx.idle.inhibited();
        let name = if inhibited {
            &self.config.inhibit_icon
        } else {
            &self.config.idle_icon
        };
        icssed::row![
            ctx.icon(name.as_str()),
            icssed::text(ctx.locale.tr("power.keep_awake")).kind("title"),
            icssed::toggle(inhibited)
                .on_toggle(Message::SetIdle)
                .switch(),
        ]
        .kind("idle")
        .class_if("active", inhibited)
        .into()
    }
}

/// The battery: its icon, the percent, what it's doing.
fn battery<'a>(ctx: Context<'a>, b: &'a Battery) -> Element<'a, Message> {
    icssed::row![
        ctx.icon(b.icon_name()),
        icssed::text(format!("{:.0}%", b.percentage)).kind("percent"),
        icssed::text(battery_status(ctx.locale, b)).kind("status"),
    ]
    .kind("battery")
    .class_if("warning", b.warning == Warning::Low)
    .class_if("danger", b.warning == Warning::Critical)
    .into()
}

/// A section's title.
fn header<'a>(class: &str, title: &'a str) -> Element<'a, Message> {
    icssed::row![icssed::text(title).kind("title")]
        .kind("header")
        .class(class)
        .into()
}

/// The profile picker: a pressed button per profile, the active one
/// `:checked`, its icon over its name.
fn profiles<'a>(ctx: Context<'a>, available: &[String], active: &str) -> Element<'a, Message> {
    // Known ones in their order, then whatever else the daemon has.
    let ordered = PROFILES
        .iter()
        .filter_map(|p| available.iter().find(|a| a == p))
        .chain(available.iter().filter(|a| !PROFILES.contains(&a.as_str())));
    let toggles = ordered.map(|p| {
        icssed::toggle(p == active)
            .on_toggle(move |_| Message::SetProfile(p.clone()))
            .icon(ctx.named(profile_icon(p)))
            .label(profile_label(ctx.locale, p))
            .class(profile_class(p))
            .into()
    });
    icssed::row(toggles).kind("profiles").into()
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
