//! Gadgets: the self-contained widgets a panel is made of.
//!
//! Each gadget is a plain iced component (state + `Message` + `update` +
//! `view` + `subscription`). The set is closed and compiled in, so the
//! "registry" is just [`AnyGadget`] plus a `match` in [`AnyGadget::create`].
//! Adding a gadget: one file under `gadgets/`, one variant in `AnyGadget`
//! and in [`Message`], one arm in each `match` below.
//!
//! Shared state (the compositor's workspaces, later audio, tray, ...) is
//! owned by the daemon and reaches gadgets read-only through [`Context`];
//! a gadget that wants to change it returns an [`Action`] carrying a
//! command, which the daemon executes.

use std::sync::atomic::{AtomicU64, Ordering};

use iced::widget::{Space, container};
use iced::{Element, Subscription, Task, widget, window};
use iced_wayland_subscriber::OutputInfo;

use crate::compositor::{self, Compositor};
use crate::config::{Config, Section};
use crate::gadgets::clock::{self, Clock};
use crate::gadgets::themes::{self, Themes};
use crate::gadgets::tray::{self, TrayGadget};
use crate::gadgets::workspaces::{self, Workspaces};
use crate::icons::Icons;
use crate::theme::{Node, Theme};

/// Daemon-owned state a gadget can read while building its view.
#[derive(Clone, Copy)]
pub struct Shared<'a> {
    pub compositor: &'a Compositor,
    pub theme: &'a Theme,
    pub icons: &'a Icons,
    pub tray: &'a crate::tray::Tray,
}

/// What a gadget gets in `view`: the shared state plus its own place in
/// the element tree, to derive the nodes of its widgets from
/// (`ctx.node.child("workspace")`). Derefs to [`Shared`], so
/// `ctx.compositor` and `ctx.theme` read as before.
#[derive(Clone)]
pub struct Context<'a> {
    pub shared: Shared<'a>,
    pub node: Node,
}

impl<'a> std::ops::Deref for Context<'a> {
    type Target = Shared<'a>;

    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}

/// What `update` asks the daemon to do. Like a `Task`, but it can also
/// carry requests only the daemon can fulfil.
pub enum Action<M> {
    None,
    Run(Task<M>),
    Compositor(compositor::Command),
    Tray(crate::tray::Command),
    Theme(crate::theme::Command),
    /// Open a popup surface hanging off the widget tagged `anchor`,
    /// sized by [`Gadget::popup_size`]. Gadgets don't build this by
    /// hand, they call [`Popup::toggle`].
    OpenPopup {
        anchor: widget::Id,
    },
    ClosePopup(window::Id),
    Many(Vec<Action<M>>),
}

/// A gadget's popup surface, as far as the gadget is concerned: the
/// widget it hangs from and whether it's open. The gadget keeps one as a
/// field and hands it out through [`Gadget::popup`]; the panel reports
/// the surface coming and going through it, whoever closed it (the
/// gadget, or the compositor on a click outside).
///
/// A gadget with several widgets a popup may hang from (one per tray
/// item) numbers them: [`Popup::anchor_nth`] / [`Popup::toggle_nth`].
pub struct Popup {
    /// Makes the anchor ids unique across gadgets and windows (the
    /// runtime searches every window for them).
    serial: u64,
    id: Option<window::Id>,
}

impl Popup {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            serial: NEXT.fetch_add(1, Ordering::Relaxed),
            id: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.id.is_some()
    }

    fn anchor_id(&self, n: usize) -> widget::Id {
        widget::Id::from(format!("popup-anchor-{}-{n}", self.serial))
    }

    /// Tag `content` as the widget the popup hangs from.
    pub fn anchor<'a, M: 'a>(&self, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
        self.anchor_nth(0, content)
    }

    pub fn anchor_nth<'a, M: 'a>(
        &self,
        n: usize,
        content: impl Into<Element<'a, M>>,
    ) -> Element<'a, M> {
        container(content).id(self.anchor_id(n)).into()
    }

    /// Close the popup if open, else open one.
    pub fn toggle<M>(&mut self) -> Action<M> {
        self.toggle_nth(0)
    }

    /// Close the popup if open, else open one on anchor `n`.
    pub fn toggle_nth<M>(&mut self, n: usize) -> Action<M> {
        match self.id.take() {
            Some(id) => Action::ClosePopup(id),
            None => Action::OpenPopup {
                anchor: self.anchor_id(n),
            },
        }
    }

    /// Close it if open.
    pub fn close<M>(&mut self) -> Action<M> {
        match self.id.take() {
            Some(id) => Action::ClosePopup(id),
            None => Action::None,
        }
    }

    fn opened(&mut self, id: window::Id) {
        self.id = Some(id);
    }

    fn closed(&mut self) {
        self.id = None;
    }
}

