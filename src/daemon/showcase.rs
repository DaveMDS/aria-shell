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
//! layout, the room around the parts and between them, is aria.css's
//! (`showcase`).
//!
//! ```text
//! showcase > parts > column                 its own boxes; in them, the parts' roots
//! ├─ popup > gadget.clock | gadget.themes   the calendar, a menu
//! ├─ launcher
//! ├─ exiter
//! ├─ osd.volume
//! ├─ notification.normal | notification.critical
//! ├─ panel > slot.start > gadget#showcase   the bar's buttons, in a gadget of no kind
//! │  ╰─ button > icon | label
//! ╰─ popup#showcase > buttons               every kind of button, labelled by its selector
//!    ├─ row > button[.flat|.primary|.danger|.active][:disabled] > icon | label
//!    ├─ row > button.plain[.active] > icon | label
//!    ├─ row > button.radio[.active] > check, label
//!    ╰─ tabs > tab[.active] > label
//! ```

use iced::widget::mouse_area;
use iced::window::Id;
use iced::{Event, Subscription, Task, keyboard};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::OutputId;
use icssed::{Element, Tab};

use crate::components::exiter::Exiter;
use crate::components::launcher::Launcher;
use crate::components::osd;
use crate::daemon::view::dialog_surface;
use crate::gadgets::{AnyGadget, Context};
use crate::locale::Locale;
use crate::services::notifications::{IconSource, Notification, Timeout, Urgency};
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
/// The sample toggles: the toolkit's three looks, and the shell's plain
/// one.
const TOGGLES: [&str; 4] = ["", "check", "switch", "plain"];
const TABS: usize = 3;

/// The icon the sample buttons draw.
const BUTTON_ICON: &str = "dialog-information-symbolic";

