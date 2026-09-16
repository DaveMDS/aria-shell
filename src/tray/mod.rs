//! The status notifier items (the "system tray") over DBus, and their
//! `com.canonical.dbusmenu` menus.
//!
//! One [`Tray`] lives in the daemon, shaped like `Compositor`: the
//! [`Tray::subscription`] is the single session-bus connection (it
//! serves the `org.kde.StatusNotifierWatcher`, or uses the one another
//! bar already owns, and watches every registered item), the [`Event`]s
//! it yields go through [`Tray::apply`], gadgets read the resulting
//! [`Item`]s from their view context and act on them with a [`Command`]
//! the daemon runs with [`Tray::run`].
//!
//! Reference: <https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/>

mod dbus;
pub mod menu;

use std::collections::HashMap;
use std::sync::Arc;

use iced::widget::image;
use iced::{Subscription, Task};
use zbus::Connection;

use crate::icons::Icon;
pub use menu::Menu;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    Passive,
    #[default]
    Active,
    NeedsAttention,
}

/// An icon the app sent as pixels (`IconPixmap`), already converted to
/// straight RGBA.
#[derive(Clone, PartialEq, Eq)]
pub struct Pixmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Pixmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pixmap({}x{})", self.width, self.height)
    }
}

/// The item's properties, as last read from the bus.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Props {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub icon_name: String,
    pub icon_pixmap: Option<Pixmap>,
    pub attention_icon_name: String,
    pub attention_pixmap: Option<Pixmap>,
    /// A directory the app ships its icons in, searched before the
    /// theme.
    pub icon_theme_path: String,
    /// Tooltip title and text, joined.
    pub tooltip: String,
    /// A left click should open the menu, not `Activate`.
    pub item_is_menu: bool,
    /// Object path of its `com.canonical.dbusmenu`, when it has one.
    pub menu: Option<String>,
}

pub struct Item {
    /// `<bus name><object path>`, what the watcher lists.
    pub key: String,
    pub props: Props,
    /// The pixmap icons as iced handles, created once per change.
    icon: Option<Icon>,
    attention_icon: Option<Icon>,
}

impl Item {
    fn new(key: String, props: Props) -> Self {
        let mut item = Self {
            key,
            props: Props::default(),
            icon: None,
            attention_icon: None,
        };
        item.set(props);
        item
    }

    fn set(&mut self, props: Props) {
        if props.icon_pixmap != self.props.icon_pixmap {
            self.icon = props.icon_pixmap.as_ref().map(handle);
        }
        if props.attention_pixmap != self.props.attention_pixmap {
            self.attention_icon = props.attention_pixmap.as_ref().map(handle);
        }
        self.props = props;
    }

    fn attention(&self) -> bool {
        self.props.status == Status::NeedsAttention
    }

    /// The icon name to show for the current status, if any.
    pub fn icon_name(&self) -> Option<&str> {
        let name = if self.attention() && !self.props.attention_icon_name.is_empty() {
            &self.props.attention_icon_name
        } else {
            &self.props.icon_name
        };
        (!name.is_empty()).then_some(name.as_str())
    }

    /// The pixmap icon for the current status, when the app sent one.
    pub fn pixmap_icon(&self) -> Option<&Icon> {
        if self.attention() && self.attention_icon.is_some() {
            self.attention_icon.as_ref()
        } else {
            self.icon.as_ref()
        }
    }

    pub fn icon_theme_path(&self) -> Option<&str> {
        let p = self.props.icon_theme_path.as_str();
        (!p.is_empty()).then_some(p)
    }
}

fn handle(p: &Pixmap) -> Icon {
    Icon::Raster(image::Handle::from_rgba(p.width, p.height, p.rgba.clone()))
}

