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
use crate::gadget::{self, AnyGadget, Context, Shared};
use crate::theme::{self, Node, Theme};

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

impl Slot {
    /// The class selectors see: `slot.start`, ...
    fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
        }
    }
}

pub struct Panel {
    /// Config section this panel was built from, e.g. `panel:2`.
    pub section: String,
    pub output: OutputId,
    config: PanelConfig,
    /// `panel[.top|.bottom][#instance][output=..]`, the root of every
    /// node on this bar.
    node: Node,
    /// Bar thickness the surface was created (or last resized) with.
    /// Layer-shell needs it up front, so it comes from the theme's
    /// `min-height` on `panel`, not from the content.
    height: u32,
    gadgets: Vec<Entry>,
    /// Open popups, by window id, to the index of the gadget that owns
    /// each. The panel mints the ids so the daemon only has to map them
    /// back to the panel.
    popups: BTreeMap<window::Id, usize>,
}

struct Entry {
    slot: Slot,
    gadget: AnyGadget,
    /// `panel > slot.<slot> > gadget.<kind>[#instance]:nth`.
    node: Node,
    /// `popup > gadget.<kind>[#instance]`.
    popup_node: Node,
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
    pub fn new(
        section: String,
        config: PanelConfig,
        shell: &Config,
        theme: &Theme,
        output: &OutputInfo,
    ) -> Self {
        let node = Node::root("panel")
            .class(match config.position {
                Position::Top => "top",
                Position::Bottom => "bottom",
            })
            .id_opt(instance_id(&section))
            .attr("output", output.name.clone().unwrap_or_default());
        let mut gadgets = Vec::new();
        for (slot, names) in [
            (Slot::Start, &config.items_start),
            (Slot::Center, &config.items_center),
            (Slot::End, &config.items_end),
        ] {
            let slot_node = node.child("slot").class(slot.name());
            let created: Vec<(&String, AnyGadget)> = names
                .iter()
                .filter_map(|name| Some((name, AnyGadget::create(name, shell, output)?)))
                .collect();
            let count = created.len();
            for (i, (name, gadget)) in created.into_iter().enumerate() {
                let id = instance_id(name);
                gadgets.push(Entry {
                    node: slot_node
                        .child("gadget")
                        .class(gadget.kind())
                        .id_opt(id)
                        .nth(i, count),
                    popup_node: Node::root("popup")
                        .child("gadget")
                        .class(gadget.kind())
                        .id_opt(id),
                    slot,
                    gadget,
                });
            }
        }
        let height = Self::themed_height(theme, &node);
        Self {
            section,
            output: OutputId::from(output),
            config,
            node,
            height,
            gadgets,
            popups: BTreeMap::new(),
        }
    }

    fn themed_height(theme: &Theme, node: &Node) -> u32 {
        theme
            .resolve(node)
            .min_height
            .unwrap_or(theme::DEFAULT_PANEL_HEIGHT)
            .max(1.0) as u32
    }

    fn anchor(&self) -> Anchor {
        let edge = match self.config.position {
            Position::Top => Anchor::Top,
            Position::Bottom => Anchor::Bottom,
        };
        edge | Anchor::Left | Anchor::Right
    }

    pub fn layer_settings(&self) -> NewLayerShellSettings {
        NewLayerShellSettings {
            anchor: self.anchor(),
            size: LayerSize::fill_width(self.height),
            layer: self.config.layer,
            exclusive_zone: Some(self.height as i32),
            margin: None,
            keyboard_interactivity: KeyboardInteractivity::None,
            output_option: OutputOption::GlobalName(self.output.0),
            namespace: Some("aria-panel".to_owned()),
            ..Default::default()
        }
    }

    /// The theme changed: if the bar thickness did too, the new layout
    /// (and exclusive zone) to send for this surface.
    pub fn resize(&mut self, theme: &Theme) -> Option<(Anchor, LayerSize, i32)> {
        let height = Self::themed_height(theme, &self.node);
        if height == self.height {
            return None;
        }
        self.height = height;
        Some((self.anchor(), LayerSize::fill_width(height), height as i32))
    }

    pub fn position(&self) -> Position {
        self.config.position
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Gadget(i, m) => {
                let Some(Entry { gadget: g, .. }) = self.gadgets.get_mut(i) else {
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
            && let Some(Entry { gadget: g, .. }) = self.gadgets.get_mut(i)
        {
            g.popup_closed();
        }
    }

    pub fn view<'a>(&'a self, shared: Shared<'a>) -> Element<'a, Message> {
        let theme = shared.theme;
        let section = |slot: Slot| {
            let slot_node = self.node.child("slot").class(slot.name());
            let children = self
                .gadgets
                .iter()
                .enumerate()
                .filter(move |(_, e)| e.slot == slot)
                .map(move |(i, e)| {
                    let ctx = Context {
                        shared,
                        node: e.node.clone(),
                    };
                    // Centred, so a gadget given `height: fill` by the
                    // theme keeps its content in the middle of the bar.
                    theme
                        .container(
                            &e.node,
                            e.gadget.view(ctx).map(move |m| Message::Gadget(i, m)),
                        )
                        .align_y(iced::Alignment::Center)
                        .into()
                });
            theme
                .row(&slot_node, children)
                .align_y(iced::Alignment::Center)
        };
        let bar = row![
            container(section(Slot::Start))
                .width(Length::Fill)
                .align_left(Length::Fill),
            container(section(Slot::Center))
                .width(Length::Fill)
                .center_x(Length::Fill),
            container(section(Slot::End))
                .width(Length::Fill)
                .align_right(Length::Fill),
        ]
        .height(Length::Fill)
        .align_y(iced::Alignment::Center);
        theme
            .container(&self.node, bar)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Content of the popup surface `id`: the gadget's popup view inside
    /// a `popup` root filling the surface.
    pub fn popup_view<'a>(&'a self, id: window::Id, shared: Shared<'a>) -> Element<'a, Message> {
        let Some((i, e)) = self
            .popups
            .get(&id)
            .and_then(|&i| Some((i, self.gadgets.get(i)?)))
        else {
            return Space::new().into();
        };
        let ctx = Context {
            shared,
            node: e.popup_node.clone(),
        };
        let content = e.gadget.popup_view(ctx).map(move |m| Message::Gadget(i, m));
        shared
            .theme
            .container(&Node::root("popup"), content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(self.gadgets.iter().enumerate().map(|(i, e)| {
            e.gadget
                .subscription()
                .with(i)
                .map(|(i, m)| Message::Gadget(i, m))
        }))
    }
}

/// The `id` of a `[Name:id]` section, `None` for `[Name]`.
fn instance_id(section: &str) -> Option<&str> {
    section
        .split_once(':')
        .map(|(_, id)| id)
        .filter(|id| !id.is_empty())
}
