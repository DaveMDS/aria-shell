//! The shell's surfaces: Elm components with their own `Message`,
//! `update` and `view` on layer (or lock) surfaces the daemon opens,
//! owns and routes to.

pub mod dialog;
pub mod exiter;
pub mod launcher;
pub mod locker;
pub mod osd;
pub mod panel;
pub mod wallpaper;

use iced::window::Id;
use iced_exwlshell::reexport::{Anchor, LayerSize, NewLayerShellSettings};

/// What a component asks of its surfaces after a change: the runtime's
/// surface messages are the daemon's to send, the component only says
/// which surfaces to open, close, resize or draw again.
#[derive(Default)]
pub struct Surfaces {
    pub open: Vec<(Id, NewLayerShellSettings)>,
    pub close: Vec<Id>,
    pub resize: Vec<(Id, Anchor, LayerSize)>,
    pub redraw: Vec<Id>,
}
