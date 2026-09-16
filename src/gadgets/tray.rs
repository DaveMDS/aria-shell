//! Tray gadget: one icon per status notifier item. A left click
//! activates the item (or opens its menu when it says it's a menu), a
//! right click opens its `dbusmenu` in a popup (or asks the app for its
//! own context menu when it has none), the middle button is the
//! secondary activation, the wheel scrolls it.
//!
//! Holds no item state: it reads the daemon's [`Tray`](crate::tray::Tray)
//! from the view context. The popup shows the menu the daemon loaded
//! for the clicked item (a `widgets::menu::Menu`); submenus unfold in
//! place and the popup is resized (its size is a function of the state,
//! see [`Gadget::popup_size`]).

use std::time::{Duration, Instant};

use iced::Element;
use iced::mouse::ScrollDelta;
use iced::widget::{Space, mouse_area};
use iced_wayland_subscriber::OutputInfo;

use crate::config::{RawSection, Section};
use crate::gadget::{Action, Context, Gadget, Popup};
use crate::theme;
use crate::tray::{Command, Orientation, Status};
use crate::widgets::menu::{self, Menu};

/// `[Tray]` section: no keys yet (the Python one had none either).
#[derive(Debug, Clone)]
pub struct TrayConfig;

impl Section for TrayConfig {
    const NAME: &'static str = "Tray";

    fn from_raw(_raw: &RawSection) -> Self {
        Self
    }
}

/// Icon size when the theme doesn't set `height` on `item icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;
const LOADING: &str = "…";
/// One wheel click on the continuous axis, in the units compositors
/// use (libinput's), and how long after a discrete event its continuous
/// twin may follow.
const WHEEL_CLICK: f32 = 15.0;
const WHEEL_TWIN: Duration = Duration::from_millis(100);

