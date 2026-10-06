//! Power: the battery and the peripherals' charge (UPower), whether the
//! machine runs on battery, and the power profiles (power-profiles-daemon
//! or tuned-ppd). Shaped like `Network`: [`Power::subscription`] re-reads
//! both services after their signals (upower.rs), gadgets read the
//! result from their view context and act with a [`Command`] the daemon
//! runs with [`Power::run`]. Idle reads `on_battery` from here.
//!
//! The low battery notification follows UPower's own `WarningLevel`
//! (its `PercentageLow` / `PercentageCritical`, system-wide), sent on
//! the session bus as any app's: our notification daemon or another.

mod upower;

use iced::{Subscription, Task};
use zbus::Connection;

use crate::config::{RawSection, Section};

/// `[Power]` section: the daemon's key and the gadget's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerConfig {
    /// A notification when the battery gets low, and critical.
    pub notify_low: bool,
    /// The bar's three parts.
    pub show_battery: bool,
    pub show_percent: bool,
    pub show_profile: bool,
    pub show_idle: bool,
    /// A program (the power settings) run by the popup's button and a
    /// right click; none when empty.
    pub settings_command: String,
    /// The eye's icon names: idle may come, and idle held by the user.
    pub idle_icon: String,
    pub inhibit_icon: String,
}

impl Section for PowerConfig {
    const NAME: &'static str = "Power";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            notify_low: raw.bool_or("notify_low", true),
            show_battery: raw.bool_or("show_battery", true),
            show_percent: raw.bool_or("show_percent", true),
            show_profile: raw.bool_or("show_profile", true),
            show_idle: raw.bool_or("show_idle", true),
            settings_command: raw.str_or("settings_command", ""),
            idle_icon: raw.str_or("idle_icon", "view-conceal-symbolic"),
            inhibit_icon: raw.str_or("inhibit_icon", "view-reveal-symbolic"),
        }
    }
}

/// A UPower device's `State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Charging,
    Discharging,
    Empty,
    Full,
    /// Plugged in, not charging (a charge threshold, a full battery).
    NotCharging,
    Unknown,
}

impl State {
    pub fn from_upower(v: u32) -> Self {
        match v {
            1 => Self::Charging,
            2 | 6 => Self::Discharging,
            3 => Self::Empty,
            4 => Self::Full,
            5 => Self::NotCharging,
            _ => Self::Unknown,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Charging => "charging",
            Self::Discharging => "discharging",
            Self::Empty => "empty",
            Self::Full => "full",
            Self::NotCharging => "not-charging",
            Self::Unknown => "unknown",
        }
    }
}

/// UPower's `WarningLevel` for the battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Warning {
    #[default]
    None,
    Low,
    /// Critical, or past it (UPower's action is coming).
    Critical,
}

impl Warning {
    pub fn from_upower(v: u32) -> Self {
        match v {
            3 => Self::Low,
            4 | 5 => Self::Critical,
            _ => Self::None,
        }
    }
}

/// The machine's battery, as UPower's display device sums it up.
#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    pub percentage: f64,
    pub state: State,
    /// Seconds, 0 when unknown.
    pub time_to_empty: u64,
    pub time_to_full: u64,
    /// Watts, in or out.
    pub energy_rate: f64,
    pub warning: Warning,
    pub icon: String,
    /// Health: the full charge against the design one, percent.
    pub capacity: Option<f64>,
}

/// A peripheral with a battery: a mouse, a keyboard, a headset...
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub path: String,
    /// UPower's `Type`.
    pub kind: u32,
    pub model: String,
    pub percentage: f64,
    pub state: State,
    pub icon: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    pub on_battery: bool,
    pub battery: Option<Battery>,
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profiles {
    /// `power-saver`, `balanced`, `performance`.
    pub active: String,
    pub available: Vec<String>,
    /// Why `performance` is held back (`lap-detected`,
    /// `high-operating-temperature`); empty when it isn't.
    pub degraded: String,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// The system bus.
    Bus(Connection),
    /// UPower's state; `None` while it isn't running.
    UPower(Option<Box<Snapshot>>),
    /// The profiles; `None` while nobody serves them.
    Profiles(Option<Profiles>),
    /// The low battery notification got this id.
    Notified(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    SetProfile(String),
}

pub struct Power {
    config: PowerConfig,
    bus: Option<Connection>,
    upower: Option<Snapshot>,
    profiles: Option<Profiles>,
    /// The warning last notified, and the notification's id (to replace
    /// it, or close it once the battery is fine).
    warned: Warning,
    notification: u32,
}

/// What [`Power::apply`] found worth telling: the battery reached a
/// warning level (the daemon words the notification).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Low {
    pub warning: Warning,
    pub percentage: f64,
    pub time_to_empty: u64,
}

