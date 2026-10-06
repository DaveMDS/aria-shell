//! The shell's surfaces: Elm components with their own `Message`,
//! `update` and `view` on layer (or lock) surfaces the daemon opens,
//! owns and routes to.

pub mod dialog;
pub mod exiter;
pub mod launcher;
pub mod locker;
pub mod osd;
pub mod panel;
pub mod picker;
pub mod wallpaper;

use iced::window::Id;
use iced_exwlshell::actions::IcedNewPopupSettings;
use iced_exwlshell::reexport::{Anchor, KeyboardInteractivity, LayerSize, NewLayerShellSettings};

/// What a component asks of its surfaces after a change: the runtime's
/// surface messages are the daemon's to send, the component only says
/// which surfaces to open (layer surfaces, or popups off one), close,
/// resize, move or draw again, and which take the keyboard.
#[derive(Default)]
pub struct Surfaces {
    /// Keyboard interactivity changes, first: a popup wanting the
    /// keyboard must map on a surface that already has it.
    pub keyboard: Vec<(Id, KeyboardInteractivity)>,
    pub open: Vec<(Id, NewLayerShellSettings)>,
    pub popup: Vec<(Id, IcedNewPopupSettings)>,
    pub close: Vec<Id>,
    pub resize: Vec<(Id, Anchor, LayerSize)>,
    /// Popups placed again (their size changed).
    pub reposition: Vec<(Id, IcedNewPopupSettings)>,
    pub redraw: Vec<Id>,
}

impl Surfaces {
    /// These and `other`'s, in one.
    pub fn and(mut self, other: Surfaces) -> Surfaces {
        self.keyboard.extend(other.keyboard);
        self.open.extend(other.open);
        self.popup.extend(other.popup);
        self.close.extend(other.close);
        self.resize.extend(other.resize);
        self.reposition.extend(other.reposition);
        self.redraw.extend(other.redraw);
        self
    }
}
