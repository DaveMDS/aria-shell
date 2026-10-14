//! The building blocks of every surface: the theme (the stylesheets,
//! compiled into the toolkit's sheet), the reusable pieces a gadget or
//! a component embeds over the toolkit's widgets (plain Elm components
//! whose messages the host maps: the menu, the calendar), and the
//! shapes the shell shows things in (a gadget's popup, a
//! notification's toast).

pub mod calendar;
pub mod menu;
pub mod popup;
pub mod theme;
pub mod toast;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::hash::Hash;

use iced::advanced::widget::operation::{Operation, Outcome, Scrollable};
use iced::window::Id;
use iced::{Rectangle, Size, Task, Vector, widget};
use iced_wayland_subscriber::OutputId;
use icssed::Measure;

use crate::shared::Shared;
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

/// What measuring a surface's content takes: the state its view reads,
/// the toolkit's measure (a tree kept per surface), each output's
/// scale.
#[derive(Clone, Copy)]
pub struct Measuring<'a> {
    pub shared: Shared<'a>,
    pub measure: &'a RefCell<Measure>,
    pub scales: &'a Scales,
}

/// Each output's scale, device pixels per logical one, times the UI's
/// (`[general] scale`): what its surfaces' roots are made with. The
/// compositor tells an output's per surface (`wp_fractional_scale_v1`),
/// which iced answers when asked (`window::scale_factor`); until a
/// surface on the output has told it, the output's integer scale.
///
/// iced draws every surface at the UI's scale too: a view is laid out
/// in the UI's units, the compositor's are that much bigger. What's
/// measured or read from the theme goes to the compositor through
/// [`Scales::ui`] ([`size`], the margins, a popup's anchor).
#[derive(Debug)]
pub struct Scales {
    outputs: BTreeMap<OutputId, f32>,
    ui: f32,
}

impl Default for Scales {
    fn default() -> Self {
        Self {
            outputs: BTreeMap::new(),
            ui: 1.0,
        }
    }
}

impl Scales {
    pub fn of(&self, output: OutputId) -> f32 {
        self.outputs.get(&output).copied().unwrap_or(1.0) * self.ui
    }

    /// The UI's scale: the compositor's units per the UI's.
    pub fn ui(&self) -> f32 {
        self.ui
    }

    /// `[general] scale`: whether it's another.
    pub fn set_ui(&mut self, scale: f32) -> bool {
        std::mem::replace(&mut self.ui, scale) != scale
    }

    /// A new output, at its integer `scale` until a surface tells the
    /// real one.
    pub fn output_added(&mut self, output: OutputId, scale: i32) {
        self.outputs.entry(output).or_insert(scale.max(1) as f32);
    }

    pub fn output_removed(&mut self, output: OutputId) {
        self.outputs.remove(&output);
    }

    /// A surface on `output` is drawn at `scale`: whether it's another.
    pub fn set(&mut self, output: OutputId, scale: f32) -> bool {
        self.outputs.insert(output, scale) != Some(scale)
    }
}

/// The size of surface `key`'s content `element`, within `max`: its
/// layout, run headlessly (`icssed::Measure`, a tree kept per surface);
/// what a layer surface or a popup opens at. Both in the compositor's
/// units: the UI's scale times the layout's.
pub fn size<'a, M>(
    m: Measuring<'_>,
    key: impl Hash,
    element: icssed::Element<'a, M>,
    max: Size,
) -> (u32, u32) {
    // Laid out in the UI's units, within the room in them.
    let ui = m.scales.ui();
    let size = m.measure.borrow_mut().size(key, element, max * (1.0 / ui));
    let surface = |v: f32| ((v.trunc() * ui).ceil() as u32).max(1);
    (surface(size.width), surface(size.height))
}

/// The room a surface on `output` may take: the output's logical size
/// (a box that fills takes all of it, a list grows up to it). An output
/// whose size isn't known yet gets a large one.
pub fn room_on(output: Option<&iced_wayland_subscriber::OutputInfo>) -> Size {
    output
        .and_then(|o| o.logical_size)
        .map_or(Size::new(8192.0, 8192.0), |(w, h)| {
            Size::new(w as f32, h as f32)
        })
}

/// Bounds of the box tagged `id`, in the coordinates of the
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

/// Bounds of the scroll `id`, its viewport, in the coordinates of the
/// surface it's in (its content's widgets report where they'd be
/// unscrolled). The id must be unique across windows.
pub fn scroll_bounds(id: widget::Id) -> Task<Option<Rectangle>> {
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

        fn scrollable(
            &mut self,
            id: Option<&widget::Id>,
            bounds: Rectangle,
            _content_bounds: Rectangle,
            _translation: Vector,
            _state: &mut dyn Scrollable,
        ) {
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
