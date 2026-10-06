//! The OSD: a short-lived bar on every output when something it
//! watches changes (the default output's volume, the microphone's,
//! their mute, a screen's brightness, the microphone in use, Wi‑Fi on
//! or off, the network and
//! a VPN going up or down, the charger, the power profile, the hold on
//! idle), whatever changed it: a keybind running `wpctl`, a gadget,
//! another app. It only shows, it never changes anything; `aria-shell
//! osd show` shows one from a script.
//!
//! The daemon owns one [`Osd`]: after every change of the shared
//! state it calls [`Osd::observe`], which compares what it watches
//! with the last reading ([`change`], a pure function) and says what
//! to show. [`Osd::show`] asks for one overlay surface per output
//! ([`Surfaces`], which the daemon opens), the daemon draws
//! [`Osd::view`] on each, and they all close when the last change is
//! `duration` old (a serial-checked timer, so a held volume key keeps
//! the bar up and updates it in place). A brightness change shows on
//! the screens it happened on, each with its own level.
//!
//! ```text
//! osd.<volume|microphone|brightness|recording|wifi|network|vpn|charger|profile|idle|custom>[.muted][output="<connector>"]
//! ├─ icon
//! ├─ meter > fill         the level, with a value
//! ├─ value                the percent, with a value
//! ╰─ label                the text, with one
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use iced::widget::{Space, row};
use iced::window::Id;
use iced::{Alignment, Element, Length, Padding, Point, Rectangle, Size, Task};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};

use crate::components::Surfaces;
use crate::config::{RawSection, Section};
use crate::locale::Locale;
use crate::services::audio::{Audio, Kind as Channel};
use crate::services::brightness::Brightness;
use crate::services::icons::Icons;
use crate::services::idle::Idle;
use crate::services::network::{DeviceKind, Network, Summary};
use crate::services::power::{self, Power};
use crate::ui::graph;
use crate::ui::theme::{self, Node, Theme};

/// Surface size when the theme doesn't set `width` / `height` on `osd`.
const DEFAULT_SIZE: (f32, f32) = (320.0, 56.0);
/// Icon size when the theme doesn't set `height` on `osd icon`.
const DEFAULT_ICON: f32 = 24.0;
/// An app started listening to a microphone; none listens any more.
const ICON_RECORDING: &str = "audio-input-microphone-symbolic";
const ICON_NOT_RECORDING: &str = "microphone-disabled-symbolic";
const ICON_WIFI_ON: &str = "network-wireless-symbolic";
const ICON_WIFI_OFF: &str = "network-wireless-disabled-symbolic";
const ICON_VPN_UP: &str = "network-vpn-symbolic";
const ICON_VPN_DOWN: &str = "network-vpn-disconnected-symbolic";
const ICON_BRIGHTNESS: &str = "display-brightness-symbolic";

/// Every watch, in the order the default `show` lists them.
const ALL_WATCHES: &[&str] = &[
    "volume",
    "microphone",
    "brightness",
    "recording",
    "wifi",
    "network",
    "vpn",
    "charger",
    "profile",
    "idle",
];

/// `[osd]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsdConfig {
    /// What to watch; `none`: only `aria-shell osd show`.
    pub show: Vec<Watch>,
    /// How long it stays after the last change.
    pub duration: Duration,
    pub position: Position,
    /// Distance in px from the edge it's anchored to (none when
    /// centred).
    pub margin: i32,
}

impl Section for OsdConfig {
    const NAME: &'static str = "osd";

    fn from_raw(raw: &RawSection) -> Self {
        let show = raw
            .list_or("show", ALL_WATCHES)
            .iter()
            .filter(|name| *name != "none")
            .filter_map(|name| {
                let watch = Watch::parse(name);
                if watch.is_none() {
                    log::warn!("[osd] show: unknown {name:?} ({})", ALL_WATCHES.join(" | "));
                }
                watch
            })
            .collect();
        let position = match raw.get("position") {
            None => Position::Bottom,
            Some(p) => Position::parse(p).unwrap_or_else(|| {
                log::warn!("[osd] invalid position {p:?} (top | center | bottom), using bottom");
                Position::Bottom
            }),
        };
        Self {
            show,
            duration: Duration::from_secs(raw.u64_or("duration", 2).max(1)),
            position,
            margin: raw.u64_or("margin", 100).min(10_000) as i32,
        }
    }
}

/// Something the OSD watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Watch {
    /// The default output: its level and mute.
    Volume,
    /// The default input: its level and mute.
    Microphone,
    /// A screen's brightness.
    Brightness,
    /// The microphone in use by some app, or free again.
    Recording,
    /// Wi‑Fi on / off.
    Wifi,
    /// Connected / disconnected.
    Network,
    /// A VPN up / down.
    Vpn,
    /// The charger plugged in / out (with a battery).
    Charger,
    /// Another power profile.
    Profile,
    /// Idle held by the user / let go.
    Idle,
}

