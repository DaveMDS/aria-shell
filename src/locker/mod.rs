//! The lock screen: one surface per output over everything, the user's
//! avatar and name, the time, a password field; the right password (or
//! Enter, with `password_prompt = no`) unlocks.
//!
//! A component the daemon owns while the session is locked, like the
//! launcher, not a gadget. The Wayland side (`ext-session-lock-v1`) is
//! the runtime's: `Message::Lock` makes it create a lock surface on
//! every output (and on any that appears meanwhile), each announced as a
//! `ShellEvent::NewShell` of type `SessionLock`; `Message::UnLock`
//! destroys them. This is one state drawn on every one of those
//! surfaces: the password typed on one monitor is the password.
//! Checking it is `pam.rs`, off the UI thread.

pub mod pam;

use std::collections::BTreeMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::widget::{Space, column, image, operation};
use iced::{Alignment, ContentFit, Element, Event, Subscription, Task, widget, window};
use iced_wayland_subscriber::OutputId;

use crate::config::{RawSection, Section};
use crate::gadget::Shared;
use crate::theme::{self, Node};
use crate::time;

/// `[locker]` section. Same keys and defaults as the Python
/// implementation.
#[derive(Debug, Clone)]
pub struct LockerConfig {
    /// Ask the password; off, Enter or the button unlocks.
    pub password_prompt: bool,
    pub show_avatar: bool,
    pub show_username: bool,
    pub show_time: bool,
    pub show_date: bool,
    pub time_format: String,
    pub date_format: String,
    /// The PAM service the password is checked with; empty:
    /// `aria-shell` when `/etc/pam.d/aria-shell` is installed, else
    /// `login`.
    pub pam_service: String,
}

impl Section for LockerConfig {
    const NAME: &'static str = "locker";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            password_prompt: raw.bool_or("password_prompt", true),
            show_avatar: raw.bool_or("show_avatar", true),
            show_username: raw.bool_or("show_username", true),
            show_time: raw.bool_or("show_time", true),
            show_date: raw.bool_or("show_date", true),
            time_format: raw.str_or("time_format", "%H:%M"),
            date_format: raw.str_or("date_format", "%A %d %B"),
            pam_service: raw
                .get("pam_service")
                .filter(|s| !s.is_empty())
                .map_or_else(|| pam::default_service().to_owned(), str::to_owned),
        }
    }
}

/// The eye button's icons: show the password, hide it again.
const PEEK_ICON: &str = "view-reveal-symbolic";
const CONCEAL_ICON: &str = "view-conceal-symbolic";

pub struct Locker {
    config: LockerConfig,
    /// The lock surfaces the runtime created, with their output once
    /// it said which.
    windows: BTreeMap<window::Id, Option<OutputId>>,
    /// The compositor confirmed the lock.
    pub locked: bool,
    password: String,
    /// The password shown in clear (the eye button).
    peek: bool,
    state: State,
    /// The login name (for PAM) and the name shown (the gecos, else
    /// the login).
    login: String,
    display_name: String,
    avatar: Option<image::Handle>,
    now: DateTime<Local>,
    /// The password field, the same on every surface: focusing it
    /// focuses it everywhere, and the compositor picks the surface
    /// that gets the keys.
    input: widget::Id,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Idle,
    /// PAM is checking the password.
    Busy,
    /// The last attempt failed, with PAM's message when it gave one.
    Failed(Option<String>),
}

#[derive(Debug, Clone)]
pub enum Message {
    Password(String),
    /// Enter, or the Unlock button.
    Submit,
    /// The eye button: show/hide the password.
    TogglePeek,
    AuthDone(Result<(), String>),
    Tick(DateTime<Local>),
}

pub enum Action {
    Run(Task<Message>),
    /// The daemon sends `Message::UnLock`.
    Unlock,
}

