//! Apps gadget: a dock's icons. First the pinned apps, always there:
//! the instance's own (`apps`), then those pinned from the menu, the
//! same on every bar; after a separator the other apps with windows
//! open, in the order they opened. An app is its desktop entry, a
//! window's found as its icon is (`[apps_class_map]` included); a
//! window with none is an app of its own, by its class.
//!
//! A left click launches a pinned app not running, focuses the window
//! of an app with one, and shows the windows of an app with more: its
//! name and its windows, to pick one. (Not a click cycling them:
//! Hyprland's focus warps the pointer onto the window, away from the
//! icon.) The middle button launches it again, the right one opens its
//! whole menu: its windows, its desktop actions, pin or unpin, close.
//! The pointer resting on it shows its name in a tip.
//!
//! Holds no app state: the windows are the daemon's `Compositor`, the
//! pinned ones its `Apps`, the entries its `Icons`; every click's
//! target is found in `view`, when the click is built.

use iced::Rectangle;
use iced::widget::mouse_area;
use iced_wayland_subscriber::OutputInfo;
use icssed::Element;

use crate::config::{RawSection, Section};
use crate::gadgets::{Action, Context, Gadget, Popup};
use crate::services::apps::{Command, desktop_id};
use crate::services::compositor::{self, Window};
use crate::services::icons::desktop::DesktopEntry;
use crate::ui::menu::{self, Item, Menu};
use crate::ui::tip::Tip;

/// Shown while an app's icon isn't resolved, its room kept.
const PLACEHOLDER: &str = "application-x-executable";

/// `[Apps]` section.
#[derive(Debug, Clone)]
pub struct AppsConfig {
    /// This instance's pinned apps, desktop ids, before the shared
    /// ones; the menu doesn't unpin them.
    pub apps: Vec<String>,
}

impl Section for AppsConfig {
    const NAME: &'static str = "Apps";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            apps: raw
                .list_or("apps", &[])
                .iter()
                .map(|name| desktop_id(name))
                .collect(),
        }
    }
}

pub struct AppsGadget {
    config: AppsConfig,
    popup: Popup,
    /// The app whose menu the popup shows, by key.
    menu_for: Option<String>,
    /// Which of its menus.
    shown: Shown,
    menu: Menu,
    /// The tip shown: the app's key, what it's about.
    tip: Option<(String, Rectangle)>,
}

/// What the popup shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Everything: the right button's.
    Menu,
    /// Its name and its windows: the left button's, with more than one.
    Windows,
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Focus this window.
    Focus(String),
    /// Run this desktop entry, or one of its actions.
    Launch(String, Option<String>),
    /// Close these windows.
    Close(Vec<String>),
    Pin(String),
    Unpin(String),
    /// Show this menu of the app with this key, at anchor `n`.
    Open(String, usize, Shown),
    Menu(menu::Message),
    /// The tip of the app with this key and name: shown about this
    /// rectangle, or gone.
    Tip(String, String, Option<Rectangle>),
}

impl Gadget for AppsGadget {
    type Config = AppsConfig;
    type Message = Message;