impl Power {
    pub fn new(config: PowerConfig) -> Self {
        Self {
            config,
            bus: None,
            upower: None,
            profiles: None,
            warned: Warning::None,
            notification: 0,
        }
    }

    pub fn set_config(&mut self, config: PowerConfig) {
        self.config = config;
    }

    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::run(upower::events)
    }

    /// Apply an event: whether what gadgets see changed, and a battery
    /// newly low to notify (or `None`), plus a follow-up (closing the
    /// notification once the battery is fine).
    pub fn apply(&mut self, event: Event) -> (bool, Option<Low>, Task<Event>) {
        match event {
            Event::Bus(conn) => {
                self.bus = Some(conn);
                (false, None, Task::none())
            }
            Event::UPower(snapshot) => {
                let snapshot = snapshot.map(|s| *s);
                if snapshot == self.upower {
                    return (false, None, Task::none());
                }
                self.upower = snapshot;
                let (low, task) = self.check_warning();
                (true, low, task)
            }
            Event::Profiles(profiles) => {
                let changed = profiles != self.profiles;
                self.profiles = profiles;
                (changed, None, Task::none())
            }
            Event::Notified(id) => {
                self.notification = id;
                (false, None, Task::none())
            }
        }
    }

    /// A notification when the battery reaches a warning level while
    /// discharging; the one shown goes once it's back to none.
    fn check_warning(&mut self) -> (Option<Low>, Task<Event>) {
        let (warning, low) = match self.battery() {
            Some(b) if b.state != State::Charging => (
                b.warning,
                Low {
                    warning: b.warning,
                    percentage: b.percentage,
                    time_to_empty: b.time_to_empty,
                },
            ),
            _ => (
                Warning::None,
                Low {
                    warning: Warning::None,
                    percentage: 0.0,
                    time_to_empty: 0,
                },
            ),
        };
        if warning == self.warned {
            return (None, Task::none());
        }
        let rising = warning > self.warned;
        self.warned = warning;
        if warning == Warning::None {
            let id = std::mem::take(&mut self.notification);
            if id == 0 {
                return (None, Task::none());
            }
            let close = Task::future(async move {
                if let Err(e) = crate::services::notifications::client::close_notification(id).await
                {
                    log::debug!("power: closing the notification: {e}");
                }
            })
            .discard();
            return (None, close);
        }
        log::info!("power: battery {warning:?} at {:.0}%", low.percentage);
        let notify = self.config.notify_low && rising;
        (notify.then_some(low), Task::none())
    }

    /// Send the low battery notification, replacing the previous one.
    pub fn notify(&self, summary: String, body: String, critical: bool) -> Task<Event> {
        let replaces = self.notification;
        let icon = self.battery().map(|b| b.icon.clone()).unwrap_or_default();
        Task::future(async move {
            match crate::services::notifications::client::notify(
                replaces, icon, summary, body, critical,
            )
            .await
            {
                Ok(id) => Some(Event::Notified(id)),
                Err(e) => {
                    log::warn!("power: can't notify: {e}");
                    None
                }
            }
        })
        .and_then(Task::done)
    }

    pub fn run(&mut self, command: Command) -> Task<Event> {
        match command {
            Command::SetProfile(profile) => {
                let Some(conn) = self.bus.clone() else {
                    return Task::none();
                };
                log::info!("power: profile {profile}");
                Task::future(async move {
                    if let Err(e) = upower::set_profile(conn, profile).await {
                        log::warn!("power: can't set the profile: {e}");
                    }
                })
                .discard()
            }
        }
    }

    pub fn config(&self) -> &PowerConfig {
        &self.config
    }

    pub fn on_battery(&self) -> bool {
        self.upower.as_ref().is_some_and(|u| u.on_battery)
    }

    pub fn battery(&self) -> Option<&Battery> {
        self.upower.as_ref()?.battery.as_ref()
    }

    pub fn devices(&self) -> &[Device] {
        self.upower.as_ref().map_or(&[], |u| &u.devices)
    }

    pub fn profiles(&self) -> Option<&Profiles> {
        self.profiles.as_ref().filter(|p| !p.available.is_empty())
    }

    /// The icon names UPower gives the battery and the peripherals, for
    /// the daemon to resolve.
    pub fn icon_names(&self) -> impl Iterator<Item = &str> {
        self.battery()
            .map(|b| b.icon.as_str())
            .into_iter()
            .chain(self.devices().iter().map(|d| d.icon.as_str()))
            .filter(|n| !n.is_empty())
    }

    /// For `aria-shell debug power`.
    pub fn describe(&self) -> String {
        let mut parts = vec![format!(
            "upower={} on_battery={}",
            self.upower.is_some(),
            self.on_battery()
        )];
        match self.battery() {
            Some(b) => parts.push(format!(
                "battery={:.0}% {} warning={:?} empty_in={}s full_in={}s rate={:.1}W icon={}",
                b.percentage,
                b.state.name(),
                b.warning,
                b.time_to_empty,
                b.time_to_full,
                b.energy_rate,
                b.icon
            )),
            None => parts.push("battery=none".to_owned()),
        }
        for d in self.devices() {
            parts.push(format!("device={:?} {:.0}%", d.model, d.percentage));
        }
        match self.profiles() {
            Some(p) => parts.push(format!(
                "profile={} available={} degraded={:?}",
                p.active,
                p.available.join(","),
                p.degraded
            )),
            None => parts.push("profile=none".to_owned()),
        }
        parts.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn snapshot(percentage: f64, state: State, warning: Warning) -> Event {
        Event::UPower(Some(Box::new(Snapshot {
            on_battery: state == State::Discharging,
            battery: Some(Battery {
                percentage,
                state,
                time_to_empty: 600,
                time_to_full: 0,
                energy_rate: 8.0,
                warning,
                icon: "battery-level-10-symbolic".to_owned(),
                capacity: Some(90.0),
            }),
            devices: Vec::new(),
        })))
    }

    fn power(text: &str) -> Power {
        Power::new(Config::parse(text).section(None))
    }

    #[test]
    fn upower_values() {
        assert_eq!(State::from_upower(1), State::Charging);
        assert_eq!(
            State::from_upower(6),
            State::Discharging,
            "pending discharge"
        );
        assert_eq!(State::from_upower(5), State::NotCharging);
        assert_eq!(State::from_upower(0), State::Unknown);
        assert_eq!(Warning::from_upower(1), Warning::None);
        assert_eq!(Warning::from_upower(3), Warning::Low);
        assert_eq!(
            Warning::from_upower(5),
            Warning::Critical,
            "action is past critical"
        );
    }

    #[test]
    fn low_battery_once_per_level() {
        let mut p = power("[Power]\n");
        let (changed, low, _) = p.apply(snapshot(30.0, State::Discharging, Warning::None));
        assert!(changed && low.is_none());
        assert!(p.on_battery());
        let (_, low, _) = p.apply(snapshot(9.0, State::Discharging, Warning::Low));
        assert_eq!(low.map(|l| l.warning), Some(Warning::Low));
        let (_, low, _) = p.apply(snapshot(8.0, State::Discharging, Warning::Low));
        assert!(low.is_none(), "the same level again");
        let (_, low, _) = p.apply(snapshot(4.0, State::Discharging, Warning::Critical));
        assert_eq!(low.map(|l| l.warning), Some(Warning::Critical));
        let (_, low, _) = p.apply(snapshot(5.0, State::Charging, Warning::Critical));
        assert!(low.is_none(), "charging: no warning");
        let (_, low, _) = p.apply(snapshot(4.0, State::Discharging, Warning::Critical));
        assert_eq!(
            low.map(|l| l.warning),
            Some(Warning::Critical),
            "unplugged again while critical"
        );
    }

    #[test]
    fn low_battery_can_be_quiet() {
        let mut p = power("[Power]\nnotify_low = no\n");
        let (_, low, _) = p.apply(snapshot(9.0, State::Discharging, Warning::Low));
        assert!(low.is_none());
    }

    #[test]
    fn profiles_need_one_at_least() {
        let mut p = power("[Power]\n");
        let _ = p.apply(Event::Profiles(Some(Profiles {
            active: String::new(),
            available: Vec::new(),
            degraded: String::new(),
        })));
        assert!(p.profiles().is_none());
    }
}
