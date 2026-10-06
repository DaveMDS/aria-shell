//! Brightness: the screens whose brightness the shell can set, a
//! laptop's backlight (sysfs, written through logind) and external
//! monitors over DDC/CI (`ddcutil`), each tied to its output by
//! connector. Shaped like `Audio`: [`Brightness::subscription`] runs
//! the worker (worker.rs), which finds the screens and reads their
//! levels; gadgets and the command socket change them with a
//! [`Command`] the daemon runs with [`Brightness::run`]. The new level
//! is the state at once (the gadget's slider and the OSD follow it),
//! the worker writes it, only the latest one when writes pile up (a
//! held key, a dragged slider: a DDC write takes ~100 ms).
//!
//! A backlight tells every change, whoever made it (the kernel
//! notifies `actual_brightness`); a monitor doesn't: its own buttons go
//! unseen until the next read (the gadget's popup opening reads again).

mod backlight;
mod ddc;
mod worker;

use std::collections::HashMap;

use iced::Subscription;
use iced::futures::channel::mpsc;

use crate::config::{RawSection, Section};

/// A step never goes below this percent: down to dark is `set 0`.
const STEP_FLOOR: u32 = 1;

/// `[Brightness]`: the daemon's keys and the gadget's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrightnessConfig {
    /// The laptop's panel (`/sys/class/backlight`).
    pub backlight: bool,
    /// Percent per wheel click, and per `aria-shell brightness up`.
    pub step: u32,
    /// What the wheel on the gadget changes.
    pub wheel: WheelTarget,
    /// The percent as text after the icon on the bar.
    pub show_percent: bool,
    /// A program (the display settings) run by the popup's button and
    /// a right click; none when empty.
    pub settings_command: String,
}

/// What the gadget's wheel changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelTarget {
    /// Every screen.
    All,
    /// The screen of the bar it's on.
    Output,
}

impl Section for BrightnessConfig {
    const NAME: &'static str = "Brightness";

    fn from_raw(raw: &RawSection) -> Self {
        let wheel = match raw.get("wheel") {
            None | Some("all") => WheelTarget::All,
            Some("output") => WheelTarget::Output,
            Some(other) => {
                log::warn!("[Brightness] invalid wheel {other:?} (all | output), using all");
                WheelTarget::All
            }
        };
        Self {
            backlight: raw.bool_or("backlight", true),
            step: raw.u64_or("step", 5).clamp(1, 100) as u32,
            wheel,
            show_percent: raw.bool_or("show_percent", false),
            settings_command: raw.str_or("settings_command", ""),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Backlight,
    Ddc,
}

/// A level and its maximum, in the device's own units (a backlight's
/// can be 0..120000, a monitor's is usually 0..100).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    pub value: u32,
    pub max: u32,
}

impl Level {
    pub fn percent(self) -> u32 {
        if self.max == 0 {
            return 0;
        }
        ((self.value as u64 * 100 + self.max as u64 / 2) / self.max as u64) as u32
    }

    /// The raw value for `percent`.
    fn raw(self, percent: u32) -> u32 {
        ((percent.min(100) as u64 * self.max as u64 + 50) / 100) as u32
    }

