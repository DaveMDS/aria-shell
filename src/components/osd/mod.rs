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

use iced::window::Id;
use iced::{Padding, Point, Rectangle, Size, Task};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};
use icssed::{Element, Node};

use crate::config::{RawSection, Section};
use crate::locale::Locale;
use crate::services::audio::Audio;
use crate::services::brightness::Brightness;
use crate::services::icons::Icons;
use crate::services::idle::Idle;
use crate::services::network::Network;
use crate::services::power::Power;
use crate::ui::theme::Theme;
use crate::ui::{self, Measuring, Surfaces, popup};
pub use watch::Watch;
use watch::{ALL_WATCHES, Watched, change};
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
    windows: BTreeMap<OutputId, Shown>,
}

/// A surface of the OSD, as measured.
#[derive(Debug, Clone, Copy)]
struct Shown {
    window: Id,
    size: (u32, u32),
    /// The room around its box for the shadow.
    room: Padding,
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
    /// without one, sized to what it shows there (measured at the
    /// output's `scale`), a new size and frame for the others; the
    /// surfaces on other outputs go. With the timer that closes them,
    /// yielding the serial [`Osd::expired`] wants back.
    pub fn show(
        &mut self,
        content: Content,
        outputs: &BTreeMap<OutputId, OutputInfo>,
        m: Measuring<'_>,
    ) -> (Surfaces, Task<u64>) {
        let names: Vec<&str> = outputs.values().filter_map(|o| o.name.as_deref()).collect();
        let shown_on = content.shown_on(&names);
        self.content = Some(content);
        self.serial += 1;
        let mut surfaces = Surfaces::default();
        self.windows.retain(|output, shown| {
            let name = outputs.get(output).and_then(|o| o.name.as_deref());
            let keep = shown_on(name.unwrap_or("?"));
            if !keep {
                surfaces.close.push(shown.window);
            }
            keep
        });
        for (&output, info) in outputs {
            if !shown_on(info.name.as_deref().unwrap_or_default()) {
                continue;
            }
            if let Some(&shown) = self.windows.get(&output) {
                surfaces = surfaces.and(self.measure(output, info, shown.window, m));
                surfaces.redraw.push(shown.window);
                continue;
            }
            let window = Id::unique();
            let (size, room) = self.size(info, window, m);
            self.windows.insert(output, Shown { window, size, room });
            surfaces.open.push((
                window,
                NewLayerShellSettings {
                    anchor: anchor(self.config.position),
                    size: LayerSize::px(size.0, size.1),
                    layer: Layer::Overlay,
                    exclusive_zone: None,
                    margin: Some(self.margin(room)),
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

    /// The size of the surface `window` on output `info`, and the room
    /// around its box for the shadow.
    fn size(&self, info: &OutputInfo, window: Id, m: Measuring<'_>) -> ((u32, u32), Padding) {
        let output = info.name.as_deref().unwrap_or_default();
        let root = m
            .shared
            .theme
            .root("osd", m.scales.of(OutputId::from(info)));
        let room = match &self.content {
            Some(content) => popup::room(node(root, content, output)),
            None => Padding::ZERO,
        };
        let size = ui::size(
            &mut m.measure.borrow_mut(),
            window,
            self.view_at::<()>(root, m.shared.icons, output),
            ui::room_on(Some(info)),
        );
        (size, room)
    }

    /// Surface `window` measured again: its new size and margins, when
    /// they changed.
    fn measure(
        &mut self,
        output: OutputId,
        info: &OutputInfo,
        window: Id,
        m: Measuring<'_>,
    ) -> Surfaces {
        let (size, room) = self.size(info, window, m);
        let margin = self.margin(room);
        let anchor = anchor(self.config.position);
        let Some(shown) = self.windows.get_mut(&output) else {
            return Surfaces::default();
        };
        let mut surfaces = Surfaces::default();
        if shown.size != size {
            shown.size = size;
            surfaces
                .resize
                .push((window, anchor, LayerSize::px(size.0, size.1)));
        }
        if shown.room != room {
            shown.room = room;
            surfaces.margin.push((window, margin));
        }
        surfaces
    }

    /// The margins that put the box `margin` from its edge, its shadow's
    /// room nearer.
    fn margin(&self, room: Padding) -> (i32, i32, i32, i32) {
        match self.config.position {
            Position::Top => (self.config.margin - room.top as i32, 0, 0, 0),
            Position::Center => (0, 0, 0, 0),
            Position::Bottom => (0, 0, self.config.margin - room.bottom as i32, 0),
        }
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
            close: std::mem::take(&mut self.windows)
                .into_values()
                .map(|shown| shown.window)
                .collect(),
            ..Surfaces::default()
        }
    }

    /// Output `output` went away: its surface goes.
    pub fn output_removed(&mut self, output: OutputId) -> Surfaces {
        Surfaces {
            close: self
                .windows
                .remove(&output)
                .map(|shown| shown.window)
                .into_iter()
                .collect(),
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

    /// The theme or a scale changed: the surfaces are measured again.
    pub fn relayout(
        &mut self,
        outputs: &BTreeMap<OutputId, OutputInfo>,
        m: Measuring<'_>,
    ) -> Surfaces {
        let shown: Vec<(OutputId, Id)> = self.windows().collect();
        let mut surfaces = Surfaces::default();
        for (output, window) in shown {
            if let Some(info) = outputs.get(&output) {
                surfaces = surfaces.and(self.measure(output, info, window, m));
            }
        }
        surfaces
    }

    /// The open surfaces, with their output.
    pub fn windows(&self) -> impl Iterator<Item = (OutputId, Id)> + '_ {
        self.windows.iter().map(|(&o, shown)| (o, shown.window))
    }

    pub fn output_of(&self, window: Id) -> Option<OutputId> {
        self.windows
            .iter()
            .find(|(_, shown)| shown.window == window)
            .map(|(o, _)| *o)
    }

    /// Where its surface on `output` is, on an output with rectangle
    /// `rect`, as asked: centred, from its edge past the bars'
    /// exclusive zones there (`bars`: the top one's height, the bottom
    /// one's), by the margin.
    pub fn rect(&self, output: OutputId, rect: Rectangle, bars: (f32, f32)) -> Option<Rectangle> {
        let shown = self.windows.get(&output)?;
        let (w, h) = (shown.size.0 as f32, shown.size.1 as f32);
        let room = shown.room;
        let margin = self.config.margin as f32;
        let x = rect.x + (rect.width - w) / 2.0;
        let y = match self.config.position {
            Position::Top => rect.y + bars.0 + margin - room.top,
            Position::Center => rect.y + (rect.height - h) / 2.0,
            Position::Bottom => rect.y + rect.height - bars.1 - margin + room.bottom - h,
        };
        Some(Rectangle::new(Point::new(x, y), Size::new(w, h)))
    }

    pub fn icon_names(&self) -> impl Iterator<Item = &str> {
        self.content.iter().filter_map(|c| c.icon.as_deref())
    }

    /// The content on the surface of output `output` (its name), at
    /// `scale`.
    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        theme: &'a Theme,
        icons: &'a Icons,
        output: &str,
        scale: f32,
    ) -> Element<'a, M> {
        self.view_at(theme.root("osd", scale), icons, output)
    }

    /// [`Osd::view`] from its root.
    fn view_at<'a, M: Clone + 'a>(
        &'a self,
        root: Node<'a>,
        icons: &'a Icons,
        output: &str,
    ) -> Element<'a, M> {
        match &self.content {
            Some(content) => view(node(root, content, output), icons, content, output),
            None => icssed::column![].at(root).into(),
        }
    }
}

/// The box of `content` on output `output`, from `root` (an `osd`
/// root): its kind and mute as classes, the output as an attribute.
pub fn node<'a>(root: Node<'a>, content: &Content, output: &str) -> Node<'a> {
    root.class(content.kind.name())
        .class_if("muted", content.muted)
        .attr("output", output)
}

/// What a surface of the OSD shows for `content` on output `output`:
/// the pill, its box `node` ([`node`]): the icon, the level and the
/// percent, the text; each one only when it has one.
pub fn view<'a, M: Clone + 'a>(
    node: Node<'a>,
    icons: &'a Icons,
    content: &'a Content,
    output: &str,
) -> Element<'a, M> {
    let mut parts: Vec<Element<'a, M>> = Vec::new();
    if let Some(name) = &content.icon {
        let icons: &'a dyn icssed::Icons = icons;
        parts.push(icssed::icon((icons, name.as_str())).into());
    }
    let value = content.value_on(output);
    if let Some(value) = value {
        parts.push(icssed::meter(value as f32 / 100.0).into());
    }
    if let Some(text) = &content.text {
        parts.push(icssed::text(text.as_str()).into());
    }
    if let Some(value) = value {
        parts.push(icssed::text(format!("{value}%")).kind("value").into());
    }
    icssed::surface(icssed::row(parts)).at(node).into()
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