impl Locker {
    pub fn new(config: LockerConfig) -> Self {
        let (login, gecos, home) = user_info();
        let display_name = if gecos.is_empty() {
            login.clone()
        } else {
            gecos
        };
        let avatar = config
            .show_avatar
            .then(|| avatar_path(&home, &login))
            .flatten()
            .and_then(|path| load_avatar(&path));
        Self {
            config,
            windows: BTreeMap::new(),
            locked: false,
            password: String::new(),
            peek: false,
            state: State::Idle,
            login,
            display_name,
            avatar,
            now: Local::now(),
            input: widget::Id::unique(),
        }
    }

    /// The runtime created a lock surface.
    pub fn add_window(&mut self, id: window::Id) {
        self.windows.insert(id, None);
    }

    /// ... and said which output it's on.
    pub fn set_output(&mut self, id: window::Id, output: OutputId) {
        if let Some(slot) = self.windows.get_mut(&id) {
            *slot = Some(output);
        }
    }

    /// A lock surface went away; whether it was one.
    pub fn remove_window(&mut self, id: window::Id) -> bool {
        self.windows.remove(&id).is_some()
    }

    pub fn has_window(&self, id: window::Id) -> bool {
        self.windows.contains_key(&id)
    }

    /// Every lock surface with its output, for `debug surfaces`.
    pub fn windows(&self) -> impl Iterator<Item = (window::Id, OutputId)> + '_ {
        self.windows
            .iter()
            .filter_map(|(id, out)| out.map(|o| (*id, o)))
    }

    /// Give the password field the keyboard, once a surface exists.
    pub fn focus(&self) -> Task<Message> {
        operation::focus(self.input.clone())
    }

    /// Icon names the view may draw, for the daemon to resolve.
    pub fn icon_names(&self) -> impl Iterator<Item = &'static str> {
        (self.config.show_avatar && self.avatar.is_none())
            .then_some("avatar-default")
            .into_iter()
            .chain(
                self.config
                    .password_prompt
                    .then_some([PEEK_ICON, CONCEAL_ICON])
                    .into_iter()
                    .flatten(),
            )
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Password(p) => {
                if self.state != State::Busy {
                    self.password = p;
                    self.state = State::Idle;
                }
                Action::Run(Task::none())
            }
            Message::TogglePeek => {
                // The click took the keyboard from the field: give it back.
                self.peek = !self.peek;
                Action::Run(self.focus())
            }
            Message::Submit => {
                if !self.config.password_prompt {
                    log::info!("unlocking without credentials (password_prompt = no)");
                    return Action::Unlock;
                }
                if self.state == State::Busy {
                    return Action::Run(Task::none());
                }
                self.state = State::Busy;
                let password = std::mem::take(&mut self.password);
                let user = self.login.clone();
                let service = self.config.pam_service.clone();
                log::info!("checking the password with PAM ({service})");
                Action::Run(Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            pam::authenticate(&service, &user, password)
                        })
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()))
                    },
                    Message::AuthDone,
                ))
            }
            Message::AuthDone(Ok(())) => {
                log::info!("password accepted");
                Action::Unlock
            }
            Message::AuthDone(Err(e)) => {
                // PAM's own words when it had some (an account locked
                // by `pam_faillock` says so; libpam translates them
                // itself), else ours, at view time.
                log::warn!("password refused: {e}");
                let text = (e != "Authentication failure" && !e.is_empty()).then_some(e);
                self.state = State::Failed(text);
                Action::Run(self.focus())
            }
            Message::Tick(now) => {
                self.now = now;
                Action::Run(Task::none())
            }
        }
    }

    /// The content of one lock surface: the daemon centres it in the
    /// `locker` root container of that output.
    pub fn view<'a>(&'a self, shared: Shared<'a>, root: &Node) -> Element<'a, Message> {
        let theme = shared.theme;
        let node = root.child("box");
        let px = |l: Option<theme::Length>| match l {
            Some(theme::Length::Px(px)) => Some(px),
            _ => None,
        };
        let mut items: Vec<Element<'a, Message>> = Vec::new();
        if self.config.show_avatar {
            let n = node.child("avatar");
            let s = theme.resolve(&n);
            let size = px(s.height.or(s.width)).unwrap_or(128.0);
            let picture: Element<'a, Message> = match &self.avatar {
                Some(handle) => image(handle.clone())
                    .width(size)
                    .height(size)
                    .content_fit(ContentFit::Cover)
                    .border_radius(s.border_radius)
                    .into(),
                None => match shared.icons.get_name("avatar-default", None) {
                    Some(icon) => icon.view(size, s.color),
                    None => Space::new().width(size).height(size).into(),
                },
            };
            items.push(theme.container(&n, picture).into());
        }
        if self.config.show_username {
            let n = node.child("username");
            items.push(
                theme
                    .container(&n, theme.text(&n, &self.display_name))
                    .into(),
            );
        }
        if self.config.show_time {
            let n = node.child("time");
            let text = shared.locale.date(&self.now, &self.config.time_format);
            items.push(theme.container(&n, theme.text(&n, text)).into());
        }
        if self.config.show_date {
            let n = node.child("date");
            let text = shared.locale.date(&self.now, &self.config.date_format);
            items.push(theme.container(&n, theme.text(&n, text)).into());
        }
        let busy = self.state == State::Busy;
        let auth = node
            .child("auth")
            .class_if("error", matches!(self.state, State::Failed(_)))
            .class_if("busy", busy);
        let mut fields: Vec<Element<'a, Message>> = Vec::new();
        if self.config.password_prompt {
            let field = auth.child("field");
            let n = field.child("input");
            let mut input = theme
                .text_input(
                    &n,
                    shared.locale.tr("locker.enter_password"),
                    &self.password,
                )
                .id(self.input.clone())
                .secure(!self.peek)
                .on_submit(Message::Submit);
            if !busy {
                input = input.on_input(Message::Password);
            }
            // The eye: the password in clear while it's on.
            let peek = field.child("peek").class_if("on", self.peek);
            let icon_node = peek.child("icon");
            let icon_style = theme.resolve(&icon_node);
            let name = if self.peek { CONCEAL_ICON } else { PEEK_ICON };
            let eye: Element<'a, Message> =
                match (shared.icons.get_name(name, None), px(icon_style.height)) {
                    (Some(icon), Some(size)) => theme
                        .container(&icon_node, icon.view(size, icon_style.color))
                        .into(),
                    _ => theme
                        .text(
                            &icon_node,
                            shared.locale.tr(if self.peek {
                                "locker.hide_password"
                            } else {
                                "locker.show_password"
                            }),
                        )
                        .into(),
                };
            let eye = theme
                .button(&peek, eye)
                .on_press_maybe((!busy).then_some(Message::TogglePeek));
            fields.push(
                theme
                    .row(&field, [theme.tag(&n, input).into(), eye.into()])
                    .align_y(Alignment::Center)
                    .into(),
            );
        }
        let message = match &self.state {
            State::Failed(Some(text)) => Some(text.as_str()),
            State::Failed(None) => Some(shared.locale.tr("locker.auth_failed")),
            State::Busy => Some(shared.locale.tr("locker.unlocking")),
            State::Idle => None,
        };
        if let Some(text) = message {
            let n = auth.child("message");
            fields.push(theme.container(&n, theme.text(&n, text)).into());
        }
        let button = auth.child("button");
        fields.push(
            theme
                .button(
                    &button,
                    theme.text(&button.child("text"), shared.locale.tr("locker.unlock")),
                )
                .on_press_maybe((!busy).then_some(Message::Submit))
                .into(),
        );
        let auth_gap = theme.resolve(&auth).gap;
        items.push(
            theme
                .container(
                    &auth,
                    column(fields).spacing(auth_gap).align_x(Alignment::Center),
                )
                .into(),
        );
        let gap = theme.resolve(&node).gap;
        theme
            .container(&node, column(items).spacing(gap).align_x(Alignment::Center))
            .into()
    }

    /// The clock tick, when a time or a date is shown: every minute, or
    /// every second when a format shows them. Without a password field,
    /// Enter anywhere is the button (the field's own `on_submit`
    /// otherwise).
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = Vec::new();
        if self.config.show_time || self.config.show_date {
            let seconds = time::shows_seconds(&self.config.time_format)
                || time::shows_seconds(&self.config.date_format);
            let step = if seconds { 1 } else { 60 };
            subs.push(
                Subscription::run_with(step, |step| time::aligned_ticks(*step)).map(Message::Tick),
            );
        }
        if !self.config.password_prompt {
            subs.push(iced::event::listen_with(
                |event, _status, _window| match event {
                    Event::Keyboard(keyboard::Event::KeyPressed { key, .. })
                        if key.as_ref() == Key::Named(Named::Enter) =>
                    {
                        Some(Message::Submit)
                    }
                    _ => None,
                },
            ));
        }
        Subscription::batch(subs)
    }
}