#[derive(Debug, Clone)]
pub enum Event {
    /// The session bus is up; commands can be sent.
    Connected(Connection),
    Added(String, Props),
    Updated(String, Props),
    Removed(String),
    /// The app changed its menu layout or item properties.
    MenuChanged(String),
    /// A menu layout was fetched.
    Menu(String, Arc<Menu>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// What a gadget asks the daemon to do to an item (by key). Clicks get
/// the pointer position from the daemon.
#[derive(Debug, Clone)]
pub enum Command {
    Activate(String),
    SecondaryActivate(String),
    ContextMenu(String),
    Scroll(String, i32, Orientation),
    /// Fetch the menu layout (`AboutToShow` first, apps fill it then).
    LoadMenu(String),
    /// A submenu is about to be shown: same, for its node.
    ExpandMenu(String, i32),
    MenuClick(String, i32),
}

#[derive(Default)]
pub struct Tray {
    conn: Option<Connection>,
    /// In registration order.
    pub items: Vec<Item>,
    /// Loaded menu layouts, by item key.
    menus: HashMap<String, Arc<Menu>>,
}

impl Tray {
    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::run(dbus::events)
    }

    /// Apply an event; the task is the follow-up it needs (a menu
    /// reload when the app changed a menu that's loaded).
    pub fn apply(&mut self, event: Event) -> Task<Event> {
        match event {
            Event::Connected(conn) => self.conn = Some(conn),
            Event::Added(key, props) => {
                log::info!("tray: {key} ({:?}) registered", props.id);
                match self.items.iter_mut().find(|i| i.key == key) {
                    Some(item) => item.set(props),
                    None => self.items.push(Item::new(key, props)),
                }
            }
            Event::Updated(key, props) => {
                if let Some(item) = self.items.iter_mut().find(|i| i.key == key) {
                    item.set(props);
                }
            }
            Event::Removed(key) => {
                log::info!("tray: {key} unregistered");
                self.items.retain(|i| i.key != key);
                self.menus.remove(&key);
            }
            Event::MenuChanged(key) => {
                if self.menus.contains_key(&key) {
                    return self.run(Command::LoadMenu(key), (0, 0));
                }
            }
            Event::Menu(key, menu) => {
                self.menus.insert(key, menu);
            }
        }
        Task::none()
    }

    pub fn item(&self, key: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.key == key)
    }

    /// The menu of an item, once loaded.
    pub fn menu(&self, key: &str) -> Option<&Arc<Menu>> {
        self.menus.get(key)
    }

    /// Send a command; `cursor` is where the pointer is, in global
    /// coordinates, for the click methods.
    pub fn run(&self, command: Command, cursor: (i32, i32)) -> Task<Event> {
        let Some(conn) = self.conn.clone() else {
            log::warn!("tray: no bus connection, dropping {command:?}");
            return Task::none();
        };
        let menu_path = |key: &str| self.item(key).and_then(|i| i.props.menu.clone());
        match command.clone() {
            Command::Activate(key) => {
                Task::future(dbus::activate(conn, key, "Activate", cursor)).discard()
            }
            Command::SecondaryActivate(key) => {
                Task::future(dbus::activate(conn, key, "SecondaryActivate", cursor)).discard()
            }
            Command::ContextMenu(key) => {
                Task::future(dbus::activate(conn, key, "ContextMenu", cursor)).discard()
            }
            Command::Scroll(key, delta, orientation) => {
                Task::future(dbus::scroll(conn, key, delta, orientation)).discard()
            }
            Command::LoadMenu(key) | Command::ExpandMenu(key, _) => {
                let Some(path) = menu_path(&key) else {
                    return Task::none();
                };
                let node = match command {
                    Command::ExpandMenu(_, node) => node,
                    _ => 0,
                };
                Task::future(menu::load(conn, key.clone(), path, node))
                    .and_then(move |menu| Task::done(Event::Menu(key.clone(), Arc::new(menu))))
            }
            Command::MenuClick(key, id) => {
                let Some(path) = menu_path(&key) else {
                    return Task::none();
                };
                Task::future(menu::click(conn, key, path, id)).discard()
            }
        }
    }
}

/// `":1.49/StatusNotifierItem"` -> (`":1.49"`, `"/StatusNotifierItem"`).
fn split_key(key: &str) -> (&str, &str) {
    match key.find('/') {
        Some(i) => key.split_at(i),
        None => (key, "/StatusNotifierItem"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_split() {
        assert_eq!(
            split_key(":1.49/StatusNotifierItem"),
            (":1.49", "/StatusNotifierItem")
        );
        assert_eq!(
            split_key(":1.12/org/ayatana/NotificationItem/nm_applet"),
            (":1.12", "/org/ayatana/NotificationItem/nm_applet")
        );
        assert_eq!(split_key(":1.7"), (":1.7", "/StatusNotifierItem"));
    }

    #[test]
    fn attention_icon_wins_only_when_needed() {
        let mut props = Props {
            icon_name: "app".into(),
            attention_icon_name: "app-alert".into(),
            ..Props::default()
        };
        let mut item = Item::new("k".into(), props.clone());
        assert_eq!(item.icon_name(), Some("app"));
        props.status = Status::NeedsAttention;
        item.set(props.clone());
        assert_eq!(item.icon_name(), Some("app-alert"));
        props.attention_icon_name.clear();
        item.set(props);
        assert_eq!(item.icon_name(), Some("app"));
    }

    #[test]
    fn menu_reload_only_when_loaded() {
        let mut tray = Tray::default();
        // No connection: nothing to run either way, but the state must
        // hold what was loaded.
        let _ = tray.apply(Event::Added("k".into(), Props::default()));
        assert!(tray.menu("k").is_none());
        let _ = tray.apply(Event::Menu("k".into(), Arc::new(Menu::default())));
        assert!(tray.menu("k").is_some());
        let _ = tray.apply(Event::Removed("k".into()));
        assert!(tray.menu("k").is_none());
        assert!(tray.items.is_empty());
    }
}
