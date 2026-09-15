//! A panel: one layer-shell bar on one output, holding gadgets in three
//! slots (start / center / end).

use iced::widget::{container, row};
use iced::{Element, Length, Subscription};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};

use crate::config::{Config, RawSection, Section};
use crate::gadget::{self, Action, AnyGadget, Context};

/// Bar thickness. Fixed for now: layer-shell needs a size up front and
/// iced can't report a content size before the first layout.
const HEIGHT: u32 = 32;

/// `[panel]` section, one per bar (`[panel:2]` for a second one). Keys
/// and defaults match the Python implementation; `size`, `align`,
/// `margin`, `opacity` are not honoured yet.
#[derive(Debug, Clone)]
pub struct PanelConfig {
    /// `all`, or the connector names (`DP-1 HDMI-A-1`) to show this bar on.
    pub outputs: Vec<String>,
    pub position: Position,
    pub layer: Layer,
    pub items_start: Vec<String>,
    pub items_center: Vec<String>,
    pub items_end: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Top,
    Bottom,
}

impl Section for PanelConfig {
    const NAME: &'static str = "panel";

    fn from_raw(raw: &RawSection) -> Self {
        let position = match raw.get("position") {
            None | Some("top") => Position::Top,
            Some("bottom") => Position::Bottom,
            Some(other) => {
                log::warn!("invalid panel position {other:?}, using top");
                Position::Top
            }
        };
        let layer = match raw.get("layer") {
            None | Some("bottom") => Layer::Bottom,
            Some("top") => Layer::Top,
            Some("overlay") => Layer::Overlay,
            Some(other) => {
                log::warn!("invalid panel layer {other:?}, using bottom");
                Layer::Bottom
            }
        };
        Self {
            outputs: raw.list_or("outputs", &["all"]),
            position,
            layer,
            items_start: raw.list_or("items_start", &[]),
            items_center: raw.list_or("items_center", &[]),
            items_end: raw.list_or("items_end", &[]),
        }
    }
}

impl PanelConfig {
    /// The `[panel*]` sections to instantiate. With none configured, a
    /// single default bar with just a clock.
    pub fn all(config: &Config) -> Vec<(String, Self)> {
        let names = config.instances(Self::NAME);
        if names.is_empty() {
            let mut default = config.section::<Self>(None);
            default.items_center = vec!["Clock".to_owned()];
            return vec![(Self::NAME.to_owned(), default)];
        }
        names
            .into_iter()
            .map(|n| {
                let cfg = config.section(Some(&n));
                (n, cfg)
            })
            .collect()
    }

    pub fn wants_output(&self, output: &OutputInfo) -> bool {
        self.outputs.iter().any(|o| o == "all")
            || output
                .name
                .as_ref()
                .is_some_and(|name| self.outputs.contains(name))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Start,
    Center,
    End,
}

pub struct Panel {
    /// Config section this panel was built from, e.g. `panel:2`.
    pub section: String,
    pub output: OutputId,
    config: PanelConfig,
    gadgets: Vec<(Slot, AnyGadget)>,
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Index into `gadgets`, then the gadget's own message.
    Gadget(usize, gadget::Message),
}

impl Panel {
    pub fn new(section: String, config: PanelConfig, shell: &Config, output: &OutputInfo) -> Self {
        let mut gadgets = Vec::new();
        for (slot, names) in [
            (Slot::Start, &config.items_start),
            (Slot::Center, &config.items_center),
            (Slot::End, &config.items_end),
        ] {
            for name in names {
                if let Some(g) = AnyGadget::create(name, shell, output) {
                    gadgets.push((slot, g));
                }
            }
        }
        Self {
            section,
            output: OutputId::from(output),
            config,
            gadgets,
        }
    }

    pub fn layer_settings(&self) -> NewLayerShellSettings {
        let edge = match self.config.position {
            Position::Top => Anchor::Top,
            Position::Bottom => Anchor::Bottom,
        };
        NewLayerShellSettings {
            anchor: edge | Anchor::Left | Anchor::Right,
            size: LayerSize::fill_width(HEIGHT),
            layer: self.config.layer,
            exclusive_zone: Some(HEIGHT as i32),
            margin: None,
            keyboard_interactivity: KeyboardInteractivity::None,
            output_option: OutputOption::GlobalName(self.output.0),
            namespace: Some("aria-panel".to_owned()),
            ..Default::default()
        }
    }

    pub fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Gadget(i, m) => match self.gadgets.get_mut(i) {
                Some((_, g)) => g.update(m).map(move |m| Message::Gadget(i, m)),
                None => Action::None,
            },
        }
    }

    pub fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let section = |slot| {
            row(self
                .gadgets
                .iter()
                .enumerate()
                .filter(move |(_, (s, _))| *s == slot)
                .map(move |(i, (_, g))| g.view(ctx).map(move |m| Message::Gadget(i, m))))
            .spacing(8)
        };
        row![
            container(section(Slot::Start)).width(Length::Fill),
            container(section(Slot::Center))
                .width(Length::Fill)
                .center_x(Length::Fill),
            container(section(Slot::End))
                .width(Length::Fill)
                .align_right(Length::Fill),
        ]
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(
            self.gadgets
                .iter()
                .enumerate()
                .map(|(i, (_, g))| g.subscription().with(i).map(|(i, m)| Message::Gadget(i, m))),
        )
    }
}
