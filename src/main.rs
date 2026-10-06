mod commands;
mod components;
mod config;
mod daemon;
mod gadgets;
mod locale;
mod process;
mod services;
mod shared;
mod time;
mod ui;
mod watch;

use std::collections::BTreeMap;
use std::path::PathBuf;

use iced::window::Id;
use iced::{Color, Point, Rectangle, Task};
use iced_exwlshell::build_pattern::daemon;
use iced_exwlshell::redraw::Scope;
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::shell::{self, ShellEvent, ShellReceiver};
use iced_exwlshell::to_exwlshell_message;
use iced_wayland_subscriber::{OutputId, OutputInfo};

use audio::Audio;
use brightness::Brightness;
use commands::{Command, OpenCommand, Reply, ToggleCommand};
use components::{dialog, exiter, launcher, locker, osd, panel, picker, wallpaper};
use compositor::Compositor;
use config::{Config, GeneralConfig};
use daemon::surface_tasks;
use dialog::Dialog;
use exiter::{Exiter, ExiterConfig};
use icons::Icons;
use idle::Idle;
use launcher::Launcher;
use locale::Locale;
use locker::Locker;
use network::Network;
use notifications::Notifications;
use osd::Osd;
use panel::{Action, Panel};
use picker::Picker;
use places::Places;
use power::Power;
use screenshot::Screenshot;
use services::{
    audio, brightness, compositor, icons, idle, network, notifications, places, power, screenshot,
    scripts, sysmon, tray,
};
use shared::Shared;
use sysmon::SysMon;
use tray::Tray;
use ui::Surfaces;
use ui::theme::{self, Theme};
use ui::toast::{self, Toasts};
use wallpaper::Wallpapers;

/// Top-level message. `#[to_exwlshell_message]` adds the variants the
/// runtime needs to open/close/reconfigure surfaces (`NewLayerShell`,
/// `RemoveWindow`, `Lock`/`UnLock`, ...).
#[to_exwlshell_message]
#[derive(Debug, Clone)]
enum Message {
    /// Surface and monitor lifecycle from the runtime.
    Shell(ShellEvent),
    /// Routed to the panel shown in that window.
    Panel(Id, panel::Message),
    /// Workspaces/windows changes from the compositor IPC.
    Compositor(compositor::Event),
    /// Watched files (config, theme) or directories (icons) changed.
    Files(watch::Changed),
    /// The icon index finished building.
    Icons(icons::Event),
    /// Tray items coming, going and changing, over DBus.
    Tray(tray::Event),
    /// Notifications coming and going, over DBus.
    Notifications(notifications::Event),
    /// A click on a notification's surface.
    Toast(toast::Message),
    /// A system reading, or the process table.
    SysMon(sysmon::Event),
    Audio(audio::Event),
    Network(network::Event),
    /// Idle stages, sleeps.
    Idle(idle::Event),
    /// The battery, the peripherals, the power profiles.
    Power(power::Event),
    /// The screens' brightness.
    Brightness(brightness::Event),
    /// Captures and saved pictures.
    Screenshot(screenshot::Event),
    /// The screenshot picker's input, while it's open.
    Picker(picker::Message),
    /// UDisks2's devices, mounts and ejects.
    Places(places::Event),
    /// A gadget's program ran.
    Scripts(scripts::Event),
    /// From the command socket (`aria-shell launcher toggle`).
    Command(Command),
    /// A key on a window: a bar holding the keyboard for its popup
    /// passes it on (`Panel::wants_keyboard`), the rest is dropped.
    PanelKey(Id, iced::keyboard::Event),
    /// Routed to the open launcher.
    Launcher(launcher::Message),
    /// From the launcher's event subscription: only meant for it when
    /// the window is its own.
    LauncherEvent(Id, launcher::Message),
    /// Routed to the lock screen.
    Locker(locker::Message),
    /// Routed to the open exit menu.
    Exiter(exiter::Message),
    /// From the exit menu's subscription: for it when the window is
    /// its own (`None`: not from a window, the countdown).
    ExiterEvent(Option<Id>, exiter::Message),
    /// A wallpaper image finished decoding.
    Wallpaper(wallpaper::Event),
    /// The OSD shown with this serial has been up its duration.
    OsdExpired(u64),
    /// A press or release on window `Id` while a dialog is open: a
    /// click outside closes it.
    DialogPointer(Id, dialog::PointerEvent),
    /// A mouse button was pressed on window `Id` and no widget took it:
    /// closes the popups, when the window isn't one of them.
    PressedOutside(Id),
    /// The pointer moved over one of our surfaces (for `debug cursor`).
    Cursor(Id, Point),
    /// The pointer entered or left one of our surfaces.
    PointerCrossed(Id),
    /// The widget tree answered `debug widgets`: element paths and
    /// surface-local rectangles.
    Widgets(Reply, Option<String>, Vec<(String, Rectangle)>),
    /// The anchor of a popup about to open was located in its panel.
    PopupAnchor {
        popup: Id,
        panel: Id,
        anchor: Rectangle,
    },
    /// Nothing to update: a new frame of that surface (`None`: of all of
    /// them), for a change `Message::redraw_scope` couldn't tell from
    /// the message that made it.
    Redraw(Option<Id>),
}