impl Watch {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "volume" => Self::Volume,
            "microphone" => Self::Microphone,
            "brightness" => Self::Brightness,
            "recording" => Self::Recording,
            "wifi" => Self::Wifi,
            "network" => Self::Network,
            "vpn" => Self::Vpn,
            "charger" => Self::Charger,
            "profile" => Self::Profile,
            "idle" => Self::Idle,
            _ => return None,
        })
    }
}

/// The edge it shows at, horizontally centred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Top,
    Center,
    Bottom,
}

impl Position {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "top" => Self::Top,
            "center" => Self::Center,
            "bottom" => Self::Bottom,
            _ => return None,
        })
    }
}

/// What a [`Content`] is about: the root's class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Volume,
    Microphone,
    Brightness,
    Recording,
    Wifi,
    Network,
    Vpn,
    Charger,
    Profile,
    Idle,
    /// From `aria-shell osd show`.
    Custom,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Volume => "volume",
            Self::Microphone => "microphone",
            Self::Brightness => "brightness",
            Self::Recording => "recording",
            Self::Wifi => "wifi",
            Self::Network => "network",
            Self::Vpn => "vpn",
            Self::Charger => "charger",
            Self::Profile => "profile",
            Self::Idle => "idle",
            Self::Custom => "custom",
        }
    }
}

/// What the OSD shows: an icon, a level with its percent, a text; any
/// of them may be missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub kind: Kind,
    /// An icon name from the icon theme.
    pub icon: Option<String>,
    /// A percent; above 100 fills the bar and shows as is.
    pub value: Option<u32>,
    pub text: Option<String>,
    pub muted: bool,
    /// Only on these outputs (connectors), each with its own percent in
    /// place of `value`; `None`: on every output.
    pub outputs: Option<BTreeMap<String, u32>>,
}

impl Content {
    /// Whether it shows on the output named so, among those present:
    /// on all of them when it names none of these.
    pub fn shown_on(&self, present: &[&str]) -> impl Fn(&str) -> bool + use<> {
        let only: Option<BTreeSet<String>> = self
            .outputs
            .as_ref()
            .map(|m| {
                m.keys()
                    .filter(|o| present.contains(&o.as_str()))
                    .cloned()
                    .collect()
            })
            .filter(|s: &BTreeSet<String>| !s.is_empty());
        move |output| only.as_ref().is_none_or(|s| s.contains(output))
    }

    /// The percent on `output`.
    fn value_on(&self, output: &str) -> Option<u32> {
        self.outputs
            .as_ref()
            .and_then(|m| m.get(output).copied())
            .or(self.value)
    }
}

/// The default output or input as last read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Level {
    /// The server's name of the device: another one is a change too
    /// (headphones plugged in).
    device: String,
    percent: u32,
    muted: bool,
    icon: &'static str,
}

impl Level {
    fn read(audio: &Audio, kind: Channel) -> Option<Self> {
        audio.default_of(kind).map(|c| Self {
            device: c.name.clone(),
            percent: (c.volume * 100.0).round() as u32,
            muted: c.muted,
            icon: c.icon_name(),
        })
    }

    /// What to show when it went from `old` to `new`.
    fn change(old: &Option<Self>, new: &Option<Self>, kind: Kind) -> Option<Content> {
        let (Some(o), Some(n)) = (old, new) else {
            return None;
        };
        if o.device == n.device && o.percent == n.percent && o.muted == n.muted {
            return None;
        }
        Some(Content {
            kind,
            icon: Some(n.icon.to_owned()),
            value: Some(n.percent),
            text: None,
            muted: n.muted,
            outputs: None,
        })
    }
}

/// The network as last read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Link {
    connected: bool,
    /// The SSID or the profile's name.
    label: String,
    /// Wired or Wi‑Fi: the connection's, or the one that went down.
    kind: Option<DeviceKind>,
    icon: &'static str,
}

/// The charger as last read: only a change of `plugged` shows, the
/// rest is what it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Charger {
    plugged: bool,
    /// The battery's charge.
    percent: u32,
    /// UPower's battery icon.
    icon: String,
}

/// A screen's brightness as last read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Screen {
    /// Its connector, when known.
    output: Option<String>,
    percent: u32,
}

/// The user's hold on idle as last read; the icon is the Power
/// gadget's eye for it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hold {
    held: bool,
    icon: String,
}

/// What the OSD watches, as last read; `None` until known (at start,
/// and again while a source is away), so the first reading shows
/// nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watched {
    output: Option<Level>,
    input: Option<Level>,
    /// The screens whose level is known, by id.
    brightness: BTreeMap<String, Screen>,
    /// Some app listens to a microphone.
    recording: Option<bool>,
    /// Wi‑Fi enabled (with a Wi‑Fi device).
    wifi: Option<bool>,
    network: Option<Link>,
    /// The VPN up, by name.
    vpn: Option<Option<String>>,
    charger: Option<Charger>,
    /// The active power profile.
    profile: Option<String>,
    idle: Option<Hold>,
}

