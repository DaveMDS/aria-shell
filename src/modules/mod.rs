pub mod clock;

use crate::config::AriaConfig;
use crate::module::{GadgetRunContext, Module};

/// The "registry" -- replaces Python's `_loaded_gadgets`/dynamic
/// `importlib` with a plain enum, since the module set is closed and
/// known at compile time. Adding a future module (e.g. Workspaces) is:
/// one new file + one variant here + one match arm in `request_gadget`
/// + one `Message` variant in `main.rs`. No trait redesign.
pub enum GadgetSlot {
    Clock(clock::ClockState),
    // Workspaces(workspaces::WorkspacesState),   <-- future
}

impl GadgetSlot {
    pub fn view(&self) -> iced::Element<'_, crate::Message> {
        match self {
            GadgetSlot::Clock(state) => {
                clock::ClockModule::view(state).map(crate::Message::Clock)
            }
        }
    }
}

/// Mirrors `aria_shell.module.request_module_gadget(name, monitor)`.
/// `name` can be `"Clock"` or `"Clock:2"` exactly as in Python (instance
/// id after `:`).
pub fn request_gadget(name: &str, output_name: &str) -> Option<GadgetSlot> {
    let module_kind = name.split(':').next().unwrap_or(name);
    match module_kind {
        "Clock" => {
            let cfg = AriaConfig::global().section(Some(name));
            let ctx = GadgetRunContext {
                config: cfg,
                output_name: output_name.to_owned(),
            };
            Some(GadgetSlot::Clock(clock::ClockModule::gadget_factory(ctx)))
        }
        // "Workspaces" => { ... }   <-- future
        _ => {
            eprintln!("Cannot find module \"{module_kind}\" for gadget \"{name}\"");
            None
        }
    }
}
