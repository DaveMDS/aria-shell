//! A panel: one layer-shell bar on one output, holding gadgets in three
//! slots (start / center / end), and their popups: xdg popup surfaces
//! child of the bar's ([`crate::ui::popup`]), one at a time, which the
//! panel opens, sizes and places ([`Surfaces`]) as its gadgets ask. The
//! bar's thickness and the popups' sizes are their content's, measured
//! at the output's scale; so is its length, when it doesn't span its
//! edge (`align`).

use std::collections::BTreeMap;
use std::time::Duration;

use iced::{Rectangle, Size, Subscription, window};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};
use icssed::{Element, Node};

use crate::config::{Config, RawSection, Section};
use crate::gadgets::{self, AnyGadget, Context};
use crate::services::scripts;
use crate::shared::Shared;
use crate::ui::popup::{self, Side};
use crate::ui::theme::Theme;
use crate::ui::{self, Measuring, Surfaces};

/// `[panel]` section, one per bar (`[panel:2]` for a second one). Keys
/// and defaults match the Python implementation, but for its `size` and
/// `align`, one `align` here, and `layer`, `top` unless said (under the
/// windows a bar is rarely wanted); `margin`, `opacity` are not honoured.
#[derive(Debug, Clone)]
pub struct PanelConfig {
    /// `all`, or the connector names (`DP-1 HDMI-A-1`) to show this bar on.
    pub outputs: Vec<String>,
    pub position: Position,
    pub layer: Layer,
    pub align: Align,
    pub autohide: Autohide,
    pub items_start: Vec<String>,
    pub items_center: Vec<String>,
    pub items_end: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Top,
    Bottom,
}

/// Whether the bar hides at its edge, shown while the pointer rests
/// there (it then keeps no room from the windows): `smart` shows it on
/// a desktop without windows too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Autohide {
    No,
    Yes,
    Smart,
}

/// Where the bar is along its edge: all of it, or as long as its
/// gadgets at its start, middle or end. At the start or the end it's in
/// a corner, where layer-shell reserves no room (no exclusive zone):
/// windows go under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Stretch,
    Start,
    Center,
    End,
}

impl Align {
    /// `panel[align=..]`.
    fn name(self) -> &'static str {
        match self {
            Self::Stretch => "stretch",
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
        }
    }
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
            None | Some("top") => Layer::Top,
            Some("bottom") => Layer::Bottom,
            Some("overlay") => Layer::Overlay,
            Some(other) => {
                log::warn!("invalid panel layer {other:?}, using top");
                Layer::Top
            }
        };
        let align = match raw.get("align") {
            None | Some("stretch") => Align::Stretch,
            Some("start") => Align::Start,
            Some("center") => Align::Center,
            Some("end") => Align::End,
            Some(other) => {
                log::warn!("invalid panel align {other:?}, using stretch");
                Align::Stretch
            }
        };
        let autohide = match raw.get("autohide") {
            None | Some("no") => Autohide::No,
            Some("yes") => Autohide::Yes,
            Some("smart") => Autohide::Smart,
            Some(other) => {
                log::warn!("invalid panel autohide {other:?}, using no");
                Autohide::No
            }
        };
        if autohide != Autohide::No && layer == Layer::Bottom {
            log::warn!("panel autohide on the bottom layer: windows cover it, use top");
        }
        Self {
            outputs: raw.list_or("outputs", &["all"]),
            position,
            layer,
            align,
            autohide,
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
    /// The output's connector: `panel[output=..]`, `popup[output=..]`.
    output_name: String,
    /// The output's logical size: what the bar spans, the room its
    /// popups have.
    room: Size,
    config: PanelConfig,
    /// The scale its output's surfaces are drawn at, for the roots.
    pub scale: f32,
    /// Bar thickness the surface was created (or last resized) with:
    /// its content's (`min-height` a minimum), measured, since
    /// layer-shell needs it up front.
    height: u32,
    /// Its length, measured as the thickness: the content's when it
    /// doesn't stretch (the output's otherwise, unused).
    width: u32,
    gadgets: Vec<Entry>,
    /// Open popups, by window id. The panel mints the ids so the daemon
    /// only has to map them back to the panel.
    popups: BTreeMap<window::Id, Popup>,
    /// Whether the surface was last made keyboard-interactive, for a
    /// popup with a text field (see [`Panel::wants_keyboard`]).
    pub keyboard: bool,
    /// With `autohide`: whether it's hidden now (`panel.hidden`, only a
    /// strip at its edge taking the pointer).
    hidden: bool,
    /// Whether the pointer is over the bar.
    pointer: bool,
    /// Whether its output shows a workspace without windows (`smart`).
    empty: bool,
    /// The timer armed to hide (`true`) or show it, and its serial: one
    /// armed before the last is stale.
    timer: Option<bool>,
    serial: u64,
}