impl Watched {
    /// Read the shared state. The network passing through "connecting"
    /// keeps the last reading: switching networks shows the new one,
    /// not a disconnection first.
    fn read(
        audio: &Audio,
        network: &Network,
        power: &Power,
        idle: &Idle,
        brightness: &Brightness,
        previous: &Watched,
    ) -> Self {
        let output = Level::read(audio, Channel::Output);
        let input = Level::read(audio, Channel::Input);
        let brightness = brightness
            .displays()
            .iter()
            .filter_map(|d| {
                Some((
                    d.id.clone(),
                    Screen {
                        output: d.output.clone(),
                        percent: d.percent()?,
                    },
                ))
            })
            .collect();
        let recording = audio.recordings().map(|mut apps| apps.next().is_some());
        let nm = network.running() && !network.devices().is_empty();
        let wifi = (nm && network.devices_of(DeviceKind::Wifi).next().is_some())
            .then(|| network.wireless_enabled());
        let vpn = nm.then(|| network.active_vpn().map(str::to_owned));
        let charger = power.battery().map(|b| Charger {
            plugged: !power.on_battery(),
            percent: b.percentage.round() as u32,
            icon: b.icon_name().to_owned(),
        });
        let profile = power.profiles().map(|p| p.active.clone());
        let held = idle.inhibited();
        let config = power.config();
        let idle = Some(Hold {
            held,
            icon: if held {
                config.inhibit_icon.clone()
            } else {
                config.idle_icon.clone()
            },
        });
        let network = if !nm {
            None
        } else {
            let s = network.summary();
            let wireless = network.wireless_enabled();
            if s.connecting && !s.connected {
                previous.network.clone()
            } else if s.connected {
                Some(Link {
                    connected: true,
                    label: s.label.clone(),
                    kind: s.kind,
                    icon: s.icon_name(true, wireless),
                })
            } else {
                // The kind that went down, not the device NetworkManager
                // would try next (Wi‑Fi, when there is one).
                let kind = previous.network.as_ref().and_then(|p| p.kind).or(s.kind);
                let down = Summary {
                    kind,
                    ..Summary::default()
                };
                Some(Link {
                    connected: false,
                    label: String::new(),
                    kind,
                    icon: down.icon_name(true, wireless),
                })
            }
        };
        Self {
            output,
            input,
            brightness,
            recording,
            wifi,
            network,
            vpn,
            charger,
            profile,
            idle,
        }
    }
}

/// What changed from `old` to `new` among what `show` watches, as
/// something to show; when several changed at once, the first in the
/// order of [`Watch`] (Wi‑Fi turned off before the disconnection it
/// brings). A value becoming known isn't a change.
pub fn change(old: &Watched, new: &Watched, show: &[Watch], locale: &Locale) -> Option<Content> {
    if show.contains(&Watch::Volume)
        && let Some(content) = Level::change(&old.output, &new.output, Kind::Volume)
    {
        return Some(content);
    }
    if show.contains(&Watch::Microphone)
        && let Some(content) = Level::change(&old.input, &new.input, Kind::Microphone)
    {
        return Some(content);
    }
    if show.contains(&Watch::Brightness)
        && let Some(content) = brightness_change(&old.brightness, &new.brightness)
    {
        return Some(content);
    }
    if show.contains(&Watch::Recording)
        && let (Some(was), Some(now)) = (old.recording, new.recording)
        && was != now
    {
        let (icon, key) = if now {
            (ICON_RECORDING, "osd.recording.started")
        } else {
            (ICON_NOT_RECORDING, "osd.recording.stopped")
        };
        return Some(Content {
            kind: Kind::Recording,
            icon: Some(icon.to_owned()),
            value: None,
            text: Some(locale.tr(key).to_owned()),
            muted: false,
            outputs: None,
        });
    }
    if show.contains(&Watch::Wifi)
        && let (Some(was), Some(now)) = (old.wifi, new.wifi)
        && was != now
    {
        let (icon, key) = if now {
            (ICON_WIFI_ON, "osd.wifi.on")
        } else {
            (ICON_WIFI_OFF, "osd.wifi.off")
        };
        return Some(notice(Kind::Wifi, icon, locale.tr(key)));
    }
    if show.contains(&Watch::Network)
        && let (Some(o), Some(n)) = (&old.network, &new.network)
        && (o.connected != n.connected || o.label != n.label)
    {
        let text = if n.connected {
            locale.fmt("osd.network.connected", &[("name", &n.label)])
        } else {
            locale.tr("osd.network.disconnected").to_owned()
        };
        return Some(Content {
            kind: Kind::Network,
            icon: Some(n.icon.to_owned()),
            value: None,
            text: Some(text),
            muted: false,
            outputs: None,
        });
    }
    if show.contains(&Watch::Vpn)
        && let (Some(was), Some(now)) = (&old.vpn, &new.vpn)
        && was != now
    {
        return Some(match now {
            Some(name) => notice(
                Kind::Vpn,
                ICON_VPN_UP,
                &locale.fmt("osd.vpn.connected", &[("name", name)]),
            ),
            None => notice(Kind::Vpn, ICON_VPN_DOWN, locale.tr("osd.vpn.disconnected")),
        });
    }
    if show.contains(&Watch::Charger)
        && let (Some(o), Some(n)) = (&old.charger, &new.charger)
        && o.plugged != n.plugged
    {
        let key = if n.plugged {
            "osd.charger.plugged"
        } else {
            "osd.charger.unplugged"
        };
        let text = locale.fmt(key, &[("n", &n.percent)]);
        return Some(notice(Kind::Charger, &n.icon, &text));
    }
    if show.contains(&Watch::Profile)
        && let (Some(was), Some(now)) = (&old.profile, &new.profile)
        && was != now
    {
        let name = power::profile_label(locale, now);
        return Some(notice(
            Kind::Profile,
            &power::profile_icon(now),
            &locale.fmt("osd.profile", &[("name", &name)]),
        ));
    }
    if show.contains(&Watch::Idle)
        && let (Some(o), Some(n)) = (&old.idle, &new.idle)
        && o.held != n.held
    {
        let key = if n.held {
            "osd.idle.on"
        } else {
            "osd.idle.off"
        };
        return Some(notice(Kind::Idle, &n.icon, locale.tr(key)));
    }
    None
}

