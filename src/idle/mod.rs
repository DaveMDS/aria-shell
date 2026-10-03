//! Idle: what happens when nobody touches the machine. Three stages,
//! each with its own timeout from the last input (on AC and on
//! battery): the session locks, the screens go off (back on at the
//! first input), the machine suspends. The screen is locked before any
//! sleep too (lid, `systemctl suspend`), and on logind's `Lock`
//! (`loginctl lock-session`).
//!
//! Nothing goes idle while held: by the user (the Power gadget's eye,
//! `aria-shell idle inhibit`), by a media player playing (`inhibit_when_playing`),
//! or by an app's Wayland idle inhibitor (a fullscreen video; the
//! compositor's business, honoured by the timers themselves).
//!
//! One [`Idle`] lives in the daemon. The timers and the screens' power
//! are a Wayland connection of its own (wayland.rs), logind the system
//! bus (logind.rs), the power source `Power`'s; locking is the daemon's,
//! told by [`Idle::apply`]. The shell does the locking and the screens,
//! logind does the power: the suspend is a `Suspend` call, not a
//! program.

mod logind;
mod wayland;

use std::time::Duration;

use iced::{Subscription, Task};
use zbus::Connection;

use crate::config::{Config, RawSection, Section};

pub use logind::SleepLock;

/// `[Idle]` section; the timeouts on battery come from `[Idle:battery]`
/// ([`IdleConfig::load`]). The eye on the bar is the Power gadget's.
#[derive(Debug, Clone, PartialEq)]
pub struct IdleConfig {
    pub ac: Timeouts,
    pub battery: Timeouts,
    /// Lock the session before the machine sleeps.
    pub lock_before_sleep: bool,
    /// A media player playing holds the stages.
    pub inhibit_when_playing: bool,
}

impl Section for IdleConfig {
    const NAME: &'static str = "Idle";

    /// The battery's timeouts are the AC ones here.
    fn from_raw(raw: &RawSection) -> Self {
        let ac = Timeouts::from_raw(raw, Self::NAME, Timeouts::default());
        Self {
            ac,
            battery: ac,
            lock_before_sleep: raw.bool_or("lock_before_sleep", true),
            inhibit_when_playing: raw.bool_or("inhibit_when_playing", true),
        }
    }
}

/// The section with the timeouts on battery.
const BATTERY: &str = "Idle:battery";

impl IdleConfig {
    /// `[Idle]`, with `[Idle:battery]` over its timeouts.
    pub fn load(config: &Config) -> Self {
        let mut idle: Self = config.section(None);
        idle.battery = Timeouts::from_raw(&config.raw_section(BATTERY), BATTERY, idle.ac);
        idle
    }
}

/// The stages' timeouts on one power source; `None`: never.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Timeouts {
    pub lock: Option<Duration>,
    pub screen_off: Option<Duration>,
    pub suspend: Option<Duration>,
}

impl Timeouts {
    /// The keys of `section`, `base`'s value where one is empty.
    fn from_raw(raw: &RawSection, section: &str, base: Self) -> Self {
        let read = |key: &str, base: Option<Duration>| {
            let Some(value) = raw.get(key) else {
                return base;
            };
            parse_duration(value).unwrap_or_else(|| {
                log::warn!("[{section}] {key}: invalid duration {value:?} (30s, 5m, 1h or 0), never");
                None
            })
        };
        Self {
            lock: read("lock", base.lock),
            screen_off: read("screen_off", base.screen_off),
            suspend: read("suspend", base.suspend),
        }
    }

    fn get(self, stage: Stage) -> Option<Duration> {
        match stage {
            Stage::Lock => self.lock,
            Stage::ScreenOff => self.screen_off,
            Stage::Suspend => self.suspend,
        }
    }
}

/// `30s`, `5m`, `1h`: `Some(None)` for `0` (never), `None` when it
/// isn't a duration (the unit is required).
pub fn parse_duration(text: &str) -> Option<Option<Duration>> {
    let text = text.trim();
    if text == "0" {
        return Some(None);
    }
    let unit = match text.chars().last()? {
        's' => 1,
        'm' => 60,
        'h' => 60 * 60,
        _ => return None,
    };
    let n: u64 = text[..text.len() - 1].trim().parse().ok()?;
    Some((n > 0).then(|| Duration::from_secs(n * unit)))
}