impl Message {
    /// The surfaces that need a new frame after this message (the
    /// runtime's redraw policy, asked before `update`): every one of
    /// them by default, which also redraws the wallpapers for a clock
    /// tick or a pointer move. Messages changing the shared state
    /// redraw nothing here: their `update` asks frames of the surfaces
    /// showing it (`AriaShell::redraw_shared`).
    fn redraw_scope(&self) -> Scope {
        match self {
            // Only answered.
            Message::Widgets(..) | Message::Command(Command::Debug(..)) => Scope::None,
            // Only closes surfaces.
            Message::OsdExpired(_) => Scope::None,
            // Only runs a program.
            Message::Command(Command::Open(_)) => Scope::None,
            // The pointer's surface, for its hover styles: an iced widget
            // keeps its status (hovered, pressed) in itself, rebuilt as
            // unknown with every message and known again only when
            // drawn; until then a hover change asks no frame.
            Message::Cursor(id, _) | Message::PointerCrossed(id) => Scope::Window(*id),
            Message::Compositor(_)
            | Message::Tray(_)
            | Message::Scripts(_)
            | Message::SysMon(_)
            | Message::Audio(_)
            | Message::Network(_)
            | Message::Power(_)
            | Message::Brightness(_)
            | Message::Screenshot(_)
            | Message::Picker(_)
            | Message::Places(_) => Scope::None,
            // A gadget's own state: its popups follow (`update`).
            Message::Panel(id, _) | Message::PanelKey(id, _) | Message::Redraw(Some(id)) => {
                Scope::Window(*id)
            }
            _ => Scope::All,
        }
    }
}

struct AriaShell {
    config: Config,
    general: GeneralConfig,
    /// The theme in use, chosen at runtime (the `Themes` gadget) or
    /// from `[general] style` / `color_scheme`; a config reload keeps
    /// the runtime choice.
    style: Option<String>,
    scheme: theme::Scheme,
    shell_events: ShellReceiver,
    compositor: Compositor,
    theme: Theme,
    locale: Locale,
    icons: Icons,
    tray: Tray,
    notifications: Notifications,
    sysmon: SysMon,
    audio: Audio,
    network: Network,
    idle: Idle,
    power: Power,
    brightness: Brightness,
    screenshot: Screenshot,
    places: Places,
    scripts: scripts::Scripts,
    /// Monitors currently present, to rebuild the panels on a config
    /// change.
    outputs: BTreeMap<OutputId, OutputInfo>,
    /// One entry per open layer surface.
    panels: BTreeMap<Id, Panel>,
    /// The background surfaces.
    wallpapers: Wallpapers,
    /// The launcher, while shown, on its dialog surface.
    launcher: Option<(Dialog, Launcher)>,
    /// The exit menu, while shown.
    exiter: Option<(Dialog, Exiter)>,
    /// The lock screen, from the `lock` command to the unlock.
    locker: Option<Locker>,
    /// The screenshot picker, while it's open.
    picker: Option<Picker>,
    /// Last pointer position reported by one of our surfaces.
    cursor: Option<(Id, Point)>,
    /// One layer surface per notification shown.
    toasts: Toasts,
    /// The OSD, and its surfaces while shown.
    osd: Osd,
}

