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

use iced::{Element, Subscription, Task};
use iced_wayland_subscriber::OutputInfo;

use crate::compositor::{self, Compositor};
use crate::config::{Config, Section};
use crate::gadgets::clock::{self, Clock};
use crate::gadgets::workspaces::{self, Workspaces};

/// Daemon-owned state a gadget can read while building its view.
#[derive(Clone, Copy)]
pub struct Context<'a> {
    pub compositor: &'a Compositor,
}

/// What `update` asks the daemon to do. Like a `Task`, but it can also
/// carry requests only the daemon can fulfil.
pub enum Action<M> {
    None,
    Run(Task<M>),
    Compositor(compositor::Command),
}

impl<M: Send + 'static> Action<M> {
    pub fn map<N: Send + 'static>(self, f: impl Fn(M) -> N + Send + Sync + 'static) -> Action<N> {
        match self {
            Self::None => Action::None,
            Self::Run(task) => Action::Run(task.map(f)),
            Self::Compositor(cmd) => Action::Compositor(cmd),
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

    pub fn subscription(&self) -> Subscription<Message> {
        match self {
            Self::Clock(g) => g.subscription().map(Message::Clock),
            Self::Workspaces(g) => g.subscription().map(Message::Workspaces),
        }
    }
}
