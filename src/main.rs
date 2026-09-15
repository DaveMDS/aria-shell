mod compositor;
mod config;
mod gadget;
mod gadgets;
mod panel;
mod widgets;

use std::collections::BTreeMap;

use iced::advanced::widget::operation::{Operation, Outcome};
use iced::window::Id;
use iced::{Element, Rectangle, Subscription, Task, widget};
use iced_exwlshell::build_pattern::daemon;
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::shell::{self, ShellEvent, ShellReceiver};
use iced_exwlshell::to_layer_message;
use iced_wayland_subscriber::{OutputId, OutputInfo};

use compositor::Compositor;
use config::Config;
use gadget::Context;
use panel::{Action, Panel, PanelConfig};

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
}

struct AriaShell {
    config: Config,
    shell_events: ShellReceiver,
    compositor: Compositor,
    /// One entry per open layer surface.
    panels: BTreeMap<Id, Panel>,
    /// Open popup surfaces, to the panel each hangs off.
    popups: BTreeMap<Id, Id>,
}

impl AriaShell {
    fn new(shell_events: ShellReceiver) -> Self {
        Self {
            config: Config::load(),
            shell_events,
            compositor: Compositor::detect(),
            panels: BTreeMap::new(),
            popups: BTreeMap::new(),
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
            _ => Task::none(), // runtime variants, handled by the runtime
        }
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
            ShellEvent::OutputAdded(output) => self.open_panels(&output),
            ShellEvent::OutputRemoved(output) => {
                let gone = OutputId::from(&output);
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
            let panel = Panel::new(section, cfg, &self.config, output);
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
        let ctx = Context {
            compositor: &self.compositor,
        };
        if let Some(panel) = self.panels.get(&window) {
            return panel.view(ctx).map(move |m| Message::Panel(window, m));
        }
        if let Some(&owner) = self.popups.get(&window)
            && let Some(panel) = self.panels.get(&owner)
        {
            return panel
                .popup_view(window, ctx)
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
        Subscription::batch(
            [
                self.shell_events.listen().map(Message::Shell),
                self.compositor.subscription().map(Message::Compositor),
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
