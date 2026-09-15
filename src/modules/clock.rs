use std::collections::HashMap;

use chrono::{DateTime, Local};

use crate::config::{ConfigSection, get_or};
use crate::module::{GadgetRunContext, Module};

/// Mirrors `aria_shell.modules.clock.ClockConfigModel`.
#[derive(Debug, Clone)]
pub struct ClockConfig {
    pub format: String,
    /// Parsed but UNUSED in this spike -- the calendar popover (Python's
    /// `AriaPopover`/`toggle_calendar`) is explicitly out of scope.
    #[allow(dead_code)]
    pub tooltip_format: String,
}

impl ConfigSection for ClockConfig {
    const SECTION: &'static str = "Clock";

    fn from_section(raw: &HashMap<String, String>) -> Self {
        Self {
            format: get_or(raw, "format", "%H:%M"),
            tooltip_format: get_or(raw, "tooltip_format", "%A %d %B %Y"),
        }
    }
}

/// Mirrors `ClockGadget`'s state (its `Gtk.Label` text), minus the
/// click-handling/popover bits (out of scope for this spike).
pub struct ClockState {
    format: String,
    text: String,
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Mirrors `ClockModule.timer_cb(instance)`.
    Tick(DateTime<Local>),
}

pub struct ClockModule;

impl Module for ClockModule {
    type Config = ClockConfig;
    type Message = Message;
    type State = ClockState;

    fn gadget_factory(ctx: GadgetRunContext<ClockConfig>) -> ClockState {
        let now = Local::now();
        ClockState {
            text: now.format(&ctx.config.format).to_string(), // "perform a
            // first update", mirrors ClockModule.gadget_factory calling
            // self.timer_cb(instance) right after construction
            format: ctx.config.format,
        }
    }

    fn update(state: &mut ClockState, msg: Message) -> iced::Task<Message> {
        match msg {
            Message::Tick(now) => {
                state.text = now.format(&state.format).to_string();
            }
        }
        iced::Task::none()
    }

    fn view(state: &ClockState) -> iced::Element<'_, Message> {
        // Mirrors `Gtk.Label`; no click handler -- the calendar popover
        // is out of scope for this spike.
        iced::widget::text(state.text.clone()).into()
    }

    fn subscription(_state: &ClockState) -> iced::Subscription<Message> {
        // The real tick subscription lives at the top level (main.rs) and
        // broadcasts to every Clock instance -- see the doc comment on
        // `Module::subscription`.
        iced::Subscription::none()
    }
}
