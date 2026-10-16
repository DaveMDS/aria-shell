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
//! hover still shows; only the toolkit's widgets, in a box of their
//! own, work, on a state of their own. Escape, a click beside the parts or the command
//! again close it; a theme change redraws it like any surface. Its own
//! layout, the room around the parts and between them, is aria.css's
//! (`showcase`).
//!
//! ```text
//! showcase > parts                            its own boxes; in them, the parts' roots
//! ├─ panel#showcase > slot                    a bar over the columns, of samples:
//! │  ├─ gadget#showcase > indicator[.<state>] of no kind, only what every gadget gets:
//! │  │                                        the states, .active .warning .danger .muted
//! │  ╰─ gadget.<kind> > indicator.<state>     a gadget's states, as it shows them
//! │     ╰─ icon | label                       their selector in a tooltip
//! ╰─ columns > column
//!    ├─ popup > gadget.clock                  the calendar
//!    ├─ popup > gadget#showcase > column      of no kind: a menu with every
//!    │  ╰─ menu, button.picker                kind of item, the Themes picker
//!    ├─ launcher, widgets                     the toolkit's widgets, below
//!    ├─ exiter                                its default button focused
//!    ├─ osd.volume
//!    ├─ notification.normal | notification.critical
//!    ╰─ widgets#graph                         a graph, in a box of its own
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
use crate::gadgets::themes::ThemesConfig;
use crate::gadgets::{AnyGadget, Context};
use crate::locale::Locale;
use crate::services::notifications::{IconSource, Notification, Timeout, Urgency};
use crate::ui::menu::{self, Item};
use crate::ui::{popup, toast};
use crate::{AriaShell, Message as Shell, process};

/// The gadgets whose popups show, by their config section.
const GADGETS: &[&str] = &["Clock"];

/// A sample button on the bar: its classes, its icon, its label. Not a
/// gadget's view: only what the theme's rules meet, `gadget.<kind> >
/// indicator.<state> > icon | label`.
struct Sample {
    classes: &'static [&'static str],
    icon: Option<&'static str>,
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
            sample(&["workspace", "danger"], None, Some("3")),
        ]),
        (None, &[
            sample(&[], Some("dialog-information-symbolic"), None),
            sample(&["active"], Some("dialog-information-symbolic"), Some("active")),
            sample(&["warning"], Some("dialog-warning-symbolic"), Some("warning")),
            sample(&["danger"], Some("dialog-error-symbolic"), Some("danger")),
            sample(&["muted"], Some("dialog-information-symbolic"), Some("muted")),
        ]),
    ]),
    ("center", &[
        (Some("system-monitor"), &[
            sample(&["mem", "warning"], Some("computer-symbolic"), Some("78%")),
            sample(&["temp", "danger"], Some("computer-symbolic"), Some("92°C")),
        ]),
        (Some("clock"), &[sample(&[], None, Some("10:42"))]),
        (Some("custom"), &[sample(&["warning"], Some("dialog-warning-symbolic"), Some("!"))]),
        (Some("tray"), &[sample(&["warning"], Some("mail-unread-symbolic"), None)]),
    ]),
    ("end", &[
        (Some("notifications"), &[
            sample(&["active"], Some("preferences-system-notifications-symbolic"), Some("3")),
            sample(&["muted"], Some("notifications-disabled-symbolic"), None),
        ]),
        (Some("network"), &[
            sample(&["warning"], Some("network-wireless-no-route-symbolic"), None),
            sample(&["muted"], Some("network-offline-symbolic"), None),
        ]),
        (Some("audio"), &[
            sample(&["muted"], Some("audio-volume-muted-symbolic"), Some("40%")),
        ]),
        (Some("brightness"), &[
            sample(&["muted"], Some("display-brightness-symbolic"), None),
        ]),
        (Some("power"), &[
            sample(&["battery"], Some("battery-level-100-symbolic"), None),
            sample(&["battery"], Some("battery-level-80-charging-symbolic"), None),
            sample(&["battery", "warning"], Some("battery-level-20-symbolic"), None),
            sample(&["battery", "danger"], Some("battery-caution-symbolic"), None),
            sample(&["idle", "active"], Some("view-reveal-symbolic"), None),
        ]),
    ]),
];

