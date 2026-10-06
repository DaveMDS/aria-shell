//! State the daemon owns and shares: each service is fed by an external
//! source (`subscription()` → `apply(Event)`), reaches gadgets read-only
//! through `gadget::Context` and is changed by `run(Command)`, from a
//! gadget's `Action`. No view of its own, but for the screenshot picker
//! and the notifications' toasts.

pub mod audio;
pub mod brightness;
pub mod compositor;
pub mod icons;
pub mod idle;
pub mod network;
pub mod notifications;
pub mod places;
pub mod power;
pub mod screenshot;
pub mod scripts;
pub mod sysmon;
pub mod tray;