impl AriaShell {
    fn new(shell_events: ShellReceiver) -> (Self, Task<Message>) {
        let config = Config::load();
        autostart(&config);
        let general: GeneralConfig = config.section(None);
        let style = general.style.clone();
        let scheme = general.color_scheme;
        let theme = Theme::load(&config, style.as_deref(), scheme);
        let locale = Locale::new(&general.language);
        let icons = Icons::new(&config, locale.languages());
        let load_icons = icons.load().map(Message::Icons);
        let notifications = Notifications::new(config.section(None));
        let sysmon = SysMon::new(config.section(None));
        let idle = Idle::new(idle::IdleConfig::load(&config));
        let power = Power::new(config.section(None));
        let brightness = Brightness::new(config.section(None));
        let osd = Osd::new(config.section(None));
        let screenshot = Screenshot::new(&config);
        let shell = Self {
            config,
            general,
            style,
            scheme,
            shell_events,
            compositor: Compositor::detect(),
            theme,
            locale,
            icons,
            tray: Tray::default(),
            notifications,
            sysmon,
            audio: Audio::default(),
            network: Network::default(),
            idle,
            power,
            brightness,
            screenshot,
            places: Places::default(),
            scripts: scripts::Scripts::default(),
            outputs: BTreeMap::new(),
            panels: BTreeMap::new(),
            wallpapers: Wallpapers::default(),
            launcher: None,
            exiter: None,
            locker: None,
            picker: None,
            cursor: None,
            toasts: Toasts::default(),
            osd,
        };
        (shell, load_icons)
    }