pub struct TrayGadget {
    popup: Popup,
    /// Key of the item whose menu the popup shows.
    menu_for: Option<String>,
    menu: Menu,
    /// Continuous scroll not yet worth a click, and when the last
    /// discrete event came (a wheel sends both forms of the same click).
    scroll_pending: f32,
    scroll_discrete_at: Option<Instant>,
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
            scroll_pending: 0.0,
            scroll_discrete_at: None,
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Activate(key) => Action::Tray(Command::Activate(key)),
            Message::SecondaryActivate(key) => Action::Tray(Command::SecondaryActivate(key)),
            Message::ContextMenu(key) => Action::Tray(Command::ContextMenu(key)),
            Message::Scroll(key, delta) => match self.scroll_steps(delta) {
                Some((steps, orientation)) => {
                    Action::Tray(Command::Scroll(key, steps, orientation))
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
        let theme = ctx.theme;
        let count = ctx.tray.items.len();
        let items = ctx.tray.items.iter().enumerate().map(|(n, item)| {
            let node = ctx
                .node
                .child("item")
                .class_if("passive", item.props.status == Status::Passive)
                .class_if("attention", item.props.status == Status::NeedsAttention)
                .nth(n, count);
            let icon_node = node.child("icon");
            let style = theme.resolve(&icon_node);
            let size = style
                .height
                .or(style.width)
                .and_then(|l| match l {
                    theme::Length::Px(px) => Some(px),
                    _ => None,
                })
                .unwrap_or(DEFAULT_ICON_SIZE);
            let icon = item.pixmap_icon().or_else(|| {
                item.icon_name()
                    .and_then(|name| ctx.icons.get_name(name, item.icon_theme_path()))
            });
            let icon: Element<'a, Message> = match icon {
                Some(icon) => icon.view(size, style.color),
                None => Space::new().width(size).height(size).into(),
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
            let button = theme.button(&node, icon).on_press(left);
            let scroll_key = key.clone();
            mouse_area(self.popup.anchor_nth(n, button))
                .on_right_press(right)
                .on_middle_press(Message::SecondaryActivate(key))
                .on_scroll(move |delta| Message::Scroll(scroll_key.clone(), delta))
                .into()
        });
        theme
            .row(&ctx.node, items)
            .align_y(iced::Alignment::Center)
            .into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_closed(&mut self) {
        self.menu_for = None;
        self.menu.reset();
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let node = ctx.node.child("menu");
        let menu = self.menu_for.as_deref().and_then(|key| ctx.tray.menu(key));
        match menu {
            Some(menu) => self
                .menu
                .view(theme, &node, menu.items().to_vec())
                .map(Message::Menu),
            None => theme
                .container(&node, theme.text(&node.child("text"), LOADING))
                .into(),
        }
    }

    fn popup_size(&self, ctx: Context<'_>) -> (u32, u32) {
        let theme = ctx.theme;
        let node = ctx.node.child("menu");
        let menu = self.menu_for.as_deref().and_then(|key| ctx.tray.menu(key));
        let size = match menu {
            Some(menu) => self.menu.size(theme, &node, menu.items()),
            None => {
                let text = theme.measure(&node.child("text"), LOADING);
                let pad = theme.resolve(&node).padding;
                iced::Size::new(
                    text.width + pad.left + pad.right,
                    text.height + pad.top + pad.bottom,
                )
            }
        };
        (
            size.width.ceil().max(1.0) as u32,
            size.height.ceil().max(1.0) as u32,
        )
    }
}

impl TrayGadget {
    /// Wheel clicks to send for a scroll event, positive upwards (as
    /// KDE's host does): discrete events as they are, continuous ones
    /// accumulated into clicks, dropped when they merely repeat a
    /// discrete one (a wheel produces both).
    fn scroll_steps(&mut self, delta: ScrollDelta) -> Option<(i32, Orientation)> {
        let now = Instant::now();
        let (x, y) = match delta {
            ScrollDelta::Lines { x, y } => {
                self.scroll_discrete_at = Some(now);
                self.scroll_pending = 0.0;
                (x, y)
            }
            ScrollDelta::Pixels { x, y } => {
                let twin = self
                    .scroll_discrete_at
                    .is_some_and(|t| now.duration_since(t) < WHEEL_TWIN);
                if twin {
                    return None;
                }
                let (x, y) = if y != 0.0 { (0.0, y) } else { (x, 0.0) };
                self.scroll_pending += x + y;
                let clicks = (self.scroll_pending / WHEEL_CLICK).trunc();
                self.scroll_pending -= clicks * WHEEL_CLICK;
                if y != 0.0 {
                    (0.0, clicks)
                } else {
                    (clicks, 0.0)
                }
            }
        };
        let (steps, orientation) = if y != 0.0 {
            (y, Orientation::Vertical)
        } else {
            (x, Orientation::Horizontal)
        };
        let steps = steps as i32;
        (steps != 0).then_some((steps, orientation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gadget() -> TrayGadget {
        TrayGadget {
            popup: Popup::new(),
            menu_for: None,
            menu: Menu::new(),
            scroll_pending: 0.0,
            scroll_discrete_at: None,
        }
    }

    #[test]
    fn wheel_clicks() {
        let mut g = gadget();
        assert_eq!(
            g.scroll_steps(ScrollDelta::Lines { x: 0.0, y: -2.0 }),
            Some((-2, Orientation::Vertical))
        );
        // The continuous twin of the same wheel event.
        assert_eq!(
            g.scroll_steps(ScrollDelta::Pixels { x: 0.0, y: -30.0 }),
            None
        );
        assert_eq!(
            g.scroll_steps(ScrollDelta::Lines { x: 1.0, y: 0.0 }),
            Some((1, Orientation::Horizontal))
        );
        assert_eq!(g.scroll_steps(ScrollDelta::Pixels { x: 0.0, y: 0.0 }), None);
    }

    #[test]
    fn touchpad_accumulates() {
        let mut g = gadget();
        assert_eq!(
            g.scroll_steps(ScrollDelta::Pixels { x: 0.0, y: 10.0 }),
            None
        );
        assert_eq!(
            g.scroll_steps(ScrollDelta::Pixels { x: 0.0, y: 10.0 }),
            Some((1, Orientation::Vertical))
        );
        // 5 left over, plus 10: another click.
        assert_eq!(
            g.scroll_steps(ScrollDelta::Pixels { x: 0.0, y: 10.0 }),
            Some((1, Orientation::Vertical))
        );
        assert_eq!(
            g.scroll_steps(ScrollDelta::Pixels { x: 0.0, y: -40.0 }),
            Some((-2, Orientation::Vertical))
        );
    }
}
