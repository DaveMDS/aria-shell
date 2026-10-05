//! The OSD: a short-lived bar on every output when something it
//! watches changes (the default output's volume, the microphone's,
//! their mute, the network going up or down), whatever changed it: a
//! keybind running `wpctl`, the Audio gadget, another app. It only
//! shows, it never changes anything; `aria-shell osd show` shows one
//! from a script.
//!
//! The daemon owns one [`Osd`]: after every change of the shared
//! state it calls [`Osd::observe`], which compares what it watches
//! with the last reading ([`change`], a pure function) and says what
//! to show. The daemon opens one overlay surface per output, draws
//! [`Osd::view`] on each, and closes them all when the last change is
//! `duration` old (a serial-checked timer, so a held volume key keeps
//! the bar up and updates it in place).
//!
//! ```text
//! osd.<volume|microphone|network|custom>[.muted][output="<connector>"]
//! ├─ icon
//! ├─ meter > fill         the level, with a value
//! ├─ value                the percent, with a value
//! ╰─ label                the text, with one
//! ```

use std::collections::BTreeMap;
use std::time::Duration;

use iced::widget::{Space, row};
use iced::window::Id;
use iced::{Alignment, Element, Length};
use iced_wayland_subscriber::OutputId;

use crate::audio::{Audio, Kind as Channel};
use crate::config::{RawSection, Section};
use crate::gadgets;
use crate::icons::Icons;
use crate::locale::Locale;
use crate::network::{DeviceKind, Network, Summary};
use crate::theme::{self, Node, Theme};
use crate::widgets::graph;

/// Surface size when the theme doesn't set `width` / `height` on `osd`.
const DEFAULT_SIZE: (f32, f32) = (320.0, 56.0);
/// Icon size when the theme doesn't set `height` on `osd icon`.
const DEFAULT_ICON: f32 = 24.0;

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
            .list_or("show", &["volume", "microphone", "network"])
            .iter()
            .filter(|name| *name != "none")
            .filter_map(|name| {
                let watch = Watch::parse(name);
                if watch.is_none() {
                    log::warn!("[osd] show: unknown {name:?} (volume | microphone | network)");
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
    /// Connected / disconnected.
    Network,
}

impl Watch {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "volume" => Self::Volume,
            "microphone" => Self::Microphone,
            "network" => Self::Network,
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
    Network,
    /// From `aria-shell osd show`.
    Custom,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Volume => "volume",
            Self::Microphone => "microphone",
            Self::Network => "network",
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
            icon: gadgets::audio::level_icon(c),
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

/// What the OSD watches, as last read; `None` until known (at start,
/// and again while a source is away), so the first reading shows
/// nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watched {
    output: Option<Level>,
    input: Option<Level>,
    network: Option<Link>,
}

impl Watched {
    /// Read the shared state. The network passing through "connecting"
    /// keeps the last reading: switching networks shows the new one,
    /// not a disconnection first.
    fn read(audio: &Audio, network: &Network, previous: &Watched) -> Self {
        let output = Level::read(audio, Channel::Output);
        let input = Level::read(audio, Channel::Input);
        let network = if !network.running() || network.devices().is_empty() {
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
                    icon: gadgets::network::bar_icon(&s, true, wireless),
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
                    icon: gadgets::network::bar_icon(&down, true, wireless),
                })
            }
        };
        Self {
            output,
            input,
            network,
        }
    }
}

/// What changed from `old` to `new` among what `show` watches, as
/// something to show: the volume first, then the microphone, then the
/// network when several changed at once. A value becoming known isn't
/// a change.
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
        });
    }
    None
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
    pub windows: BTreeMap<OutputId, Id>,
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

    pub fn config(&self) -> &OsdConfig {
        &self.config
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
        locale: &Locale,
    ) -> Option<Content> {
        let new = Watched::read(audio, network, &self.watched);
        if new == self.watched {
            return None;
        }
        let shown = change(&self.watched, &new, &self.config.show, locale);
        self.watched = new;
        shown
    }

    /// Show `content` (in place of what's shown); the serial its timer
    /// must carry.
    pub fn show(&mut self, content: Content) -> u64 {
        self.content = Some(content);
        self.serial += 1;
        self.serial
    }

    /// The timer of show `serial` ran out: whether it was the last show
    /// (then the surfaces go).
    pub fn expired(&mut self, serial: u64) -> bool {
        if serial != self.serial {
            return false;
        }
        self.content = None;
        true
    }

    /// Forget what's shown (the surfaces are going).
    pub fn hide(&mut self) {
        self.content = None;
        self.windows.clear();
    }

    pub fn output_of(&self, window: Id) -> Option<OutputId> {
        self.windows
            .iter()
            .find(|(_, w)| **w == window)
            .map(|(o, _)| *o)
    }

    pub fn icon_names(&self) -> impl Iterator<Item = &str> {
        self.content.iter().filter_map(|c| c.icon.as_deref())
    }

    /// The surface size: the theme's `osd { width; height }`.
    pub fn size(&self, theme: &Theme) -> (u32, u32) {
        let s = theme.resolve(&Node::root("osd"));
        let px = |l: Option<theme::Length>, default: f32| match l {
            Some(theme::Length::Px(px)) => px.max(1.0) as u32,
            _ => default as u32,
        };
        (px(s.width, DEFAULT_SIZE.0), px(s.height, DEFAULT_SIZE.1))
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
        if let Some(value) = content.value {
            parts.push(graph::meter(
                theme,
                &node.child("meter"),
                value as f32 / 100.0,
            ));
        }
        if let Some(text) = &content.text {
            let label = node.child("label");
            let mut label = theme.container(&label, theme.text(&label, text.clone()));
            if content.value.is_none() {
                label = label.width(Length::Fill);
            }
            parts.push(label.into());
        }
        if let Some(value) = content.value {
            let value_node = node.child("value");
            parts.push(
                theme
                    .container(&value_node, theme.text(&value_node, format!("{value}%")))
                    .into(),
            );
        }
        theme
            .container(
                &node,
                row(parts)
                    .spacing(style.gap)
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[Watch] = &[Watch::Volume, Watch::Microphone, Watch::Network];

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
        assert_eq!(
            c.show,
            vec![Watch::Volume, Watch::Microphone, Watch::Network]
        );
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