    fn shared(&self) -> Shared<'_> {
        Shared {
            compositor: &self.compositor,
            theme: &self.theme,
            locale: &self.locale,
            icons: &self.icons,
            tray: &self.tray,
            notifications: &self.notifications,
            sysmon: &self.sysmon,
            audio: &self.audio,
            network: &self.network,
            idle: &self.idle,
            power: &self.power,
            brightness: &self.brightness,
            places: &self.places,
            scripts: &self.scripts,
        }
    }

    /// Keep an icon resolved for every window there is, and for what
    /// the launcher lists.
    fn resolve_icons(&mut self) {
        if !self.icons.is_loaded() {
            return;
        }
        for w in &self.compositor.windows {
            self.icons.resolve(&w.class);
        }
        if let Some((_, launcher)) = &self.launcher {
            for id in launcher.visible_ids() {
                self.icons.resolve(id);
            }
        }
        for item in &self.tray.items {
            if let Some(name) = item.icon_name() {
                self.icons.resolve_name(name, item.icon_theme_path());
            }
        }
        let classes: Vec<String> = self.audio.app_classes().map(str::to_owned).collect();
        for class in classes {
            self.icons.resolve(&class);
        }
        let names: Vec<String> = self
            .panels
            .values()
            .flat_map(Panel::icon_names)
            .chain(self.scripts.icon_names().map(str::to_owned))
            .chain(self.notifications.icon_names().map(str::to_owned))
            .chain(self.audio.icon_names().map(str::to_owned))
            .chain(self.power.icon_names().map(str::to_owned))
            .chain(self.places.icon_names().map(str::to_owned))
            .chain(self.osd.icon_names().map(str::to_owned))
            .chain(
                self.locker
                    .iter()
                    .flat_map(Locker::icon_names)
                    .map(str::to_owned),
            )
            .chain(
                self.exiter
                    .iter()
                    .flat_map(|(_, e)| e.icon_names())
                    .map(str::to_owned),
            )
            .chain(
                self.launcher
                    .iter()
                    .flat_map(|(_, l)| l.icon_names())
                    .map(str::to_owned),
            )
            .collect();
        for name in names {
            self.icons.resolve_name(&name, None);
        }
    }

    /// Where the pointer was last seen, in global coordinates (as
    /// estimated by [`AriaShell::surfaces`]), for the tray's click
    /// methods.
    fn cursor_global(&self) -> (i32, i32) {
        let Some((window, p)) = self.cursor else {
            return (0, 0);
        };
        self.surfaces()
            .into_iter()
            .find(|(id, ..)| *id == window)
            .map(|(_, _, _, r)| ((r.x + p.x) as i32, (r.y + p.y) as i32))
            .unwrap_or((0, 0))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Shell(event) => self.on_shell_event(event),
            Message::PanelKey(id, event) => {
                if self.panels.get(&id).is_some_and(|p| p.keyboard) {
                    self.update(Message::Panel(id, panel::Message::Key(event)))
                } else {
                    Task::none()
                }
            }
            Message::Panel(id, m) => match self.panels.get_mut(&id) {
                Some(panel) => {
                    let action = panel.update(m);
                    // A gadget's own state may change what its popup
                    // shows (a submenu unfolded) or which icons it
                    // draws (a Custom's output named one).
                    self.resolve_icons();
                    let redraw = if action.is_local() {
                        self.redraw_popups(id)
                    } else {
                        Task::done(Message::Redraw(None))
                    };
                    // The keyboard first: a popup wanting it must map
                    // on a bar that already has it.
                    Task::batch([self.sync_popups(), self.perform(id, action), redraw])
                }
                None => Task::none(),
            },
            Message::Redraw(_) | Message::PointerCrossed(_) => Task::none(),
            Message::Compositor(event) => {
                self.compositor.apply(event);
                self.resolve_icons();
                self.redraw_shared()
            }
            Message::Icons(event) => {
                self.icons.apply(event);
                if let Some((_, launcher)) = &mut self.launcher
                    && let Some(index) = self.icons.index()
                {
                    launcher.set_apps(index);
                }
                self.resolve_icons();
                Task::none()
            }
            Message::Tray(event) => {
                let follow_up = self.tray.apply(event).map(Message::Tray);
                self.resolve_icons();
                Task::batch([follow_up, self.sync_popups(), self.redraw_shared()])
            }
            Message::Scripts(event) => {
                self.scripts.apply(event);
                // The output may name an icon.
                self.resolve_icons();
                self.redraw_shared()
            }
            Message::Notifications(event) => {
                let follow_up = self.notifications.apply(event).map(Message::Notifications);
                self.resolve_icons();
                Task::batch([follow_up, self.sync_toasts()])
            }
            Message::SysMon(event) => {
                self.sysmon.apply(event);
                self.redraw_shared()
            }
            Message::Audio(event) => {
                if !self.audio.apply(event) {
                    return Task::none();
                }
                self.idle.set_playing(
                    self.audio
                        .players()
                        .iter()
                        .any(|p| p.status == audio::PlaybackStatus::Playing),
                );
                let osd = self.observe_osd();
                self.resolve_icons();
                Task::batch([osd, self.sync_popups(), self.redraw_shared()])
            }
            Message::Network(event) => {
                let (changed, follow_up) = self.network.apply(event);
                let follow_up = follow_up.map(Message::Network);
                if !changed {
                    return follow_up;
                }
                let osd = self.observe_osd();
                Task::batch([follow_up, osd, self.sync_popups(), self.redraw_shared()])
            }
            Message::Command(Command::Osd(content)) => self.show_osd(content),
            Message::OsdExpired(serial) => surface_tasks(self.osd.expired(serial)),
            Message::Toast(m) => {
                let signals = self
                    .notifications
                    .run(notifications::Command::from(m))
                    .map(Message::Notifications);
                Task::batch([signals, self.sync_toasts(), self.sync_popups()])
            }
            Message::Command(Command::Launcher(cmd)) => match (cmd, self.launcher.is_some()) {
                (ToggleCommand::Show | ToggleCommand::Toggle, false) => self.open_launcher(),
                (ToggleCommand::Hide | ToggleCommand::Toggle, true) => self.close_launcher(),
                _ => Task::none(),
            },
            Message::Command(Command::Exiter(cmd)) => match (cmd, self.exiter.is_some()) {
                (ToggleCommand::Show | ToggleCommand::Toggle, false) => {
                    self.open_exiter(Exiter::new(self.config.section(None)))
                }
                (ToggleCommand::Hide | ToggleCommand::Toggle, true) => self.close_exiter(),
                _ => Task::none(),
            },
            Message::ExiterEvent(window, m) => match (&self.exiter, window) {
                (Some(_), None) => self.update(Message::Exiter(m)),
                (Some((dialog, _)), Some(w)) if dialog.is_window(w) => {
                    self.update(Message::Exiter(m))
                }
                _ => Task::none(),
            },
            Message::Exiter(m) => {
                let Some((_, exiter)) = &mut self.exiter else {
                    return Task::none();
                };
                match exiter.update(m) {
                    exiter::Action::Run(task) => {
                        Task::batch([task.map(Message::Exiter), self.sync_exiter()])
                    }
                    exiter::Action::Close => self.close_exiter(),
                    exiter::Action::Perform(command) => {
                        Task::batch([self.close_exiter(), self.perform_exit(command)])
                    }
                }
            }
            Message::Wallpaper(event) => {
                self.wallpapers.apply(event);
                Task::none()
            }
            Message::Command(Command::Lock) => self.lock(),
            Message::Command(Command::Open(what)) => {
                self.open(what);
                Task::none()
            }
            Message::Command(Command::Idle(cmd)) => {
                self.idle.run(cmd);
                self.observe_osd()
            }
            Message::Power(event) => {
                let (changed, low, follow_up) = self.power.apply(event);
                self.idle.set_on_battery(self.power.on_battery());
                let mut tasks = vec![follow_up.map(Message::Power)];
                if let Some(low) = low {
                    tasks.push(self.power.notify_low(low, &self.locale).map(Message::Power));
                }
                if changed {
                    tasks.push(self.observe_osd());
                    self.resolve_icons();
                    tasks.push(self.sync_popups());
                    tasks.push(self.redraw_shared());
                }
                Task::batch(tasks)
            }
            Message::Places(event) => {
                let (changed, failure) = self
                    .places
                    .apply(event, self.general.file_manager.as_deref());
                let mut tasks = Vec::new();
                if let Some(failure) = failure {
                    tasks.push(failure.notify(&self.locale).map(Message::Places));
                }
                if changed {
                    tasks.push(self.sync_popups());
                    tasks.push(self.redraw_shared());
                }
                Task::batch(tasks)
            }
            Message::Brightness(event) => {
                if self.brightness.apply(event) {
                    self.brightness_changed()
                } else {
                    Task::none()
                }
            }
            Message::Command(Command::Volume(cmd)) => {
                let config: gadgets::audio::AudioConfig = self.config.section(None);
                self.audio
                    .run(cmd.command(config.step, config.max_volume))
                    .map(Message::Audio)
            }
            Message::Command(Command::Screenshot(cmd)) => {
                if cmd.target == screenshot::Target::Pick && self.picker.is_some() {
                    log::debug!("screenshot: the picker is open already");
                    return Task::none();
                }
                self.screenshot
                    .run(cmd, &self.outputs, &self.compositor)
                    .map(Message::Screenshot)
            }
            Message::Screenshot(event) => {
                let (task, frozen) = self.screenshot.apply(event);
                let task = task.map(Message::Screenshot);
                let Some(frozen) = frozen else {
                    return task;
                };
                let focused = self.compositor.focused_output.as_deref();
                let (picker, surfaces) = Picker::open(frozen, &self.outputs, focused);
                self.picker = Some(picker);
                Task::batch([task, surface_tasks(surfaces)])
            }
            Message::Picker(m) => {
                let Some(picker) = &mut self.picker else {
                    return Task::none();
                };
                match picker.update(m) {
                    picker::Action::Redraw(ids) => surface_tasks(Surfaces {
                        redraw: ids,
                        ..Surfaces::default()
                    }),
                    picker::Action::Cancel => {
                        log::info!("screenshot: picking cancelled");
                        self.close_picker()
                    }
                    picker::Action::Take { rect, destination } => {
                        let cut = self.screenshot.cut(picker.shots.clone(), rect, destination);
                        Task::batch([self.close_picker(), cut.map(Message::Screenshot)])
                    }
                }
            }
            Message::Command(Command::Brightness(cmd)) => {
                if self.brightness.run(cmd) {
                    self.brightness_changed()
                } else {
                    Task::none()
                }
            }
            Message::Idle(event) => {
                let locker = match &self.locker {
                    None => idle::Locker::None,
                    Some(l) if l.locked => idle::Locker::Locked,
                    Some(_) => idle::Locker::Locking,
                };
                let (lock, task) = self.idle.apply(event, locker);
                let task = task.map(Message::Idle);
                if lock {
                    Task::batch([task, self.lock()])
                } else {
                    task
                }
            }
            Message::Locker(m) => {
                let Some(locker) = &mut self.locker else {
                    return Task::none();
                };
                match locker.update(m) {
                    locker::Action::Run(task) => task.map(Message::Locker),
                    locker::Action::Unlock => {
                        log::info!("unlocking the session");
                        self.locker = None;
                        Task::done(Message::UnLock)
                    }
                }
            }
            Message::Command(Command::Debug(cmd, reply)) => self.debug(cmd, reply),
            Message::Widgets(reply, filter, rects) => {
                reply.send(self.describe_widgets(&filter, rects));
                Task::none()
            }
            Message::LauncherEvent(window, m) => match &self.launcher {
                Some((dialog, _)) if dialog.is_window(window) => self.update(Message::Launcher(m)),
                _ => Task::none(),
            },
            Message::DialogPointer(window, event) => {
                if let Some((dialog, _)) = &mut self.launcher
                    && dialog.pointer(window, event)
                {
                    return self.close_launcher();
                }
                if let Some((dialog, _)) = &mut self.exiter
                    && dialog.pointer(window, event)
                {
                    return self.close_exiter();
                }
                Task::none()
            }
            Message::PressedOutside(window) => {
                if self.panels.values().any(|p| p.has_popup(window)) {
                    Task::none()
                } else {
                    self.close_popups(None)
                }
            }
            Message::Launcher(m) => {
                let Some((_, launcher)) = &mut self.launcher else {
                    return Task::none();
                };
                match launcher.update(m, &self.theme) {
                    launcher::Action::Run(task) => {
                        self.resolve_icons();
                        task.map(Message::Launcher)
                    }
                    launcher::Action::Close => self.close_launcher(),
                    launcher::Action::Exit(name) => {
                        let config: ExiterConfig = self.config.section(None);
                        let close = self.close_launcher();
                        let Some(button) = config.button(&name) else {
                            return close;
                        };
                        // The exit menu's own flow: its confirmation
                        // when the button wants one, else the command.
                        let next = if button.confirm && config.ask_confirm {
                            match Exiter::confirming(config.clone(), &name) {
                                Some(exiter) => self.open_exiter(exiter),
                                None => Task::none(),
                            }
                        } else {
                            self.perform_exit(button.command.clone())
                        };
                        Task::batch([close, next])
                    }
                }
            }
            Message::Files(watch::Changed(paths)) => {
                let config_changed = self
                    .config
                    .path()
                    .is_some_and(|p| paths.contains(&p.to_path_buf()));
                let theme_changed = self.theme.files().iter().any(|f| paths.contains(f));
                let wallpapers: Vec<PathBuf> = self
                    .wallpapers
                    .files()
                    .filter(|f| paths.contains(f))
                    .cloned()
                    .collect();
                if config_changed && self.general.reload_config {
                    self.reload_config()
                } else if theme_changed && self.general.reload_style {
                    log::info!("theme file changed, reloading");
                    self.reload_theme()
                } else if !wallpapers.is_empty() {
                    log::info!("wallpaper file(s) changed, reloading");
                    Task::batch(
                        wallpapers
                            .into_iter()
                            .map(|p| self.wallpapers.load(p).map(Message::Wallpaper)),
                    )
                } else {
                    // An icon or applications directory: something was
                    // installed or removed.
                    log::info!("icon directories changed, rebuilding the index");
                    self.icons.load().map(Message::Icons)
                }
            }
            Message::Cursor(window, position) => {
                self.cursor = Some((window, position));
                Task::none()
            }
            Message::PopupAnchor {
                popup,
                panel,
                anchor,
            } => {
                let mut panels = std::mem::take(&mut self.panels);
                let surfaces = panels
                    .get_mut(&panel)
                    .map(|bar| bar.place_popup(panel, popup, anchor, self.shared()));
                self.panels = panels;
                surfaces.map_or_else(Task::none, surface_tasks)
            }
            _ => Task::none(), // runtime variants, handled by the runtime
        }
    }

    /// Carry out what the panel in window `panel` asked for.
    fn perform(&mut self, panel: Id, action: Action) -> Task<Message> {
        match action {
            Action::None => Task::none(),
            Action::Run(task) => task.map(move |m| Message::Panel(panel, m)),
            Action::Compositor(cmd) => self.compositor.run(cmd).map(Message::Compositor),
            Action::Tray(cmd) => self.tray.run(cmd, self.cursor_global()).map(Message::Tray),
            Action::Script(cmd) => {
                self.scripts.run(cmd);
                Task::none()
            }
            Action::Notifications(cmd) => {
                let signals = self.notifications.run(cmd).map(Message::Notifications);
                Task::batch([signals, self.sync_toasts(), self.sync_popups()])
            }
            Action::SysMon(cmd) => self.sysmon.run(cmd).map(Message::SysMon),
            Action::Audio(cmd) => self.audio.run(cmd).map(Message::Audio),
            Action::Network(cmd) => self.network.run(cmd).map(Message::Network),
            Action::Idle(cmd) => {
                self.idle.run(cmd);
                self.observe_osd()
            }
            Action::Power(cmd) => self.power.run(cmd).map(Message::Power),
            // After the popup it came from is gone from the screen: the
            // menu closed with this same action, the compositor shows
            // that in its next frames.
            Action::Screenshot(cmd) => Task::perform(
                tokio::time::sleep(std::time::Duration::from_millis(150)),
                move |_| Message::Command(Command::Screenshot(cmd)),
            ),
            Action::Brightness(cmd) => {
                if self.brightness.run(cmd) {
                    self.brightness_changed()
                } else {
                    Task::none()
                }
            }
            Action::Places(cmd) => self
                .places
                .run(cmd, self.general.file_manager.as_deref())
                .map(Message::Places),
            Action::Theme(cmd) => {
                match cmd {
                    theme::Command::ToggleScheme => self.scheme = self.scheme.toggled(),
                    theme::Command::SetScheme(scheme) => self.scheme = scheme,
                    theme::Command::SetStyle(style) => self.style = style,
                }
                log::info!(
                    "theme: {} ({})",
                    self.style.as_deref().unwrap_or("base"),
                    self.scheme.name()
                );
                self.reload_theme()
            }
            Action::OpenPopup { id, anchor } => {
                if !self.panels.get(&panel).is_some_and(|p| p.has_popup(id)) {
                    return Task::none();
                }
                // One popup at a time (the compositor's grab already
                // dismisses the open one on Hyprland, not on Sway,
                // where a click on our own surfaces is delivered).
                let close = self.close_popups(Some(id));
                // Only the widget tree knows where the anchor is: ask it,
                // then open the popup there.
                let open = ui::bounds(anchor).map(move |bounds| Message::PopupAnchor {
                    popup: id,
                    panel,
                    anchor: bounds.unwrap_or_default(),
                });
                Task::batch([close, open])
            }
            Action::ClosePopup(id) => Task::done(Message::RemoveWindow(id)),
            Action::Many(actions) => Task::batch(
                actions
                    .into_iter()
                    .map(|a| self.perform(panel, a))
                    .collect::<Vec<_>>(),
            ),
        }
    }

    /// Carry out an exit menu action: a program, or the compositor's
    /// own exit.
    fn perform_exit(&mut self, command: exiter::Command) -> Task<Message> {
        match command {
            exiter::Command::Program(line) => {
                process::run(&line);
                Task::none()
            }
            exiter::Command::Logout => {
                log::info!("asking the compositor to end the session");
                self.compositor
                    .run(compositor::Command::Exit)
                    .map(Message::Compositor)
            }
        }
    }

    /// `aria-shell lock`: ask the compositor for the session lock; the
    /// surfaces come back as `NewShell` events. Whatever is open goes.
    /// `aria-shell open ...`: the terminal or the file manager of
    /// `[general]`, logged when there's none.
    fn open(&self, what: OpenCommand) {
        match what {
            OpenCommand::Terminal => match &self.general.terminal {
                Some(terminal) => process::run_argv(&process::in_terminal(terminal, &[])),
                None => log::warn!("open terminal: no terminal ([general] terminal)"),
            },
            OpenCommand::FileManager(dir) => match &self.general.file_manager {
                Some(file_manager) => {
                    let dir = dir
                        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
                        .unwrap_or_else(|| PathBuf::from("/"));
                    process::run_argv(&process::on_file(file_manager, &dir));
                }
                None => log::warn!("open file-manager: no file manager ([general] file_manager)"),
            },
        }
    }
}

