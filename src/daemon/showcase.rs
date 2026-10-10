//! `aria-shell showcase`: every part of the UI at once, on one surface,
//! to compare them while working on a theme and to take its picture.
//!
//! Not the real surfaces: the launcher and the exit menu are one dialog
//! at a time, a popup closes at the first click elsewhere, the OSD and
//! the toasts expire. Their views instead, drawn by the same code in
//! the same root boxes, each at the size its own surface would have,
//! side by side on one transparent layer surface over the focused
//! output, between its bars: what shows through is the desktop, as on
//! the real ones. Inert: what the parts would do is dropped, their
//! hover still shows; only the sample plain buttons, radios and tabs
//! follow a click. Escape, a click beside the parts or the command
//! again close it; a theme change redraws it like any surface. Its own
//! layout (the room around the parts and between them) isn't themed.
//!
//! ```text
//! (the surface)                             its own roots, side by side
//! ├─ popup > gadget.clock | gadget.themes   the calendar, a menu
//! ├─ launcher
//! ├─ exiter
//! ├─ osd.volume
//! ├─ notification.normal | notification.critical
//! ├─ panel > slot.start > gadget#showcase   the bar's buttons, in a gadget of no kind
//! │  ╰─ button > icon | text
//! ╰─ popup#showcase > buttons               every kind of button, labelled by its selector
//!    ├─ button[.flat|.primary|.danger|.active][:disabled] > icon | text
//!    ├─ button.plain[.active] > icon | text
//!    ├─ button.radio[.active] > check, label
//!    ╰─ tabs > button.tab[.active] > text
//! ```

use iced::widget::{column, container, mouse_area, row};
use iced::window::Id;
use iced::{Alignment, Element, Event, Length, Subscription, Task, keyboard};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::OutputId;

use crate::components::exiter::Exiter;
use crate::components::launcher::Launcher;
use crate::components::osd::{self, Osd};
use crate::components::panel::Panel;
use crate::daemon::view::dialog_surface;
use crate::gadgets::{AnyGadget, Context};
use crate::locale::Locale;
use crate::services::notifications::{IconSource, Notification, Timeout, Urgency};
use crate::ui::menu::{RADIO_OFF, RADIO_ON};
use crate::ui::tabs::{self, Tab};
use crate::ui::theme::{self, Node};
use crate::ui::{popup, toast};
use crate::{AriaShell, Message as Shell};

/// The gadgets whose popups show, by their config section.
const GADGETS: &[&str] = &["Clock", "Themes"];

/// The bar's sample buttons: with an icon, a text, or both.
const BAR_BUTTONS: &[(bool, Option<&str>)] = &[
    (true, None),
    (true, Some("gadget > button")),
    (false, Some("gadget > button")),
];

/// The other buttons, in rows of [`BUTTONS_ROW`]: a class, whether it's
/// enabled, and whether it has an icon, a text (its selector) or both.
const BUTTONS: &[(Option<&str>, bool, bool, bool)] = &[
    (None, true, false, true),
    (None, true, true, true),
    (None, true, true, false),
    (Some("flat"), true, false, true),
    (Some("flat"), true, true, true),
    (Some("flat"), true, true, false),
    (Some("primary"), true, false, true),
    (Some("danger"), true, false, true),
    (Some("active"), true, false, true),
    (None, false, false, true),
    (Some("primary"), false, false, true),
    (Some("danger"), false, false, true),
];
const BUTTONS_ROW: usize = 3;

/// The sample plain buttons, a click turns one on and off: whether it
/// has an icon, a text (its selector) or both.
const PLAINS: [(bool, bool); 3] = [(false, true), (true, true), (true, false)];

/// How many sample radios and tabs: a click chooses one.
const RADIOS: usize = 2;
const TABS: usize = 3;

/// The icon the sample buttons draw.
const BUTTON_ICON: &str = "dialog-information-symbolic";

