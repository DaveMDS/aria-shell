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

use iced::widget::{Space, container};
use iced::{Element, Subscription, Task, widget, window};
use iced_wayland_subscriber::OutputInfo;

use crate::compositor::{self, Compositor};
use crate::config::{Config, Section};
use crate::gadgets::clock::{self, Clock};
use crate::gadgets::workspaces::{self, Workspaces};
use crate::icons::Icons;
use crate::theme::{Node, Theme};

/// Daemon-owned state a gadget can read while building its view.
#[derive(Clone, Copy)]
pub struct Shared<'a> {
    pub compositor: &'a Compositor,
    pub theme: &'a Theme,
    pub icons: &'a Icons,
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
    /// Open a popup surface of `size` pixels, hanging off the widget
    /// tagged `anchor`. Gadgets don't build this by hand, they call
    /// [`Popup::toggle`].
    OpenPopup {
        anchor: widget::Id,
        size: (u32, u32),
    },
    ClosePopup(window::Id),
}

/// A gadget's popup surface, as far as the gadget is concerned: the
/// widget it hangs from and whether it's open. The gadget keeps one as a
/// field and hands it out through [`Gadget::popup`]; the panel reports
/// the surface coming and going through it, whoever closed it (the
/// gadget, or the compositor on a click outside).
pub struct Popup {
    anchor: widget::Id,
    id: Option<window::Id>,
}

impl Popup {
    pub fn new() -> Self {
        Self {
            anchor: widget::Id::unique(),
            id: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.id.is_some()
    }

    /// Tag `content` as the widget the popup hangs from.
    pub fn anchor<'a, M: 'a>(&self, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
        container(content).id(self.anchor.clone()).into()
    }

    /// Close the popup if open, else open one of `size` pixels.
    pub fn toggle<M>(&mut self, size: (u32, u32)) -> Action<M> {
        match self.id.take() {
            Some(id) => Action::ClosePopup(id),
            None => Action::OpenPopup {
                anchor: self.anchor.clone(),
                size,
            },
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
        match self {
            Self::None => Action::None,
            Self::Run(task) => Action::Run(task.map(f)),
            Self::Compositor(cmd) => Action::Compositor(cmd),
            Self::OpenPopup { anchor, size } => Action::OpenPopup { anchor, size },
            Self::ClosePopup(id) => Action::ClosePopup(id),
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
}

#[derive(Clone, Debug)]
pub enum Message {
    Clock(clock::Message),
    Workspaces(workspaces::Message),
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
        }
    }

    pub fn update(&mut self, message: Message) -> Action<Message> {
        match (self, message) {
            (Self::Clock(g), Message::Clock(m)) => g.update(m).map(Message::Clock),
            (Self::Workspaces(g), Message::Workspaces(m)) => g.update(m).map(Message::Workspaces),
            _ => Action::None,
        }
    }

    pub fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        match self {
            Self::Clock(g) => g.view(ctx).map(Message::Clock),
            Self::Workspaces(g) => g.view(ctx).map(Message::Workspaces),
        }
    }

    pub fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        match self {
            Self::Clock(g) => g.popup_view(ctx).map(Message::Clock),
            Self::Workspaces(g) => g.popup_view(ctx).map(Message::Workspaces),
        }
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        match self {
            Self::Clock(g) => g.popup(),
            Self::Workspaces(g) => g.popup(),
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
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match self {
            Self::Clock(g) => g.subscription().map(Message::Clock),
            Self::Workspaces(g) => g.subscription().map(Message::Workspaces),
        }
    }
}
