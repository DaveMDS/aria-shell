//! A panel: one layer-shell bar on one output, holding gadgets in three
//! slots (start / center / end).

use std::collections::BTreeMap;

use iced::widget::{Space, container, row};
use iced::{Element, Length, Rectangle, Subscription, Task, widget, window};
use iced_exwlshell::actions::IcedNewPopupSettings;
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
    PixelSize, PopupAnchor, PopupGravity,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};

use crate::compositor;
use crate::config::{Config, RawSection, Section};
use crate::gadget::{self, AnyGadget, Context};

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

/// Placement of a popup hanging off a widget with `anchor` bounds in the
/// surface of a bar at `position`: centred on the widget, on the side
/// away from the screen edge.
pub fn popup_settings(
    parent: window::Id,
    position: Position,
    anchor: Rectangle,
    size: (u32, u32),
) -> IcedNewPopupSettings {
    let (edge, gravity) = match position {
        Position::Top => (PopupAnchor::Bottom, PopupGravity::Bottom),
        Position::Bottom => (PopupAnchor::Top, PopupGravity::Top),
    };
    IcedNewPopupSettings::new(
        parent,
        PixelSize::px(size.0.max(1), size.1.max(1)),
        (anchor.x as i32, anchor.y as i32),
        PixelSize::px((anchor.width as u32).max(1), (anchor.height as u32).max(1)),
    )
    .anchor(edge)
    .gravity(gravity)
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
    /// Open popups, by window id, to the index of the gadget that owns
    /// each. The panel mints the ids so the daemon only has to map them
    /// back to the panel.
    popups: BTreeMap<window::Id, usize>,
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Index into `gadgets`, then the gadget's own message.
    Gadget(usize, gadget::Message),
}

/// What `update` asks the daemon to do; [`gadget::Action`] with the
/// popup bookkeeping the panel already did.
pub enum Action {
    None,
    Run(Task<Message>),
    Compositor(compositor::Command),
    /// Open the popup surface `id` as a child of this panel's surface,
    /// hanging off the widget tagged `anchor`.
    OpenPopup {
        id: window::Id,
        anchor: widget::Id,
        size: (u32, u32),
    },
    ClosePopup(window::Id),
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
            popups: BTreeMap::new(),
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

    pub fn position(&self) -> Position {
        self.config.position
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Gadget(i, m) => {
                let Some((_, g)) = self.gadgets.get_mut(i) else {
                    return Action::None;
                };
                match g.update(m) {
                    gadget::Action::None => Action::None,
                    gadget::Action::Run(task) => {
                        Action::Run(task.map(move |m| Message::Gadget(i, m)))
                    }
                    gadget::Action::Compositor(cmd) => Action::Compositor(cmd),
                    gadget::Action::OpenPopup { anchor, size } => {
                        let id = window::Id::unique();
                        self.popups.insert(id, i);
                        g.popup_opened(id);
                        Action::OpenPopup { id, anchor, size }
                    }
                    gadget::Action::ClosePopup(id) => {
                        self.popups.remove(&id);
                        Action::ClosePopup(id)
                    }
                }
            }
        }
    }

    /// The popup surface `id` is gone, whoever closed it.
    pub fn popup_closed(&mut self, id: window::Id) {
        if let Some(i) = self.popups.remove(&id)
            && let Some((_, g)) = self.gadgets.get_mut(i)
        {
            g.popup_closed();
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

    /// Content of the popup surface `id`.
    pub fn popup_view<'a>(&'a self, id: window::Id, ctx: Context<'a>) -> Element<'a, Message> {
        match self
            .popups
            .get(&id)
            .and_then(|&i| Some((i, self.gadgets.get(i)?)))
        {
            Some((i, (_, g))) => g.popup_view(ctx).map(move |m| Message::Gadget(i, m)),
            None => Space::new().into(),
        }
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
