mod config;
mod module;
mod modules;
mod panel;
mod service;

use std::time::Duration;

use chrono::Local;
use iced_exwlshell::layershell::application;
use iced_exwlshell::to_layer_message;

use config::AriaConfig;
use module::Module;
use modules::GadgetSlot;
use panel::PanelState;

/// Top-level Message enum, one variant per module -- see the design note
/// on `module::Module` for why this is a flat enum and not `dyn
/// Any`/type-erasure. `#[to_layer_message]` injects the shell-lifecycle
/// variants `iced_exwlshell` needs alongside ours.
#[to_layer_message]
#[derive(Clone, Debug)]
pub(crate) enum Message {
    Clock(modules::clock::Message),
    /// Top-level ticker, broadcast to every gadget slot -- mirrors
    /// Python's single `Timer` looping over every `self.gadgets` entry.
    Tick(chrono::DateTime<Local>),
    // Workspaces(modules::workspaces::Message),   <-- future
}

struct AriaShell {
    panel: PanelState,
}

impl AriaShell {
    fn new() -> (Self, iced::Task<Message>) {
        let output_name = "default".to_owned(); // no real output
        // enumeration in this spike -- multi-monitor is out of scope.
        (
            Self {
                panel: PanelState::new_default(&output_name),
            },
            iced::Task::none(),
        )
    }

    fn namespace() -> String {
        "aria-panel".into() // mirrors AriaWindow namespace='aria-shell'
    }

    fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::Tick(now) => {
                for slot in self.panel.all_slots_mut() {
                    match slot {
                        GadgetSlot::Clock(state) => {
                            let _ = modules::clock::ClockModule::update(
                                state,
                                modules::clock::Message::Tick(now),
                            );
                        }
                    }
                }
                iced::Task::none()
            }
            Message::Clock(_inner) => iced::Task::none(), // Clock doesn't
            // originate messages of its own yet (no click handling)
            _ => iced::Task::none(), // shell-lifecycle variants injected
                                      // by #[to_layer_message]
        }
    }

    fn view(&self) -> iced::Element<'_, Message> {
        self.panel.view()
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        // Idiomatic iced pattern (Elm-architecture subscriptions, not a
        // GLib/Service timer): a single top-level tick, mirrors
        // ClockModule.timer_cb(instance=None) broadcasting to every
        // self.gadgets entry from one GLib timeout.
        iced::time::every(Duration::from_secs(1)).map(|_instant| Message::Tick(Local::now()))
    }
}

fn main() -> iced_exwlshell::Result {
    let _ = AriaConfig::global(); // load aria.conf eagerly, mirrors
    // AriaShell startup calling AriaConfig().load_conf()

    application(
        AriaShell::new,
        AriaShell::namespace,
        AriaShell::update,
        AriaShell::view,
    )
    .subscription(AriaShell::subscription)
    .layer_settings(panel::panel_layer_settings())
    .run()
}
