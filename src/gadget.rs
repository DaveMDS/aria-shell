//! Gadgets: the self-contained widgets a panel is made of.
//!
//! Each gadget is a plain iced component (state + `Message` + `update` +
//! `view` + `subscription`). The set is closed and compiled in, so the
//! "registry" is just [`AnyGadget`] plus a `match` in [`AnyGadget::create`].
//! Adding a gadget: one file under `gadgets/`, one variant in `AnyGadget`
//! and in [`Message`], one arm in each `match` below.

use iced::{Element, Subscription, Task};
use iced_wayland_subscriber::OutputInfo;

use crate::config::{Config, Section};
use crate::gadgets::clock::{self, Clock};

pub trait Gadget: Sized {
    type Config: Section;
    type Message: Clone + std::fmt::Debug + Send + 'static;

    /// `output` is the monitor the owning panel is shown on.
    fn new(config: Self::Config, output: &OutputInfo) -> Self;

    fn update(&mut self, message: Self::Message) -> Task<Self::Message>;

    fn view(&self) -> Element<'_, Self::Message>;

    /// Per-instance event source (timers, IPC sockets, ...). The panel
    /// keys it by gadget index, so identical subscriptions on two gadgets
    /// stay distinct.
    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::none()
    }
}

/// One variant per gadget type.
pub enum AnyGadget {
    Clock(Clock),
}

#[derive(Clone, Debug)]
pub enum Message {
    Clock(clock::Message),
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
            _ => {
                log::warn!("unknown gadget {name:?}");
                None
            }
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match (self, message) {
            (Self::Clock(g), Message::Clock(m)) => g.update(m).map(Message::Clock),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        match self {
            Self::Clock(g) => g.view().map(Message::Clock),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match self {
            Self::Clock(g) => g.subscription().map(Message::Clock),
        }
    }
}
