//! The notification daemon: `org.freedesktop.Notifications` on the
//! session bus, and the notifications it currently shows.
//!
//! One [`Notifications`] lives in the daemon, shaped like `Tray`: the
//! [`Notifications::subscription`] is the bus connection serving the
//! interface (`dbus.rs`), the [`Event`]s it yields go through
//! [`Notifications::apply`], the daemon shows one layer surface per
//! [`Notification`] (`toast.rs` draws and measures it) and acts on the
//! user's clicks with a [`Command`] run by [`Notifications::run`], which
//! emits the `NotificationClosed` / `ActionInvoked` signals.
//!
//! Reference: <https://specifications.freedesktop.org/notification-spec/latest/>

mod dbus;
pub mod toast;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use iced::{Subscription, Task};
use zbus::Connection;

use crate::config::{RawSection, Section};
use crate::icons::Icon;

/// `[Notifications]` section: the daemon's settings and the bar
/// gadget's, together (as `[Tray]` is the one place for the tray).
#[derive(Debug, Clone)]
pub struct NotificationsConfig {
    pub enabled: bool,
    /// Seconds a notification stays when the app leaves it to us
    /// (`expire_timeout = -1`, what most send).
    pub duration: u64,
    pub position: Position,
    /// How many notifications the history keeps (0: none).
    pub history: usize,
    /// Icon names (from the icon theme) for the gadget's bell, and for
    /// it while do-not-disturb is on.
    pub icon: String,
    pub dnd_icon: String,
}

impl Section for NotificationsConfig {
    const NAME: &'static str = "Notifications";

    fn from_raw(raw: &RawSection) -> Self {
        let position = match raw.get("position") {
            None => Position::default(),
            Some(p) => Position::parse(p).unwrap_or_else(|| {
                log::warn!("invalid position {p:?} for notifications, using top-right");
                Position::default()
            }),
        };
        Self {
            enabled: raw.bool_or("enabled", true),
            duration: raw.u64_or("duration", 20).max(1),
            position,
            history: raw.u64_or("history", 50) as usize,
            icon: raw.str_or("icon", "preferences-system-notifications-symbolic"),
            dnd_icon: raw.str_or("dnd_icon", "notifications-disabled-symbolic"),
        }
    }
}

/// The screen corner (or edge middle) the notifications stack from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    TopLeft,
    #[default]
    TopRight,
    TopCenter,
    BottomLeft,
    BottomRight,
    BottomCenter,
}

impl Position {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "top-left" => Self::TopLeft,
            "top-right" => Self::TopRight,
            "top-center" => Self::TopCenter,
            "bottom-left" => Self::BottomLeft,
            "bottom-right" => Self::BottomRight,
            "bottom-center" => Self::BottomCenter,
            _ => return None,
        })
    }

    pub fn is_top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopRight | Self::TopCenter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

impl Urgency {
    /// The class the toast's root node carries.
    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::Critical => "critical",
        }
    }
}

/// Why a notification went away, as the `NotificationClosed` signal
/// reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Expired = 1,
    Dismissed = 2,
    /// `CloseNotification` from an app.
    Closed = 3,
}

/// The icon an app asked for: a theme icon name or a file (`app_icon`,
/// or the `image-path` hint which wins over it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconSource {
    Name(String),
    Path(PathBuf),
}