/// The icons the widgets' box draws.
const WIDGET_ICONS: &[&str] = &[
    "starred-symbolic",
    "folder-symbolic",
    "system-search-symbolic",
    "view-reveal-symbolic",
];

/// A sample button's selector, `gadget.<kind> > indicator.<class>...`.
fn selector(kind: Option<&str>, sample: &Sample) -> String {
    let gadget = match kind {
        Some(kind) => format!("gadget.{kind}"),
        None => "gadget#showcase".to_owned(),
    };
    let classes: String = sample.classes.iter().map(|c| format!(".{c}")).collect();
    format!("{gadget} > indicator{classes}")
}

/// The samples' icons.
fn sample_icons() -> impl Iterator<Item = &'static str> {
    BAR.iter()
        .flat_map(|(_, gadgets)| gadgets.iter())
        .flat_map(|(_, samples)| samples.iter())
        .filter_map(|s| s.icon)
}

/// The state of the widgets' box, what its widgets change.
struct Widgets {
    toggle: bool,
    check: bool,
    radio: usize,
    switch: bool,
    value: f32,
    input: String,
    password: String,
    peek: bool,
    choice: Option<usize>,
    tab: usize,
}

impl Default for Widgets {
    fn default() -> Self {
        Self {
            toggle: true,
            check: true,
            radio: 0,
            switch: true,
            value: 40.0,
            input: String::new(),
            password: "password".to_owned(),
            peek: false,
            choice: None,
            tab: 0,
        }
    }
}

/// What the widgets' box's widgets do.
#[derive(Debug, Clone)]
pub(crate) enum Widget {
    Toggle(bool),
    Check(bool),
    Radio(usize),
    Switch(bool),
    Value(f32),
    Input(String),
    Password(String),
    Peek,
    Choose(usize),
    Tab(usize),
}

impl Widgets {
    fn update(&mut self, widget: Widget) {
        match widget {
            Widget::Toggle(on) => self.toggle = on,
            Widget::Check(on) => self.check = on,
            Widget::Radio(i) => self.radio = i,
            Widget::Switch(on) => self.switch = on,
            Widget::Value(value) => self.value = value,
            Widget::Input(text) => self.input = text,
            Widget::Password(text) => self.password = text,
            Widget::Peek => self.peek = !self.peek,
            Widget::Choose(choice) => self.choice = Some(choice),
            Widget::Tab(tab) => self.tab = tab,
        }
    }
}

