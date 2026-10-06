//! A panel: one layer-shell bar on one output, holding gadgets in three
//! slots (start / center / end), and their popups: xdg popup surfaces
//! child of the bar's ([`crate::ui::popup`]), one at a time, which the
//! panel opens, sizes and places ([`Surfaces`]) as its gadgets ask.

use std::collections::BTreeMap;

use iced::widget::{Space, container, row};
use iced::{Element, Length, Rectangle, Subscription, Task, widget, window};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};

use crate::components::Surfaces;

use crate::config::{Config, RawSection, Section};
use crate::gadgets::{self, AnyGadget, Context, Shared};
use crate::services::compositor;
use crate::services::scripts;
use crate::services::tray;
use crate::ui::popup::{self, Side};
use crate::ui::theme::{self, Node, Theme};

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
    /// Open popups, by window id. The panel mints the ids so the daemon
    /// only has to map them back to the panel.
    popups: BTreeMap<window::Id, Popup>,
    /// Whether the surface was last made keyboard-interactive, for a
    /// popup with a text field (see [`Panel::wants_keyboard`]).
    pub keyboard: bool,
}

/// An open popup.
struct Popup {
    /// The index of the gadget that owns it.
    gadget: usize,
    /// Once its widget was located: its anchor in the bar's surface
    /// (the widget's width, the bar's whole height), and the surface
    /// size it was last given.
    placed: Option<(Rectangle, (u32, u32))>,
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
    Gadget(usize, gadgets::Message),
    /// A key the compositor sent to the bar's surface, for the open
    /// popup that wants the keyboard (see [`Panel::wants_keyboard`]).
    Key(iced::keyboard::Event),
}

/// What `update` asks the daemon to do; [`gadgets::Action`] with the
/// popup bookkeeping the panel already did.
pub enum Action {
    None,
    Run(Task<Message>),
    Compositor(compositor::Command),
    Tray(tray::Command),
    Theme(theme::Command),
    Script(scripts::Command),
    Notifications(crate::services::notifications::Command),
    SysMon(crate::services::sysmon::Command),
    Audio(crate::services::audio::Command),
    Network(crate::services::network::Command),
    Idle(crate::services::idle::Command),
    Power(crate::services::power::Command),
    Brightness(crate::services::brightness::Command),
    Screenshot(crate::services::screenshot::Command),
    Places(crate::services::places::Command),
    /// Open the popup surface `id` as a child of this panel's surface,
    /// hanging off the widget tagged `anchor`.
    OpenPopup {
        id: window::Id,
        anchor: widget::Id,
    },
    ClosePopup(window::Id),
    Many(Vec<Action>),
}

