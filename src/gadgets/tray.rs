//! Tray gadget: one icon per status notifier item. A left click
//! activates the item (or opens its menu when it says it's a menu), a
//! right click opens its `dbusmenu` in a popup (or asks the app for its
//! own context menu when it has none), the middle button is the
//! secondary activation, the wheel scrolls it.
//!
//! Holds no item state: it reads the daemon's [`Tray`](crate::services::tray::Tray)
//! from the view context. The popup shows the menu the daemon loaded
//! for the clicked item (a `ui::menu::Menu`); submenus unfold in
//! place and the popup is resized (its size is a function of the state,
//! see [`Gadget::popup_size`]).

use iced::mouse::ScrollDelta;
use iced::widget::mouse_area;
use iced_wayland_subscriber::OutputInfo;
use icssed::Element;

use crate::config::{RawSection, Section};
use crate::gadgets::{Action, Axis, Context, Gadget, Popup, Wheel};
use crate::services::tray::{Command, Orientation, Status};
use crate::ui::menu::{self, Menu};

/// `[Tray]` section: no keys yet (the Python one had none either).
#[derive(Debug, Clone)]
pub struct TrayConfig;

impl Section for TrayConfig {
    const NAME: &'static str = "Tray";

    fn from_raw(_raw: &RawSection) -> Self {
        Self
    }
}

const LOADING: &str = "…";

pub struct TrayGadget {
    popup: Popup,
    /// Key of the item whose menu the popup shows.
    menu_for: Option<String>,
    menu: Menu,
    wheel: Wheel,
}

#[derive(Clone, Debug)]
pub enum Message {
    Activate(String),
    SecondaryActivate(String),
    ContextMenu(String),
    Scroll(String, ScrollDelta),
    /// Show the menu of the item at index `n` (its anchor).
    OpenMenu(String, usize),
    Menu(menu::Message),
}

impl Gadget for TrayGadget {
    type Config = TrayConfig;
    type Message = Message;

    fn new(_config: TrayConfig, _output: &OutputInfo) -> Self {
        Self {
            popup: Popup::new(),
            menu_for: None,
            menu: Menu::new(),
            wheel: Wheel::default(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Activate(key) => Action::Tray(Command::Activate(key)),
            Message::SecondaryActivate(key) => Action::Tray(Command::SecondaryActivate(key)),
            Message::ContextMenu(key) => Action::Tray(Command::ContextMenu(key)),
            Message::Scroll(key, delta) => match self.wheel.clicks(delta) {
                Some((clicks, axis)) => {
                    let orientation = match axis {
                        Axis::Horizontal => Orientation::Horizontal,
                        Axis::Vertical => Orientation::Vertical,
                    };
                    Action::Tray(Command::Scroll(key, clicks, orientation))
                }
                None => Action::None,
            },
            Message::OpenMenu(key, n) => {
                if self.popup.is_open() && self.menu_for.as_deref() == Some(&key) {
                    return self.popup.close();
                }
                // Another item's menu may be open: swap it.
                let close = self.popup.close();
                self.menu_for = Some(key.clone());
                self.menu.reset();
                Action::Many(vec![
                    close,
                    Action::Tray(Command::LoadMenu(key)),
                    self.popup.toggle_nth(n),
                ])
            }
            Message::Menu(m) => {
                let Some(key) = self.menu_for.clone() else {
                    return Action::None;
                };
                match self.menu.update(m) {
                    menu::Event::None => Action::None,
                    menu::Event::Clicked(id) => Action::Many(vec![
                        Action::Tray(Command::MenuClick(key, id)),
                        self.popup.close(),
                    ]),
                    // Apps fill submenus on AboutToShow.
                    menu::Event::Unfolded(id) => Action::Tray(Command::ExpandMenu(key, id)),
                }
            }
        }
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let count = ctx.tray.items.len();
        let icons: &dyn icssed::Icons = ctx.icons;
        let items = ctx.tray.items.iter().enumerate().map(|(n, item)| {
            let node = ctx
                .node
                .child_at("button", n, count)
                .class_if("passive", item.props.status == Status::Passive)
                .class_if("attention", item.props.status == Status::NeedsAttention);
            let file = item.pixmap_icon().or_else(|| {
                item.icon_name()
                    .and_then(|name| ctx.icons.get_name(name, item.icon_theme_path()))
            });
            // Its room is kept until the icon is known.
            let icon = match file {
                Some(file) => icssed::icon(node.child("icon"), file.clone()),
                None => icssed::icon(
                    node.child("icon"),
                    (icons, item.icon_name().unwrap_or_default()),
                ),
            };
            let key = item.key.clone();
            let has_menu = item.props.menu.is_some();
            let left = if has_menu && item.props.item_is_menu {
                Message::OpenMenu(key.clone(), n)
            } else {
                Message::Activate(key.clone())
            };
            let right = if has_menu {
                Message::OpenMenu(key.clone(), n)
            } else {
                Message::ContextMenu(key.clone())
            };
            let button = icssed::button(node, [icon.into()])
                .on_press(left)
                .id(self.popup.anchor_nth(n));
            let scroll_key = key.clone();
            mouse_area(button)
                .on_right_press(right)
                .on_middle_press(Message::SecondaryActivate(key))
                .on_scroll(move |delta| Message::Scroll(scroll_key.clone(), delta))
                .into()
        });
        icssed::row(ctx.node, items).into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_closed(&mut self) {
        self.menu_for = None;
        self.menu.reset();
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let node = ctx.node.child("menu");
        match self.menu_for.as_deref().and_then(|key| ctx.tray.menu(key)) {
            Some(menu) => self
                .menu
                .view(node, menu.items().to_vec())
                .map(Message::Menu),
            None => {
                icssed::column(node, [icssed::text(node.child("label"), LOADING).into()]).into()
            }
        }
    }
}