impl<M: Send + 'static> Action<M> {
    pub fn map<N: Send + 'static>(self, f: impl Fn(M) -> N + Send + Sync + 'static) -> Action<N> {
        self.map_dyn(std::sync::Arc::new(f))
    }

    /// `map` with a shared function, so `Many` can recurse without
    /// nesting closure types.
    fn map_dyn<N: Send + 'static>(
        self,
        f: std::sync::Arc<dyn Fn(M) -> N + Send + Sync>,
    ) -> Action<N> {
        match self {
            Self::None => Action::None,
            Self::Run(task) => Action::Run(task.map(move |m| f(m))),
            Self::Compositor(cmd) => Action::Compositor(cmd),
            Self::Tray(cmd) => Action::Tray(cmd),
            Self::Theme(cmd) => Action::Theme(cmd),
            Self::OpenPopup { anchor } => Action::OpenPopup { anchor },
            Self::ClosePopup(id) => Action::ClosePopup(id),
            Self::Many(actions) => {
                Action::Many(actions.into_iter().map(|a| a.map_dyn(f.clone())).collect())
            }
        }
    }
}

pub trait Gadget: Sized {
    type Config: Section;
    type Message: Clone + std::fmt::Debug + Send + 'static;

    /// `output` is the monitor the owning panel is shown on.
    fn new(config: Self::Config, output: &OutputInfo) -> Self;

    fn update(&mut self, message: Self::Message) -> Action<Self::Message>;

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Self::Message>;

    /// The gadget's [`Popup`], if it has one.
    fn popup(&mut self) -> Option<&mut Popup> {
        None
    }

    /// Content of the popup.
    fn popup_view<'a>(&'a self, _ctx: Context<'a>) -> Element<'a, Self::Message> {
        Space::new().into()
    }

    /// Size of the popup content, in pixels, from the gadget's state and
    /// the shared one (a Wayland surface needs it up front, iced can't
    /// size it from the content). Asked when the popup opens and again
    /// after any state change: a different answer resizes the surface.
    fn popup_size(&self, _ctx: Context<'_>) -> (u32, u32) {
        (1, 1)
    }

    /// The popup surface is gone, whoever closed it.
    fn popup_closed(&mut self) {}

    /// Icon names (from the icon theme) the gadget draws, for the
    /// daemon to resolve; read back with `ctx.icons.get_name`.
    fn icon_names(&self) -> Vec<String> {
        Vec::new()
    }

    /// Per-instance event source (timers, ...). The panel keys it by
    /// gadget index, so identical subscriptions on two gadgets stay
    /// distinct. Shared sources (compositor IPC, DBus) don't go here: they
    /// belong to the daemon and are read through [`Context`].
    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::none()
    }
}

/// One variant per gadget type.
pub enum AnyGadget {
    Clock(Clock),
    Workspaces(Workspaces),
    Tray(TrayGadget),
    Themes(Themes),
}

#[derive(Clone, Debug)]
pub enum Message {
    Clock(clock::Message),
    Workspaces(workspaces::Message),
    Tray(tray::Message),
    Themes(themes::Message),
}

