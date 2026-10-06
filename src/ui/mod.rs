//! The building blocks of every surface: the theme (the CSS-like files,
//! resolved per widget, and the themed widgets built from them), and
//! the reusable pieces a gadget or a component embeds: plain Elm
//! components whose messages the host maps (the menu, the calendar),
//! drawn ones (the graphs), and the shapes the shell shows things in (a
//! gadget's popup, a notification's toast).

pub mod calendar;
pub mod graph;
pub mod menu;
pub mod popup;
pub mod theme;
pub mod toast;

use iced::advanced::widget::operation::{Operation, Outcome};
use iced::window::Id;
use iced::{Rectangle, Task, widget};
use iced_exwlshell::actions::IcedNewPopupSettings;
use iced_exwlshell::reexport::{Anchor, KeyboardInteractivity, LayerSize, NewLayerShellSettings};

/// What a component asks of its surfaces after a change: the runtime's
/// surface messages are the daemon's to send, the component only says
/// which surfaces to open (layer surfaces, or popups off one), close,
/// resize, move or draw again, and which take the keyboard. What
/// every owner of surfaces answers: a component, the panel for its
/// popups, the toasts.
#[derive(Default)]
pub struct Surfaces {
    /// Keyboard interactivity changes, first: a popup wanting the
    /// keyboard must map on a surface that already has it.
    pub keyboard: Vec<(Id, KeyboardInteractivity)>,
    pub open: Vec<(Id, NewLayerShellSettings)>,
    pub popup: Vec<(Id, IcedNewPopupSettings)>,
    pub close: Vec<Id>,
    pub resize: Vec<(Id, Anchor, LayerSize)>,
    /// New margins (top, right, bottom, left): a toast moving in its
    /// stack.
    pub margin: Vec<(Id, (i32, i32, i32, i32))>,
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
        self.margin.extend(other.margin);
        self.reposition.extend(other.reposition);
        self.redraw.extend(other.redraw);
        self
    }
}

/// Bounds of the `container` tagged `id`, in the coordinates of the
/// surface it's in. The runtime walks every window, so the id must be
/// unique across them.
pub fn bounds(id: widget::Id) -> Task<Option<Rectangle>> {
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
