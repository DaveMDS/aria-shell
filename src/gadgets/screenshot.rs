//! Screenshot gadget: an icon. A left click opens the picker, a right
//! click a menu taking the active window, the bar's screen or every
//! screen at once. Its settings are the daemon's `[Screenshot]`
//! section, `icon` its own.

use iced::widget::mouse_area;
use iced_wayland_subscriber::OutputInfo;
use icssed::Element;

use crate::gadgets::{Action, Context, Gadget, Popup};
use crate::locale::Locale;
use crate::services::screenshot::{Command, Destination, ScreenshotConfig, Target};
use crate::ui::menu::{self, Item, Menu};

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
        let button = mouse_area(
            icssed::button![ctx.icon(self.icon.as_str())]
                .kind("indicator")
                .on_press(Message::Pick)
                .id(self.popup.anchor()),
        )
        .on_right_press(Message::OpenMenu);
        icssed::row![button].into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_closed(&mut self) {
        self.menu.reset();
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        self.menu.view(items(ctx.locale)).map(Message::Menu)
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