/// `[autostart]`: `name = command line` pairs, run in file order once
/// when the shell starts (not on a config reload).
fn autostart(config: &Config) {
    for (name, line) in config.pairs("autostart") {
        if line.is_empty() {
            log::warn!("[autostart] {name}: no command line");
            continue;
        }
        log::info!("autostart {name}");
        process::run(&line);
    }
}

fn main() -> iced_exwlshell::Result {
    // With arguments we're the client: send them to the running shell.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        match commands::send(&args) {
            Ok(reply) => {
                if !reply.is_empty() {
                    println!("{reply}");
                }
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("aria_shell=info"))
        .init();

    // Before anything runs (autostart, the surfaces, the socket, which
    // a second shell would take from the first).
    let _instance = match commands::single_instance() {
        Ok(instance) => instance,
        Err(e) => {
            log::error!("{e}");
            std::process::exit(1);
        }
    };

    let (shell_broadcast, shell_events) = shell::channel();

    daemon(
        move || AriaShell::new(shell_events.clone()),
        "aria-shell",
        AriaShell::update,
        AriaShell::view,
    )
    .subscription(AriaShell::subscription)
    .redraw_scope(Message::redraw_scope)
    // Surfaces start transparent: what shows is the theme's `panel` /
    // `popup` background, which may itself be translucent or rounded.
    .style(|_, theme| iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: theme.palette().text,
    })
    .settings(Settings {
        shell_broadcast,
        layer_settings: LayerShellSettings {
            // No initial surface: panels are opened per output as the
            // compositor announces them.
            start_mode: StartMode::Background,
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}
