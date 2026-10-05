//! Screenshot gadget: an icon. A left click opens the picker, a right
//! click a menu taking the active window, the bar's screen or every
//! screen at once. Its settings are the daemon's `[Screenshot]`
//! section, `icon` its own.

use iced::Element;
use iced::widget::{Space, mouse_area};
use iced_wayland_subscriber::OutputInfo;

use crate::gadget::{Action, Context, Gadget, Popup};
use crate::locale::Locale;
use crate::screenshot::{Command, Destination, ScreenshotConfig, Target};
use crate::theme;
use crate::widgets::menu::{self, Item, Menu};

/// Icon size when the theme doesn't set `height` on `icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;

const ID_WINDOW: i32 = 0;
const ID_SCREEN: i32 = 1;
const ID_ALL: i32 = 2;

pub struct ScreenshotGadget {
    icon: String,
    /// The bar's output, for "this screen".
    output: Option<String>,
    popup: Popup,
    menu: Menu,
}

#[derive(Clone, Debug)]
pub enum Message {
    Pick,
    OpenMenu,
    Menu(menu::Message),
}

impl Gadget for ScreenshotGadget {
    type Config = ScreenshotConfig;
    type Message = Message;

    fn new(config: ScreenshotConfig, output: &OutputInfo) -> Self {
        Self {
            icon: config.icon,
            output: output.name.clone(),
            popup: Popup::new(),
            menu: Menu::new(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Pick => self.take(Target::Pick),
            Message::OpenMenu => self.popup.toggle(),
            Message::Menu(m) => match self.menu.update(m) {
                menu::Event::Clicked(ID_WINDOW) => self.take(Target::Window),
                menu::Event::Clicked(ID_SCREEN) => self.take(Target::Output(self.output.clone())),
                menu::Event::Clicked(ID_ALL) => self.take(Target::All),
                _ => Action::None,
            },
        }
    }

    fn icon_names(&self) -> Vec<String> {
        vec![self.icon.clone()]
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
        let icon: Element<'a, Message> = match ctx.icons.get_name(&self.icon, None) {
            Some(icon) => icon.view(size, style.color),
            None => Space::new().width(size).height(size).into(),
        };
        let button = theme.button(&button, icon).on_press(Message::Pick);
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
            .view(ctx.theme, &node, items(ctx.locale))
            .map(Message::Menu)
    }

    fn popup_size(&self, ctx: Context<'_>) -> (u32, u32) {
        let node = ctx.node.child("menu");
        let size = self.menu.size(ctx.theme, &node, &items(ctx.locale));
        (
            size.width.ceil().max(1.0) as u32,
            size.height.ceil().max(1.0) as u32,
        )
    }
}

impl ScreenshotGadget {
    /// The menu closes first: the daemon waits for it to be gone
    /// before capturing (see `main.rs`, `Action::Screenshot`).
    fn take(&mut self, target: Target) -> Action<Message> {
        let command = Command {
            target,
            destination: Destination::File { edit: false },
        };
        Action::Many(vec![self.popup.close(), Action::Screenshot(command)])
    }
}

fn items(locale: &Locale) -> Vec<Item> {
    vec![
        Item::new(ID_WINDOW, locale.tr("screenshot.window")),
        Item::new(ID_SCREEN, locale.tr("screenshot.screen")),
        Item::new(ID_ALL, locale.tr("screenshot.all")),
    ]
}
