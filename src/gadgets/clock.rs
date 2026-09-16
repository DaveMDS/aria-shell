//! Clock gadget: the current time, formatted with a `strftime` pattern.
//! A click opens a month calendar in a popup.

use std::time::Duration;

use chrono::{DateTime, Local, Timelike};
use iced::futures::stream;
use iced::{Element, Subscription};
use iced_wayland_subscriber::OutputInfo;

use crate::config::{RawSection, Section};
use crate::gadget::{Action, Context, Gadget, Popup};
use crate::widgets::calendar::{self, Calendar};

/// `[Clock]` section. Same keys and defaults as the Python implementation.
#[derive(Debug, Clone)]
pub struct ClockConfig {
    /// strftime pattern, see
    /// <https://docs.rs/chrono/latest/chrono/format/strftime/index.html>
    pub format: String,
}

impl Section for ClockConfig {
    const NAME: &'static str = "Clock";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            format: raw.str_or("format", "%H:%M"),
        }
    }
}

pub struct Clock {
    config: ClockConfig,
    now: DateTime<Local>,
    popup: Popup,
    calendar: Calendar,
}

#[derive(Clone, Debug)]
pub enum Message {
    Tick(DateTime<Local>),
    Toggle,
    Calendar(calendar::Message),
}

impl Gadget for Clock {
    type Config = ClockConfig;
    type Message = Message;

    fn new(config: ClockConfig, _output: &OutputInfo) -> Self {
        let now = Local::now();
        Self {
            config,
            now,
            popup: Popup::new(),
            calendar: Calendar::new(now.date_naive()),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Tick(now) => self.now = now,
            Message::Toggle => {
                if !self.popup.is_open() {
                    self.calendar.show(self.now.date_naive());
                }
                return self.popup.toggle();
            }
            Message::Calendar(m) => self.calendar.update(m),
        }
        Action::None
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let button = ctx.node.child("button");
        let label = ctx.theme.text(
            &button.child("text"),
            self.now.format(&self.config.format).to_string(),
        );
        self.popup
            .anchor(ctx.theme.button(&button, label).on_press(Message::Toggle))
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_size(&self, _ctx: Context<'_>) -> (u32, u32) {
        Calendar::SIZE
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        self.calendar
            .view(
                self.now.date_naive(),
                ctx.theme,
                &ctx.node.child("calendar"),
            )
            .map(Message::Calendar)
    }

    fn subscription(&self) -> Subscription<Message> {
        // Tick once per second only when seconds are displayed, otherwise
        // once per minute; either way aligned to the wall-clock boundary
        // so the displayed value never lags or skips.
        let step = if shows_seconds(&self.config.format) {
            1
        } else {
            60
        };
        Subscription::run_with(step, |step| aligned_ticks(*step)).map(Message::Tick)
    }
}

/// Crude but sufficient: any specifier that renders seconds or finer.
fn shows_seconds(format: &str) -> bool {
    [
        "%S", "%T", "%X", "%s", "%f", "%.f", "%3f", "%6f", "%9f", "%c", "%+",
    ]
    .iter()
    .any(|spec| format.contains(spec))
}

/// Yields the current time every `step` seconds, on the boundary.
fn aligned_ticks(step: u32) -> impl stream::Stream<Item = DateTime<Local>> {
    stream::unfold((), move |()| async move {
        let now = Local::now();
        let elapsed = now.second() % step;
        let wait = Duration::from_secs(u64::from(step - elapsed))
            .saturating_sub(Duration::from_nanos(u64::from(now.nanosecond())));
        tokio::time::sleep(wait).await;
        Some((Local::now(), ()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_detection() {
        assert!(shows_seconds("%H:%M:%S"));
        assert!(shows_seconds("%T"));
        assert!(!shows_seconds("%H:%M"));
        assert!(!shows_seconds("%e %b %Y  %H:%M"));
    }
}
