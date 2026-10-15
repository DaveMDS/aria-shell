//! The exit menu: a grid of buttons (lock, suspend, hibernate, logout,
//! reboot, shutdown, and whatever `[exiter] buttons` adds), each
//! running a program; the dangerous ones ask a confirmation in place,
//! auto-confirmed after a countdown. `aria-shell exiter toggle`.
//!
//! A component on a [`crate::components::dialog::Dialog`] surface, the launcher's
//! shape: the daemon owns it while it's open, sizes the surface from
//! [`Exiter::size`] (the content changes between the grid and the
//! confirmation) and carries out [`Action::Perform`]. The launcher
//! shows the same buttons as a row of icons and sends their names
//! here (`Exiter::confirming`).

use std::sync::Arc;

use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::widget::operation;
use iced::{Event, Subscription, Task, widget, window};
use icssed::Element;

use crate::config::{RawSection, Section};
use crate::locale::Locale;
use crate::shared::Shared;
use crate::time;

/// `[exiter]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExiterConfig {
    /// Buttons per row, 1..=10.
    pub columns: usize,
    /// Whether `!` commands ask before running.
    pub ask_confirm: bool,
    /// Seconds before a pending confirmation runs by itself; 0 waits
    /// forever.
    pub confirm_timeout: u64,
    /// In the order of `buttons =`.
    pub buttons: Arc<Vec<Button>>,
    /// The button focused on open (`default_button =` its name, the first one
    /// otherwise): Enter presses it, the arrows move from it. `None`
    /// only without buttons.
    pub focus: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub name: String,
    pub label: Label,
    /// Icon names to try in order (a theme may lack the first).
    pub icons: Vec<String>,
    pub command: Command,
    pub confirm: bool,
}

/// What a button says: a catalogue key for the standard ones, the
/// user's text otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    Key(&'static str),
    Text(String),
}

impl Label {
    pub fn text<'a>(&'a self, locale: &Locale) -> &'a str {
        match self {
            Self::Key(key) => locale.tr(key),
            Self::Text(text) => text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// A command line, run as `[Custom]` commands are.
    Program(String),
    /// `auto`: the compositor ends the session itself.
    Logout,
}

/// The standard buttons: name, label key, icons to try, command,
/// asks confirmation.
const STANDARD: &[(&str, &str, &[&str], &str, bool)] = &[
    (
        "lock",
        "exiter.lock",
        &["system-lock-screen-symbolic"],
        "aria-shell lock",
        false,
    ),
    (
        "suspend",
        "exiter.suspend",
        &["system-suspend-symbolic", "weather-clear-night-symbolic"],
        "systemctl suspend",
        false,
    ),
    (
        "hibernate",
        "exiter.hibernate",
        &["system-hibernate-symbolic", "drive-harddisk-symbolic"],
        "systemctl hibernate",
        false,
    ),
    (
        "logout",
        "exiter.logout",
        &["system-log-out-symbolic"],
        "auto",
        true,
    ),
    (
        "reboot",
        "exiter.reboot",
        &["system-reboot-symbolic"],
        "systemctl reboot",
        true,
    ),
    (
        "shutdown",
        "exiter.shutdown",
        &["system-shutdown-symbolic"],
        "systemctl poweroff",
        true,
    ),
];

const DEFAULT_BUTTONS: &[&str] = &[
    "lock",
    "suspend",
    "hibernate",
    "logout",
    "reboot",
    "shutdown",
];

impl Section for ExiterConfig {
    const NAME: &'static str = "exiter";