/// The screens whose level changed, each on its output with its
/// percent; everywhere when one of them has no known output. A screen
/// coming or going isn't a change.
fn brightness_change(
    old: &BTreeMap<String, Screen>,
    new: &BTreeMap<String, Screen>,
) -> Option<Content> {
    let changed: Vec<&Screen> = new
        .iter()
        .filter(|(id, n)| old.get(*id).is_some_and(|o| o.percent != n.percent))
        .map(|(_, n)| n)
        .collect();
    let first = changed.first()?;
    let outputs = changed
        .iter()
        .map(|s| Some((s.output.clone()?, s.percent)))
        .collect::<Option<BTreeMap<String, u32>>>();
    Some(Content {
        kind: Kind::Brightness,
        icon: Some(ICON_BRIGHTNESS.to_owned()),
        value: Some(first.percent),
        text: None,
        muted: false,
        outputs,
    })
}

/// An icon and a text.
fn notice(kind: Kind, icon: &str, text: &str) -> Content {
    Content {
        kind,
        icon: Some(icon.to_owned()),
        value: None,
        text: Some(text.to_owned()),
        muted: false,
        outputs: None,
    }
}

pub struct Osd {
    config: OsdConfig,
    watched: Watched,
    /// What's on screen, while it is.
    content: Option<Content>,
    /// Bumped by every [`Osd::show`]: only the timer of the last one
    /// closes the surfaces.
    serial: u64,
    /// One surface per output while shown.
    windows: BTreeMap<OutputId, Id>,
}

impl Osd {
    pub fn new(config: OsdConfig) -> Self {
        Self {
            config,
            watched: Watched::default(),
            content: None,
            serial: 0,
            windows: BTreeMap::new(),
        }
    }

    /// A new config; the last reading stays, so a reload isn't a change.
    pub fn set_config(&mut self, config: OsdConfig) {
        self.config = config;
    }

    /// Read the shared state again: what to show, if what's watched
    /// changed.
    pub fn observe(
        &mut self,
        audio: &Audio,
        network: &Network,
        power: &Power,
        idle: &Idle,
        brightness: &Brightness,
        locale: &Locale,
    ) -> Option<Content> {
        let new = Watched::read(audio, network, power, idle, brightness, &self.watched);
        if new == self.watched {
            return None;
        }
        let shown = change(&self.watched, &new, &self.config.show, locale);
        self.watched = new;
        shown
    }

    /// Show `content` (in place of what's shown) on every output it's
    /// for (every one, unless it names some): a surface for those
    /// without one, a new frame for the others; the surfaces on other
    /// outputs go. With the timer that closes them, yielding the serial
    /// [`Osd::expired`] wants back.
    pub fn show(
        &mut self,
        content: Content,
        outputs: &BTreeMap<OutputId, OutputInfo>,
        theme: &Theme,
    ) -> (Surfaces, Task<u64>) {
        let names: Vec<&str> = outputs.values().filter_map(|o| o.name.as_deref()).collect();
        let shown_on = content.shown_on(&names);
        self.content = Some(content);
        self.serial += 1;
        let mut surfaces = Surfaces::default();
        self.windows.retain(|output, id| {
            let name = outputs.get(output).and_then(|o| o.name.as_deref());
            let keep = shown_on(name.unwrap_or("?"));
            if !keep {
                surfaces.close.push(*id);
            }
            keep
        });
        let size = self.size(theme);
        // The box `margin` from the edge, its shadow's room nearer.
        let room = self.room(theme);
        let margin = match self.config.position {
            Position::Top => (self.config.margin - room.top as i32, 0, 0, 0),
            Position::Center => (0, 0, 0, 0),
            Position::Bottom => (0, 0, self.config.margin - room.bottom as i32, 0),
        };
        for (&output, info) in outputs {
            if !shown_on(info.name.as_deref().unwrap_or_default()) {
                continue;
            }
            if let Some(&id) = self.windows.get(&output) {
                surfaces.redraw.push(id);
                continue;
            }
            let id = Id::unique();
            self.windows.insert(output, id);
            surfaces.open.push((
                id,
                NewLayerShellSettings {
                    anchor: anchor(self.config.position),
                    size: LayerSize::px(size.0, size.1),
                    layer: Layer::Overlay,
                    exclusive_zone: None,
                    margin: Some(margin),
                    keyboard_interactivity: KeyboardInteractivity::None,
                    output_option: OutputOption::GlobalName(info.id),
                    // Shown over whatever is under the pointer: it must
                    // not take its clicks.
                    events_transparent: true,
                    namespace: Some("aria-osd".to_owned()),
                    ..Default::default()
                },
            ));
        }
        let (serial, duration) = (self.serial, self.config.duration);
        let expiry = Task::future(async move {
            tokio::time::sleep(duration).await;
            serial
        });
        (surfaces, expiry)
    }