    /// The raw value `delta` percent away: at least one unit away when
    /// the percent rounds back to the same one (a backlight with few
    /// levels), never under [`STEP_FLOOR`] (a step doesn't turn the
    /// screen off), never past the maximum.
    fn stepped(self, delta: i32) -> u32 {
        let floor = self.raw(STEP_FLOOR).max(1).min(self.max);
        let percent = (self.percent() as i32 + delta).clamp(0, 100) as u32;
        let mut raw = self.raw(percent);
        if raw == self.value {
            raw = if delta > 0 {
                self.value.saturating_add(1)
            } else if delta < 0 {
                self.value.saturating_sub(1)
            } else {
                raw
            };
        }
        let raw = raw.min(self.max);
        if delta < 0 {
            raw.max(floor.min(self.value))
        } else {
            raw
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    /// `backlight:<device>` or `ddc:<i2c bus>`.
    pub id: String,
    pub kind: Kind,
    /// The connector it's on (`eDP-1`, `HDMI-A-1`), when known.
    pub output: Option<String>,
    /// The monitor's model (DDC); empty for a backlight.
    pub model: String,
    /// `None` until read, or when it can't be.
    pub level: Option<Level>,
}

impl Display {
    pub fn percent(&self) -> Option<u32> {
        self.level.map(Level::percent)
    }
}

/// Which screens a command is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    All,
    /// The screen on that connector.
    Output(String),
    /// By [`Display::id`].
    Display(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// To a percent.
    Set(Target, u32),
    /// Up or down by `by` percent, `[Brightness] step` when `None`.
    Step {
        target: Target,
        up: bool,
        by: Option<u32>,
    },
    /// Read the monitors again (their own buttons go unseen).
    Refresh,
}

/// The worker's ear.
#[derive(Debug, Clone)]
pub struct Handle(mpsc::UnboundedSender<worker::Request>);

#[derive(Debug, Clone)]
pub enum Event {
    Ready(Handle),
    /// The screens found, all of them (levels as far as known).
    Displays(Vec<Display>),
    /// A level read.
    Level(String, Level),
    /// A raw level written.
    Written(String, u32),
    /// A write didn't go through: the level as it is (`None`:
    /// unreadable too).
    Failed(String, Option<Level>),
}

pub struct Brightness {
    config: BrightnessConfig,
    handle: Option<Handle>,
    displays: Vec<Display>,
    /// The last raw value asked of a screen, until written: the
    /// readings of the writes before it (a backlight tells each one)
    /// are ignored meanwhile.
    asked: HashMap<String, u32>,
}

impl Brightness {
    pub fn new(config: BrightnessConfig) -> Self {
        Self {
            config,
            handle: None,
            displays: Vec::new(),
            asked: HashMap::new(),
        }
    }

    pub fn set_config(&mut self, config: BrightnessConfig) {
        self.config = config;
    }

    /// The worker, started again when what it looks for changes.
    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::run_with(self.config.backlight, |backlight| {
            worker::events(*backlight)
        })
    }

    /// Apply an event: whether what gadgets see changed.
    pub fn apply(&mut self, event: Event) -> bool {
        match event {
            Event::Ready(handle) => {
                self.handle = Some(handle);
                false
            }
            Event::Displays(displays) => {
                // A level already known stays until read again.
                let displays: Vec<Display> = displays
                    .into_iter()
                    .map(|mut d| {
                        if d.level.is_none() {
                            d.level = self.get(&d.id).and_then(|old| old.level);
                        }
                        d
                    })
                    .collect();
                self.asked
                    .retain(|id, _| displays.iter().any(|d| d.id == *id));
                let changed = displays != self.displays;
                self.displays = displays;
                changed
            }
            Event::Level(id, level) => {
                if let Some(&raw) = self.asked.get(&id) {
                    if level.value != raw {
                        return false;
                    }
                    self.asked.remove(&id);
                }
                self.set_level(&id, Some(level))
            }
            Event::Written(id, raw) => {
                if self.asked.get(&id) == Some(&raw) {
                    self.asked.remove(&id);
                }
                false
            }
            Event::Failed(id, level) => {
                self.asked.remove(&id);
                self.set_level(&id, level)
            }
        }
    }

    fn set_level(&mut self, id: &str, level: Option<Level>) -> bool {
        match self.displays.iter_mut().find(|d| d.id == id) {
            Some(d) if d.level != level => {
                d.level = level;
                true
            }
            _ => false,
        }
    }

    /// Carry a command out: the new levels are the state at once, the
    /// worker writes them. Whether what gadgets see changed.
    pub fn run(&mut self, command: Command) -> bool {
        let (target, change): (Target, Box<dyn Fn(Level) -> u32>) = match command {
            Command::Refresh => {
                self.send(worker::Request::Read);
                return false;
            }
            Command::Set(target, percent) => (target, Box::new(move |l: Level| l.raw(percent))),
            Command::Step { target, up, by } => {
                let by = by.unwrap_or(self.config.step) as i32;
                let delta = if up { by } else { -by };
                (target, Box::new(move |l: Level| l.stepped(delta)))
            }
        };
        let mut writes = Vec::new();
        for d in &mut self.displays {
            let wanted = match &target {
                Target::All => true,
                Target::Output(name) => d.output.as_deref() == Some(name.as_str()),
                Target::Display(id) => d.id == *id,
            };
            let Some(level) = d.level.filter(|_| wanted) else {
                continue;
            };
            let raw = change(level);
            if raw == level.value {
                continue;
            }
            d.level = Some(Level {
                value: raw,
                ..level
            });
            writes.push((d.id.clone(), raw));
        }
        if writes.is_empty() {
            if let Target::Output(name) = &target
                && !self
                    .displays
                    .iter()
                    .any(|d| d.output.as_ref() == Some(name))
            {
                log::warn!("brightness: no screen to set on {name}");
            }
            return false;
        }
        for (id, raw) in writes {
            log::debug!("brightness: {id} to {raw}");
            self.asked.insert(id.clone(), raw);
            self.send(worker::Request::Write(id, raw));
        }
        true
    }

    /// Outputs came or went: look for the screens again (a monitor
    /// plugged in).
    pub fn outputs_changed(&self) {
        self.send(worker::Request::Detect);
    }

    fn send(&self, request: worker::Request) {
        match &self.handle {
            Some(Handle(tx)) => {
                let _ = tx.unbounded_send(request);
            }
            None => log::debug!("brightness: not ready, {request:?} dropped"),
        }
    }

    pub fn displays(&self) -> &[Display] {
        &self.displays
    }

    pub fn get(&self, id: &str) -> Option<&Display> {
        self.displays.iter().find(|d| d.id == id)
    }

    /// The screen on that connector.
    pub fn on_output(&self, output: &str) -> Option<&Display> {
        self.displays
            .iter()
            .find(|d| d.output.as_deref() == Some(output))
    }

    /// For `aria-shell debug brightness`.
    pub fn describe(&self) -> String {
        if self.displays.is_empty() {
            return "displays=none".to_owned();
        }
        self.displays
            .iter()
            .map(|d| {
                let level = match d.level {
                    Some(l) => format!("{}% ({}/{})", l.percent(), l.value, l.max),
                    None => "unknown".to_owned(),
                };
                format!(
                    "{} output={} model={:?} {level}",
                    d.id,
                    d.output.as_deref().unwrap_or("-"),
                    d.model
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(value: u32, max: u32) -> Level {
        Level { value, max }
    }

    fn display(id: &str, output: Option<&str>, value: u32, max: u32) -> Display {
        Display {
            id: id.to_owned(),
            kind: Kind::Ddc,
            output: output.map(str::to_owned),
            model: String::new(),
            level: Some(level(value, max)),
        }
    }

    fn brightness(displays: Vec<Display>) -> Brightness {
        let mut b = Brightness::new(crate::config::Config::parse("").section(None));
        b.apply(Event::Displays(displays));
        b
    }

    #[test]
    fn percent_and_raw() {
        assert_eq!(level(75, 100).percent(), 75);
        assert_eq!(level(60000, 120000).percent(), 50);
        assert_eq!(level(1, 7).percent(), 14);
        assert_eq!(level(0, 0).percent(), 0);
        assert_eq!(level(0, 120000).raw(40), 48000);
        assert_eq!(level(0, 100).raw(140), 100);
    }

    #[test]
    fn steps() {
        assert_eq!(level(75, 100).stepped(5), 80);
        assert_eq!(level(98, 100).stepped(5), 100);
        assert_eq!(level(3, 100).stepped(-5), 1, "not off by a step");
        assert_eq!(level(1, 100).stepped(-5), 1);
        assert_eq!(level(0, 100).stepped(-5), 0, "already off");
        // Seven levels: 5% rounds back to the same one, a unit moves.
        assert_eq!(level(3, 7).stepped(5), 4);
        assert_eq!(level(3, 7).stepped(-5), 2);
        assert_eq!(level(1, 7).stepped(-5), 1);
        assert_eq!(level(7, 7).stepped(5), 7);
    }

    #[test]
    fn targets() {
        let mut b = brightness(vec![
            display("ddc:0", Some("HDMI-A-1"), 75, 100),
            display("ddc:1", Some("HDMI-A-2"), 40, 100),
        ]);
        assert!(b.run(Command::Step {
            target: Target::All,
            up: true,
            by: None
        }));
        assert_eq!(b.get("ddc:0").unwrap().percent(), Some(80));
        assert_eq!(b.get("ddc:1").unwrap().percent(), Some(45));
        assert!(b.run(Command::Set(Target::Output("HDMI-A-2".into()), 10)));
        assert_eq!(b.get("ddc:0").unwrap().percent(), Some(80));
        assert_eq!(b.get("ddc:1").unwrap().percent(), Some(10));
        assert!(b.run(Command::Step {
            target: Target::Display("ddc:0".into()),
            up: false,
            by: Some(30)
        }));
        assert_eq!(b.get("ddc:0").unwrap().percent(), Some(50));
        assert!(!b.run(Command::Set(Target::Output("DP-9".into()), 10)));
        assert!(
            !b.run(Command::Set(Target::Output("HDMI-A-1".into()), 50)),
            "already there"
        );
    }

    #[test]
    fn readings_of_older_writes_are_ignored() {
        let mut b = brightness(vec![display("backlight:x", Some("eDP-1"), 50, 100)]);
        b.run(Command::Set(Target::All, 80));
        // The write before it, told by the kernel on the way.
        assert!(!b.apply(Event::Level("backlight:x".into(), level(60, 100))));
        assert_eq!(b.get("backlight:x").unwrap().percent(), Some(80));
        // The one asked: settled.
        assert!(!b.apply(Event::Level("backlight:x".into(), level(80, 100))));
        // Then anyone's change shows.
        assert!(b.apply(Event::Level("backlight:x".into(), level(30, 100))));
        // A monitor tells nothing: the write done settles it.
        b.run(Command::Set(Target::All, 60));
        b.run(Command::Set(Target::All, 70));
        b.apply(Event::Written("backlight:x".into(), 60));
        assert!(!b.apply(Event::Level("backlight:x".into(), level(60, 100))));
        b.apply(Event::Written("backlight:x".into(), 70));
        assert!(b.apply(Event::Level("backlight:x".into(), level(20, 100))));
        // A refused write: the level as it is.
        b.run(Command::Set(Target::All, 90));
        assert!(b.apply(Event::Failed("backlight:x".into(), Some(level(30, 100)))));
        assert_eq!(b.get("backlight:x").unwrap().percent(), Some(30));
    }

    #[test]
    fn detection_keeps_known_levels() {
        let mut b = brightness(vec![display("ddc:0", Some("HDMI-A-1"), 75, 100)]);
        let mut again = display("ddc:0", Some("HDMI-A-1"), 0, 100);
        again.level = None;
        let new = Display {
            level: None,
            ..display("ddc:1", Some("HDMI-A-2"), 0, 100)
        };
        assert!(b.apply(Event::Displays(vec![again, new])));
        assert_eq!(b.get("ddc:0").unwrap().percent(), Some(75));
        assert_eq!(b.get("ddc:1").unwrap().percent(), None);
    }

    #[test]
    fn config() {
        let c: BrightnessConfig = crate::config::Config::parse("").section(None);
        assert!(c.backlight);
        assert_eq!((c.step, c.wheel), (5, WheelTarget::All));
        let c: BrightnessConfig = crate::config::Config::parse(
            "[Brightness]\nbacklight = no\nstep = 10\nwheel = output\n",
        )
        .section(None);
        assert!(!c.backlight);
        assert_eq!((c.step, c.wheel), (10, WheelTarget::Output));
    }
}