/// The room around the parts and between them.
const PADDING: f32 = 24.0;
const GAP: f32 = 24.0;

/// The room around the buttons in their popup's box, and between them.
const BUTTONS_PADDING: f32 = 12.0;
const BUTTONS_GAP: f32 = 8.0;

/// An icon's size when the theme doesn't say.
const DEFAULT_ICON_SIZE: f32 = 16.0;

pub(crate) struct Showcase {
    pub window: Id,
    pub output: OutputId,
    launcher: Launcher,
    exiter: Exiter,
    /// With their popup's node, `popup[output=..] > gadget.<kind>`.
    gadgets: Vec<(AnyGadget, Node)>,
    notifications: Vec<Notification>,
    osd: osd::Content,
    /// A press landed beside the parts: the release closes.
    pressed_outside: bool,
    /// The sample plain buttons that are on, the chosen sample radio,
    /// the open sample tab.
    plains: [bool; PLAINS.len()],
    radio: usize,
    tab: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// A press on a part, or what a part would do: nothing.
    Nothing,
    /// A press beside the parts.
    PressedOutside,
    Released,
    /// Escape, on window `Id`.
    Escape(Id),
    /// A sample plain button turned on or off, a sample radio chosen,
    /// a sample tab opened.
    Plain(usize),
    Radio(usize),
    Tab(usize),
}

impl Showcase {
    /// Icon names the parts draw, for the daemon to resolve.
    pub fn icon_names(&self) -> impl Iterator<Item = String> + '_ {
        let notifications = self.notifications.iter().filter_map(|n| match &n.icon {
            Some(IconSource::Name(name)) => Some(name.clone()),
            _ => None,
        });
        self.launcher
            .icon_names()
            .chain(self.exiter.icon_names())
            .chain(self.osd.icon.as_deref())
            .map(str::to_owned)
            .chain(notifications)
            .chain(self.gadgets.iter().flat_map(|(g, _)| g.icon_names()))
            .chain([BUTTON_ICON.to_owned()])
    }

    /// Desktop ids of the launcher's results, for icon resolution.
    pub fn app_ids(&self) -> impl Iterator<Item = &str> {
        self.launcher.visible_ids()
    }

    pub fn set_apps(&mut self, apps: std::sync::Arc<crate::services::icons::Index>) {
        self.launcher.set_apps(apps);
    }
}

/// Escape on any window; the daemon checks it's the showcase's.
pub(crate) fn subscription() -> Subscription<Message> {
    iced::event::listen_with(|event, _, window| match event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) => Some(Message::Escape(window)),
        _ => None,
    })
}

/// A normal notification with an action, a critical one without.
fn notifications(locale: &Locale) -> Vec<Notification> {
    let sample = |id: u32, urgency, icon: &str, summary, body, actions| Notification {
        id,
        serial: 0,
        app_name: "aria-shell".to_owned(),
        summary: locale.tr(summary).to_owned(),
        body: locale.tr(body).to_owned(),
        icon: Some(IconSource::Name(icon.to_owned())),
        image: None,
        urgency,
        actions,
        timeout: Timeout::Never,
    };
    // Ids no real notification gets soon, for `debug widgets`.
    vec![
        sample(
            u32::MAX,
            Urgency::Normal,
            "dialog-information-symbolic",
            "showcase.notification.summary",
            "showcase.notification.body",
            vec![(
                "open".to_owned(),
                locale.tr("showcase.notification.action").to_owned(),
            )],
        ),
        sample(
            u32::MAX - 1,
            Urgency::Critical,
            "dialog-warning-symbolic",
            "showcase.critical.summary",
            "showcase.critical.body",
            Vec::new(),
        ),
    ]
}

/// What a part would do, dropped.
fn nothing<T>(_: T) -> Shell {
    Shell::Showcase(Message::Nothing)
}

