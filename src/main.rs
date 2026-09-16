mod compositor;
mod config;
mod gadget;
mod gadgets;
mod panel;
mod theme;
mod watch;
mod widgets;

use std::collections::BTreeMap;
use std::path::Path;

use iced::advanced::widget::operation::{Operation, Outcome};
use iced::window::Id;
use iced::{Color, Element, Rectangle, Subscription, Task, widget};
use iced_exwlshell::build_pattern::daemon;
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::shell::{self, ShellEvent, ShellReceiver};
use iced_exwlshell::to_layer_message;
use iced_wayland_subscriber::{OutputId, OutputInfo};

use compositor::Compositor;
use config::{Config, GeneralConfig};
use gadget::Shared;
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
    /// Watched files (config, theme) changed on disk.
    Files(watch::Changed),
}

struct AriaShell {
    config: Config,
    general: GeneralConfig,
    shell_events: ShellReceiver,
    compositor: Compositor,
    theme: Theme,
    /// Monitors currently present, to rebuild the panels on a config
    /// change.
    outputs: BTreeMap<OutputId, OutputInfo>,
    /// One entry per open layer surface.
    panels: BTreeMap<Id, Panel>,
    /// Open popup surfaces, to the panel each hangs off.
    popups: BTreeMap<Id, Id>,
}

impl AriaShell {
    fn new(shell_events: ShellReceiver) -> Self {
        let config = Config::load();
        let general = config.section(None);
        let theme = Theme::load(&config);
        Self {
            config,
            general,
            shell_events,
            compositor: Compositor::detect(),
            theme,
            outputs: BTreeMap::new(),
            panels: BTreeMap::new(),
            popups: BTreeMap::new(),
        }
    }

    fn shared(&self) -> Shared<'_> {
        Shared {
            compositor: &self.compositor,
            theme: &self.theme,
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
                Task::none()
            }
            Message::Files(watch::Changed(paths)) => {
                let config_changed = self
                    .config
                    .path()
                    .is_some_and(|p| paths.contains(&p.to_path_buf()));
                if config_changed && self.general.reload_config {
                    self.reload_config()
                } else if self.general.reload_style {
                    self.reload_theme()
                } else {
                    Task::none()
                }
            }
            _ => Task::none(), // runtime variants, handled by the runtime
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
        let mut tasks: Vec<Task<Message>> = self
            .popups
            .keys()
            .chain(self.panels.keys())
            .map(|&id| Task::done(Message::RemoveWindow(id)))
            .collect();
        self.popups.clear();
        self.panels.clear();
        let outputs: Vec<OutputInfo> = self.outputs.values().cloned().collect();
        tasks.extend(outputs.iter().map(|o| self.open_panels(o)));
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
                let Some(position) = self.panels.get(&panel).map(Panel::position) else {
                    return Task::none();
                };
                self.popups.insert(id, panel);
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
                widget_bounds(anchor).map(move |bounds| Message::NewPopUp {
                    settings: panel::popup_settings(
                        panel,
                        position,
                        bounds.unwrap_or_default(),
                        size,
                    ),
                    id,
                })
            }
            Action::ClosePopup(id) => {
                self.popups.remove(&id);
                Task::done(Message::RemoveWindow(id))
            }
        }
    }

    fn on_shell_event(&mut self, event: ShellEvent) -> Task<Message> {
        match event {
            ShellEvent::OutputAdded(output) => {
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
                if self.panels.remove(&id).is_some() {
                    self.popups.retain(|_, panel| *panel != id);
                } else if let Some(panel) = self.popups.remove(&id)
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
        if let Some(panel) = self.panels.get(&window) {
            return panel.view(shared).map(move |m| Message::Panel(window, m));
        }
        if let Some(&owner) = self.popups.get(&window)
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
        Subscription::batch(
            [
                self.shell_events.listen().map(Message::Shell),
                self.compositor.subscription().map(Message::Compositor),
                watch::watch(&files).map(Message::Files),
            ]
            .into_iter()
            .chain(panels),
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

fn main() -> iced_exwlshell::Result {
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