/// How long the pointer rests at the edge before a hidden bar shows.
const SHOW_DELAY: Duration = Duration::from_millis(250);
/// How long the bar stays once the pointer left it.
const HIDE_DELAY: Duration = Duration::from_millis(600);
/// The strip of a hidden bar that takes the pointer, at its edge.
const STRIP: i32 = 2;

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
    /// The `id` of its `[Name:id]` section: `gadget.<kind>#<id>`.
    id: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Index into `gadgets`, then the gadget's own message.
    Gadget(usize, gadgets::Message),
    /// A key the compositor sent to the bar's surface, for the open
    /// popup that wants the keyboard (see [`Panel::wants_keyboard`]).
    Key(iced::keyboard::Event),
}

/// What `update` asks the daemon to do: a gadget's [`gadgets::Action`],
/// its messages routed back to the gadget through this panel.
pub type Action = gadgets::Action<Message>;

impl Panel {
    /// The bar of section `section` on `output`, at `scale`; its
    /// thickness is measured afterwards ([`Panel::resize`]).
    pub fn new(
        section: String,
        config: PanelConfig,
        shell: &Config,
        output: &OutputInfo,
        scale: f32,
    ) -> Self {
        let mut gadgets = Vec::new();
        for (slot, names) in [
            (Slot::Start, &config.items_start),
            (Slot::Center, &config.items_center),
            (Slot::End, &config.items_end),
        ] {
            let created: Vec<(&String, AnyGadget)> = names
                .iter()
                .filter_map(|name| Some((name, AnyGadget::create(name, shell, output)?)))
                .collect();
            for (name, gadget) in created {
                gadgets.push(Entry {
                    slot,
                    gadget,
                    id: instance_id(name).map(str::to_owned),
                });
            }
        }
        let hidden = config.autohide != Autohide::No;
        Self {
            section,
            output: OutputId::from(output),
            output_name: output.name.clone().unwrap_or_default(),
            room: ui::room_on(Some(output)),
            config,
            scale,
            height: 1,
            width: 1,
            gadgets,
            popups: BTreeMap::new(),
            keyboard: false,
            hidden,
            pointer: false,
            empty: false,
            timer: None,
            serial: 0,
        }
    }