    fn from_raw(raw: &RawSection) -> Self {
        let columns = raw.u64_or("columns", 3).clamp(1, 10) as usize;
        let mut buttons = Vec::new();
        for name in raw.list_or("buttons", DEFAULT_BUTTONS) {
            let standard = STANDARD.iter().find(|(n, ..)| *n == name);
            // `name = [!]command`; a standard button has a default.
            let line = raw.get(&name).map(str::to_owned).or_else(|| {
                standard.map(|(_, _, _, cmd, confirm)| {
                    format!("{}{cmd}", if *confirm { "!" } else { "" })
                })
            });
            let Some(line) = line else {
                log::warn!("[exiter] button {name:?} has no command, skipped");
                continue;
            };
            let (confirm, line) = match line.strip_prefix('!') {
                Some(rest) => (true, rest.trim()),
                None => (false, line.as_str()),
            };
            let command = if line == "auto" {
                Command::Logout
            } else {
                Command::Program(line.to_owned())
            };
            let label = match raw.get(&format!("{name}_label")) {
                Some(text) => Label::Text(text.to_owned()),
                None => match standard {
                    Some((_, key, ..)) => Label::Key(key),
                    None => Label::Text(name.clone()),
                },
            };
            let icons = match raw.get(&format!("{name}_icon")) {
                Some(icon) => vec![icon.to_owned()],
                None => standard
                    .map(|(_, _, icons, ..)| icons.iter().map(|i| (*i).to_owned()).collect())
                    .unwrap_or_default(),
            };
            buttons.push(Button {
                name,
                label,
                icons,
                command,
                confirm,
            });
        }
        let focus = raw
            .get("default_button")
            .and_then(|name| {
                let i = buttons.iter().position(|b| b.name == name);
                if i.is_none() {
                    log::warn!(
                        "[exiter] default_button = {name}: no such button, the first focused"
                    );
                }
                i
            })
            .or((!buttons.is_empty()).then_some(0));
        Self {
            columns,
            ask_confirm: raw.bool_or("ask_confirm", true),
            confirm_timeout: raw.u64_or("confirm_timeout", 30),
            buttons: Arc::new(buttons),
            focus,
        }
    }
}

impl ExiterConfig {
    pub fn button(&self, name: &str) -> Option<&Button> {
        self.buttons.iter().find(|b| b.name == name)
    }
}

pub struct Exiter {
    config: ExiterConfig,
    confirm: Option<Confirm>,
    /// The id of the button focused on open.
    focused: widget::Id,
}

/// A button waiting for its confirmation.
struct Confirm {
    button: usize,
    /// Seconds before it runs by itself, when there is a timeout.
    left: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A click on that button, or Enter on it.
    Activate(usize),
    /// Enter, where no button took it: the confirmation's.
    Enter,
    /// Esc: back to the grid, or close.
    Escape,
    Cancel,
    Ok,
    Tick,
    /// The keyboard left the surface.
    Unfocused,
    /// A press the dialog's content took (see `dialog::content`).
    Nothing,
}

pub enum Action {
    Run(Task<Message>),
    /// Run this and close.
    Perform(Command),
    Close,
}

impl Exiter {
    pub fn new(config: ExiterConfig) -> Self {
        Self {
            config,
            confirm: None,
            focused: widget::Id::unique(),
        }
    }

    /// Open straight on the confirmation of button `name` (from the
    /// launcher's row); `None` when there's no such button.
    pub fn confirming(config: ExiterConfig, name: &str) -> Option<Self> {
        let i = config.buttons.iter().position(|b| b.name == name)?;
        let mut exiter = Self::new(config);
        exiter.confirm = Some(exiter.pending(i));
        Some(exiter)
    }

    /// Focus the `default_button =` button, once the surface exists (and back
    /// on the grid from a confirmation).
    pub fn focus(&self) -> Task<Message> {
        match self.config.focus {
            Some(_) if self.confirm.is_none() => operation::focus(self.focused.clone()),
            _ => Task::none(),
        }
    }

    fn pending(&self, button: usize) -> Confirm {
        Confirm {
            button,
            left: (self.config.confirm_timeout > 0).then_some(self.config.confirm_timeout),
        }
    }

    /// Icon names the view may draw, for the daemon to resolve.
    pub fn icon_names(&self) -> impl Iterator<Item = &str> {
        self.config
            .buttons
            .iter()
            .flat_map(|b| b.icons.iter().map(String::as_str))
    }

    /// Activate button `i`: its confirmation, or the command itself.
    fn activate(&mut self, i: usize) -> Action {
        let Some(button) = self.config.buttons.get(i) else {
            return Action::Run(Task::none());
        };
        if button.confirm && self.config.ask_confirm {
            self.confirm = Some(self.pending(i));
            Action::Run(Task::none())
        } else {
            Action::Perform(button.command.clone())
        }
    }

