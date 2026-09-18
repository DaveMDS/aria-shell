//! A modal surface: the launcher, the exit menu. One layer surface on
//! the focused output, centred by the compositor, with the keyboard
//! (`Exclusive`), and a transparent full-screen surface on every
//! output under it (`Layer::Top`, above the bars) so a click anywhere
//! else closes it and is swallowed, as a compositor does for a popup's.
//! The daemon opens the surfaces this asks for, routes `Closed` and
//! the clicks back, and draws the content; one dialog at a time.
//!
//! Learned on the launcher (see RS-PORT.md): Hyprland routes every
//! pointer event to an exclusive-keyboard layer while it's mapped, so
//! a click outside arrives tagged with the dialog's window (with
//! surface-local coordinates past its size, or none at all when the
//! pointer never entered it: the bar button that opened the dialog,
//! clicked again). What tells a click outside from one inside, on
//! every compositor, is iced's event status: the dialog's content sits
//! in a `mouse_area` that captures presses, so a press *ignored* by
//! the widget tree on any of the dialog's windows (a grab, or the
//! dialog past its edges) is outside. It's noted on the press and
//! acted on at the release ([`Dialog::released`]): closing on the
//! press destroys the surface before its release, and Hyprland then
//! swallows the next click.

use iced::event::Status;
use iced::widget::{Space, mouse_area};
use iced::window::Id;
use iced::{Element, Event, Point, Rectangle, Size, Subscription};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};

pub struct Dialog {
    pub window: Id,
    pub output: OutputId,
    pub size: (u32, u32),
    /// The click-catching surfaces, one per output.
    pub grabs: Vec<(Id, OutputId)>,
    /// A press landed outside: the next release closes.
    pressed_outside: bool,
}

impl Dialog {
    /// The surfaces to open: the dialog on `output`, sized `size`, and
    /// a grab on each of `outputs`. `namespace` names the layer
    /// surface (`aria-launcher`; the grabs get `-grab`).
    pub fn open(
        namespace: &str,
        output: &OutputInfo,
        size: (u32, u32),
        outputs: impl IntoIterator<Item = OutputInfo>,
    ) -> (Self, Vec<(Id, NewLayerShellSettings)>) {
        let mut surfaces = Vec::new();
        let mut grabs = Vec::new();
        for o in outputs {
            let id = Id::unique();
            grabs.push((id, OutputId::from(&o)));
            surfaces.push((
                id,
                NewLayerShellSettings {
                    anchor: Anchor::all(),
                    size: LayerSize::FILL,
                    layer: Layer::Top,
                    exclusive_zone: Some(-1),
                    margin: None,
                    keyboard_interactivity: KeyboardInteractivity::None,
                    output_option: OutputOption::GlobalName(o.id),
                    namespace: Some(format!("{namespace}-grab")),
                    ..Default::default()
                },
            ));
        }
        let window = Id::unique();
        log::info!(
            "opening {namespace} on output {:?} as window {window:?}, grabs {grabs:?}",
            output.name
        );
        surfaces.push((window, Self::settings(namespace, output, size)));
        let dialog = Self {
            window,
            output: OutputId::from(output),
            size,
            grabs,
            pressed_outside: false,
        };
        (dialog, surfaces)
    }

    fn settings(namespace: &str, output: &OutputInfo, size: (u32, u32)) -> NewLayerShellSettings {
        NewLayerShellSettings {
            anchor: Anchor::empty(),
            size: LayerSize::px(size.0.max(1), size.1.max(1)),
            layer: Layer::Overlay,
            exclusive_zone: Some(-1),
            margin: None,
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            output_option: OutputOption::GlobalName(output.id),
            namespace: Some(namespace.to_owned()),
            ..Default::default()
        }
    }

    /// Every window of the dialog, to remove.
    pub fn windows(&self) -> impl Iterator<Item = Id> + '_ {
        std::iter::once(self.window).chain(self.grabs.iter().map(|(id, _)| *id))
    }

    /// The anchor and size for a `LayoutChange` after the content
    /// changed size.
    pub fn resize(&mut self, size: (u32, u32)) -> (Anchor, LayerSize) {
        self.size = size;
        (Anchor::empty(), LayerSize::px(size.0.max(1), size.1.max(1)))
    }

    pub fn is_window(&self, id: Id) -> bool {
        self.window == id
    }

    pub fn is_grab(&self, id: Id) -> bool {
        self.grabs.iter().any(|(g, _)| *g == id)
    }

    pub fn owns(&self, id: Id) -> bool {
        self.is_window(id) || self.is_grab(id)
    }

    /// A pointer event on window `window`; whether the dialog should
    /// close: a press no widget took on one of its windows, followed
    /// by its release.
    pub fn pointer(&mut self, window: Id, event: PointerEvent) -> bool {
        if !self.owns(window) {
            return false;
        }
        match event {
            PointerEvent::PressedOutside => {
                log::debug!("press outside the dialog, on window {window:?}");
                self.pressed_outside = true;
                false
            }
            PointerEvent::Released => std::mem::take(&mut self.pressed_outside),
        }
    }

    /// Where the dialog is, given its output's rectangle: centred (the
    /// compositor places an unanchored layer surface so).
    pub fn rect(&self, output: Rectangle) -> Rectangle {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        Rectangle::new(
            Point::new(
                output.x + (output.width - w) / 2.0,
                output.y + (output.height - h) / 2.0,
            ),
            Size::new(w, h),
        )
    }
}

/// The content of a grab surface: transparent, nothing takes a click
/// (so the press is ignored, which is how it's told apart).
pub fn grab_view<'a, M: 'a>() -> Element<'a, M> {
    Space::new()
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// The dialog's content, taking every press on it so that only a
/// press past the content is ignored. `nothing` is the message the
/// capture produces (the components have a no-op one).
pub fn content<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    nothing: M,
) -> Element<'a, M> {
    mouse_area(content).on_press(nothing).into()
}

#[derive(Debug, Clone, Copy)]
pub enum PointerEvent {
    /// A mouse button was pressed and no widget took it.
    PressedOutside,
    /// A mouse button was released.
    Released,
}

/// The pointer events [`Dialog::pointer`] wants, from every window.
pub fn pointer_events() -> Subscription<(Id, PointerEvent)> {
    iced::event::listen_with(|event, status, window| match (event, status) {
        (Event::Mouse(iced::mouse::Event::ButtonPressed(_)), Status::Ignored) => {
            Some((window, PointerEvent::PressedOutside))
        }
        (Event::Mouse(iced::mouse::Event::ButtonReleased(_)), _) => {
            Some((window, PointerEvent::Released))
        }
        _ => None,
    })
}
