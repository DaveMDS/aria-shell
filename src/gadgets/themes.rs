//! Themes gadget: an icon showing the colour scheme in use. A left
//! click toggles light/dark, a right click opens a menu with the two
//! schemes and every theme found in the theme directories (the base
//! alone first); picking one restyles the shell live. Nothing is
//! persisted: `[general] style` / `color_scheme` rule again at the next
//! start.
//!
//! Holds no theme state: the current scheme and theme name come from
//! `ctx.theme`; the list is rescanned when the menu opens.

use iced::widget::mouse_area;
use iced_wayland_subscriber::OutputInfo;
use icssed::Element;

use crate::config::{RawSection, Section};
use crate::gadgets::{Action, Context, Gadget, Popup};
use crate::locale::Locale;
use crate::ui::menu::{self, Item, Menu};
use crate::ui::theme::{self, Command, Scheme};

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
        let button = ctx.node.child("button");
        let name = match ctx.theme.scheme() {
            Scheme::Light => &self.config.light_icon,
            Scheme::Dark => &self.config.dark_icon,
        };
        let icons: &dyn icssed::Icons = ctx.icons;
        let icon = icssed::icon(button.child("icon"), (icons, name.as_str()));
        let button = mouse_area(
            icssed::button(button, [icon.into()])
                .on_press(Message::Toggle)
                .id(self.popup.anchor()),
        )
        .on_right_press(Message::OpenMenu);
        icssed::row(ctx.node, [button.into()]).into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_closed(&mut self) {
        self.menu.reset();
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        self.menu
            .view(ctx.node.child("menu"), self.items(ctx.theme, ctx.locale))
            .map(Message::Menu)
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