pub(crate) struct Showcase {
    pub window: Id,
    pub output: OutputId,
    launcher: Launcher,
    exiter: Exiter,
    /// Their popups show, as `popup[output=..] > gadget.<kind>`.
    gadgets: Vec<AnyGadget>,
    /// The sample popup's menu, its first submenu unfolded.
    menu: menu::Menu,
    /// `[Themes]`: the picker under the menu runs its command, as the
    /// gadget's does (the showcase stays: a zoom on the theme's work).
    themes: ThemesConfig,
    notifications: Vec<Notification>,
    osd: osd::Content,
    widgets: Widgets,
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
    Widget(Widget),
    /// The picker under the sample menu.
    Picker,
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
            .chain(WIDGET_ICONS.iter().map(|&name| name.to_owned()))
            .chain([self.themes.picker_icon.clone()])
    }

    /// The exit menu's default button focused, as on its own surface,
    /// once the showcase's exists.
    pub fn focus(&self) -> Task<Message> {
        self.exiter.focus().map(|_| Message::Nothing)
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
/// The sample menu's unfolded submenu.
const MENU_UNFOLDED: i32 = 9;

/// A menu with every kind of item the shell's menus show (an app's in
/// the tray has them all), each labelled with its selector: an icon, a
/// plain one, a disabled one, the toggles, separators, a submenu
/// unfolded and one folded.
fn menu_items() -> Vec<Item> {
    let icon = |id, label: &str, name: &str| Item {
        icon_name: name.to_owned(),
        ..Item::new(id, label)
    };
    let submenu = |id, label: &str, children| Item {
        submenu: true,
        children,
        ..Item::new(id, label)
    };
    vec![
        icon(1, "item > icon", "starred-symbolic"),
        Item::new(2, "item"),
        Item {
            enabled: false,
            ..Item::new(3, "item:disabled")
        },
        Item::separator(4),
        Item::new(5, "checkbox").check(true),
        Item::new(6, "radio").radio(true),
        Item::new(7, "radio").radio(false),
        Item::separator(8),
        submenu(
            MENU_UNFOLDED,
            "item.submenu.expanded",
            vec![
                icon(10, "submenu > item", "folder-symbolic"),
                Item::new(11, "submenu > item"),
            ],
        ),
        submenu(12, "item.submenu", vec![Item::new(13, "item")]),
    ]
}

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
            menu: {
                let mut menu = menu::Menu::new();
                menu.update(menu::Message::ToggleSubmenu(MENU_UNFOLDED));
                menu
            },
            themes: self.config.section(None),
            notifications: notifications(&self.locale),
            osd: osd::Content {
                kind: osd::Kind::Volume,
                icon: Some("audio-volume-medium-symbolic".to_owned()),
                value: Some(40),
                text: None,
                muted: false,
                outputs: None,
            },
            widgets: Widgets::default(),
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
            Message::Widget(widget) => {
                showcase.widgets.update(widget);
                Task::none()
            }
            Message::Picker => {
                process::run(&showcase.themes.picker_command);
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
        let menu = showcase.menu.view(menu_items(), &self.icons).map(nothing);
        // The Themes gadget's picker, working; disabled without one.
        let themes = &showcase.themes;
        let button = icssed::button![
            icssed::icon((
                &self.icons as &dyn icssed::Icons,
                themes.picker_icon.as_str()
            )),
            icssed::text(self.locale.tr("themes.picker"))
        ]
        .class("picker")
        .on_press(Shell::Showcase(Message::Picker))
        .disabled_if(themes.picker_command.is_empty());
        let menu = icssed::column![icssed::column![menu, button]]
            .kind("gadget")
            .named("showcase");
        let menu = inert(popup::view(popup, menu));
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
            column(popups.chain([menu]).collect()),
            column(vec![
                launcher,
                inert(self.widgets_view(&showcase.widgets, scale))
            ]),
            column(
                [exiter, osd]
                    .into_iter()
                    .chain(toasts)
                    .chain([inert(self.graph_view(scale))])
                    .collect()
            ),
        ]
        .kind("columns");
        let parts = icssed::column![inert(self.bar_view(scale)), columns].kind("parts");
        mouse_area(icssed::column![parts].at(theme.root("showcase", scale)))
            .on_press(Shell::Showcase(Message::PressedOutside))
            .on_release(Shell::Showcase(Message::Released))
            .into()
    }

    /// The toolkit's widgets, each family on a line, in their states:
    /// what the gadgets' popups are made of. Their labels are what
    /// they show, the theme's names. They work, but for the disabled
    /// ones: the slider moves the meter and the disabled slider; Tab
    /// moves the focus among them.
    fn widgets_view<'a>(&'a self, state: &'a Widgets, scale: f32) -> Element<'a, Shell> {
        let icons: &dyn icssed::Icons = &self.icons;
        let star = || icssed::icon((icons, "starred-symbolic"));
        let on = |widget| Shell::Showcase(Message::Widget(widget));
        let buttons = icssed::row![
            icssed::button![star(), icssed::text("button")].on_press(nothing(())),
            icssed::button![icssed::text("flat")]
                .class("flat")
                .on_press(nothing(())),
            icssed::button![icssed::text("primary")]
                .class("primary")
                .on_press(nothing(())),
            icssed::button![icssed::text("danger")]
                .class("danger")
                .on_press(nothing(())),
            icssed::tooltip(icssed::button![star()].on_press(nothing(())), "tooltip"),
        ];
        let radios = (0..2).map(|i| {
            let radio = icssed::toggle(state.radio == i)
                .on_toggle(move |_| on(Widget::Radio(i)))
                .radio();
            if i == 0 { radio.label("radio") } else { radio }.into()
        });
        let toggle = icssed::toggle(state.toggle)
            .on_toggle(move |c| on(Widget::Toggle(c)))
            .icon((icons, "starred-symbolic"))
            .label("toggle");
        let check = icssed::toggle(state.check)
            .on_toggle(move |c| on(Widget::Check(c)))
            .checkbox()
            .label("check");
        let switch = icssed::toggle(state.switch)
            .on_toggle(move |c| on(Widget::Switch(c)))
            .switch();
        let toggles = icssed::row(
            [toggle.into(), check.into()]
                .into_iter()
                .chain(radios)
                .chain([switch.into()]),
        );
        let values = icssed::row![
            icssed::slider(0.0..=100.0, state.value, move |v| on(Widget::Value(v))).step(1.0),
            icssed::meter(state.value / 100.0),
            icssed::meter(state.value / 100.0).class("thin"),
        ];
        let fields = icssed::row![
            icssed::input("input", &state.input)
                .on_input(move |t| on(Widget::Input(t)))
                .leading(icssed::icon((icons, "system-search-symbolic"))),
            icssed::input("password", &state.password)
                .on_input(move |t| on(Widget::Password(t)))
                .secure(!state.peek)
                .trailing(
                    icssed::toggle(state.peek)
                        .on_toggle(move |_| on(Widget::Peek))
                        .class("peek")
                        .icon((icons, "view-reveal-symbolic")),
                ),
            icssed::select(["one", "two", "three"], state.choice, move |c| {
                on(Widget::Choose(c))
            })
            .placeholder("select"),
        ];
        let disabled = icssed::row![
            icssed::button![icssed::text("button")].disabled_if(true),
            icssed::toggle(true)
                .on_toggle(nothing)
                .checkbox()
                .disabled_if(true),
            icssed::toggle(true)
                .on_toggle(nothing)
                .radio()
                .disabled_if(true),
            icssed::toggle(true)
                .on_toggle(nothing)
                .switch()
                .disabled_if(true),
            icssed::slider(0.0..=100.0, state.value, nothing).disabled_if(true),
            icssed::select(["select"], Some(0), nothing).disabled_if(true),
        ];
        let tabs = icssed::tabs(
            ["tab", "tab", "tab"]
                .into_iter()
                .enumerate()
                .map(|(i, title)| {
                    icssed::Tab::new(title, on(Widget::Tab(i))).active(i == state.tab)
                }),
        );
        // Tab among them, the focus shown (`:focus-visible`).
        icssed::focus_scope(
            icssed::column![
                buttons.kind("line"),
                toggles.kind("line"),
                values.kind("line"),
                fields.kind("line"),
                disabled.kind("line"),
                tabs,
            ]
            .at(self.theme.root("widgets", scale)),
        )
        .into()
    }

    /// A graph, in a box of its own: `surface.widgets#graph`.
    fn graph_view(&self, scale: f32) -> Element<'_, Shell> {
        let load = (0..60).map(|i| {
            let t = i as f32 / 6.0;
            50.0 + 30.0 * t.sin() + 10.0 * (t * 3.1).cos()
        });
        let graph = icssed::graph(60).series(load).max(100.0).label("graph");
        icssed::column![graph]
            .at(self.theme.root("widgets", scale).id("graph"))
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
            content.push(icssed::icon((icons, name)).into());
        }
        if let Some(label) = sample.label {
            content.push(icssed::text(label).into());
        }
        let button = sample
            .classes
            .iter()
            .fold(
                icssed::button(content).kind("indicator"),
                |button, class| button.class(class),
            )
            .on_press(nothing(()));
        icssed::tooltip(button, selector(kind, sample)).into()
    }
}