    /// The timer of show `serial` ran out: if it was the last show, the
    /// surfaces go.
    pub fn expired(&mut self, serial: u64) -> Surfaces {
        if serial != self.serial {
            return Surfaces::default();
        }
        self.close()
    }

    /// Take it off every output now.
    pub fn close(&mut self) -> Surfaces {
        self.content = None;
        Surfaces {
            close: std::mem::take(&mut self.windows).into_values().collect(),
            ..Surfaces::default()
        }
    }

    /// Output `output` went away: its surface goes.
    pub fn output_removed(&mut self, output: OutputId) -> Surfaces {
        Surfaces {
            close: self.windows.remove(&output).into_iter().collect(),
            ..Surfaces::default()
        }
    }

    /// Surface `window` was closed: whether it was one of ours.
    pub fn closed(&mut self, window: Id) -> bool {
        match self.output_of(window) {
            Some(output) => {
                self.windows.remove(&output);
                true
            }
            None => false,
        }
    }

    /// The theme changed: the surfaces take its size.
    pub fn relayout(&self, theme: &Theme) -> Surfaces {
        let (w, h) = self.size(theme);
        let anchor = anchor(self.config.position);
        Surfaces {
            resize: self
                .windows
                .values()
                .map(|&id| (id, anchor, LayerSize::px(w, h)))
                .collect(),
            ..Surfaces::default()
        }
    }

    /// The open surfaces, with their output.
    pub fn windows(&self) -> impl Iterator<Item = (OutputId, Id)> + '_ {
        self.windows.iter().map(|(&o, &id)| (o, id))
    }

    pub fn output_of(&self, window: Id) -> Option<OutputId> {
        self.windows
            .iter()
            .find(|(_, w)| **w == window)
            .map(|(o, _)| *o)
    }

    /// Where the surface is on an output with rectangle `output`, as
    /// asked: centred, from its edge past the bars' exclusive zones
    /// there (`bars`: the top one's height, the bottom one's), by the
    /// margin.
    pub fn rect(&self, theme: &Theme, output: Rectangle, bars: (f32, f32)) -> Rectangle {
        let (w, h) = self.size(theme);
        let (w, h) = (w as f32, h as f32);
        let room = self.room(theme);
        let margin = self.config.margin as f32;
        let x = output.x + (output.width - w) / 2.0;
        let y = match self.config.position {
            Position::Top => output.y + bars.0 + margin - room.top,
            Position::Center => output.y + (output.height - h) / 2.0,
            Position::Bottom => output.y + output.height - bars.1 - margin + room.bottom - h,
        };
        Rectangle::new(Point::new(x, y), Size::new(w, h))
    }

    pub fn icon_names(&self) -> impl Iterator<Item = &str> {
        self.content.iter().filter_map(|c| c.icon.as_deref())
    }

    /// The surface size: the theme's `osd { width; height }`, plus the
    /// room for its shadow ([`Osd::room`]).
    pub fn size(&self, theme: &Theme) -> (u32, u32) {
        let root = Node::root("osd");
        let s = theme.resolve(&root);
        let px = |l: Option<theme::Length>, default: f32| match l {
            Some(theme::Length::Px(px)) => px.max(1.0),
            _ => default,
        };
        let room = theme.shadow_room(&root);
        (
            (px(s.width, DEFAULT_SIZE.0) + room.left + room.right) as u32,
            (px(s.height, DEFAULT_SIZE.1) + room.top + room.bottom) as u32,
        )
    }

    /// The room around the box for its shadow, inside the surface.
    pub fn room(&self, theme: &Theme) -> Padding {
        theme.shadow_room(&Node::root("osd"))
    }

    /// The content on the surface of output `output`.
    pub fn view<'a, M: 'a>(
        &'a self,
        theme: &'a Theme,
        icons: &Icons,
        output: &str,
    ) -> Element<'a, M> {
        let Some(content) = &self.content else {
            return Space::new().into();
        };
        let node = Node::root("osd")
            .class(content.kind.name())
            .class_if("muted", content.muted)
            .attr("output", output.to_owned());
        let style = theme.resolve(&node);
        let mut parts: Vec<Element<'a, M>> = Vec::new();
        if let Some(name) = &content.icon {
            let icon_node = node.child("icon");
            let s = theme.resolve(&icon_node);
            let size = match s.height.or(s.width) {
                Some(theme::Length::Px(px)) => px,
                _ => DEFAULT_ICON,
            };
            let icon = match icons.get_name(name, None) {
                Some(icon) => icon.view(size, s.color),
                None => Space::new().width(size).height(size).into(),
            };
            parts.push(theme.container(&icon_node, icon).into());
        }
        let value = content.value_on(output);
        if let Some(value) = value {
            parts.push(graph::meter(
                theme,
                &node.child("meter"),
                value as f32 / 100.0,
            ));
        }
        if let Some(text) = &content.text {
            let label = node.child("label");
            let mut label = theme.container(&label, theme.text(&label, text.clone()));
            if value.is_none() {
                label = label.width(Length::Fill);
            }
            parts.push(label.into());
        }
        if let Some(value) = value {
            let value_node = node.child("value");
            parts.push(
                theme
                    .container(&value_node, theme.text(&value_node, format!("{value}%")))
                    .into(),
            );
        }
        let pill = theme
            .container(
                &node,
                row(parts)
                    .spacing(style.gap)
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill);
        theme.surface(&Node::root("osd"), pill).into()
    }
}

