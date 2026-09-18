//! Themes gadget: an icon showing the colour scheme in use. A left
//! click toggles light/dark, a right click opens a menu with the two
//! schemes and every theme found in the theme directories (the base
//! alone first); picking one restyles the shell live. Nothing is
//! persisted: `[general] style` / `color_scheme` rule again at the next
//! start.
//!
//! Holds no theme state: the current scheme and theme name come from
//! `ctx.theme`; the list is rescanned when the menu opens.

use iced::Element;
use iced::widget::{Space, mouse_area};
use iced_wayland_subscriber::OutputInfo;

use crate::config::{RawSection, Section};
use crate::gadget::{Action, Context, Gadget, Popup};
use crate::locale::Locale;
use crate::theme::{self, Command, Scheme};
use crate::widgets::menu::{self, Item, Menu};

/// `[Themes]` section.
#[derive(Debug, Clone)]
pub struct ThemesConfig {
    /// Icon names (from the icon theme) shown while each scheme is on.
    pub light_icon: String,
    pub dark_icon: String,
}

impl Section for ThemesConfig {
    const NAME: &'static str = "Themes";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            light_icon: raw.str_or("light_icon", "weather-clear-symbolic"),
            dark_icon: raw.str_or("dark_icon", "weather-clear-night-symbolic"),
        }
    }
}

/// Icon size when the theme doesn't set `height` on `icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;

/// Row ids: the two schemes, the base, then the themes by index.
const ID_LIGHT: i32 = -1;
const ID_DARK: i32 = -2;
const ID_SEPARATOR: i32 = -3;
const ID_BASE: i32 = 0;

pub struct Themes {
    config: ThemesConfig,
    popup: Popup,
    menu: Menu,
    /// Theme names found when the menu was opened.
    available: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Message {
    Toggle,
    OpenMenu,
    Menu(menu::Message),
}

impl Gadget for Themes {
    type Config = ThemesConfig;
    type Message = Message;

    fn new(config: ThemesConfig, _output: &OutputInfo) -> Self {
        Self {
            config,
            popup: Popup::new(),
            menu: Menu::new(),
            available: Vec::new(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Toggle => Action::Theme(Command::ToggleScheme),
            Message::OpenMenu => {
                if !self.popup.is_open() {
                    self.available = theme::available()
                        .into_iter()
                        .map(|(name, _)| name)
                        .collect();
                }
                self.popup.toggle()
            }
            Message::Menu(m) => match self.menu.update(m) {
                menu::Event::Clicked(ID_LIGHT) => self.pick(Command::SetScheme(Scheme::Light)),
                menu::Event::Clicked(ID_DARK) => self.pick(Command::SetScheme(Scheme::Dark)),
                menu::Event::Clicked(ID_BASE) => self.pick(Command::SetStyle(None)),
                menu::Event::Clicked(id) => match self.available.get((id - 1) as usize) {
                    Some(name) => self.pick(Command::SetStyle(Some(name.clone()))),
                    None => Action::None,
                },
                menu::Event::None | menu::Event::Unfolded(_) => Action::None,
            },
        }
    }

    fn icon_names(&self) -> Vec<String> {
        vec![
            self.config.light_icon.clone(),
            self.config.dark_icon.clone(),
        ]
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let button = ctx.node.child("button");
        let icon_node = button.child("icon");
        let style = theme.resolve(&icon_node);
        let size = style
            .height
            .or(style.width)
            .and_then(|l| match l {
                theme::Length::Px(px) => Some(px),
                _ => None,
            })
            .unwrap_or(DEFAULT_ICON_SIZE);
        let name = match theme.scheme() {
            Scheme::Light => &self.config.light_icon,
            Scheme::Dark => &self.config.dark_icon,
        };
        let icon: Element<'a, Message> = match ctx.icons.get_name(name, None) {
            Some(icon) => icon.view(size, style.color),
            None => Space::new().width(size).height(size).into(),
        };
        let button = theme.button(&button, icon).on_press(Message::Toggle);
        mouse_area(self.popup.anchor(button))
            .on_right_press(Message::OpenMenu)
            .into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_closed(&mut self) {
        self.menu.reset();
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let node = ctx.node.child("menu");
        self.menu
            .view(ctx.theme, &node, self.items(ctx.theme, ctx.locale))
            .map(Message::Menu)
    }

    fn popup_size(&self, ctx: Context<'_>) -> (u32, u32) {
        let node = ctx.node.child("menu");
        let size = self
            .menu
            .size(ctx.theme, &node, &self.items(ctx.theme, ctx.locale));
        (
            size.width.ceil().max(1.0) as u32,
            size.height.ceil().max(1.0) as u32,
        )
    }
}

impl Themes {
    fn pick(&mut self, command: Command) -> Action<Message> {
        Action::Many(vec![Action::Theme(command), self.popup.close()])
    }

    /// The menu rows for the current state.
    fn items(&self, theme: &theme::Theme, locale: &Locale) -> Vec<Item> {
        let scheme = theme.scheme();
        let current = theme.name();
        let mut items = vec![
            Item::new(ID_LIGHT, locale.tr("themes.light")).radio(scheme == Scheme::Light),
            Item::new(ID_DARK, locale.tr("themes.dark")).radio(scheme == Scheme::Dark),
            Item::separator(ID_SEPARATOR),
            Item::new(ID_BASE, locale.tr("themes.base")).radio(current.is_none()),
        ];
        items.extend(self.available.iter().enumerate().map(|(i, name)| {
            Item::new(i as i32 + 1, name.as_str()).radio(current == Some(name.as_str()))
        }));
        items
    }
}
