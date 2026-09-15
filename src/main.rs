mod config;
mod gadget;
mod gadgets;
mod panel;

use std::collections::BTreeMap;

use iced::window::Id;
use iced::{Element, Subscription, Task};
use iced_exwlshell::build_pattern::daemon;
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::shell::{self, ShellEvent, ShellReceiver};
use iced_exwlshell::to_layer_message;
use iced_wayland_subscriber::{OutputId, OutputInfo};

use config::Config;
use panel::{Panel, PanelConfig};

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
}

struct AriaShell {
    config: Config,
    shell_events: ShellReceiver,
    /// One entry per open layer surface.
    panels: BTreeMap<Id, Panel>,
}

impl AriaShell {
    fn new(shell_events: ShellReceiver) -> Self {
        Self {
            config: Config::load(),
            shell_events,
            panels: BTreeMap::new(),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Shell(event) => self.on_shell_event(event),
            Message::Panel(id, m) => match self.panels.get_mut(&id) {
                Some(panel) => panel.update(m).map(move |m| Message::Panel(id, m)),
                None => Task::none(),
            },
            _ => Task::none(), // runtime variants, handled by the runtime
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
                self.panels.remove(&id);
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
        match self.panels.get(&window) {
            Some(panel) => panel.view().map(move |m| Message::Panel(window, m)),
            None => iced::widget::Space::new().into(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let panels = self.panels.iter().map(|(id, panel)| {
            panel
                .subscription()
                .with(*id)
                .map(|(id, m)| Message::Panel(id, m))
        });
        Subscription::batch(
            std::iter::once(self.shell_events.listen().map(Message::Shell)).chain(panels),
        )
    }
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