impl IconSource {
    /// `app_icon` / `image-path` as apps send them: an absolute path, a
    /// `file://` URI or an icon name; empty means none.
    pub fn parse(s: &str) -> Option<Self> {
        if s.is_empty() {
            None
        } else if let Some(path) = s.strip_prefix("file://") {
            Some(Self::Path(PathBuf::from(path)))
        } else if s.starts_with('/') {
            Some(Self::Path(PathBuf::from(s)))
        } else {
            Some(Self::Name(s.to_owned()))
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Name(n) => Some(n),
            Self::Path(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Notification {
    /// What the app got back from `Notify`, and what the signals name.
    pub id: u32,
    /// Distinguishes one `Notify` from the one replacing it, for the
    /// expiry timer of the first not to close the second.
    pub serial: u64,
    pub app_name: String,
    pub summary: String,
    /// Plain text: the markup the spec allows is stripped.
    pub body: String,
    pub icon: Option<IconSource>,
    /// The `image-data` hint, as an iced handle; wins over `icon`.
    pub image: Option<Icon>,
    pub urgency: Urgency,
    /// `(key, label)` pairs; the `default` one is what a click on the
    /// notification invokes and has no button.
    pub actions: Vec<(String, String)>,
    pub timeout: Timeout,
}

/// The `expire_timeout` argument: `-1` leaves it to the server, `0`
/// means never, else milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timeout {
    Default,
    Never,
    After(Duration),
}

impl Timeout {
    pub fn parse(expire_timeout: i32) -> Self {
        match expire_timeout {
            i32::MIN..=-1 => Self::Default,
            0 => Self::Never,
            ms => Self::After(Duration::from_millis(ms as u64)),
        }
    }
}

impl Notification {
    /// The actions shown as buttons.
    pub fn buttons(&self) -> impl Iterator<Item = &(String, String)> {
        self.actions.iter().filter(|(key, _)| key != "default")
    }

    pub fn has_default_action(&self) -> bool {
        self.actions.iter().any(|(key, _)| key == "default")
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    /// The session bus is up; signals can be emitted.
    Connected(Connection),
    /// `Notify`: a new notification, or one replacing the notification
    /// with the same id.
    Notify(Box<Notification>),
    /// `CloseNotification` from an app.
    Close(u32),
    /// Its timer ran out (if the serial still matches).
    Expired(u32, u64),
}

/// What the toasts and the bar gadget ask the daemon to do. The three
/// on one notification (by id) act on its toast, if up, and drop it
/// from the history: the user dealt with it.
#[derive(Debug, Clone)]
pub enum Command {
    /// The user clicked it: invoke `default` if the app offered it,
    /// then close.
    Activate(u32),
    Invoke(u32, String),
    Dismiss(u32),
    /// Close every toast on screen; the history keeps them.
    DismissAll,
    ToggleDnd,
    /// The history was looked at: nothing is new any more.
    MarkSeen,
    /// Empty the history (and close the toasts up).
    Clear,
}

/// A notification in the history: as received, and whether the user
/// has seen the list since.
#[derive(Debug, Clone)]
pub struct Entry {
    pub notification: Notification,
    pub received: SystemTime,
    pub seen: bool,
}

pub struct Notifications {
    config: NotificationsConfig,
    conn: Option<Connection>,
    /// The toasts on screen, newest first.
    pub items: Vec<Notification>,
    /// What came in, newest first, up to the configured count.
    history: Vec<Entry>,
    /// Do not disturb: no toasts but the critical ones.
    dnd: bool,
    /// Icons of the notifications whose icon is a file, by path (read
    /// once per path while any shows it).
    files: Vec<(PathBuf, Icon)>,
}

impl Notifications {
    pub fn new(config: NotificationsConfig) -> Self {
        Self {
            config,
            conn: None,
            items: Vec::new(),
            history: Vec::new(),
            dnd: false,
            files: Vec::new(),
        }
    }

    pub fn dnd(&self) -> bool {
        self.dnd
    }

    /// Newest first.
    pub fn history(&self) -> &[Entry] {
        &self.history
    }

    /// How many the user hasn't looked at.
    pub fn unseen(&self) -> usize {
        self.history.iter().filter(|e| !e.seen).count()
    }

    /// A config reload: what shows stays, new notifications follow the
    /// new settings.
    pub fn set_config(&mut self, config: NotificationsConfig) {
        self.config = config;
    }

    pub fn config(&self) -> &NotificationsConfig {
        &self.config
    }

    pub fn subscription(&self) -> Subscription<Event> {
        if !self.config.enabled {
            return Subscription::none();
        }
        Subscription::run(dbus::events)
    }

    /// How long `n` stays, if not forever: the app's own timeout, else
    /// the configured duration; a critical notification left to us
    /// stays until dismissed.
    fn timeout(&self, n: &Notification) -> Option<Duration> {
        match n.timeout {
            Timeout::Never => None,
            Timeout::After(d) => Some(d),
            Timeout::Default if n.urgency == Urgency::Critical => None,
            Timeout::Default => Some(Duration::from_secs(self.config.duration)),
        }
    }

    /// Apply an event; the task is its follow-up (the expiry timer of a
    /// new notification, the signal for a closed one).
    pub fn apply(&mut self, event: Event) -> Task<Event> {
        match event {
            Event::Connected(conn) => self.conn = Some(conn),
            Event::Notify(n) => {
                log::info!(
                    "notification {} from {:?}: {:?}",
                    n.id,
                    n.app_name,
                    n.summary
                );
                if let Some(IconSource::Path(path)) = &n.icon
                    && !self.files.iter().any(|(p, _)| p == path)
                {
                    self.files
                        .push((path.clone(), Icon::from_path(path.clone())));
                }
                // Into the history, replacing in place or first.
                if self.config.history > 0 {
                    let entry = Entry {
                        notification: (*n).clone(),
                        received: SystemTime::now(),
                        seen: false,
                    };
                    match self.history.iter_mut().find(|e| e.notification.id == n.id) {
                        Some(e) => *e = entry,
                        None => self.history.insert(0, entry),
                    }
                    self.history.truncate(self.config.history);
                }
                // On screen, unless quiet: then only what can't wait.
                let show = !self.dnd || n.urgency == Urgency::Critical;
                let mut timer = Task::none();
                if show {
                    timer = self.timeout(&n).map_or_else(Task::none, |t| {
                        let (id, serial) = (n.id, n.serial);
                        Task::future(async move {
                            tokio::time::sleep(t).await;
                            Event::Expired(id, serial)
                        })
                    });
                    match self.items.iter_mut().find(|i| i.id == n.id) {
                        Some(item) => *item = *n,
                        None => self.items.insert(0, *n),
                    }
                } else if let Some(i) = self.items.iter().position(|i| i.id == n.id) {
                    // A replacement while quiet: the old toast goes too.
                    self.items.remove(i);
                }
                self.prune_files();
                return timer;
            }
            Event::Close(id) => return self.close(id, Reason::Closed),
            Event::Expired(id, serial) => {
                if self.items.iter().any(|i| i.id == id && i.serial == serial) {
                    return self.close(id, Reason::Expired);
                }
            }
        }
        Task::none()
    }

    /// The toast `id`, while on screen.
    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Notification `id`, on screen or in the history.
    fn lookup(&self, id: u32) -> Option<&Notification> {
        self.get(id).or_else(|| {
            self.history
                .iter()
                .find(|e| e.notification.id == id)
                .map(|e| &e.notification)
        })
    }

    /// Every notification held, for the icons and files they draw.
    fn all(&self) -> impl Iterator<Item = &Notification> {
        self.items
            .iter()
            .chain(self.history.iter().map(|e| &e.notification))
    }

    /// The icon of a file-backed notification, once read.
    pub fn file_icon(&self, path: &std::path::Path) -> Option<&Icon> {
        self.files.iter().find(|(p, _)| p == path).map(|(_, i)| i)
    }

    /// Names of the theme icons the notifications ask for, for the
    /// daemon to resolve.
    pub fn icon_names(&self) -> impl Iterator<Item = &str> {
        self.all()
            .filter_map(|n| n.icon.as_ref().and_then(IconSource::name))
    }

    fn prune_files(&mut self) {
        let used: Vec<PathBuf> = self
            .all()
            .filter_map(|n| match &n.icon {
                Some(IconSource::Path(p)) => Some(p.clone()),
                _ => None,
            })
            .collect();
        self.files.retain(|(path, _)| used.contains(path));
    }

    /// Drop `id` from the history.
    fn forget(&mut self, id: u32) {
        self.history.retain(|e| e.notification.id != id);
    }

    /// Forget `id` and tell the app why.
    fn close(&mut self, id: u32, reason: Reason) -> Task<Event> {
        let before = self.items.len();
        self.items.retain(|i| i.id != id);
        if self.items.len() == before {
            return Task::none();
        }
        log::debug!("notification {id} closed: {reason:?}");
        self.prune_files();
        match self.conn.clone() {
            Some(conn) => Task::future(dbus::closed(conn, id, reason)).discard(),
            None => Task::none(),
        }
    }

    pub fn run(&mut self, command: Command) -> Task<Event> {
        let invoke = |conn: Option<Connection>, id: u32, key: String| match conn {
            Some(conn) => Task::future(dbus::invoked(conn, id, key)).discard(),
            None => Task::none(),
        };
        let task = match command {
            Command::Activate(id) => {
                let default = self
                    .lookup(id)
                    .is_some_and(Notification::has_default_action);
                let signal = if default {
                    invoke(self.conn.clone(), id, "default".to_owned())
                } else {
                    Task::none()
                };
                self.forget(id);
                Task::batch([signal, self.close(id, Reason::Dismissed)])
            }
            Command::Invoke(id, key) => {
                self.forget(id);
                Task::batch([
                    invoke(self.conn.clone(), id, key),
                    self.close(id, Reason::Dismissed),
                ])
            }
            Command::Dismiss(id) => {
                self.forget(id);
                self.close(id, Reason::Dismissed)
            }
            Command::DismissAll => self.close_all(),
            Command::ToggleDnd => {
                self.dnd = !self.dnd;
                log::info!(
                    "notifications: do not disturb {}",
                    if self.dnd { "on" } else { "off" }
                );
                Task::none()
            }
            Command::MarkSeen => {
                self.history.iter_mut().for_each(|e| e.seen = true);
                Task::none()
            }
            Command::Clear => {
                self.history.clear();
                self.close_all()
            }
        };
        self.prune_files();
        task
    }

    /// Close every toast on screen.
    fn close_all(&mut self) -> Task<Event> {
        let ids: Vec<u32> = self.items.iter().map(|n| n.id).collect();
        Task::batch(ids.into_iter().map(|id| self.close(id, Reason::Dismissed)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: u32, serial: u64) -> Box<Notification> {
        Box::new(Notification {
            id,
            serial,
            app_name: "app".into(),
            summary: "s".into(),
            body: String::new(),
            icon: None,
            image: None,
            urgency: Urgency::Normal,
            actions: vec![
                ("default".into(), "Open".into()),
                ("ok".into(), "OK".into()),
            ],
            timeout: Timeout::Never,
        })
    }

    #[test]
    fn timeouts() {
        let n = Notifications::new(NotificationsConfig::from_raw(&RawSection::default()));
        let mut item = *notification(1, 1);
        assert_eq!(n.timeout(&item), None);
        item.timeout = Timeout::parse(-1);
        assert_eq!(n.timeout(&item), Some(Duration::from_secs(20)));
        item.timeout = Timeout::parse(1500);
        assert_eq!(n.timeout(&item), Some(Duration::from_millis(1500)));
        item.timeout = Timeout::Default;
        item.urgency = Urgency::Critical;
        assert_eq!(n.timeout(&item), None);
        assert_eq!(Timeout::parse(0), Timeout::Never);
    }

    #[test]
    fn config_defaults_and_position() {
        let cfg = NotificationsConfig::from_raw(&RawSection::default());
        assert!(cfg.enabled);
        assert_eq!(cfg.duration, 20);
        assert_eq!(cfg.position, Position::TopRight);
        assert_eq!(
            Position::parse("bottom-center"),
            Some(Position::BottomCenter)
        );
        assert_eq!(Position::parse("middle"), None);
        assert!(!Position::BottomLeft.is_top());
    }

    #[test]
    fn icon_sources() {
        assert_eq!(IconSource::parse(""), None);
        assert_eq!(
            IconSource::parse("file:///tmp/a.png"),
            Some(IconSource::Path("/tmp/a.png".into()))
        );
        assert_eq!(
            IconSource::parse("/tmp/a.png"),
            Some(IconSource::Path("/tmp/a.png".into()))
        );
        assert_eq!(
            IconSource::parse("dialog-information"),
            Some(IconSource::Name("dialog-information".into()))
        );
    }

    #[test]
    fn replace_and_expire_by_serial() {
        let mut n = Notifications::new(NotificationsConfig::from_raw(&RawSection::default()));
        let _ = n.apply(Event::Notify(notification(1, 1)));
        let _ = n.apply(Event::Notify(notification(2, 2)));
        assert_eq!(n.items.iter().map(|i| i.id).collect::<Vec<_>>(), [2, 1]);
        // Replaced in place, keeps its position.
        let _ = n.apply(Event::Notify(notification(1, 3)));
        assert_eq!(n.items.iter().map(|i| i.id).collect::<Vec<_>>(), [2, 1]);
        // The first one's timer must not close the replacement.
        let _ = n.apply(Event::Expired(1, 1));
        assert!(n.get(1).is_some());
        let _ = n.apply(Event::Expired(1, 3));
        assert!(n.get(1).is_none());
        let _ = n.apply(Event::Close(2));
        assert!(n.items.is_empty());
    }

    #[test]
    fn history_dnd_and_seen() {
        let config = crate::config::Config::parse("[Notifications]\nhistory = 3\n").section(None);
        let mut n = Notifications::new(config);
        let _ = n.apply(Event::Notify(notification(1, 1)));
        let _ = n.apply(Event::Notify(notification(2, 2)));
        assert_eq!(n.unseen(), 2);
        assert_eq!(n.items.len(), 2);
        // Expiry and an app's close keep the entry.
        let _ = n.apply(Event::Expired(1, 1));
        let _ = n.apply(Event::Close(2));
        assert!(n.items.is_empty());
        assert_eq!(n.history().len(), 2);
        // Seen: the count drops, the entries stay.
        let _ = n.run(Command::MarkSeen);
        assert_eq!(n.unseen(), 0);
        // Quiet: no toast, but the critical one; replacing an entry
        // makes it new again.
        let _ = n.run(Command::ToggleDnd);
        assert!(n.dnd());
        let _ = n.apply(Event::Notify(notification(3, 3)));
        assert!(n.items.is_empty());
        assert_eq!(n.unseen(), 1);
        let mut critical = notification(4, 4);
        critical.urgency = Urgency::Critical;
        let _ = n.apply(Event::Notify(critical));
        assert_eq!(n.items.len(), 1);
        // Capped at 3, newest first: 4, 3, 2.
        assert_eq!(
            n.history()
                .iter()
                .map(|e| e.notification.id)
                .collect::<Vec<_>>(),
            [4, 3, 2]
        );
        let _ = n.apply(Event::Notify(notification(2, 5)));
        assert_eq!(n.unseen(), 3);
        // The user dealt with one: gone from both; dismiss all keeps
        // the history; clear empties it.
        let _ = n.run(Command::Dismiss(4));
        assert!(n.items.is_empty());
        assert_eq!(n.history().len(), 2);
        let _ = n.run(Command::ToggleDnd);
        let _ = n.apply(Event::Notify(notification(5, 6)));
        let _ = n.run(Command::DismissAll);
        assert!(n.items.is_empty());
        assert_eq!(n.history().len(), 3);
        let _ = n.run(Command::Clear);
        assert!(n.history().is_empty());
    }

    #[test]
    fn default_action_has_no_button() {
        let n = notification(1, 1);
        assert!(n.has_default_action());
        assert_eq!(
            n.buttons().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            ["ok"]
        );
    }
}