/// The current user's login, gecos (its first field) and home, from
/// the passwd database (`$USER` / `$HOME` when it doesn't answer).
fn user_info() -> (String, String, String) {
    let mut passwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0u8; 16 * 1024];
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: the buffer outlives the call and `found` points into it
    // (or is null) as `getpwuid_r` documents.
    let text = |p: *const libc::c_char| unsafe {
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    };
    unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut passwd,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut found,
        );
    }
    if found.is_null() {
        log::warn!("getpwuid_r found no entry for uid {}", unsafe {
            libc::getuid()
        });
        return (
            std::env::var("USER").unwrap_or_default(),
            String::new(),
            std::env::var("HOME").unwrap_or_default(),
        );
    }
    let gecos = text(passwd.pw_gecos);
    let gecos = gecos.split(',').next().unwrap_or("").trim().to_owned();
    (text(passwd.pw_name), gecos, text(passwd.pw_dir))
}

/// The user's picture, where desktops keep it: `~/.face`,
/// `~/.face.icon`, then AccountsService's copy.
fn avatar_path(home: &str, login: &str) -> Option<PathBuf> {
    let home = Path::new(home);
    [
        home.join(".face"),
        home.join(".face.icon"),
        Path::new("/var/lib/AccountsService/icons").join(login),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// The picture decoded here, not by iced from the path: `~/.face` has
/// no extension and `image::open` (what `Handle::from_path` ends in)
/// only trusts the extension; the content says what it is.
fn load_avatar(path: &Path) -> Option<image::Handle> {
    let bytes = std::fs::read(path)
        .inspect_err(|e| log::warn!("cannot read the avatar {}: {e}", path.display()))
        .ok()?;
    let decoded = ::image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .decode()
        .inspect_err(|e| log::warn!("cannot decode the avatar {}: {e}", path.display()))
        .ok()?
        .into_rgba8();
    let (w, h) = decoded.dimensions();
    log::debug!("avatar {} ({w}x{h})", path.display());
    Some(image::Handle::from_rgba(w, h, decoded.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn config_defaults_and_keys() {
        let cfg: LockerConfig = Config::parse("").section(None);
        assert!(cfg.password_prompt && cfg.show_avatar && cfg.show_time);
        assert_eq!(cfg.time_format, "%H:%M");
        assert_eq!(cfg.date_format, "%A %d %B");
        assert!(cfg.pam_service == "aria-shell" || cfg.pam_service == "login");
        let cfg: LockerConfig = Config::parse(
            "[locker]\npassword_prompt = no\nshow_avatar = no\ntime_format = %H:%M:%S\npam_service = swaylock\n",
        )
        .section(None);
        assert!(!cfg.password_prompt && !cfg.show_avatar && cfg.show_username);
        assert_eq!(cfg.time_format, "%H:%M:%S");
        assert_eq!(cfg.pam_service, "swaylock");
    }

    #[test]
    fn avatar_lookup() {
        let dir = std::env::temp_dir().join(format!("aria-locker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let home = dir.to_str().unwrap();
        assert_eq!(avatar_path(home, "nobody-such-user"), None);
        std::fs::write(dir.join(".face.icon"), b"x").unwrap();
        assert_eq!(avatar_path(home, "x"), Some(dir.join(".face.icon")));
        std::fs::write(dir.join(".face"), b"x").unwrap();
        assert_eq!(avatar_path(home, "x"), Some(dir.join(".face")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn user_info_answers() {
        let (login, _, home) = user_info();
        assert!(!login.is_empty());
        assert!(!home.is_empty());
    }
}