pub(crate) struct Showcase {
    pub window: Id,
    pub output: OutputId,
    launcher: Launcher,
    exiter: Exiter,
    /// Their popups show, as `popup[output=..] > gadget.<kind>`.
    gadgets: Vec<AnyGadget>,
    notifications: Vec<Notification>,
    osd: osd::Content,
    /// A press landed beside the parts: the release closes.
    pressed_outside: bool,
    /// The sample plain buttons that are on, the chosen sample radio,
    /// the sample toggles that are on, the open sample tab.
    plains: [bool; PLAINS.len()],
    radio: usize,
    toggles: [bool; TOGGLES.len()],
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
    /// a sample toggle flipped, a sample tab opened.
    Plain(usize),
    Radio(usize),
    Toggle(usize),
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
            .chain(self.gadgets.iter().flat_map(AnyGadget::icon_names))
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

/// A part, taking the presses on it.
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
        let gadgets = GADGETS
            .iter()
            .filter_map(|name| AnyGadget::create(name, &self.config, &output))
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
            toggles: [true, true, false, false],
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
            Message::Toggle(i) => {
                showcase.toggles[i] = !showcase.toggles[i];
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

    /// The parts side by side, on the showcase's surface (at `scale`),
    /// each in its root as on its own surface.
    pub(crate) fn showcase_view<'a>(
        &'a self,
        showcase: &'a Showcase,
        scale: f32,
    ) -> Element<'a, Shell> {
        let shared = self.shared();
        let theme = &self.theme;
        let output = self.output_name(showcase.output);
        let popup = theme.root("popup", scale).attr("output", output);
        let popups = showcase.gadgets.iter().map(|gadget| {
            let content = gadget.popup_view(Context { shared }).map(nothing);
            let content = icssed::column![content].kind("gadget").class(gadget.kind());
            inert(popup::view(popup, content))
        });
        let launcher = dialog_surface(
            theme.root("launcher", scale),
            showcase.launcher.view(shared).map(nothing),
            nothing(()),
        );
        let exiter = dialog_surface(
            theme.root("exiter", scale),
            showcase.exiter.view(shared).map(nothing),
            nothing(()),
        );
        let osd_node = osd::node(theme.root("osd", scale), &showcase.osd, output);
        let osd = inert(osd::view(osd_node, &self.icons, &showcase.osd, output));
        let toasts = showcase.notifications.iter().map(|n| {
            let root = theme.root("notification", scale).attr("output", output);
            inert(toast::surface(root, n, &self.notifications, &self.icons).map(nothing))
        });
        let column = |children: Vec<Element<'a, Shell>>| icssed::column(children).kind("column");
        let parts = icssed::row![
            column(popups.collect()),
            column(vec![launcher]),
            column([exiter, osd].into_iter().chain(toasts).collect()),
            column(vec![
                inert(self.bar_buttons_view(scale)),
                inert(self.buttons_view(showcase, scale)),
            ]),
        ]
        .kind("parts");
        mouse_area(icssed::column![parts].at(theme.root("showcase", scale)))
            .on_press(Shell::Showcase(Message::PressedOutside))
            .on_release(Shell::Showcase(Message::Released))
            .into()
    }

    /// A bar, with the bar's buttons in a gadget of no kind: only the
    /// rules every gadget gets apply.
    fn bar_buttons_view(&self, scale: f32) -> Element<'_, Shell> {
        let buttons = BAR_BUTTONS.iter().map(|&(icon, label)| {
            icssed::button(self.button_content(icon, label.map(str::to_owned)))
                .on_press(nothing(()))
                .into()
        });
        let gadget = icssed::row(buttons).kind("gadget").named("showcase");
        icssed::row![icssed::row![gadget].kind("slot").class("start")]
            .at(self.theme.root("panel", scale))
            .into()
    }

    /// The other kinds of buttons, in a popup's box, each labelled by
    /// the selector that styles it.
    fn buttons_view<'a>(&'a self, showcase: &Showcase, scale: f32) -> Element<'a, Shell> {
        // The rows of buttons, the plain ones', the radios', the
        // toggles', the tabs.
        let mut children: Vec<Element<'a, Shell>> = Vec::new();
        for (k, chunk) in BUTTONS.chunks(BUTTONS_ROW).enumerate() {
            // The plain ones after the flat ones.
            if k == 2 {
                children.push(icssed::row(self.plains_view(showcase)).into());
            }
            let buttons = chunk.iter().map(|&(class, enabled, icon, text)| {
                let mut label = "button".to_owned();
                if let Some(class) = class {
                    label = format!("{label}.{class}");
                }
                if !enabled {
                    label.push_str(":disabled");
                }
                let button = icssed::button(self.button_content(icon, text.then_some(label)))
                    .disabled_if(!enabled);
                let button = match class {
                    Some(class) => button.class(class),
                    None => button,
                };
                match enabled {
                    true => button.on_press(nothing(())),
                    false => button,
                }
                .into()
            });
            children.push(icssed::row(buttons).into());
        }
        let radios = (0..RADIOS).map(|i| {
            let on = i == showcase.radio;
            let label = match on {
                true => "button.radio.active",
                false => "button.radio",
            };
            icssed::button![icssed::mark().kind("check"), icssed::text(label)]
                .class("radio")
                .class_if("active", on)
                .on_press(Shell::Showcase(Message::Radio(i)))
                .into()
        });
        children.push(icssed::row(radios).into());
        children.push(icssed::row(self.toggles_view(showcase)).into());
        let tabs = (0..TABS).map(|i| {
            let active = i == showcase.tab;
            let label = match active {
                true => "tab.active",
                false => "tab",
            };
            Tab::new(label, Shell::Showcase(Message::Tab(i))).active(active)
        });
        children.push(icssed::tabs(tabs).into());
        let popup = self.theme.root("popup", scale).id("showcase");
        popup::view(popup, icssed::column(children).kind("buttons"))
    }

    /// The sample toggles, each labelled by its selector.
    fn toggles_view<'a>(&'a self, showcase: &Showcase) -> Vec<Element<'a, Shell>> {
        let icons: &'a dyn icssed::Icons = &self.icons;
        TOGGLES
            .iter()
            .enumerate()
            .map(|(i, &look)| {
                let on = showcase.toggles[i];
                let mut label = match look {
                    "" => "toggle".to_owned(),
                    look => format!("toggle.{look}"),
                };
                if on {
                    label.push_str(".checked");
                }
                let toggle = icssed::toggle(on, move |_| Shell::Showcase(Message::Toggle(i)))
                    .class_if("plain", look == "plain");
                match look {
                    "check" => toggle.check(),
                    "switch" => toggle.switch(),
                    "plain" => toggle.icon((icons, BUTTON_ICON)),
                    _ => toggle,
                }
                .label(label)
                .into()
            })
            .collect()
    }

    /// The sample plain buttons.
    fn plains_view<'a>(&'a self, showcase: &Showcase) -> Vec<Element<'a, Shell>> {
        PLAINS
            .iter()
            .enumerate()
            .map(|(i, &(icon, text))| {
                let on = showcase.plains[i];
                let label = match on {
                    true => "button.plain.active",
                    false => "button.plain",
                };
                icssed::button(self.button_content(icon, text.then(|| label.to_owned())))
                    .class("plain")
                    .class_if("active", on)
                    .on_press(Shell::Showcase(Message::Plain(i)))
                    .into()
            })
            .collect()
    }

    /// A sample button's content: the icon, the label, or both.
    fn button_content<'a>(&'a self, icon: bool, label: Option<String>) -> Vec<Element<'a, Shell>> {
        let icons: &'a dyn icssed::Icons = &self.icons;
        let mut content = Vec::new();
        if icon {
            content.push(icssed::icon((icons, BUTTON_ICON)).into());
        }
        if let Some(label) = label {
            content.push(icssed::text(label).into());
        }
        content
    }
}
