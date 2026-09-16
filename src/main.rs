mod commands;
mod compositor;
mod config;
mod gadget;
mod gadgets;
mod icons;
mod launcher;
mod panel;
mod theme;
mod watch;
mod widgets;

use std::collections::BTreeMap;
use std::path::Path;

use iced::advanced::widget::operation::{Operation, Outcome};
use iced::window::Id;
use iced::{Color, Element, Event, Length, Point, Rectangle, Size, Subscription, Task, widget};
use iced_exwlshell::build_pattern::daemon;
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::shell::{self, ShellEvent, ShellReceiver};
use iced_exwlshell::to_layer_message;
use iced_wayland_subscriber::{OutputId, OutputInfo};

use commands::{Command, DebugCommand, LauncherCommand, Reply};
use compositor::Compositor;
use config::{Config, GeneralConfig};
use gadget::Shared;
use icons::Icons;
use launcher::Launcher;
use panel::{Action, Panel, PanelConfig};
use theme::{Node, Theme};

/// Top-level message. `#[to_layer_message(multi)]` adds the variants the
/// runtime needs to open/close/reconfigure surfaces (`NewLayerShell`,
/// `RemoveWindow`, ...).
#[to_layer_message(multi)]
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
    /// From the command socket (`aria-shell launcher toggle`).
    Command(Command),
    /// Routed to the open launcher.
    Launcher(launcher::Message),
    /// From the launcher's event subscription: only meant for it when
    /// the window is its own.
    LauncherEvent(Id, launcher::Message),
    /// The pointer moved over one of our surfaces (for `debug cursor`).
    Cursor(Id, Point),
    /// The widget tree answered `debug widgets`: element paths and
    /// surface-local rectangles.
    Widgets(Reply, Option<String>, Vec<(String, Rectangle)>),
    /// The anchor of a popup about to open was located in its panel.
    PopupAnchor {
        popup: Id,
        panel: Id,
        anchor: Rectangle,
        size: (u32, u32),
    },
}

struct AriaShell {
    config: Config,
    general: GeneralConfig,
    shell_events: ShellReceiver,
    compositor: Compositor,
    theme: Theme,
    icons: Icons,
    /// Monitors currently present, to rebuild the panels on a config
    /// change.
    outputs: BTreeMap<OutputId, OutputInfo>,
    /// One entry per open layer surface.
    panels: BTreeMap<Id, Panel>,
    /// Open popup surfaces, to the panel each hangs off and where it
    /// was asked to be, relative to the panel's surface (the compositor
    /// may slide it; a `debug surfaces` estimate).
    popups: BTreeMap<Id, (Id, Rectangle)>,
    /// The launcher, while shown.
    launcher: Option<OpenLauncher>,
    /// While the launcher is shown, one transparent surface per output
    /// under it, so a click anywhere else closes it.
    grabs: Vec<(Id, OutputId)>,
    /// Last pointer position reported by one of our surfaces.
    cursor: Option<(Id, Point)>,
}

struct OpenLauncher {
    window: Id,
    output: OutputId,
    size: (u32, u32),
    launcher: Launcher,
}

impl AriaShell {
    fn new(shell_events: ShellReceiver) -> (Self, Task<Message>) {
        let config = Config::load();
        let general = config.section(None);
        let theme = Theme::load(&config);
        let icons = Icons::new(&config);
        let load_icons = icons.load().map(Message::Icons);
        let shell = Self {
            config,
            general,
            shell_events,
            compositor: Compositor::detect(),
            theme,
            icons,
            outputs: BTreeMap::new(),
            panels: BTreeMap::new(),
            popups: BTreeMap::new(),
            launcher: None,
            grabs: Vec::new(),
            cursor: None,
        };
        (shell, load_icons)
    }

