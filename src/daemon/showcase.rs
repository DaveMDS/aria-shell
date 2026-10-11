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
use icssed::{Element, Node, Tab};

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
            let node = popup.child("gadget").class(gadget.kind());
            let content = gadget.popup_view(Context { shared, node }).map(nothing);
            inert(popup::view(popup, content))
        });
        let launcher = theme.root("launcher", scale);
        let launcher = dialog_surface(
            launcher,
            showcase.launcher.view(shared, launcher).map(nothing),
            nothing(()),
        );
        let exiter = theme.root("exiter", scale);
        let exiter = dialog_surface(
            exiter,
            showcase.exiter.view(shared, exiter).map(nothing),
            nothing(()),
        );
        let osd_node = osd::node(theme.root("osd", scale), &showcase.osd, output);
        let osd = inert(osd::view(osd_node, &self.icons, &showcase.osd, output));
        let toasts = showcase.notifications.iter().map(|n| {
            let node = toast::node(theme.root("notification", scale), n, output);
            inert(toast::surface(node, n, &self.notifications, &self.icons).map(nothing))
        });
        let root = theme.root("showcase", scale);
        let parts = root.child("parts");
        let column = |i: usize, children: Vec<Element<'a, Shell>>| -> Element<'a, Shell> {
            icssed::column(parts.child_at("column", i, 4), children).into()
        };
        let parts = icssed::row(
            parts,
            [
                column(0, popups.collect()),
                column(1, vec![launcher]),
                column(2, [exiter, osd].into_iter().chain(toasts).collect()),
                column(
                    3,
                    vec![
                        inert(self.bar_buttons_view(scale)),
                        inert(self.buttons_view(showcase, scale)),
                    ],
                ),
            ],
        );
        mouse_area(icssed::column(root, [parts.into()]))
            .on_press(Shell::Showcase(Message::PressedOutside))
            .on_release(Shell::Showcase(Message::Released))
            .into()
    }

    /// A bar, with the bar's buttons in a gadget of no kind: only the
    /// rules every gadget gets apply.
    fn bar_buttons_view(&self, scale: f32) -> Element<'_, Shell> {
        let panel = self.theme.root("panel", scale);
        let slot = panel.child("slot").class("start");
        let gadget = slot.child_at("gadget", 0, 1).id("showcase");
        let count = BAR_BUTTONS.len();
        let buttons = BAR_BUTTONS.iter().enumerate().map(|(i, &(icon, label))| {
            let node = gadget.child_at("button", i, count);
            let content = self.button_content(node, icon, label.map(str::to_owned));
            icssed::button(node, content).on_press(nothing(())).into()
        });
        let gadget: Element<'_, Shell> = icssed::row(gadget, buttons).into();
        icssed::row(panel, [icssed::row(slot, [gadget]).into()]).into()
    }

    /// The other kinds of buttons, in a popup's box, each labelled by
    /// the selector that styles it.
    fn buttons_view<'a>(&'a self, showcase: &Showcase, scale: f32) -> Element<'a, Shell> {
        let popup = self.theme.root("popup", scale).id("showcase");
        let list = popup.child("buttons");
        // The rows of buttons, the plain ones', the radios', the tabs.
        let count = BUTTONS.len().div_ceil(BUTTONS_ROW) + 3;
        let mut children: Vec<Element<'a, Shell>> = Vec::new();
        for (k, chunk) in BUTTONS.chunks(BUTTONS_ROW).enumerate() {
            // The plain ones after the flat ones.
            if k == 2 {
                let row = list.child_at("row", children.len(), count);
                children.push(icssed::row(row, self.plains_view(showcase, row)).into());
            }
            let row = list.child_at("row", children.len(), count);
            let buttons = chunk
                .iter()
                .enumerate()
                .map(|(i, &(class, enabled, icon, text))| {
                    let mut node = row.child_at("button", i, chunk.len()).disabled_if(!enabled);
                    let mut label = "button".to_owned();
                    if let Some(class) = class {
                        node = node.class(class);
                        label = format!("{label}.{class}");
                    }
                    if !enabled {
                        label.push_str(":disabled");
                    }
                    let content = self.button_content(node, icon, text.then_some(label));
                    let button = icssed::button(node, content);
                    match enabled {
                        true => button.on_press(nothing(())),
                        false => button,
                    }
                    .into()
                });
            children.push(icssed::row(row, buttons).into());
        }
        let row = list.child_at("row", children.len(), count);
        let radios = (0..RADIOS).map(|i| {
            let on = i == showcase.radio;
            let node = row
                .child_at("button", i, RADIOS)
                .class("radio")
                .class_if("active", on);
            let label = match on {
                true => "button.radio.active",
                false => "button.radio",
            };
            icssed::button(
                node,
                [
                    icssed::mark(node.child("check")),
                    icssed::text(node.child("label"), label).into(),
                ],
            )
            .on_press(Shell::Showcase(Message::Radio(i)))
            .into()
        });
        children.push(icssed::row(row, radios).into());
        let tabs = (0..TABS).map(|i| {
            let active = i == showcase.tab;
            let label = match active {
                true => "tab.active",
                false => "tab",
            };
            Tab::new(label, Shell::Showcase(Message::Tab(i))).active(active)
        });
        let node = list.child_at("tabs", children.len(), count);
        children.push(icssed::tabs(node, tabs));
        popup::view(popup, icssed::column(list, children).into())
    }

    /// The sample plain buttons, in their row `row`.
    fn plains_view<'a>(&'a self, showcase: &Showcase, row: Node<'a>) -> Vec<Element<'a, Shell>> {
        PLAINS
            .iter()
            .enumerate()
            .map(|(i, &(icon, text))| {
                let on = showcase.plains[i];
                let node = row
                    .child_at("button", i, PLAINS.len())
                    .class("plain")
                    .class_if("active", on);
                let label = match on {
                    true => "button.plain.active",
                    false => "button.plain",
                };
                let content = self.button_content(node, icon, text.then(|| label.to_owned()));
                icssed::button(node, content)
                    .on_press(Shell::Showcase(Message::Plain(i)))
                    .into()
            })
            .collect()
    }

    /// A sample button's content, in its box `node`: the icon, the
    /// label, or both.
    fn button_content<'a>(
        &'a self,
        node: Node<'a>,
        icon: bool,
        label: Option<String>,
    ) -> Vec<Element<'a, Shell>> {
        let icons: &'a dyn icssed::Icons = &self.icons;
        let mut content = Vec::new();
        if icon {
            content.push(icssed::icon(node.child("icon"), (icons, BUTTON_ICON)).into());
        }
        if let Some(label) = label {
            content.push(icssed::text(node.child("label"), label).into());
        }
        content
    }
}