impl AnyGadget {
    /// Instantiate the gadget named by `name`, which is a config section
    /// name: `"Clock"` or `"Clock:2"` for a second instance with its own
    /// section. Unknown names are logged and skipped.
    pub fn create(name: &str, config: &Config, output: &OutputInfo) -> Option<Self> {
        let kind = name.split(':').next().unwrap_or(name);
        match kind {
            clock::ClockConfig::NAME => {
                Some(Self::Clock(Clock::new(config.section(Some(name)), output)))
            }
            workspaces::WorkspacesConfig::NAME => Some(Self::Workspaces(Workspaces::new(
                config.section(Some(name)),
                output,
            ))),
            tray::TrayConfig::NAME => Some(Self::Tray(TrayGadget::new(
                config.section(Some(name)),
                output,
            ))),
            themes::ThemesConfig::NAME => Some(Self::Themes(Themes::new(
                config.section(Some(name)),
                output,
            ))),
            _ => {
                log::warn!("unknown gadget {name:?}");
                None
            }
        }
    }

    /// The class selectors see: `gadget.clock`, `gadget.workspaces`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Clock(_) => "clock",
            Self::Workspaces(_) => "workspaces",
            Self::Tray(_) => "tray",
            Self::Themes(_) => "themes",
        }
    }

    pub fn update(&mut self, message: Message) -> Action<Message> {
        match (self, message) {
            (Self::Clock(g), Message::Clock(m)) => g.update(m).map(Message::Clock),
            (Self::Workspaces(g), Message::Workspaces(m)) => g.update(m).map(Message::Workspaces),
            (Self::Tray(g), Message::Tray(m)) => g.update(m).map(Message::Tray),
            (Self::Themes(g), Message::Themes(m)) => g.update(m).map(Message::Themes),
            _ => Action::None,
        }
    }

    pub fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        match self {
            Self::Clock(g) => g.view(ctx).map(Message::Clock),
            Self::Workspaces(g) => g.view(ctx).map(Message::Workspaces),
            Self::Tray(g) => g.view(ctx).map(Message::Tray),
            Self::Themes(g) => g.view(ctx).map(Message::Themes),
        }
    }

    pub fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        match self {
            Self::Clock(g) => g.popup_view(ctx).map(Message::Clock),
            Self::Workspaces(g) => g.popup_view(ctx).map(Message::Workspaces),
            Self::Tray(g) => g.popup_view(ctx).map(Message::Tray),
            Self::Themes(g) => g.popup_view(ctx).map(Message::Themes),
        }
    }

    pub fn popup_size(&self, ctx: Context<'_>) -> (u32, u32) {
        match self {
            Self::Clock(g) => g.popup_size(ctx),
            Self::Workspaces(g) => g.popup_size(ctx),
            Self::Tray(g) => g.popup_size(ctx),
            Self::Themes(g) => g.popup_size(ctx),
        }
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        match self {
            Self::Clock(g) => g.popup(),
            Self::Workspaces(g) => g.popup(),
            Self::Tray(g) => g.popup(),
            Self::Themes(g) => g.popup(),
        }
    }

    pub fn popup_opened(&mut self, id: window::Id) {
        if let Some(p) = self.popup() {
            p.opened(id);
        }
    }

    pub fn popup_closed(&mut self) {
        if let Some(p) = self.popup() {
            p.closed();
        }
        match self {
            Self::Clock(g) => <Clock as Gadget>::popup_closed(g),
            Self::Workspaces(g) => <Workspaces as Gadget>::popup_closed(g),
            Self::Tray(g) => <TrayGadget as Gadget>::popup_closed(g),
            Self::Themes(g) => <Themes as Gadget>::popup_closed(g),
        }
    }

    pub fn icon_names(&self) -> Vec<String> {
        match self {
            Self::Clock(g) => g.icon_names(),
            Self::Workspaces(g) => g.icon_names(),
            Self::Tray(g) => g.icon_names(),
            Self::Themes(g) => g.icon_names(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match self {
            Self::Clock(g) => g.subscription().map(Message::Clock),
            Self::Workspaces(g) => g.subscription().map(Message::Workspaces),
            Self::Tray(g) => g.subscription().map(Message::Tray),
            Self::Themes(g) => g.subscription().map(Message::Themes),
        }
    }
}