    fn perform_confirmed(&self) -> Action {
        match self
            .confirm
            .as_ref()
            .and_then(|c| self.config.buttons.get(c.button))
        {
            Some(button) => Action::Perform(button.command.clone()),
            None => Action::Close,
        }
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Activate(i) => self.activate(i),
            Message::Enter if self.confirm.is_some() => self.perform_confirmed(),
            Message::Enter => Action::Run(Task::none()),
            Message::Ok => self.perform_confirmed(),
            Message::Cancel => {
                self.confirm = None;
                Action::Run(self.focus())
            }
            Message::Escape => {
                if self.confirm.is_some() {
                    self.confirm = None;
                    Action::Run(self.focus())
                } else {
                    Action::Close
                }
            }
            Message::Tick => match &mut self.confirm {
                Some(Confirm {
                    left: Some(left), ..
                }) => {
                    *left = left.saturating_sub(1);
                    if *left == 0 {
                        log::info!("exiter: confirmation timed out, going ahead");
                        self.perform_confirmed()
                    } else {
                        Action::Run(Task::none())
                    }
                }
                _ => Action::Run(Task::none()),
            },
            Message::Unfocused => Action::Close,
            Message::Nothing => Action::Run(Task::none()),
        }
    }

    /// The texts of the confirmation, and the button it asks about.
    fn confirm_texts(&self, locale: &Locale) -> Option<(String, Option<String>, &Button)> {
        let confirm = self.confirm.as_ref()?;
        let button = self.config.buttons.get(confirm.button)?;
        let question = locale.fmt("exiter.confirm", &[("action", &button.label.text(locale))]);
        let countdown = confirm
            .left
            .map(|n| locale.fmt("exiter.countdown", &[("n", &n)]));
        Some((question, countdown, button))
    }

    /// The keys, the keyboard leaving (as the launcher), and the
    /// countdown's seconds. The daemon checks that `window` is ours.
    /// Its dialog's box: the grid of buttons, or a button's confirmation
    /// in its place.
    pub fn view<'a>(&'a self, shared: Shared<'a>) -> Element<'a, Message> {
        let locale = shared.locale;
        if let Some((question, countdown, button)) = self.confirm_texts(locale) {
            let mut items: Vec<Element<'a, Message>> = vec![icssed::text(question).into()];
            if let Some(countdown) = countdown {
                items.push(icssed::text(countdown).kind("countdown").into());
            }
            items.push(
                icssed::row![
                    icssed::button![icssed::text(locale.tr("exiter.cancel"))]
                        .class("cancel")
                        .on_press(Message::Cancel),
                    icssed::button![icssed::text(button.label.text(locale))]
                        .class("ok")
                        .class(&button.name)
                        .class("danger")
                        .on_press(Message::Ok),
                ]
                .kind("buttons")
                .into(),
            );
            return icssed::focus_scope(icssed::column![icssed::column(items).kind("confirm")])
                .into();
        }
        let icons: &'a dyn icssed::Icons = shared.icons;
        let cols = self.config.columns.max(1);
        let rows = self
            .config
            .buttons
            .chunks(cols)
            .enumerate()
            .map(|(r, chunk)| {
                let cells = chunk.iter().enumerate().map(|(c, button)| {
                    let i = r * cols + c;
                    let icon = match shared.icons.first_of(&button.icons) {
                        Some(icon) => icssed::icon(icon.clone()),
                        None => icssed::icon((icons, "")),
                    };
                    let button = icssed::button![icon, icssed::text(button.label.text(locale))]
                        .class(&button.name)
                        .class("flat")
                        .on_press(Message::Activate(i));
                    if self.config.focus == Some(i) {
                        button.id(self.focused.clone()).into()
                    } else {
                        button.into()
                    }
                });
                icssed::row(cells).into()
            });
        // The default button has it on open (Tab gives it back after a
        // click), the arrows move it round the grid, Enter presses it.
        icssed::focus_scope(icssed::focus_scope(icssed::column(rows)).arrows()).into()
    }

    pub fn subscription(&self) -> Subscription<(Option<window::Id>, Message)> {
        let keys = iced::event::listen_with(|event, status, window| {
            let message = match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => match key.as_ref() {
                    // A focused button presses itself.
                    Key::Named(Named::Enter) if status == iced::event::Status::Ignored => {
                        Message::Enter
                    }
                    Key::Named(Named::Escape) => Message::Escape,
                    _ => return None,
                },
                Event::Window(window::Event::Unfocused) => Message::Unfocused,
                _ => return None,
            };
            Some((Some(window), message))
        });
        let ticking = self.confirm.as_ref().is_some_and(|c| c.left.is_some());
        if ticking {
            Subscription::batch([
                keys,
                Subscription::run_with(1u32, |step| time::aligned_ticks(*step))
                    .map(|_| (None, Message::Tick)),
            ])
        } else {
            keys
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn defaults() {
        let cfg: ExiterConfig = Config::parse("").section(None);
        assert_eq!(cfg.columns, 3);
        assert!(cfg.ask_confirm);
        assert_eq!(cfg.confirm_timeout, 30);
        assert_eq!(cfg.focus, Some(0), "the first");
        let names: Vec<&str> = cfg.buttons.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, DEFAULT_BUTTONS);
        let lock = cfg.button("lock").unwrap();
        assert_eq!(lock.command, Command::Program("aria-shell lock".into()));
        assert!(!lock.confirm);
        assert_eq!(lock.label, Label::Key("exiter.lock"));
        let logout = cfg.button("logout").unwrap();
        assert_eq!(logout.command, Command::Logout);
        assert!(logout.confirm);
        assert_eq!(
            cfg.button("suspend").unwrap().icons,
            ["system-suspend-symbolic", "weather-clear-night-symbolic"]
        );
    }

    #[test]
    fn buttons_from_config() {
        let cfg: ExiterConfig = Config::parse(
            "[exiter]\ncolumns = 42\nbuttons = reboot firefox nothing lock\n\
             reboot = systemctl reboot\nfirefox = !firefox --new\nfirefox_icon = firefox\n\
             firefox_label = Browser\nlock = auto\n",
        )
        .section(None);
        assert_eq!(cfg.columns, 10);
        let names: Vec<&str> = cfg.buttons.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            ["reboot", "firefox", "lock"],
            "nothing has no command"
        );
        let reboot = cfg.button("reboot").unwrap();
        assert!(!reboot.confirm, "no ! in the config: no confirmation");
        let firefox = cfg.button("firefox").unwrap();
        assert!(firefox.confirm);
        assert_eq!(firefox.command, Command::Program("firefox --new".into()));
        assert_eq!(firefox.icons, ["firefox"]);
        assert_eq!(firefox.label, Label::Text("Browser".into()));
        assert_eq!(cfg.button("lock").unwrap().command, Command::Logout);
    }

    #[test]
    fn the_focused_button() {
        let focus = |value: &str| {
            Config::parse(&format!(
                "[exiter]
default_button = {value}
"
            ))
            .section::<ExiterConfig>(None)
            .focus
        };
        assert_eq!(focus("suspend"), Some(1));
        assert_eq!(focus(""), Some(0), "empty: the first");
        assert_eq!(focus("nope"), Some(0), "no such button: the first");
    }

    #[test]
    fn confirm_flow() {
        let cfg: ExiterConfig = Config::parse("[exiter]\nconfirm_timeout = 2\n").section(None);
        let mut e = Exiter::new(cfg.clone());
        // lock runs at once.
        assert!(matches!(
            e.update(Message::Activate(0)),
            Action::Perform(Command::Program(_))
        ));
        // reboot asks, Escape goes back, Enter confirms.
        let reboot = cfg.buttons.iter().position(|b| b.name == "reboot").unwrap();
        assert!(matches!(
            e.update(Message::Activate(reboot)),
            Action::Run(_)
        ));
        assert!(e.confirm.is_some());
        assert!(matches!(e.update(Message::Escape), Action::Run(_)));
        assert!(e.confirm.is_none());
        e.update(Message::Activate(reboot));
        assert!(matches!(
            e.update(Message::Enter),
            Action::Perform(Command::Program(_))
        ));
        // The countdown runs it by itself.
        e.update(Message::Activate(reboot));
        assert!(matches!(e.update(Message::Tick), Action::Run(_)));
        assert!(matches!(e.update(Message::Tick), Action::Perform(_)));
        // Escape on the grid closes; Enter there is a button's.
        let mut e = Exiter::new(cfg.clone());
        assert!(matches!(e.update(Message::Enter), Action::Run(_)));
        assert!(matches!(e.update(Message::Escape), Action::Close));
        // Straight on a confirmation.
        let e = Exiter::confirming(cfg.clone(), "shutdown").unwrap();
        assert_eq!(e.confirm.as_ref().unwrap().left, Some(2));
        assert!(Exiter::confirming(cfg, "nope").is_none());
        // Without ask_confirm nothing asks.
        let cfg: ExiterConfig = Config::parse("[exiter]\nask_confirm = no\n").section(None);
        let mut e = Exiter::new(cfg);
        assert!(matches!(
            e.update(Message::Activate(reboot)),
            Action::Perform(_)
        ));
    }
}
