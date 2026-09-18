//! The exit menu: a grid of buttons (lock, suspend, hibernate, logout,
//! reboot, shutdown, and whatever `[exiter] buttons` adds), each
//! running a program; the dangerous ones ask a confirmation in place,
//! auto-confirmed after a countdown. `aria-shell exiter toggle`.
//!
//! A component on a [`crate::dialog::Dialog`] surface, the launcher's
//! shape: the daemon owns it while it's open, sizes the surface from
//! [`Exiter::size`] (the content changes between the grid and the
//! confirmation) and carries out [`Action::Perform`]. The launcher
//! shows the same buttons as a row of icons and sends their names
//! here (`Exiter::confirming`).

use std::sync::Arc;

use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::widget::{Space, column, container, row};
use iced::{Alignment, Element, Event, Length, Subscription, Task, window};

use crate::config::{RawSection, Section};
use crate::gadget::Shared;
use crate::locale::Locale;
use crate::theme::{self, Node, Theme};
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
        Self {
            columns,
            ask_confirm: raw.bool_or("ask_confirm", true),
            confirm_timeout: raw.u64_or("confirm_timeout", 30),
            buttons: Arc::new(buttons),
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
    selected: usize,
    confirm: Option<Confirm>,
    node: Node,
}

/// A button waiting for its confirmation.
struct Confirm {
    button: usize,
    /// Seconds before it runs by itself, when there is a timeout.
    left: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A click on that button.
    Activate(usize),
    Left,
    Right,
    Up,
    Down,
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
            selected: 0,
            confirm: None,
            node: Node::root("exiter"),
        }
    }

    /// Open straight on the confirmation of button `name` (from the
    /// launcher's row); `None` when there's no such button.
    pub fn confirming(config: ExiterConfig, name: &str) -> Option<Self> {
        let i = config.buttons.iter().position(|b| b.name == name)?;
        let mut exiter = Self::new(config);
        exiter.selected = i;
        exiter.confirm = Some(exiter.pending(i));
        Some(exiter)
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
        self.selected = i;
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

    fn step(&mut self, delta: isize) -> Action {
        let n = self.config.buttons.len() as isize;
        if n > 0 && self.confirm.is_none() {
            self.selected = (self.selected as isize + delta).rem_euclid(n) as usize;
        }
        Action::Run(Task::none())
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Activate(i) => self.activate(i),
            Message::Left => self.step(-1),
            Message::Right => self.step(1),
            Message::Up => self.step(-(self.config.columns as isize)),
            Message::Down => self.step(self.config.columns as isize),
            Message::Enter => {
                if self.confirm.is_some() {
                    self.perform_confirmed()
                } else {
                    self.activate(self.selected)
                }
            }
            Message::Ok => self.perform_confirmed(),
            Message::Cancel => {
                self.confirm = None;
                Action::Run(Task::none())
            }
            Message::Escape => {
                if self.confirm.is_some() {
                    self.confirm = None;
                    Action::Run(Task::none())
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

    /// The icon of a button at `size`: the first name the theme has,
    /// else the first name (which resolves to the generic fallback).
    fn icon<'a>(
        &self,
        shared: &Shared<'a>,
        button: &Button,
        node: &Node,
        size: f32,
    ) -> Element<'a, Message> {
        let style = shared.theme.resolve(node);
        button
            .icons
            .iter()
            .find(|name| shared.icons.has_name(name))
            .or(button.icons.first())
            .and_then(|name| shared.icons.get_name(name, None))
            .map(|icon| icon.view(size, style.color))
            .unwrap_or_else(|| Space::new().width(size).height(size).into())
    }

    /// The grid's cell: the icon's size and the widest label.
    fn cell(&self, theme: &Theme, locale: &Locale) -> (f32, f32) {
        let b = self.node.child("button");
        let bs = theme.resolve(&b);
        let icon = px(theme.resolve(&b.child("icon")).height).unwrap_or(48.0);
        let t = b.child("text");
        let label_h = theme.line_height(&t);
        let label_w = self
            .config
            .buttons
            .iter()
            .map(|button| theme.measure(&t, button.label.text(locale)).width)
            .fold(0.0, f32::max);
        (
            icon.max(label_w) + bs.padding.left + bs.padding.right,
            icon + bs.gap + label_h + bs.padding.top + bs.padding.bottom,
        )
    }

    /// The texts of the confirmation, and their node.
    fn confirm_texts(&self, locale: &Locale) -> Option<(String, Option<String>, &Button)> {
        let confirm = self.confirm.as_ref()?;
        let button = self.config.buttons.get(confirm.button)?;
        let question = locale.fmt("exiter.confirm", &[("action", &button.label.text(locale))]);
        let countdown = confirm
            .left
            .map(|n| locale.fmt("exiter.countdown", &[("n", &n)]));
        Some((question, countdown, button))
    }

    /// The surface size for the current content, the root's padding
    /// and border included.
    pub fn size(&self, theme: &Theme, locale: &Locale) -> (u32, u32) {
        let s = theme.resolve(&self.node);
        let chrome_w = s.padding.left + s.padding.right + 2.0 * s.border_width;
        let chrome_h = s.padding.top + s.padding.bottom + 2.0 * s.border_width;
        let (w, h) = match self.confirm_texts(locale) {
            Some((question, countdown, button)) => {
                let c = self.node.child("confirm");
                let cs = theme.resolve(&c);
                let t = c.child("text");
                let mut w = theme.measure(&t, &question).width;
                let mut h = theme.line_height(&t);
                if let Some(countdown) = &countdown {
                    let cd = c.child("countdown");
                    w = w.max(theme.measure(&cd, countdown).width);
                    h += cs.gap + theme.line_height(&cd);
                }
                let b = c.child("button");
                let bs = theme.resolve(&b);
                let bt = b.child("text");
                let button_w = |text: &str| {
                    theme.measure(&bt, text).width + bs.padding.left + bs.padding.right
                };
                let buttons_w = button_w(locale.tr("exiter.cancel"))
                    + cs.gap
                    + button_w(button.label.text(locale));
                w = w.max(buttons_w);
                h += cs.gap + theme.line_height(&bt) + bs.padding.top + bs.padding.bottom;
                (
                    w + cs.padding.left + cs.padding.right,
                    h + cs.padding.top + cs.padding.bottom,
                )
            }
            None => {
                let n = self.config.buttons.len().max(1);
                let cols = self.config.columns.min(n);
                let rows = n.div_ceil(cols);
                let (cw, ch) = self.cell(theme, locale);
                (
                    cols as f32 * cw + (cols - 1) as f32 * s.gap,
                    rows as f32 * ch + (rows - 1) as f32 * s.gap,
                )
            }
        };
        (
            (w + chrome_w).ceil().max(1.0) as u32,
            (h + chrome_h).ceil().max(1.0) as u32,
        )
    }

    /// The content: the daemon's `exiter` root container applies the
    /// root's padding.
    pub fn view<'a>(&'a self, shared: Shared<'a>) -> Element<'a, Message> {
        let theme = shared.theme;
        let locale = shared.locale;
        let s = theme.resolve(&self.node);
        if let Some((question, countdown, button)) = self.confirm_texts(locale) {
            let c = self.node.child("confirm");
            let cs = theme.resolve(&c);
            let t = c.child("text");
            let mut items: Vec<Element<'a, Message>> =
                vec![theme.container(&t, theme.text(&t, question)).into()];
            if let Some(countdown) = countdown {
                let cd = c.child("countdown");
                items.push(theme.container(&cd, theme.text(&cd, countdown)).into());
            }
            let cancel = c.child("button").class("cancel");
            let ok = c.child("button").class("ok").class(button.name.clone());
            items.push(
                row![
                    theme
                        .button(
                            &cancel,
                            theme.text(&cancel.child("text"), locale.tr("exiter.cancel"))
                        )
                        .on_press(Message::Cancel),
                    theme
                        .button(
                            &ok,
                            theme.text(&ok.child("text"), button.label.text(locale))
                        )
                        .on_press(Message::Ok),
                ]
                .spacing(cs.gap)
                .into(),
            );
            return theme
                .container(&c, column(items).spacing(cs.gap).align_x(Alignment::Center))
                .into();
        }
        let (cw, ch) = self.cell(theme, locale);
        let icon_size = px(theme
            .resolve(&self.node.child("button").child("icon"))
            .height)
        .unwrap_or(48.0);
        let cols = self.config.columns.max(1);
        let rows = self
            .config
            .buttons
            .chunks(cols)
            .enumerate()
            .map(|(r, chunk)| {
                let cells = chunk.iter().enumerate().map(|(c, button)| {
                    let i = r * cols + c;
                    let b = self
                        .node
                        .child("button")
                        .class(button.name.clone())
                        .class_if("selected", i == self.selected);
                    let bs = theme.resolve(&b);
                    let icon = self.icon(&shared, button, &b.child("icon"), icon_size);
                    let text = theme.text(&b.child("text"), button.label.text(locale));
                    // The cell is the widest label's: centre the content in it.
                    let content = container(
                        column![icon, text]
                            .spacing(bs.gap)
                            .align_x(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::Center);
                    theme
                        .button(&b, content)
                        .width(Length::Fixed(cw))
                        .height(Length::Fixed(ch))
                        .on_press(Message::Activate(i))
                        .into()
                });
                row(cells).spacing(s.gap).into()
            });
        column(rows).spacing(s.gap).into()
    }

    /// The keys, the keyboard leaving (as the launcher), and the
    /// countdown's seconds. The daemon checks that `window` is ours.
    pub fn subscription(&self) -> Subscription<(Option<window::Id>, Message)> {
        let keys = iced::event::listen_with(|event, _status, window| {
            let message = match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => match key.as_ref() {
                    Key::Named(Named::ArrowLeft) => Message::Left,
                    Key::Named(Named::ArrowRight) => Message::Right,
                    Key::Named(Named::ArrowUp) => Message::Up,
                    Key::Named(Named::ArrowDown) => Message::Down,
                    Key::Named(Named::Enter) => Message::Enter,
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

fn px(l: Option<theme::Length>) -> Option<f32> {
    match l {
        Some(theme::Length::Px(px)) => Some(px),
        _ => None,
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
        // Escape on the grid closes; arrows wrap.
        let mut e = Exiter::new(cfg.clone());
        assert!(matches!(e.update(Message::Escape), Action::Close));
        e.update(Message::Left);
        assert_eq!(e.selected, 5);
        e.update(Message::Down);
        assert_eq!(e.selected, 2);
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
