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
//! hover still shows. Escape, a click beside the parts or the command
//! again close it; a theme change redraws it like any surface. Its own
//! layout, the room around the parts and between them, is aria.css's
//! (`showcase`).
//!
//! ```text
//! showcase > parts                           its own boxes; in them, the parts' roots
//! ├─ panel#showcase > slot                   a bar over the columns, of samples:
//! │  ├─ gadget#showcase > button             of no kind, only what every gadget gets
//! │  ╰─ gadget.<kind> > button.<state>       a gadget's states (.active, .low, .muted, ...),
//! │     ╰─ icon | label                      their selector in a tooltip
//! ╰─ columns > column
//!    ├─ popup > gadget.clock | gadget.themes  the calendar, a menu
//!    ├─ launcher
//!    ├─ exiter
//!    ├─ osd.volume
//!    ╰─ notification.normal | notification.critical
//! ```

use iced::widget::mouse_area;
use iced::window::Id;
use iced::{Event, Subscription, Task, keyboard};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::OutputId;
use icssed::Element;

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

/// A sample button on the bar: its classes, its icon (with a class of
/// its own), its label. Not a gadget's view: only what the theme's rules
/// meet, `gadget.<kind> > button.<state> > icon | label`.
#[derive(Clone, Copy)]
struct Sample {
    classes: &'static [&'static str],
    icon: Option<&'static str>,
    icon_class: Option<&'static str>,
    label: Option<&'static str>,
}

const fn sample(
    classes: &'static [&'static str],
    icon: Option<&'static str>,
    label: Option<&'static str>,
) -> Sample {
    Sample {
        classes,
        icon,
        icon_class: None,
        label,
    }
}

/// A sample gadget: its kind (`None`: `gadget#showcase`, of no kind),
/// its buttons.
type SampleGadget = (Option<&'static str>, &'static [Sample]);

/// The bar's slots and their gadgets.
#[rustfmt::skip]
const BAR: &[(&str, &[SampleGadget])] = &[
    ("start", &[
        (Some("workspaces"), &[
            sample(&["workspace", "active"], None, Some("1")),
            sample(&["workspace"], None, Some("2")),
            sample(&["workspace", "urgent"], None, Some("3")),
        ]),
        (None, &[
            sample(&[], Some("dialog-information-symbolic"), None),
            sample(&[], Some("dialog-information-symbolic"), Some("gadget > button")),
            sample(&[], None, Some("gadget > button")),
        ]),
    ]),
    ("center", &[
        (Some("system-monitor"), &[
            sample(&["mem", "warning"], Some("computer-symbolic"), Some("78%")),
            sample(&["temp", "critical"], Some("computer-symbolic"), Some("92°C")),
        ]),
        (Some("clock"), &[sample(&[], None, Some("10:42"))]),
        (Some("custom"), &[sample(&["warning"], Some("dialog-warning-symbolic"), Some("!"))]),
        (Some("tray"), &[sample(&["attention"], Some("mail-unread-symbolic"), None)]),
    ]),
    ("end", &[
        (Some("notifications"), &[
            sample(&["new"], Some("preferences-system-notifications-symbolic"), Some("3")),
            sample(&["dnd"], Some("notifications-disabled-symbolic"), None),
        ]),
        (Some("network"), &[
            sample(&["limited"], Some("network-wireless-no-route-symbolic"), None),
            sample(&["disconnected"], Some("network-offline-symbolic"), None),
        ]),
        (Some("audio"), &[
            sample(&["muted"], Some("audio-volume-muted-symbolic"), Some("40%")),
        ]),
        (Some("brightness"), &[
            sample(&["none"], Some("display-brightness-symbolic"), None),
        ]),
        (Some("power"), &[
            Sample { icon_class: Some("battery"), ..sample(&["status", "discharging", "low"], Some("battery-level-20-symbolic"), Some("15%")) },
            Sample { icon_class: Some("battery"), ..sample(&["status", "discharging", "critical"], Some("battery-caution-symbolic"), Some("4%")) },
            sample(&["idle", "inhibited"], Some("view-reveal-symbolic"), None),
        ]),
    ]),
];

/// A sample button's selector, `gadget.<kind> > button.<class>...`,
/// down to its icon when the icon has a class.
fn selector(kind: Option<&str>, sample: &Sample) -> String {
    let gadget = match kind {
        Some(kind) => format!("gadget.{kind}"),
        None => "gadget#showcase".to_owned(),
    };
    let classes: String = sample.classes.iter().map(|c| format!(".{c}")).collect();
    let mut selector = format!("{gadget} > button{classes}");
    if let Some(class) = sample.icon_class {
        selector.push_str(&format!(" > icon.{class}"));
    }
    selector
}

/// The samples' icons.
fn sample_icons() -> impl Iterator<Item = &'static str> {
    BAR.iter()
        .flat_map(|(_, gadgets)| gadgets.iter())
        .flat_map(|(_, samples)| samples.iter())
        .filter_map(|s| s.icon)
}

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
            .chain(sample_icons().map(str::to_owned))
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
        let columns = icssed::row![
            column(popups.collect()),
            column(vec![launcher]),
            column([exiter, osd].into_iter().chain(toasts).collect()),
        ]
        .kind("columns");
        let parts = icssed::column![inert(self.bar_view(scale)), columns].kind("parts");
        mouse_area(icssed::column![parts].at(theme.root("showcase", scale)))
            .on_press(Shell::Showcase(Message::PressedOutside))
            .on_release(Shell::Showcase(Message::Released))
            .into()
    }

    /// A bar of samples, slot by slot.
    fn bar_view(&self, scale: f32) -> Element<'_, Shell> {
        let slots = BAR.iter().map(|&(slot, gadgets)| {
            let gadgets = gadgets.iter().map(|&(kind, samples)| {
                let buttons = samples.iter().map(|s| self.sample_view(kind, s));
                let gadget = icssed::row(buttons).kind("gadget");
                match kind {
                    Some(kind) => gadget.class(kind),
                    None => gadget.named("showcase"),
                }
                .into()
            });
            icssed::row(gadgets).kind("slot").class(slot).into()
        });
        // `#showcase`: not a real bar, for `debug widgets`.
        let root = self.theme.root("panel", scale).id("showcase");
        icssed::row(slots).at(root).into()
    }

    /// A sample button: its icon, its label, or both; its selector in
    /// its tooltip.
    fn sample_view(&self, kind: Option<&str>, sample: &Sample) -> Element<'_, Shell> {
        let icons: &dyn icssed::Icons = &self.icons;
        let mut content: Vec<Element<'_, Shell>> = Vec::new();
        if let Some(name) = sample.icon {
            let icon = icssed::icon((icons, name));
            content.push(match sample.icon_class {
                Some(class) => icon.class(class).into(),
                None => icon.into(),
            });
        }
        if let Some(label) = sample.label {
            content.push(icssed::text(label).into());
        }
        let button = sample
            .classes
            .iter()
            .fold(icssed::button(content), |button, class| button.class(class))
            .on_press(nothing(()));
        icssed::tooltip(button, selector(kind, sample)).into()
    }
}