impl Action {
    /// Whether what it changes right away is only the gadgets' own
    /// state, what this panel and its popups show: then only those
    /// surfaces need a new frame. A command to a daemon that changes
    /// nothing until its answer comes back (`run(&self)`) is: the
    /// answer's message redraws whatever shows it.
    pub fn is_local(&self) -> bool {
        match self {
            Self::None
            | Self::Run(_)
            | Self::Compositor(_)
            | Self::Tray(_)
            | Self::SysMon(_)
            | Self::Audio(_)
            | Self::Screenshot(_)
            | Self::Places(_) => true,
            Self::Many(actions) => actions.iter().all(Self::is_local),
            _ => false,
        }
    }
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
        let popup_root =
            Node::root("popup").attr("output", output.name.clone().unwrap_or_default());
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
                    popup_node: popup_root.child("gadget").class(gadget.kind()).id_opt(id),
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
            keyboard: false,
        }
    }

    /// Whether an open popup of this bar shows a text field.
    pub fn wants_keyboard(&self) -> bool {
        self.popups.values().any(|p| {
            self.gadgets
                .get(p.gadget)
                .is_some_and(|e| e.gadget.popup_keyboard())
        })
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

    /// Bar thickness, as the surface was requested.
    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Gadget(i, m) => {
                let Some(Entry { gadget: g, .. }) = self.gadgets.get_mut(i) else {
                    return Action::None;
                };
                let action = g.update(m);
                self.lift(i, action)
            }
            Message::Key(event) => {
                let Some(i) = self.popups.values().map(|p| p.gadget).find(|i| {
                    self.gadgets
                        .get(*i)
                        .is_some_and(|e| e.gadget.popup_keyboard())
                }) else {
                    return Action::None;
                };
                let Some(Entry { gadget: g, .. }) = self.gadgets.get_mut(i) else {
                    return Action::None;
                };
                let action = g.popup_key(event);
                self.lift(i, action)
            }
        }
    }

    /// A gadget's action as the daemon sees it: messages routed back to
    /// gadget `i`, popups given their window id.
    fn lift(&mut self, i: usize, action: gadgets::Action<gadgets::Message>) -> Action {
        match action {
            gadgets::Action::None => Action::None,
            gadgets::Action::Run(task) => Action::Run(task.map(move |m| Message::Gadget(i, m))),
            gadgets::Action::Compositor(cmd) => Action::Compositor(cmd),
            gadgets::Action::Tray(cmd) => Action::Tray(cmd),
            gadgets::Action::Theme(cmd) => Action::Theme(cmd),
            gadgets::Action::Script(cmd) => Action::Script(cmd),
            gadgets::Action::Notifications(cmd) => Action::Notifications(cmd),
            gadgets::Action::SysMon(cmd) => Action::SysMon(cmd),
            gadgets::Action::Audio(cmd) => Action::Audio(cmd),
            gadgets::Action::Network(cmd) => Action::Network(cmd),
            gadgets::Action::Idle(cmd) => Action::Idle(cmd),
            gadgets::Action::Power(cmd) => Action::Power(cmd),
            gadgets::Action::Brightness(cmd) => Action::Brightness(cmd),
            gadgets::Action::Screenshot(cmd) => Action::Screenshot(cmd),
            gadgets::Action::Places(cmd) => Action::Places(cmd),
            gadgets::Action::OpenPopup { anchor } => {
                let id = window::Id::unique();
                self.popups.insert(
                    id,
                    Popup {
                        gadget: i,
                        placed: None,
                    },
                );
                if let Some(Entry { gadget: g, .. }) = self.gadgets.get_mut(i) {
                    g.popup_opened(id);
                }
                Action::OpenPopup { id, anchor }
            }
            gadgets::Action::ClosePopup(id) => {
                self.popups.remove(&id);
                Action::ClosePopup(id)
            }
            gadgets::Action::Many(actions) => {
                Action::Many(actions.into_iter().map(|a| self.lift(i, a)).collect())
            }
        }
    }

    /// Icon names the gadgets draw, for the daemon to resolve.
    pub fn icon_names(&self) -> impl Iterator<Item = String> + '_ {
        self.gadgets.iter().flat_map(|e| e.gadget.icon_names())
    }

    /// Programs the gadgets want run, for the daemon.
    pub fn scripts(&self) -> impl Iterator<Item = scripts::Spec> + '_ {
        self.gadgets.iter().filter_map(|e| e.gadget.script())
    }

    /// The side of its widget a popup hangs on.
    fn popup_side(&self) -> Side {
        match self.config.position {
            Position::Top => Side::Below,
            Position::Bottom => Side::Above,
        }
    }

    /// Surface size for popup `id`: what its gadget wants for the
    /// content now, plus the popup's chrome.
    pub fn popup_size(&self, id: window::Id, shared: Shared<'_>) -> Option<(u32, u32)> {
        let e = self
            .popups
            .get(&id)
            .and_then(|p| self.gadgets.get(p.gadget))?;
        let ctx = Context {
            shared,
            node: e.popup_node.clone(),
        };
        Some(popup::surface_size(shared.theme, e.gadget.popup_size(ctx)))
    }

    pub fn has_popup(&self, id: window::Id) -> bool {
        self.popups.contains_key(&id)
    }

    pub fn has_popups(&self) -> bool {
        !self.popups.is_empty()
    }

    /// The open popups' surfaces.
    pub fn popup_windows(&self) -> impl Iterator<Item = window::Id> + '_ {
        self.popups.keys().copied()
    }

    /// The widget of popup `id` was located at `widget` in the bar's
    /// surface `bar`: open the popup there. The widget gives the x, the
    /// bar the y: the popup meets the bar's edge, however short the
    /// button is.
    pub fn place_popup(
        &mut self,
        bar: window::Id,
        id: window::Id,
        widget: Rectangle,
        shared: Shared<'_>,
    ) -> Surfaces {
        let Some(size) = self.popup_size(id, shared) else {
            return Surfaces::default();
        };
        let anchor = Rectangle {
            y: 0.0,
            height: self.height as f32,
            ..widget
        };
        let side = self.popup_side();
        let Some(p) = self.popups.get_mut(&id) else {
            return Surfaces::default();
        };
        p.placed = Some((anchor, size));
        let settings = popup::settings(bar, side, anchor, size, popup::room(shared.theme));
        Surfaces {
            popup: vec![(id, settings)],
            ..Surfaces::default()
        }
    }

    /// After a change of the gadgets' or the shared state: the bar
    /// takes the keyboard while a popup with a text field is open, and
    /// gives it back after (the compositor sends keys to the popup,
    /// which holds the grab); a popup whose gadget wants another size
    /// is placed again with it.
    pub fn sync_popups(&mut self, bar: window::Id, shared: Shared<'_>) -> Surfaces {
        let mut surfaces = Surfaces::default();
        let wanted = self.wants_keyboard();
        if wanted != self.keyboard {
            self.keyboard = wanted;
            let interactivity = if wanted {
                KeyboardInteractivity::Exclusive
            } else {
                KeyboardInteractivity::None
            };
            surfaces.keyboard.push((bar, interactivity));
        }
        let side = self.popup_side();
        let room = popup::room(shared.theme);
        let ids: Vec<window::Id> = self.popups.keys().copied().collect();
        for id in ids {
            let Some(size) = self.popup_size(id, shared) else {
                continue;
            };
            let Some(Popup {
                placed: Some((anchor, placed)),
                ..
            }) = self.popups.get_mut(&id)
            else {
                continue;
            };
            if *placed == size {
                continue;
            }
            *placed = size;
            let settings = popup::settings(bar, side, *anchor, size, room);
            surfaces.reposition.push((id, settings));
        }
        surfaces
    }

    /// Close every popup but `except` (another opens, a click
    /// elsewhere): their surfaces go, their gadgets are told.
    pub fn close_popups(&mut self, except: Option<window::Id>) -> Surfaces {
        let ids: Vec<window::Id> = self
            .popups
            .keys()
            .copied()
            .filter(|&id| Some(id) != except)
            .collect();
        for &id in &ids {
            self.popup_closed(id);
        }
        Surfaces {
            close: ids,
            ..Surfaces::default()
        }
    }

    /// The popup surface `id` is gone, whoever closed it: whether it
    /// was one of this bar's.
    pub fn popup_closed(&mut self, id: window::Id) -> bool {
        let Some(p) = self.popups.remove(&id) else {
            return false;
        };
        if let Some(Entry { gadget: g, .. }) = self.gadgets.get_mut(p.gadget) {
            g.popup_closed();
        }
        true
    }

    /// Where the placed popups are asked to be, relative to the bar's
    /// surface: for `debug surfaces`.
    pub fn popup_rects(&self, theme: &Theme) -> impl Iterator<Item = (window::Id, Rectangle)> {
        let side = self.popup_side();
        let room = popup::room(theme);
        self.popups.iter().filter_map(move |(&id, p)| {
            let (anchor, size) = p.placed?;
            Some((id, popup::estimate(side, anchor, size, room)))
        })
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
                    theme
                        .container(
                            &e.node,
                            e.gadget.view(ctx).map(move |m| Message::Gadget(i, m)),
                        )
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
            .and_then(|p| Some((p.gadget, self.gadgets.get(p.gadget)?)))
        else {
            return Space::new().into();
        };
        let ctx = Context {
            shared,
            node: e.popup_node.clone(),
        };
        let content = e.gadget.popup_view(ctx).map(move |m| Message::Gadget(i, m));
        let root = e
            .popup_node
            .parent()
            .cloned()
            .unwrap_or_else(|| Node::root("popup"));
        let content = shared
            .theme
            .container(&root, content)
            .width(Length::Fill)
            .height(Length::Fill);
        shared.theme.surface(&root, content).into()
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