fn anchor(position: Position) -> Anchor {
    match position {
        Position::Top => Anchor::Top,
        Position::Center => Anchor::empty(),
        Position::Bottom => Anchor::Bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[Watch] = &[
        Watch::Volume,
        Watch::Microphone,
        Watch::Brightness,
        Watch::Recording,
        Watch::Wifi,
        Watch::Network,
        Watch::Vpn,
        Watch::Charger,
        Watch::Profile,
        Watch::Idle,
    ];

    fn level(device: &str, percent: u32, muted: bool) -> Option<Level> {
        Some(Level {
            device: device.to_owned(),
            percent,
            muted,
            icon: "audio-volume-medium-symbolic",
        })
    }

    fn link(connected: bool, label: &str) -> Option<Link> {
        Some(Link {
            connected,
            label: label.to_owned(),
            kind: Some(DeviceKind::Wired),
            icon: "network-wired-symbolic",
        })
    }

    fn watched(output: Option<Level>, input: Option<Level>, network: Option<Link>) -> Watched {
        Watched {
            output,
            input,
            network,
            ..Watched::default()
        }
    }

    fn recording(in_use: Option<bool>) -> Watched {
        Watched {
            recording: in_use,
            ..Watched::default()
        }
    }

    #[test]
    fn first_reading_is_silent() {
        let en = Locale::new("en");
        let known = watched(
            level("a", 50, false),
            level("mic", 80, false),
            link(true, "Home"),
        );
        assert_eq!(change(&Watched::default(), &known, ALL, &en), None);
        // A source coming back (pulse restarted) is a first reading too.
        let away = watched(None, None, link(true, "Home"));
        assert_eq!(change(&away, &known, ALL, &en), None);
        assert_eq!(change(&known, &known, ALL, &en), None);
    }

    #[test]
    fn volume_mute_and_device() {
        let en = Locale::new("en");
        let old = watched(level("a", 50, false), level("mic", 80, false), None);
        let louder = watched(level("a", 55, false), level("mic", 80, false), None);
        let c = change(&old, &louder, ALL, &en).unwrap();
        assert_eq!((c.kind, c.value, c.muted), (Kind::Volume, Some(55), false));
        let muted = watched(level("a", 50, true), level("mic", 80, false), None);
        let c = change(&old, &muted, ALL, &en).unwrap();
        assert_eq!((c.kind, c.muted), (Kind::Volume, true));
        let other = watched(level("b", 50, false), level("mic", 80, false), None);
        assert_eq!(change(&old, &other, ALL, &en).unwrap().kind, Kind::Volume);
        assert_eq!(change(&old, &louder, &[Watch::Network], &en), None);
    }

    #[test]
    fn microphone() {
        let en = Locale::new("en");
        let on = watched(None, level("mic", 80, false), None);
        let off = watched(None, level("mic", 80, true), None);
        let c = change(&on, &off, ALL, &en).unwrap();
        assert_eq!(
            (c.kind, c.value, c.muted, c.text),
            (Kind::Microphone, Some(80), true, None)
        );
        let lower = watched(None, level("mic", 60, false), None);
        let c = change(&on, &lower, ALL, &en).unwrap();
        assert_eq!(
            (c.kind, c.value, c.muted),
            (Kind::Microphone, Some(60), false)
        );
        assert_eq!(change(&on, &lower, &[Watch::Volume], &en), None);
    }

    #[test]
    fn recording_starts_and_stops() {
        let en = Locale::new("en");
        let free = recording(Some(false));
        let in_use = recording(Some(true));
        // Known at the first listing: nothing to say.
        assert_eq!(change(&recording(None), &in_use, ALL, &en), None);
        let c = change(&free, &in_use, ALL, &en).unwrap();
        assert_eq!(c.kind, Kind::Recording);
        assert_eq!(c.text.as_deref(), Some("Microphone in use"));
        assert_eq!(c.icon.as_deref(), Some(ICON_RECORDING));
        assert_eq!(change(&in_use, &in_use, ALL, &en), None);
        let c = change(&in_use, &free, ALL, &en).unwrap();
        assert_eq!(c.text.as_deref(), Some("Microphone no longer in use"));
        assert_eq!(c.icon.as_deref(), Some(ICON_NOT_RECORDING));
        assert_eq!(change(&free, &in_use, &[Watch::Microphone], &en), None);
    }

    #[test]
    fn wifi_before_the_disconnection_it_brings() {
        let en = Locale::new("en");
        let on = Watched {
            wifi: Some(true),
            ..watched(None, None, link(true, "Home"))
        };
        let off = Watched {
            wifi: Some(false),
            ..watched(None, None, link(false, ""))
        };
        let c = change(&on, &off, ALL, &en).unwrap();
        assert_eq!(
            (c.kind, c.text.as_deref(), c.icon.as_deref()),
            (Kind::Wifi, Some("Wi‑Fi off"), Some(ICON_WIFI_OFF))
        );
        let c = change(&off, &on, ALL, &en).unwrap();
        assert_eq!(c.text.as_deref(), Some("Wi‑Fi on"));
        // Without the Wi‑Fi watch, the disconnection.
        let c = change(&on, &off, &[Watch::Network], &en).unwrap();
        assert_eq!(c.kind, Kind::Network);
    }

    #[test]
    fn vpn_up_and_down() {
        let en = Locale::new("en");
        let vpn = |v: Option<Option<&str>>| Watched {
            vpn: v.map(|v| v.map(str::to_owned)),
            ..Watched::default()
        };
        assert_eq!(
            change(&vpn(None), &vpn(Some(Some("Office"))), ALL, &en),
            None
        );
        let c = change(&vpn(Some(None)), &vpn(Some(Some("Office"))), ALL, &en).unwrap();
        assert_eq!(
            (c.kind, c.text.as_deref()),
            (Kind::Vpn, Some("VPN connected: Office"))
        );
        let c = change(&vpn(Some(Some("Office"))), &vpn(Some(None)), ALL, &en).unwrap();
        assert_eq!(c.text.as_deref(), Some("VPN disconnected"));
    }

    #[test]
    fn charger_with_the_charge() {
        let en = Locale::new("en");
        let charger = |plugged, percent| Watched {
            charger: Some(Charger {
                plugged,
                percent,
                icon: "battery-good-symbolic".to_owned(),
            }),
            ..Watched::default()
        };
        let c = change(&charger(false, 60), &charger(true, 60), ALL, &en).unwrap();
        assert_eq!(
            (c.kind, c.value, c.text.as_deref()),
            (Kind::Charger, None, Some("Charger connected, 60%"))
        );
        let c = change(&charger(true, 60), &charger(false, 60), ALL, &en).unwrap();
        assert_eq!(c.text.as_deref(), Some("On battery, 60%"));
        // The charge going up isn't news.
        assert_eq!(
            change(&charger(true, 60), &charger(true, 61), ALL, &en),
            None
        );
    }

    #[test]
    fn profile_and_idle() {
        let en = Locale::new("en");
        let profile = |p: &str| Watched {
            profile: Some(p.to_owned()),
            ..Watched::default()
        };
        let c = change(&profile("balanced"), &profile("power-saver"), ALL, &en).unwrap();
        assert_eq!(
            (c.kind, c.text.as_deref(), c.icon.as_deref()),
            (
                Kind::Profile,
                Some("Power profile: Saver"),
                Some("power-profile-power-saver-symbolic")
            )
        );
        let hold = |held| Watched {
            idle: Some(Hold {
                held,
                icon: if held { "eye-open" } else { "eye-closed" }.to_owned(),
            }),
            ..Watched::default()
        };
        let c = change(&hold(false), &hold(true), ALL, &en).unwrap();
        assert_eq!(
            (c.kind, c.text.as_deref(), c.icon.as_deref()),
            (Kind::Idle, Some("Keep awake: on"), Some("eye-open"))
        );
        let c = change(&hold(true), &hold(false), ALL, &en).unwrap();
        assert_eq!(c.text.as_deref(), Some("Keep awake: off"));
        assert_eq!(
            change(&hold(false), &hold(true), &[Watch::Profile], &en),
            None
        );
    }

    #[test]
    fn brightness_on_its_screens() {
        let en = Locale::new("en");
        let screens = |levels: &[(&str, Option<&str>, u32)]| Watched {
            brightness: levels
                .iter()
                .map(|(id, output, percent)| {
                    (
                        (*id).to_owned(),
                        Screen {
                            output: output.map(str::to_owned),
                            percent: *percent,
                        },
                    )
                })
                .collect(),
            ..Watched::default()
        };
        let old = screens(&[
            ("ddc:0", Some("HDMI-A-1"), 75),
            ("ddc:1", Some("HDMI-A-2"), 40),
        ]);
        // Read for the first time, or a monitor plugged in: nothing.
        assert_eq!(change(&Watched::default(), &old, ALL, &en), None);
        let one = screens(&[
            ("ddc:0", Some("HDMI-A-1"), 80),
            ("ddc:1", Some("HDMI-A-2"), 40),
        ]);
        let c = change(&old, &one, ALL, &en).unwrap();
        assert_eq!((c.kind, c.value), (Kind::Brightness, Some(80)));
        assert_eq!(
            c.outputs,
            Some(BTreeMap::from([("HDMI-A-1".to_owned(), 80)]))
        );
        let both = screens(&[
            ("ddc:0", Some("HDMI-A-1"), 80),
            ("ddc:1", Some("HDMI-A-2"), 45),
        ]);
        let c = change(&old, &both, ALL, &en).unwrap();
        assert_eq!(c.value_on("HDMI-A-1"), Some(80));
        assert_eq!(c.value_on("HDMI-A-2"), Some(45));
        let shown_on = c.shown_on(&["HDMI-A-1", "HDMI-A-2"]);
        assert!(shown_on("HDMI-A-1") && shown_on("HDMI-A-2"));
        let shown_on = one_content(&old, &one).shown_on(&["HDMI-A-1", "HDMI-A-2"]);
        assert!(shown_on("HDMI-A-1") && !shown_on("HDMI-A-2"));
        // Named after outputs that aren't there: everywhere.
        let shown_on = one_content(&old, &one).shown_on(&["DP-1"]);
        assert!(shown_on("DP-1"));
        // A panel on an output unknown: everywhere, with its level.
        let laptop = screens(&[("backlight:x", None, 50)]);
        let brighter = screens(&[("backlight:x", None, 55)]);
        let c = change(&laptop, &brighter, ALL, &en).unwrap();
        assert_eq!((c.value, c.outputs), (Some(55), None));
        assert_eq!(change(&old, &one, &[Watch::Volume], &en), None);
    }

    fn one_content(old: &Watched, new: &Watched) -> Content {
        change(old, new, ALL, &Locale::new("en")).unwrap()
    }

    #[test]
    fn network_up_and_down() {
        let en = Locale::new("en");
        let up = watched(None, None, link(true, "Home"));
        let down = watched(None, None, link(false, ""));
        let c = change(&up, &down, ALL, &en).unwrap();
        assert_eq!(c.kind, Kind::Network);
        assert_eq!(c.text.as_deref(), Some("Disconnected"));
        let c = change(&down, &up, ALL, &en).unwrap();
        assert_eq!(c.text.as_deref(), Some("Connected: Home"));
        let other = watched(None, None, link(true, "Office"));
        assert_eq!(
            change(&up, &other, ALL, &en).unwrap().text.as_deref(),
            Some("Connected: Office")
        );
    }

    #[test]
    fn volume_first_when_several_change() {
        let en = Locale::new("en");
        let old = watched(
            level("a", 50, false),
            level("mic", 80, false),
            link(true, "Home"),
        );
        let new = watched(
            level("a", 60, false),
            level("mic", 80, true),
            link(false, ""),
        );
        assert_eq!(change(&old, &new, ALL, &en).unwrap().kind, Kind::Volume);
        let new = watched(
            level("a", 50, false),
            level("mic", 80, true),
            link(false, ""),
        );
        assert_eq!(change(&old, &new, ALL, &en).unwrap().kind, Kind::Microphone);
    }

    #[test]
    fn config() {
        let c: OsdConfig = crate::config::Config::parse("").section(None);
        assert_eq!(c.show, ALL);
        assert_eq!(c.duration, Duration::from_secs(2));
        assert_eq!(c.position, Position::Bottom);
        assert_eq!(c.margin, 100);
        let c: OsdConfig = crate::config::Config::parse(
            "[osd]\nshow = network nope\nduration = 5\nposition = top\nmargin = 40\n",
        )
        .section(None);
        assert_eq!(c.show, vec![Watch::Network]);
        assert_eq!(c.duration, Duration::from_secs(5));
        assert_eq!(c.position, Position::Top);
        assert_eq!(c.margin, 40);
        let c: OsdConfig = crate::config::Config::parse("[osd]\nposition = left\n").section(None);
        assert_eq!(c.position, Position::Bottom);
        let c: OsdConfig = crate::config::Config::parse("[osd]\nshow = none\n").section(None);
        assert!(c.show.is_empty());
    }
}