/// `5m`, `90s`: the shortest exact spelling.
fn format_duration(d: Option<Duration>) -> String {
    match d.map(|d| d.as_secs()) {
        None => "never".to_owned(),
        Some(s) if s % 3600 == 0 => format!("{}h", s / 3600),
        Some(s) if s % 60 == 0 => format!("{}m", s / 60),
        Some(s) => format!("{s}s"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Lock,
    ScreenOff,
    Suspend,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Lock => "lock",
            Self::ScreenOff => "screen_off",
            Self::Suspend => "suspend",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    /// The Wayland connection is up.
    Connected(wayland::Handle),
    /// Nobody touched the machine for the stage's timeout.
    Idled(Stage),
    /// Somebody did, after the stage went idle.
    Resumed(Stage),
    /// The system bus.
    Bus(Connection),
    /// The machine is about to sleep: lock, then release.
    Sleeping(SleepLock),
    /// logind asks the session to lock (`loginctl lock-session`).
    LockRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// The user holds idle, or lets it go.
    SetInhibit(bool),
    ToggleInhibit,
}

/// The lock screen, as far as idle is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locker {
    None,
    /// Asked for, the compositor hasn't confirmed yet.
    Locking,
    Locked,
}

pub struct Idle {
    config: IdleConfig,
    wayland: Option<wayland::Handle>,
    bus: Option<Connection>,
    on_battery: bool,
    /// Held by the user.
    inhibited: bool,
    /// A media player is playing.
    playing: bool,
    /// The timers last asked for (`None`: none asked on this connection).
    armed: Option<Vec<(Stage, Duration)>>,
    screens_off: bool,
    /// The machine waits for the lock to sleep.
    sleep: Option<SleepLock>,
}

impl Idle {
    pub fn new(config: IdleConfig) -> Self {
        Self {
            config,
            wayland: None,
            bus: None,
            on_battery: false,
            inhibited: false,
            playing: false,
            armed: None,
            screens_off: false,
            sleep: None,
        }
    }

    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::batch([
            Subscription::run(wayland::events),
            Subscription::run_with(self.config.lock_before_sleep, |lock| logind::events(*lock)),
        ])
    }

    pub fn set_config(&mut self, config: IdleConfig) {
        self.config = config;
        self.sync();
    }

    /// Apply an event; `true` when the session should lock now.
    pub fn apply(&mut self, event: Event, locker: Locker) -> (bool, Task<Event>) {
        match event {
            Event::Connected(handle) => {
                self.wayland = Some(handle);
                self.armed = None;
                self.sync();
            }
            Event::Bus(conn) => self.bus = Some(conn),
            Event::Idled(stage) => {
                log::info!("idle: {} reached", stage.name());
                match stage {
                    Stage::Lock => return (locker == Locker::None, Task::none()),
                    Stage::ScreenOff => self.screens(false),
                    Stage::Suspend => return (false, self.suspend()),
                }
            }
            Event::Resumed(stage) => {
                log::debug!("idle: {} resumed", stage.name());
                if stage == Stage::ScreenOff {
                    self.screens(true);
                }
            }
            Event::Sleeping(sleep) => {
                if !self.config.lock_before_sleep {
                    sleep.release();
                    return (false, Task::none());
                }
                match locker {
                    Locker::Locked => sleep.release(),
                    Locker::Locking => self.sleep = Some(sleep),
                    Locker::None => {
                        self.sleep = Some(sleep);
                        return (true, Task::none());
                    }
                }
            }
            Event::LockRequested => {
                log::info!("idle: logind asks to lock the session");
                return (locker == Locker::None, Task::none());
            }
        }
        (false, Task::none())
    }

    /// The lock screen is up, or won't be: a waiting sleep may go on.
    pub fn lock_settled(&mut self) {
        if let Some(sleep) = self.sleep.take() {
            sleep.release();
        }
    }

    pub fn run(&mut self, command: Command) {
        self.inhibited = match command {
            Command::SetInhibit(on) => on,
            Command::ToggleInhibit => !self.inhibited,
        };
        log::info!(
            "idle: {}",
            if self.inhibited { "held by the user" } else { "let go by the user" }
        );
        self.sync();
    }

    /// Whether the machine runs on battery (from `Power`).
    pub fn set_on_battery(&mut self, on: bool) {
        if on != self.on_battery {
            log::info!("idle: on {}", if on { "battery" } else { "AC" });
            self.on_battery = on;
            self.sync();
        }
    }

    /// Whether a media player is playing.
    pub fn set_playing(&mut self, playing: bool) {
        if playing != self.playing {
            self.playing = playing;
            if self.config.inhibit_when_playing {
                log::debug!("idle: a player is {}", if playing { "playing" } else { "not playing" });
            }
            self.sync();
        }
    }

    /// Held by the user.
    pub fn inhibited(&self) -> bool {
        self.inhibited
    }

    /// Held by a player playing.
    pub fn held_by_player(&self) -> bool {
        self.config.inhibit_when_playing && self.playing
    }

    /// The timers for the current state.
    fn timers(&self) -> Vec<(Stage, Duration)> {
        if self.inhibited || self.held_by_player() {
            return Vec::new();
        }
        let timeouts = if self.on_battery {
            self.config.battery
        } else {
            self.config.ac
        };
        [Stage::Lock, Stage::ScreenOff, Stage::Suspend]
            .into_iter()
            .filter_map(|stage| Some((stage, timeouts.get(stage)?)))
        .collect()
    }

    /// Ask for the timers the state wants, when they changed; a hold
    /// turns the screens back on.
    fn sync(&mut self) {
        if self.wayland.is_none() {
            return;
        }
        let timers = self.timers();
        if timers.is_empty() && self.screens_off {
            self.screens(true);
        }
        if self.armed.as_ref() == Some(&timers) {
            return;
        }
        log::debug!("idle: timers {timers:?}");
        if let Some(handle) = &self.wayland {
            handle.send(wayland::Request::Timers(timers.clone()));
        }
        self.armed = Some(timers);
    }

    fn screens(&mut self, on: bool) {
        if self.screens_off != on {
            return;
        }
        log::info!("idle: screens {}", if on { "on" } else { "off" });
        self.screens_off = !on;
        if let Some(handle) = &self.wayland {
            handle.send(wayland::Request::Screens(on));
        }
    }

    fn suspend(&self) -> Task<Event> {
        let Some(conn) = self.bus.clone() else {
            log::warn!("idle: no system bus, can't suspend");
            return Task::none();
        };
        Task::future(async move {
            if let Err(e) = logind::suspend(conn).await {
                log::warn!("idle: can't suspend: {e}");
            }
        })
        .discard()
    }

    /// For `aria-shell debug idle`.
    pub fn describe(&self) -> String {
        let t = |stage: Stage| {
            format!(
                "{}={}/{}",
                stage.name(),
                format_duration(self.config.ac.get(stage)),
                format_duration(self.config.battery.get(stage))
            )
        };
        let armed = match &self.armed {
            None => "-".to_owned(),
            Some(timers) if timers.is_empty() => "none".to_owned(),
            Some(timers) => timers
                .iter()
                .map(|(stage, d)| format!("{}={}", stage.name(), format_duration(Some(*d))))
                .collect::<Vec<_>>()
                .join(","),
        };
        format!(
            "power={} inhibited={} playing={} screens={} armed={armed} {} {} {}",
            if self.on_battery { "battery" } else { "ac" },
            self.inhibited,
            self.playing,
            if self.screens_off { "off" } else { "on" },
            t(Stage::Lock),
            t(Stage::ScreenOff),
            t(Stage::Suspend),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn durations() {
        let s = |n: u64| Some(Some(Duration::from_secs(n)));
        assert_eq!(parse_duration("30s"), s(30));
        assert_eq!(parse_duration("5m"), s(300));
        assert_eq!(parse_duration(" 2h "), s(7200));
        assert_eq!(parse_duration("0"), Some(None), "never");
        assert_eq!(parse_duration("0m"), Some(None), "never");
        assert_eq!(parse_duration("5"), None, "the unit is required");
        assert_eq!(parse_duration("5d"), None);
        assert_eq!(parse_duration("m"), None);
        assert_eq!(format_duration(Some(Duration::from_secs(300))), "5m");
        assert_eq!(format_duration(Some(Duration::from_secs(90))), "90s");
        assert_eq!(format_duration(None), "never");
    }

    #[test]
    fn battery_falls_back_to_ac() {
        let config = Config::parse(
            "[Idle]\nlock = 5m\nscreen_off = 10m\nsuspend =\n[Idle:battery]\nlock = 2m\nscreen_off =\nsuspend = 15m\n",
        );
        let idle = IdleConfig::load(&config);
        let m = |n: u64| Some(Duration::from_secs(n * 60));
        assert_eq!(idle.ac, Timeouts { lock: m(5), screen_off: m(10), suspend: None });
        assert_eq!(
            idle.battery,
            Timeouts { lock: m(2), screen_off: m(10), suspend: m(15) },
            "an empty key is as on AC"
        );
        let config = Config::parse("[Idle]\nlock = 5m\n[Idle:battery]\nlock = 0\n");
        assert_eq!(IdleConfig::load(&config).battery.lock, None, "0 is never");
    }

    #[test]
    fn holds_stop_the_timers() {
        let config = Config::parse("[Idle]\nlock = 5m\nscreen_off = 10m\n");
        let mut idle = Idle::new(IdleConfig::load(&config));
        assert_eq!(idle.timers().len(), 2);
        idle.set_playing(true);
        assert!(idle.timers().is_empty(), "a player playing holds");
        idle.set_playing(false);
        idle.run(Command::ToggleInhibit);
        assert!(idle.timers().is_empty(), "the user holds");
        idle.run(Command::SetInhibit(false));
        assert_eq!(idle.timers().len(), 2);

        let config = Config::parse("[Idle]\nlock = 5m\ninhibit_when_playing = no\n");
        let mut idle = Idle::new(IdleConfig::load(&config));
        idle.set_playing(true);
        assert_eq!(idle.timers().len(), 1, "players don't count");
    }
}
