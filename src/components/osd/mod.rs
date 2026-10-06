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
//! with the last reading (`watch.rs`: [`watch::change`], a pure
//! function) and says what to show. [`Osd::show`] asks for one overlay surface per output
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

mod watch;

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
use crate::services::audio::Audio;
use crate::services::brightness::Brightness;
use crate::services::icons::Icons;
use crate::services::idle::Idle;
use crate::services::network::Network;
use crate::services::power::Power;
use crate::ui::graph;
use crate::ui::theme::{self, Node, Theme};
pub use watch::Watch;
use watch::{ALL_WATCHES, Watched, change};

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