    fn new(config: AppsConfig, _output: &OutputInfo) -> Self {
        Self {
            config,
            popup: Popup::new(),
            menu_for: None,
            shown: Shown::Menu,
            menu: Menu::new(),
            tip: None,
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        let action = match message {
            Message::Open(key, n, shown) => {
                let same = self.popup.is_open() && self.menu_for.as_deref() == Some(&key);
                if same && self.shown == shown {
                    return self.popup.close();
                }
                if same {
                    // The other menu of the same app: in the same popup.
                    self.shown = shown;
                    self.menu.reset();
                    return Action::None;
                }
                // Another app's may be open: it goes.
                let close = self.popup.close();
                self.menu_for = Some(key);
                self.shown = shown;
                self.menu.reset();
                return Action::Many(vec![close, self.popup.toggle_nth(n)]);
            }
            Message::Menu(m) => {
                self.menu.update(m);
                return Action::None;
            }
            Message::Tip(key, text, Some(about)) => {
                self.tip = Some((key, about));
                return Action::ShowTip(Tip { text, about });
            }
            Message::Tip(key, _, None) => {
                return match self.tip.take_if(|(k, _)| *k == key) {
                    Some((_, about)) => Action::HideTip(about),
                    None => Action::None,
                };
            }
            Message::Focus(id) => Action::Compositor(compositor::Command::ActivateWindow(id)),
            Message::Launch(id, action) => Action::Apps(Command::Launch { id, action }),
            Message::Close(ids) => Action::Many(
                ids.into_iter()
                    .map(|id| Action::Compositor(compositor::Command::CloseWindow(id)))
                    .collect(),
            ),
            Message::Pin(id) => Action::Apps(Command::Pin(id)),
            Message::Unpin(id) => Action::Apps(Command::Unpin(id)),
        };
        // From the menu, or from the bar while it's open: it goes.
        Action::Many(vec![self.popup.close(), action])
    }

    fn icon_names(&self) -> Vec<String> {
        vec![PLACEHOLDER.to_owned()]
    }

    fn app_ids(&self) -> Vec<String> {
        self.config.apps.clone()
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let (apps, pinned) = self.apps(ctx);
        let mut children: Vec<Element<'a, Message>> = Vec::new();
        for (n, app) in apps.into_iter().enumerate() {
            if n == pinned && n > 0 {
                children.push(icssed::row(Vec::new()).kind("separator").into());
            }
            children.push(self.app_view(app, n, ctx));
        }
        icssed::row(children).into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_closed(&mut self) {
        self.menu_for = None;
        self.menu.reset();
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let app = self.menu_for.as_deref().and_then(|key| {
            let (apps, _) = self.apps(ctx);
            apps.into_iter().find(|a| a.key == key)
        });
        let (items, targets) = match app {
            Some(app) => menu_of(&app, self.shown, ctx),
            None => (Vec::new(), Vec::new()),
        };
        self.menu.view(items, ctx.icons).map(move |m| match m {
            menu::Message::Click(id) => usize::try_from(id)
                .ok()
                .and_then(|i| targets.get(i).cloned())
                .unwrap_or(Message::Menu(m)),
            m => Message::Menu(m),
        })
    }
}

/// An app as the bar shows it.
struct App<'a> {
    /// Its desktop id, or the class of a window with no entry.
    key: String,
    /// What its icon is resolved for: a pinned name, a window's class.
    icon: &'a str,
    entry: Option<&'a DesktopEntry>,
    /// Its windows, in the order they opened.
    windows: Vec<&'a Window>,
    /// Pinned by this instance's config: the menu can't unpin it.
    fixed: bool,
    /// Pinned from the menu, on every bar.
    pinned: bool,
}

impl App<'_> {
    fn name(&self) -> &str {
        self.entry.map_or(self.key.as_str(), |e| e.name.as_str())
    }
}