    fn shared(&self) -> Shared<'_> {
        Shared {
            compositor: &self.compositor,
            theme: &self.theme,
            icons: &self.icons,
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
        if let Some(open) = &self.launcher {
            for id in open.launcher.visible_ids() {
                self.icons.resolve(id);
            }
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Shell(event) => self.on_shell_event(event),
            Message::Panel(id, m) => match self.panels.get_mut(&id) {
                Some(panel) => {
                    let action = panel.update(m);
                    self.perform(id, action)
                }
                None => Task::none(),
            },
            Message::Compositor(event) => {
                self.compositor.apply(event);
                self.resolve_icons();
                Task::none()
            }
            Message::Icons(event) => {
                self.icons.apply(event);
                if let Some(open) = &mut self.launcher
                    && let Some(index) = self.icons.index()
                {
                    open.launcher.set_apps(index);
                }
                self.resolve_icons();
                Task::none()
            }
            Message::Command(Command::Launcher(cmd)) => match (cmd, self.launcher.is_some()) {
                (LauncherCommand::Show | LauncherCommand::Toggle, false) => self.open_launcher(),
                (LauncherCommand::Hide | LauncherCommand::Toggle, true) => self.close_launcher(),
                _ => Task::none(),
            },
            Message::Command(Command::Debug(cmd, reply)) => match cmd {
                DebugCommand::Surfaces => {
                    reply.send(self.describe_surfaces());
                    Task::none()
                }
                DebugCommand::Cursor => {
                    reply.send(self.describe_cursor());
                    Task::none()
                }
                DebugCommand::Widgets(filter) => widget_rects()
                    .map(move |rects| Message::Widgets(reply.clone(), filter.clone(), rects)),
            },
            Message::Widgets(reply, filter, rects) => {
                reply.send(self.describe_widgets(&filter, rects));
                Task::none()
            }
            Message::LauncherEvent(window, m) => match &self.launcher {
                Some(open) if open.window == window => self.update(Message::Launcher(m)),
                _ => Task::none(),
            },
            Message::Launcher(m) => {
                let Some(open) = &mut self.launcher else {
                    return Task::none();
                };
                match open.launcher.update(m) {
                    launcher::Action::Run(task) => {
                        self.resolve_icons();
                        task.map(Message::Launcher)
                    }
                    launcher::Action::Close => self.close_launcher(),
                }
            }
            Message::Files(watch::Changed(paths)) => {
                let config_changed = self
                    .config
                    .path()
                    .is_some_and(|p| paths.contains(&p.to_path_buf()));
                let theme_changed = self.theme.files().iter().any(|f| paths.contains(f));
                if config_changed && self.general.reload_config {
                    self.reload_config()
                } else if theme_changed && self.general.reload_style {
                    self.reload_theme()
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
                size,
            } => {
                let Some(position) = self.panels.get(&panel).map(Panel::position) else {
                    return Task::none();
                };
                let settings = panel::popup_settings(panel, position, anchor, size);
                let estimate = panel::popup_estimate(position, anchor, size);
                self.popups.insert(popup, (panel, estimate));
                Task::done(Message::NewPopUp {
                    settings,
                    id: popup,
                })
            }
            _ => Task::none(), // runtime variants, handled by the runtime
        }
    }

    /// Logical rectangle of an output in the global space (xdg-output).
    fn output_rect(&self, output: OutputId) -> Option<Rectangle> {
        let info = self.outputs.get(&output)?;
        let (x, y) = info.logical_position?;
        let (w, h) = info.logical_size?;
        Some(Rectangle::new(
            Point::new(x as f32, y as f32),
            Size::new(w as f32, h as f32),
        ))
    }

    /// Every surface we have open: its kind, output and global
    /// rectangle, computed from what we asked the compositor for.
    fn surfaces(&self) -> Vec<(Id, &'static str, OutputId, Rectangle)> {
        let mut list = Vec::new();
        for (&id, panel) in &self.panels {
            let Some(out) = self.output_rect(panel.output) else {
                continue;
            };
            let h = panel.height() as f32;
            let y = match panel.position() {
                panel::Position::Top => out.y,
                panel::Position::Bottom => out.y + out.height - h,
            };
            list.push((
                id,
                "panel",
                panel.output,
                Rectangle::new(Point::new(out.x, y), Size::new(out.width, h)),
            ));
        }
        for (&id, &(panel, rect)) in &self.popups {
            if let Some(&(_, _, output, bar)) = list.iter().find(|(p, ..)| *p == panel) {
                list.push((id, "popup", output, rect + iced::Vector::new(bar.x, bar.y)));
            }
        }
        for &(id, output) in &self.grabs {
            if let Some(out) = self.output_rect(output) {
                list.push((id, "grab", output, out));
            }
        }
        if let Some(open) = &self.launcher
            && let Some(out) = self.output_rect(open.output)
        {
            let (w, h) = (open.size.0 as f32, open.size.1 as f32);
            list.push((
                open.window,
                "launcher",
                open.output,
                Rectangle::new(
                    Point::new(
                        out.x + (out.width - w) / 2.0,
                        out.y + (out.height - h) / 2.0,
                    ),
                    Size::new(w, h),
                ),
            ));
        }
        list
    }

    fn output_name(&self, output: OutputId) -> &str {
        self.outputs
            .get(&output)
            .and_then(|o| o.name.as_deref())
            .unwrap_or("?")
    }

    /// `debug surfaces`: `<kind> <output> <x>,<y> <w>x<h>` per surface,
    /// `;`-separated (the protocol is one line per reply).
    fn describe_surfaces(&self) -> String {
        self.surfaces()
            .into_iter()
            .map(|(_, kind, output, r)| {
                format!(
                    "{kind} {} {},{} {}x{}",
                    self.output_name(output),
                    r.x as i32,
                    r.y as i32,
                    r.width as i32,
                    r.height as i32
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// `debug widgets [selector]`: `<element path> <x>,<y> <w>x<h>` per
    /// themed widget matching the selector (theme syntax, plus
    /// `:nth-child(n)`; all of them without one), global coordinates
    /// (as estimated by [`AriaShell::surfaces`]), `;`-separated. The
    /// surface is found from the path's root: `panel[output=..]`,
    /// `launcher`, `popup[output=..]`.
    fn describe_widgets(&self, filter: &Option<String>, rects: Vec<(String, Rectangle)>) -> String {
        let selector = match filter.as_deref().map(theme::Selector::parse) {
            None => None,
            Some(Ok(s)) => Some(s),
            Some(Err(e)) => return format!("bad selector: {e}"),
        };
        let surfaces = self.surfaces();
        let origin = |path: &str| -> Option<Point> {
            let root = path.split(" > ").next()?;
            let output = root
                .split_once("[output=\"")
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(name, _)| name);
            let kind = root.split(['.', '#', '[', ':']).next()?;
            surfaces
                .iter()
                .find(|(_, k, out, _)| {
                    *k == kind && output.is_none_or(|o| self.output_name(*out) == o)
                })
                .map(|(_, _, _, r)| r.position())
        };
        rects
            .into_iter()
            .filter(|(path, _)| {
                selector
                    .as_ref()
                    .is_none_or(|s| theme::node_from_path(path).is_ok_and(|node| s.matches(&node)))
            })
            .filter_map(|(path, r)| {
                let o = origin(&path)?;
                Some(format!(
                    "{path} {},{} {}x{}",
                    (o.x + r.x) as i32,
                    (o.y + r.y) as i32,
                    r.width as i32,
                    r.height as i32
                ))
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// `debug cursor`: where the pointer was last seen over one of our
    /// surfaces, `<kind> <output> local <x>,<y> global <x>,<y>`. The
    /// local position is what the surface got; the global one assumes
    /// the surface is where [`AriaShell::surfaces`] thinks (another
    /// client's exclusive zone can shift a bar without us knowing), so a
    /// driver that placed the pointer itself can compare the two and
    /// learn the surface's real origin.
    fn describe_cursor(&self) -> String {
        let Some((window, p)) = self.cursor else {
            return "unknown".to_owned();
        };
        match self.surfaces().into_iter().find(|(id, ..)| *id == window) {
            Some((_, kind, output, r)) => format!(
                "{kind} {} local {},{} global {},{}",
                self.output_name(output),
                p.x as i32,
                p.y as i32,
                (r.x + p.x) as i32,
                (r.y + p.y) as i32
            ),
            None => "unknown".to_owned(),
        }
    }

    /// Re-read the config (and the theme, it may name another one),
    /// then close every panel and open them again for the monitors we
    /// know: same path as a monitor being plugged in.
    fn reload_config(&mut self) -> Task<Message> {
        log::info!("config changed, rebuilding panels");
        self.config = Config::load();
        self.general = self.config.section(None);
        self.theme = Theme::load(&self.config);
        let mut icons = Icons::new(&self.config);
        icons.keep_index_of(&self.icons);
        self.icons = icons;
        let mut tasks: Vec<Task<Message>> = self
            .popups
            .keys()
            .chain(self.panels.keys())
            .map(|&id| Task::done(Message::RemoveWindow(id)))
            .collect();
        self.popups.clear();
        self.panels.clear();
        tasks.push(self.close_launcher());
        self.cursor = None;
        let outputs: Vec<OutputInfo> = self.outputs.values().cloned().collect();
        tasks.extend(outputs.iter().map(|o| self.open_panels(o)));
        tasks.push(self.icons.load().map(Message::Icons));
        Task::batch(tasks)
    }

    /// Re-read the theme files; views pick the new rules up on their
    /// next redraw, bars whose thickness changed get resized. A file
    /// that doesn't parse (mid-edit, typically) keeps the current theme.
    fn reload_theme(&mut self) -> Task<Message> {
        log::info!("theme changed, reloading");
        match Theme::try_load(&self.config) {
            Ok(theme) => self.theme = theme,
            Err(e) => {
                log::error!("{}, keeping the current theme", e.message);
                return Task::none();
            }
        }
        let mut tasks = Vec::new();
        for (&id, panel) in &mut self.panels {
            if let Some((anchor, size, zone_size)) = panel.resize(&self.theme) {
                tasks.push(Task::done(Message::LayoutChange { id, anchor, size }));
                tasks.push(Task::done(Message::ExclusiveZoneChange { id, zone_size }));
            }
        }
        Task::batch(tasks)
    }

    /// Carry out what the panel in window `panel` asked for.
    fn perform(&mut self, panel: Id, action: Action) -> Task<Message> {
        match action {
            Action::None => Task::none(),
            Action::Run(task) => task.map(move |m| Message::Panel(panel, m)),
            Action::Compositor(cmd) => self.compositor.run(cmd).map(Message::Compositor),
            Action::OpenPopup { id, anchor, size } => {
                if !self.panels.contains_key(&panel) {
                    return Task::none();
                }
                // The gadget sized its content; the surface also holds
                // the `popup` root's padding and border.
                let chrome = self.theme.resolve(&Node::root("popup"));
                let pad = chrome.padding;
                let extra = 2.0 * chrome.border_width;
                let size = (
                    size.0 + (pad.left + pad.right + extra) as u32,
                    size.1 + (pad.top + pad.bottom + extra) as u32,
                );
                // Only the widget tree knows where the anchor is: ask it,
                // then open the popup there.
                widget_bounds(anchor).map(move |bounds| Message::PopupAnchor {
                    popup: id,
                    panel,
                    anchor: bounds.unwrap_or_default(),
                    size,
                })
            }
            Action::ClosePopup(id) => {
                self.popups.remove(&id);
                Task::done(Message::RemoveWindow(id))
            }
        }
    }

    /// Show the launcher on the focused output (the first one if the
    /// compositor didn't say), sized by the theme's `launcher` rule,
    /// over a click-catching surface on every output.
    fn open_launcher(&mut self) -> Task<Message> {
        let output = self
            .outputs
            .values()
            .find(|o| o.name.is_some() && o.name == self.compositor.focused_output)
            .or_else(|| self.outputs.values().next())
            .cloned();
        let Some(output) = output else {
            log::warn!("no output to show the launcher on");
            return Task::none();
        };
        let style = self.theme.resolve(&Node::root("launcher"));
        let px = |l: Option<theme::Length>, default: f32| match l {
            Some(theme::Length::Px(px)) => px.max(1.0) as u32,
            _ => default as u32,
        };
        let size = LayerSize::px(px(style.width, 500.0), px(style.height, 400.0));
        let launcher = Launcher::new(self.config.section(None), self.icons.index());
        let mut tasks = Vec::new();
        for o in self.outputs.values() {
            let id = Id::unique();
            self.grabs.push((id, OutputId::from(o)));
            tasks.push(Task::done(Message::NewLayerShell {
                settings: NewLayerShellSettings {
                    anchor: Anchor::all(),
                    size: LayerSize::FILL,
                    layer: Layer::Top,
                    exclusive_zone: Some(-1),
                    margin: None,
                    keyboard_interactivity: KeyboardInteractivity::None,
                    output_option: OutputOption::GlobalName(o.id),
                    namespace: Some("aria-launcher-grab".to_owned()),
                    ..Default::default()
                },
                id,
            }));
        }
        let id = Id::unique();
        log::info!(
            "opening the launcher on output {:?} as window {id:?}, grabs {:?}",
            output.name,
            self.grabs
        );
        tasks.push(Task::done(Message::NewLayerShell {
            settings: NewLayerShellSettings {
                anchor: Anchor::empty(),
                size,
                layer: Layer::Overlay,
                exclusive_zone: Some(-1),
                margin: None,
                keyboard_interactivity: KeyboardInteractivity::OnDemand,
                output_option: OutputOption::GlobalName(output.id),
                namespace: Some("aria-launcher".to_owned()),
                ..Default::default()
            },
            id,
        }));
        self.launcher = Some(OpenLauncher {
            window: id,
            output: OutputId::from(&output),
            size: (size.width.to_set(), size.height.to_set()),
            launcher,
        });
        self.resolve_icons();
        Task::batch(tasks)
    }

    fn close_launcher(&mut self) -> Task<Message> {
        let ids: Vec<Id> = self
            .launcher
            .take()
            .map(|open| open.window)
            .into_iter()
            .chain(
                std::mem::take(&mut self.grabs)
                    .into_iter()
                    .map(|(id, _)| id),
            )
            .collect();
        Task::batch(
            ids.into_iter()
                .map(|id| Task::done(Message::RemoveWindow(id))),
        )
    }

    fn on_shell_event(&mut self, event: ShellEvent) -> Task<Message> {
        match event {
            ShellEvent::NewShell(info) => match &self.launcher {
                // The search field can only take focus once its surface
                // exists.
                Some(open) if open.window == info.window => {
                    open.launcher.focus().map(Message::Launcher)
                }
                _ => Task::none(),
            },
            ShellEvent::OutputAdded(output) => {
                log::debug!(
                    "output {:?}: logical position {:?}, size {:?}",
                    output.name,
                    output.logical_position,
                    output.logical_size
                );
                self.outputs.insert(OutputId::from(&output), output.clone());
                self.open_panels(&output)
            }
            ShellEvent::OutputRemoved(output) => {
                let gone = OutputId::from(&output);
                self.outputs.remove(&gone);
                let ids: Vec<Id> = self
                    .panels
                    .iter()
                    .filter(|(_, p)| p.output == gone)
                    .map(|(id, _)| *id)
                    .collect();
                log::info!(
                    "output {:?} removed, closing {} panel(s)",
                    output.name,
                    ids.len()
                );
                Task::batch(ids.into_iter().map(|id| {
                    self.panels.remove(&id);
                    Task::done(Message::RemoveWindow(id))
                }))
            }
            ShellEvent::Closed(id) => {
                let is_launcher = self.launcher.as_ref().is_some_and(|l| l.window == id);
                if is_launcher || self.grabs.iter().any(|(g, _)| *g == id) {
                    // One of the launcher's surfaces went away (on our
                    // request, or not): the rest follows.
                    self.grabs.retain(|(g, _)| *g != id);
                    if is_launcher {
                        self.launcher = None;
                    }
                    return self.close_launcher();
                }
                if self.panels.remove(&id).is_some() {
                    self.popups.retain(|_, (panel, _)| *panel != id);
                } else if let Some((panel, _)) = self.popups.remove(&id)
                    && let Some(panel) = self.panels.get_mut(&panel)
                {
                    panel.popup_closed(id);
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Open every configured panel that wants this output and isn't
    /// already shown on it (the shell broadcast replays outputs to late
    /// subscribers, so an output can be announced more than once).
    fn open_panels(&mut self, output: &OutputInfo) -> Task<Message> {
        let output_id = OutputId::from(output);
        let mut tasks = Vec::new();
        for (section, cfg) in PanelConfig::all(&self.config) {
            let shown = self
                .panels
                .values()
                .any(|p| p.output == output_id && p.section == section);
            if shown || !cfg.wants_output(output) {
                continue;
            }
            log::info!("opening [{section}] on output {:?}", output.name);
            let panel = Panel::new(section, cfg, &self.config, &self.theme, output);
            let id = Id::unique();
            tasks.push(Task::done(Message::NewLayerShell {
                settings: panel.layer_settings(),
                id,
            }));
            self.panels.insert(id, panel);
        }
        Task::batch(tasks)
    }

    fn view(&self, window: Id) -> Element<'_, Message> {
        let shared = self.shared();
        if let Some(open) = &self.launcher
            && open.window == window
        {
            let root: Element<'_, launcher::Message> = self
                .theme
                .container(&Node::root("launcher"), open.launcher.view(shared))
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
            return root.map(Message::Launcher);
        }
        if self.grabs.iter().any(|(g, _)| *g == window) {
            return launcher::grab_view().map(Message::Launcher);
        }
        if let Some(panel) = self.panels.get(&window) {
            return panel.view(shared).map(move |m| Message::Panel(window, m));
        }
        if let Some(&(owner, _)) = self.popups.get(&window)
            && let Some(panel) = self.panels.get(&owner)
        {
            return panel
                .popup_view(window, shared)
                .map(move |m| Message::Panel(owner, m));
        }
        widget::Space::new().into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let panels = self.panels.iter().map(|(id, panel)| {
            panel
                .subscription()
                .with(*id)
                .map(|(id, m)| Message::Panel(id, m))
        });
        let mut files = Vec::new();
        if self.general.reload_config {
            files.extend(self.config.path().map(Path::to_path_buf));
        }
        if self.general.reload_style {
            files.extend(self.theme.files().iter().cloned());
        }
        files.extend(self.icons.watch_dirs());
        let launcher = self.launcher.iter().map(|open| {
            open.launcher
                .subscription()
                .map(|(w, m)| Message::LauncherEvent(w, m))
        });
        Subscription::batch(
            [
                self.shell_events.listen().map(Message::Shell),
                self.compositor.subscription().map(Message::Compositor),
                commands::listen().map(Message::Command),
                watch::watch(&files).map(Message::Files),
                iced::event::listen_with(|event, _, window| match event {
                    Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::Cursor(window, position))
                    }
                    _ => None,
                }),
            ]
            .into_iter()
            .chain(panels)
            .chain(launcher),
        )
    }
}

/// Bounds of the `container` tagged `id`, in the coordinates of the
/// surface it's in. The runtime walks every window, so the id must be
/// unique across them.
fn widget_bounds(id: widget::Id) -> Task<Option<Rectangle>> {
    struct Find {
        id: widget::Id,
        bounds: Option<Rectangle>,
    }

    impl Operation<Option<Rectangle>> for Find {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<Rectangle>>)) {
            if self.bounds.is_none() {
                operate(self);
            }
        }

        fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
            if id == Some(&self.id) {
                self.bounds = Some(bounds);
            }
        }

        fn finish(&self) -> Outcome<Option<Rectangle>> {
            Outcome::Some(self.bounds)
        }
    }

    iced::advanced::widget::operate(Find { id, bounds: None })
}

/// Element path and bounds of every widget the theme helpers tagged, in
/// every window (the runtime runs the operation on all of them; the
/// path's root tells the surface apart).
fn widget_rects() -> Task<Vec<(String, Rectangle)>> {
    struct Collect(Vec<(String, Rectangle)>);

    impl Operation<Vec<(String, Rectangle)>> for Collect {
        fn traverse(
            &mut self,
            operate: &mut dyn FnMut(&mut dyn Operation<Vec<(String, Rectangle)>>),
        ) {
            operate(self);
        }

        fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
            if let Some(path) = id.and_then(theme::widget_path) {
                self.0.push((path, bounds));
            }
        }

        fn finish(&self) -> Outcome<Vec<(String, Rectangle)>> {
            Outcome::Some(self.0.clone())
        }
    }

    iced::advanced::widget::operate(Collect(Vec::new()))
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

    let (shell_broadcast, shell_events) = shell::channel();

    daemon(
        move || AriaShell::new(shell_events.clone()),
        "aria-shell",
        AriaShell::update,
        AriaShell::view,
    )
    .subscription(AriaShell::subscription)
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