    /// `panel[.top|.bottom][#instance][output=..][align=..]`, the root
    /// of every node on this bar.
    fn root<'a>(&self, theme: &'a Theme) -> Node<'a> {
        let position = match self.config.position {
            Position::Top => "top",
            Position::Bottom => "bottom",
        };
        let root = theme.root("panel", self.scale).class(position);
        with_id(root, instance_id(&self.section))
            .class_if("hidden", self.hidden)
            .attr("output", &self.output_name)
            .attr("align", self.config.align.name())
    }

    /// `popup[output=..]`, the root of a popup of this bar.
    fn popup_root<'a>(&self, theme: &'a Theme) -> Node<'a> {
        theme
            .root("popup", self.scale)
            .attr("output", &self.output_name)
    }

    /// Whether an open popup of this bar shows a text field.
    pub fn wants_keyboard(&self) -> bool {
        self.popups.values().any(|p| {
            self.gadgets
                .get(p.gadget)
                .is_some_and(|e| e.gadget.popup_keyboard())
        })
    }

    fn anchor(&self) -> Anchor {
        let edge = match self.config.position {
            Position::Top => Anchor::Top,
            Position::Bottom => Anchor::Bottom,
        };
        match self.config.align {
            Align::Stretch => edge | Anchor::Left | Anchor::Right,
            Align::Start => edge | Anchor::Left,
            Align::Center => edge,
            Align::End => edge | Anchor::Right,
        }
    }

    fn layer_size(&self) -> LayerSize {
        match self.config.align {
            Align::Stretch => LayerSize::fill_width(self.height),
            _ => LayerSize::px(self.width, self.height),
        }
    }

    /// The room it keeps windows out of: none in a corner, where
    /// layer-shell would ignore it.
    fn exclusive_zone(&self) -> i32 {
        if self.reserves() {
            self.height as i32
        } else {
            0
        }
    }

    /// Whether it keeps its thickness from the windows ([`Align`]): a
    /// bar that hides doesn't.
    pub fn reserves(&self) -> bool {
        self.config.autohide == Autohide::No
            && matches!(self.config.align, Align::Stretch | Align::Center)
    }

    /// The pointer entered the bar (`true`) or left it.
    pub fn pointer_crossed(&mut self, inside: bool) {
        self.pointer = inside;
    }

    /// Whether its output shows a workspace without windows: a `smart`
    /// bar shows then.
    pub fn set_empty(&mut self, empty: bool) {
        self.empty = empty;
    }

    /// Whether it should be hidden now: the pointer away, no popup
    /// open, and for a `smart` one some window on its desktop.
    fn wants_hidden(&self) -> bool {
        let shown = match self.config.autohide {
            Autohide::No => true,
            Autohide::Yes => false,
            Autohide::Smart => self.empty,
        };
        !shown && !self.pointer && self.popups.is_empty() && !self.keyboard
    }

    /// After a crossing or a popup closed: the timer to arm (its serial
    /// and delay) when the bar wants to show or hide, unless one for it
    /// is armed already.
    pub fn autohide_timer(&mut self) -> Option<(u64, Duration)> {
        let hide = self.wants_hidden();
        if hide == self.hidden {
            self.timer = None;
            return None;
        }
        if self.timer == Some(hide) {
            return None;
        }
        self.serial += 1;
        self.timer = Some(hide);
        Some((self.serial, if hide { HIDE_DELAY } else { SHOW_DELAY }))
    }

    /// Timer `serial` went off: whether the bar was shown or hidden.
    pub fn autohide_elapsed(&mut self, serial: u64) -> bool {
        if serial != self.serial {
            return false;
        }
        let Some(hide) = self.timer.take() else {
            return false;
        };
        if hide != self.wants_hidden() || hide == self.hidden {
            return false;
        }
        self.hidden = hide;
        true
    }

    /// The part of its surface that takes the pointer (x, y, width,
    /// height): all of it, or the strip at its edge while hidden.
    pub fn input_region(&self) -> (i32, i32, i32, i32) {
        let width = match self.config.align {
            Align::Stretch => self.room.width as i32,
            _ => self.width as i32,
        };
        let height = self.height as i32;
        match (self.hidden, self.config.position) {
            (false, _) => (0, 0, width, height),
            (true, Position::Top) => (0, 0, width, STRIP),
            (true, Position::Bottom) => (0, height - STRIP, width, STRIP),
        }
    }

    /// Its output's connector name.
    pub fn output_name(&self) -> &str {
        &self.output_name
    }

    pub fn autohide(&self) -> bool {
        self.config.autohide != Autohide::No
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    /// Whether its length is its content's, measured again after every
    /// change.
    pub fn fits(&self) -> bool {
        self.config.align != Align::Stretch
    }

    /// Where it is on an output whose rectangle is `out`.
    pub fn rect_in(&self, out: Rectangle) -> Rectangle {
        let (w, h) = match self.config.align {
            Align::Stretch => (out.width, self.height as f32),
            _ => (self.width as f32, self.height as f32),
        };
        let x = match self.config.align {
            Align::Stretch | Align::Start => out.x,
            Align::Center => out.x + ((out.width - w) / 2.0).floor(),
            Align::End => out.x + out.width - w,
        };
        let y = match self.config.position {
            Position::Top => out.y,
            Position::Bottom => out.y + out.height - h,
        };
        Rectangle::new(iced::Point::new(x, y), Size::new(w, h))
    }

    pub fn layer_settings(&self) -> NewLayerShellSettings {
        NewLayerShellSettings {
            anchor: self.anchor(),
            size: self.layer_size(),
            layer: self.config.layer,
            exclusive_zone: Some(self.exclusive_zone()),
            margin: None,
            keyboard_interactivity: KeyboardInteractivity::None,
            output_option: OutputOption::GlobalName(self.output.0),
            namespace: Some("aria-panel".to_owned()),
            ..Default::default()
        }
    }

    /// Measure the bar again (it's new, the theme or the scale changed,
    /// or its content when it [`fits`](Self::fits) it): if its size
    /// changed, the new layout (and exclusive zone) to send for its
    /// surface `bar`.
    pub fn resize(
        &mut self,
        bar: window::Id,
        m: Measuring<'_>,
    ) -> Option<(Anchor, LayerSize, i32)> {
        let (width, height) = ui::size(
            &mut m.measure.borrow_mut(),
            bar,
            self.view(m.shared),
            self.room,
        );
        if (width, height) == (self.width, self.height) {
            return None;
        }
        let thicker = height != self.height;
        (self.width, self.height) = (width, height);
        if !thicker && !self.fits() {
            return None;
        }
        Some((self.anchor(), self.layer_size(), self.exclusive_zone()))
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
    /// gadget `i`, the popups it opens and closes kept track of.
    fn lift(&mut self, i: usize, action: gadgets::Action<gadgets::Message>) -> Action {
        self.track_popups(i, &action);
        action.map(move |m| Message::Gadget(i, m))
    }

    fn track_popups(&mut self, i: usize, action: &gadgets::Action<gadgets::Message>) {
        match action {
            gadgets::Action::OpenPopup { id, .. } => {
                self.popups.insert(
                    *id,
                    Popup {
                        gadget: i,
                        placed: None,
                    },
                );
            }
            gadgets::Action::ClosePopup(id) => {
                self.popups.remove(id);
            }
            gadgets::Action::Many(actions) => {
                for a in actions {
                    self.track_popups(i, a);
                }
            }
            _ => {}
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

    /// The room around a popup's box for its shadow.
    fn popup_room(&self, theme: &Theme) -> iced::Padding {
        popup::room(self.popup_root(theme))
    }

    /// Surface size for popup `id`: its content's, measured, within the
    /// output's height left by the bar.
    fn popup_size(&self, id: window::Id, m: Measuring<'_>) -> Option<(u32, u32)> {
        let content = self.popup_view(id, m.shared)?;
        let room = Size::new(self.room.width, self.room.height - self.height as f32);
        Some(ui::size(&mut m.measure.borrow_mut(), id, content, room))
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
        m: Measuring<'_>,
    ) -> Surfaces {
        let Some(size) = self.popup_size(id, m) else {
            return Surfaces::default();
        };
        let anchor = Rectangle {
            y: 0.0,
            height: self.height as f32,
            ..widget
        };
        let side = self.popup_side();
        let room = self.popup_room(m.shared.theme);
        let Some(p) = self.popups.get_mut(&id) else {
            return Surfaces::default();
        };
        p.placed = Some((anchor, size));
        let settings = popup::settings(bar, side, anchor, size, room);
        Surfaces {
            popup: vec![(id, settings)],
            ..Surfaces::default()
        }
    }

    /// After a change of the gadgets' or the shared state: the bar
    /// takes the keyboard while a popup with a text field is open, and
    /// gives it back after (the compositor sends keys to the popup,
    /// which holds the grab); a popup whose content wants another size
    /// is placed again with it.
    pub fn sync_popups(&mut self, bar: window::Id, m: Measuring<'_>) -> Surfaces {
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
        let room = self.popup_room(m.shared.theme);
        let ids: Vec<window::Id> = self.popups.keys().copied().collect();
        for id in ids {
            let Some(size) = self.popup_size(id, m) else {
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
        let room = self.popup_room(theme);
        self.popups.iter().filter_map(move |(&id, p)| {
            let (anchor, size) = p.placed?;
            Some((id, popup::estimate(side, anchor, size, room)))
        })
    }

    pub fn view<'a>(&'a self, shared: Shared<'a>) -> Element<'a, Message> {
        let ctx = Context { shared };
        let slot = |slot: Slot| {
            let gadgets = self
                .gadgets
                .iter()
                .enumerate()
                .filter(move |(_, e)| e.slot == slot)
                .map(move |(i, e)| {
                    // Its view is the gadget's box, `gadget.<kind>[#instance]`.
                    let view = e.gadget.view(ctx).map(move |m| Message::Gadget(i, m));
                    let gadget = icssed::slot(view).kind("gadget").class(e.gadget.kind());
                    match &e.id {
                        Some(id) => gadget.named(id),
                        None => gadget,
                    }
                    .into()
                });
            icssed::row(gadgets)
                .kind("slot")
                .class(slot.name())
                .auto_width_if(self.fits())
        };
        // A bar as long as its gadgets: the config's, not the theme's.
        icssed::row![slot(Slot::Start), slot(Slot::Center), slot(Slot::End)]
            .at(self.root(shared.theme))
            .auto_width_if(self.fits())
            .into()
    }

    /// Content of the popup surface `id`: the gadget's popup view in a
    /// `popup` root's box, its shadow's room around; `None` when it
    /// isn't one of this bar's.
    pub fn popup_view<'a>(
        &'a self,
        id: window::Id,
        shared: Shared<'a>,
    ) -> Option<Element<'a, Message>> {
        let (i, e) = self
            .popups
            .get(&id)
            .and_then(|p| Some((p.gadget, self.gadgets.get(p.gadget)?)))?;
        let content = e
            .gadget
            .popup_view(Context { shared })
            .map(move |m| Message::Gadget(i, m));
        // On trial, Power's alone: its kind a class of the root,
        // `popup.power[#instance]`, no box of its own.
        if e.gadget.kind() == "power" {
            let root = self.popup_root(shared.theme).class(e.gadget.kind());
            return Some(popup::view(with_id(root, e.id.as_deref()), content));
        }
        // In a box of its own, `gadget.<kind>[#instance]`.
        let gadget = icssed::column![content]
            .kind("gadget")
            .class(e.gadget.kind());
        let gadget = match &e.id {
            Some(id) => gadget.named(id),
            None => gadget,
        };
        Some(popup::view(self.popup_root(shared.theme), gadget))
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

/// `node#id`, when there is one.
fn with_id<'a>(node: Node<'a>, id: Option<&str>) -> Node<'a> {
    match id {
        Some(id) => node.id(id),
        None => node,
    }
}

/// The `id` of a `[Name:id]` section, `None` for `[Name]`.
fn instance_id(section: &str) -> Option<&str> {
    section
        .split_once(':')
        .map(|(_, id)| id)
        .filter(|id| !id.is_empty())
}