impl AppsGadget {
    /// Every app shown, the pinned ones first: how many they are.
    fn apps<'a>(&'a self, ctx: Context<'a>) -> (Vec<App<'a>>, usize) {
        let mut apps: Vec<App<'a>> = Vec::new();
        let fixed = self.config.apps.iter().map(|p| (p, true));
        let shared = ctx.apps.pinned().iter().map(|p| (p, false));
        for (name, fixed) in fixed.chain(shared) {
            let entry = ctx.icons.entry(name);
            let key = entry.map_or_else(|| desktop_id(name), |e| e.id.clone());
            match apps.iter_mut().find(|a| a.key == key) {
                Some(app) => app.pinned |= !fixed,
                None => apps.push(App {
                    key,
                    icon: name,
                    entry,
                    windows: Vec::new(),
                    fixed,
                    pinned: !fixed,
                }),
            }
        }
        let pinned = apps.len();
        let mut windows: Vec<&Window> = ctx.compositor.windows.iter().collect();
        windows.sort_by_key(|w| w.opened);
        for win in windows {
            let entry = ctx.icons.entry(&win.class);
            let key = entry.map_or_else(|| win.class.to_ascii_lowercase(), |e| e.id.clone());
            match apps.iter_mut().find(|a| a.key == key) {
                Some(app) => app.windows.push(win),
                None => apps.push(App {
                    key,
                    icon: &win.class,
                    entry,
                    windows: vec![win],
                    fixed: false,
                    pinned: false,
                }),
            }
        }
        (apps, pinned)
    }

    fn app_view<'a>(&'a self, app: App<'a>, n: usize, ctx: Context<'a>) -> Element<'a, Message> {
        let icon = match ctx.icons.get(app.icon) {
            Some(icon) => icssed::icon(icon.clone()),
            None => ctx.icon(PLACEHOLDER),
        };
        let dots = app.windows.iter().map(|w| {
            icssed::row(Vec::new())
                .kind("dot")
                .class_if("active", w.active)
                .into()
        });
        let left = match (app.windows.as_slice(), app.entry) {
            ([win], _) => Some(Message::Focus(win.id.clone())),
            ([], Some(entry)) => Some(Message::Launch(entry.id.clone(), None)),
            ([], None) => None,
            _ => Some(Message::Open(app.key.clone(), n, Shown::Windows)),
        };
        let mut button = icssed::button![icon, icssed::row(dots).kind("windows")]
            .kind("indicator")
            .class("app")
            .class_if("pinned", app.fixed || app.pinned)
            .class_if("running", !app.windows.is_empty())
            .class_if("active", app.windows.iter().any(|w| w.active))
            .class_if("danger", app.windows.iter().any(|w| w.urgent))
            .attr("app", &app.key)
            .id(self.popup.anchor_nth(n));
        if let Some(left) = left {
            button = button.on_press(left);
        }
        let mut area =
            mouse_area(button).on_right_press(Message::Open(app.key.clone(), n, Shown::Menu));
        if let Some(entry) = app.entry {
            area = area.on_middle_press(Message::Launch(entry.id.clone(), None));
        }
        let (key, name) = (app.key.clone(), app.name().to_owned());
        icssed::tooltip(area, name.clone())
            .on_tip(move |about| Message::Tip(key.clone(), name.clone(), about))
            .into()
    }
}

/// The app's menu, and what each item's click is (an item's id is its
/// index there): its name, its windows, then for the whole menu its
/// desktop actions, open or a new window, pin or unpin, close.
fn menu_of(app: &App<'_>, shown: Shown, ctx: Context<'_>) -> (Vec<Item>, Vec<Message>) {
    // Ids no click comes back with: the header's, the separators'.
    let mut items: Vec<Item> = vec![Item::header(-1, app.name())];
    let mut targets: Vec<Message> = Vec::new();
    let mut add = |items: &mut Vec<Item>, label: String, target: Message| {
        let id = i32::try_from(targets.len()).unwrap_or(i32::MAX);
        items.push(Item::new(id, label));
        targets.push(target);
    };
    // Between groups: never two in a row, nor under the header.
    let separator = |items: &mut Vec<Item>| {
        if items.last().is_some_and(|i| !i.separator && !i.header) {
            items.push(Item::separator(-1 - items.len() as i32));
        }
    };
    for win in &app.windows {
        let title = if win.title.is_empty() {
            app.name().to_owned()
        } else {
            win.title.clone()
        };
        add(&mut items, title, Message::Focus(win.id.clone()));
    }
    if shown == Shown::Windows {
        return (items, targets);
    }
    if let Some(entry) = app.entry {
        separator(&mut items);
        for action in &entry.actions {
            let target = Message::Launch(entry.id.clone(), Some(action.id.clone()));
            add(&mut items, action.name.clone(), target);
        }
        separator(&mut items);
        let launch = if app.windows.is_empty() {
            "apps.open"
        } else {
            "apps.new_window"
        };
        add(
            &mut items,
            ctx.locale.tr(launch).to_owned(),
            Message::Launch(entry.id.clone(), None),
        );
    }
    separator(&mut items);
    if app.pinned {
        add(
            &mut items,
            ctx.locale.tr("apps.unpin").to_owned(),
            Message::Unpin(app.key.clone()),
        );
    } else if !app.fixed && app.entry.is_some() {
        add(
            &mut items,
            ctx.locale.tr("apps.pin").to_owned(),
            Message::Pin(app.key.clone()),
        );
    }
    if !app.windows.is_empty() {
        let close = if app.windows.len() == 1 {
            "apps.close"
        } else {
            "apps.close_all"
        };
        let ids = app.windows.iter().map(|w| w.id.clone()).collect();
        add(
            &mut items,
            ctx.locale.tr(close).to_owned(),
            Message::Close(ids),
        );
    }
    (items, targets)
}