/// A part, at its surface's size, taking the presses on it.
fn part(content: Element<'_, Shell>, (w, h): (u32, u32)) -> Element<'_, Shell> {
    container(inert(content))
        .width(w as f32)
        .height(h as f32)
        .into()
}

/// `content`, taking the presses on it.
fn inert(content: Element<'_, Shell>) -> Element<'_, Shell> {
    mouse_area(content).on_press(nothing(())).into()
}

impl AriaShell {
    /// Show the showcase on the focused output (the first one if the
    /// compositor didn't say); the dialogs and the popups go, it would
    /// hide them.
    pub(crate) fn open_showcase(&mut self) -> Task<Shell> {
        let Some(output) = self.focused_output().cloned() else {
            log::warn!("no output to show the showcase on");
            return Task::none();
        };
        let window = Id::unique();
        log::info!(
            "opening the showcase on output {:?} as window {window:?}",
            output.name
        );
        let popup_root =
            Node::root("popup").attr("output", output.name.clone().unwrap_or_default());
        let gadgets = GADGETS
            .iter()
            .filter_map(|name| {
                let gadget = AnyGadget::create(name, &self.config, &output)?;
                let node = popup_root.child("gadget").class(gadget.kind());
                Some((gadget, node))
            })
            .collect();
        self.showcase = Some(Showcase {
            window,
            output: OutputId::from(&output),
            launcher: Launcher::new(
                self.config.section(None),
                self.config.section(None),
                self.general.terminal.clone(),
                self.icons.index(),
            ),
            exiter: Exiter::new(self.config.section(None)),
            gadgets,
            notifications: notifications(&self.locale),
            osd: osd::Content {
                kind: osd::Kind::Volume,
                icon: Some("audio-volume-medium-symbolic".to_owned()),
                value: Some(40),
                text: None,
                muted: false,
                outputs: None,
            },
            pressed_outside: false,
            plains: [false, true, false],
            radio: 0,
            tab: 0,
        });
        self.resolve_icons();
        let settings = NewLayerShellSettings {
            anchor: Anchor::all(),
            size: LayerSize::FILL,
            layer: Layer::Top,
            // Between the bars, not over them.
            exclusive_zone: None,
            margin: None,
            // Escape once clicked; the keyboard stays where it was,
            // for the editor with the theme open.
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            output_option: OutputOption::GlobalName(output.id),
            namespace: Some("aria-showcase".to_owned()),
            ..Default::default()
        };
        Task::batch([
            self.close_launcher(),
            self.close_exiter(),
            self.close_popups(None),
            Task::done(Shell::NewLayerShell {
                settings,
                id: window,
            }),
        ])
    }

    pub(crate) fn close_showcase(&mut self) -> Task<Shell> {
        match self.showcase.take() {
            Some(showcase) => Task::done(Shell::RemoveWindow(showcase.window)),
            None => Task::none(),
        }
    }

    pub(crate) fn showcase_update(&mut self, message: Message) -> Task<Shell> {
        let Some(showcase) = &mut self.showcase else {
            return Task::none();
        };
        match message {
            Message::Nothing => Task::none(),
            Message::Plain(i) => {
                showcase.plains[i] = !showcase.plains[i];
                Task::none()
            }
            Message::Radio(i) => {
                showcase.radio = i;
                Task::none()
            }
            Message::Tab(i) => {
                showcase.tab = i;
                Task::none()
            }
            Message::PressedOutside => {
                showcase.pressed_outside = true;
                Task::none()
            }
            // On the release: closing on the press destroys the surface
            // before its release, which Hyprland then gives the next
            // click (see `dialog`).
            Message::Released if std::mem::take(&mut showcase.pressed_outside) => {
                self.close_showcase()
            }
            Message::Escape(window) if window == showcase.window => self.close_showcase(),
            Message::Released | Message::Escape(_) => Task::none(),
        }
    }

    /// The parts in three columns, centred: the popups, the launcher,
    /// then the exit menu, the OSD and the toasts.
    pub(crate) fn showcase_view<'a>(&'a self, showcase: &'a Showcase) -> Element<'a, Shell> {
        let shared = self.shared();
        let theme = &self.theme;
        let output = self.output_name(showcase.output);
        let popups = showcase.gadgets.iter().map(|(gadget, node)| {
            let ctx = Context {
                shared,
                node: node.clone(),
            };
            let size = popup::surface_size(theme, gadget.popup_size(ctx.clone()));
            let box_node = node
                .parent()
                .cloned()
                .unwrap_or_else(|| Node::root("popup"));
            part(
                popup::view(theme, &box_node, gadget.popup_view(ctx).map(nothing)),
                size,
            )
        });
        let launcher = part(
            dialog_surface(
                theme,
                "launcher",
                showcase.launcher.view(shared).map(nothing),
                nothing(()),
            ),
            Launcher::size(theme),
        );
        let exiter = part(
            dialog_surface(
                theme,
                "exiter",
                showcase.exiter.view(shared).map(nothing),
                nothing(()),
            ),
            showcase.exiter.size(theme, &self.locale),
        );
        let osd = part(
            osd::view(theme, &self.icons, &showcase.osd, output),
            Osd::size(theme),
        );
        let toasts = showcase.notifications.iter().map(|n| {
            let node = toast::node(n, output);
            let size = toast::surface_size(theme, &node, n, &self.notifications, &self.icons);
            part(
                toast::surface(theme, &node, n, &self.notifications, &self.icons).map(nothing),
                size,
            )
        });
        let parts = row![
            column(popups).spacing(GAP),
            launcher,
            column![exiter, osd].extend(toasts).spacing(GAP),
            column![
                inert(self.bar_buttons_view()),
                inert(self.buttons_view(showcase))
            ]
            .spacing(GAP),
        ]
        .spacing(GAP);
        let content = container(parts)
            .padding(PADDING)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center);
        mouse_area(content)
            .on_press(Shell::Showcase(Message::PressedOutside))
            .on_release(Shell::Showcase(Message::Released))
            .into()
    }

    /// A bar as thick as the real ones, with the bar's buttons in a
    /// gadget of no kind: only the rules every gadget gets apply.
    fn bar_buttons_view(&self) -> Element<'_, Shell> {
        let theme = &self.theme;
        let panel = Node::root("panel");
        let slot = panel.child("slot").class("start");
        let gadget = slot.child("gadget").id("showcase");
        let count = BAR_BUTTONS.len();
        let buttons = BAR_BUTTONS.iter().enumerate().map(|(i, &(icon, label))| {
            let node = gadget.child("button").nth(i, count);
            let content = self.button_content(&node, icon, label.map(str::to_owned));
            theme.button(&node, content).on_press(nothing(())).into()
        });
        let buttons = row(buttons)
            .spacing(theme.resolve(&gadget).gap)
            .height(Length::Fill)
            .align_y(Alignment::Center);
        let slot = theme
            .row(&slot, [theme.container(&gadget, buttons).into()])
            .height(Length::Fill)
            .align_y(Alignment::Center);
        theme
            .container(&panel, slot)
            .height(Panel::themed_height(theme, &panel) as f32)
            .into()
    }

    /// The other kinds of buttons, in a popup's box, each labelled by
    /// the selector that styles it.
    fn buttons_view<'a>(&'a self, showcase: &Showcase) -> Element<'a, Shell> {
        let theme = &self.theme;
        let popup = Node::root("popup").id("showcase");
        let list = popup.child("buttons");
        let count = BUTTONS.len();
        let mut buttons = BUTTONS
            .iter()
            .enumerate()
            .map(|(i, &(class, enabled, icon, text))| {
                let mut node = list.child("button").nth(i, count).disabled_if(!enabled);
                let mut label = "button".to_owned();
                if let Some(class) = class {
                    node = node.class(class);
                    label = format!("{label}.{class}");
                }
                if !enabled {
                    label.push_str(":disabled");
                }
                let content = self.button_content(&node, icon, text.then_some(label));
                theme
                    .button(&node, content)
                    .on_press_maybe(enabled.then(|| nothing(())))
                    .into()
            })
            .collect::<Vec<Element<'_, Shell>>>();
        let mut rows: Vec<Element<'_, Shell>> = Vec::new();
        while !buttons.is_empty() {
            let rest = buttons.split_off(BUTTONS_ROW.min(buttons.len()));
            let first = std::mem::replace(&mut buttons, rest);
            rows.push(
                row(first)
                    .spacing(BUTTONS_GAP)
                    .align_y(Alignment::Center)
                    .into(),
            );
        }
        let plains = PLAINS.iter().enumerate().map(|(i, &(icon, text))| {
            let on = showcase.plains[i];
            let node = list
                .child("button")
                .class("plain")
                .class_if("active", on)
                .nth(i, PLAINS.len());
            let label = if on {
                "button.plain.active"
            } else {
                "button.plain"
            };
            let content = self.button_content(&node, icon, text.then(|| label.to_owned()));
            theme
                .button(&node, content)
                .on_press(Shell::Showcase(Message::Plain(i)))
                .into()
        });
        // After the flat ones.
        rows.insert(
            2,
            row(plains)
                .spacing(BUTTONS_GAP)
                .align_y(Alignment::Center)
                .into(),
        );
        let radios = (0..RADIOS).map(|i| {
            let on = i == showcase.radio;
            let node = list
                .child("button")
                .class("radio")
                .class_if("active", on)
                .nth(i, RADIOS);
            let (glyph, label) = match on {
                true => (RADIO_ON, "button.radio.active"),
                false => (RADIO_OFF, "button.radio"),
            };
            let content = row![
                theme.text(&node.child("check"), glyph),
                theme.text(&node.child("label"), label),
            ]
            .spacing(theme.resolve(&node).gap)
            .align_y(Alignment::Center);
            theme
                .button(&node, content)
                .on_press(Shell::Showcase(Message::Radio(i)))
                .into()
        });
        rows.push(row(radios).spacing(BUTTONS_GAP).into());
        let tabs = (0..TABS).map(|i| {
            let active = i == showcase.tab;
            Tab {
                name: None,
                label: if active {
                    "button.tab.active"
                } else {
                    "button.tab"
                },
                active,
                on_press: Shell::Showcase(Message::Tab(i)),
            }
        });
        let tabs = tabs::view(theme, &list.child("tabs"), tabs, Length::Shrink);
        let content = theme.container(
            &popup,
            column(rows)
                .push(tabs)
                .spacing(BUTTONS_GAP)
                .padding(BUTTONS_PADDING),
        );
        container(content).padding(theme.shadow_room(&popup)).into()
    }

    /// A sample button's content: the icon, the label, or both.
    fn button_content(&self, node: &Node, icon: bool, label: Option<String>) -> Element<'_, Shell> {
        let mut content: Vec<Element<'_, Shell>> = Vec::new();
        if icon {
            content.push(self.button_icon(&node.child("icon")));
        }
        if let Some(label) = label {
            content.push(self.theme.text(&node.child("text"), label).into());
        }
        row(content)
            .spacing(self.theme.resolve(node).gap)
            .align_y(Alignment::Center)
            .into()
    }

    /// A sample button's icon, at the theme's `height` for `node`.
    fn button_icon(&self, node: &Node) -> Element<'_, Shell> {
        let style = self.theme.resolve(node);
        let size = match style.height.or(style.width) {
            Some(theme::Length::Px(px)) => px,
            _ => DEFAULT_ICON_SIZE,
        };
        match self.icons.get_name(BUTTON_ICON, None) {
            Some(icon) => icon.view(size, style.color),
            None => iced::widget::Space::new().width(size).height(size).into(),
        }
    }
}
