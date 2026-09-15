use iced::widget::{container, row};
use iced::{Element, Length};

use iced_exwlshell::reexport::{Anchor, KeyboardInteractivity, Layer, LayerSize};
use iced_exwlshell::settings::LayerShellSettings;

/// Panel bar height in pixels -- fixed for this spike (no auto-sizing to
/// content, see the `exclusive_zone` note below).
const PANEL_HEIGHT: u32 = 32;

use crate::Message;
use crate::modules::{GadgetSlot, request_gadget};

/// Mirrors `AriaWindow.__init__` + `AriaPanel.__init__` combined, trimmed
/// hard to what a single top-anchored panel bar needs -- no
/// `grab_display`/`hide_on_escape`/`KeyboardMode::EXCLUSIVE`/generic
/// reusable window base (that generality served the
/// launcher/lock-screen/exiter windows too in Python, all out of scope
/// here).
pub fn panel_layer_settings() -> LayerShellSettings {
    LayerShellSettings {
        // mirrors PanelConfig.size == "fill": TOP + LEFT + RIGHT anchors,
        // full width, fixed height (default `LayerSize::FILL` would fill
        // the whole remaining output height since only Top is anchored
        // vertically -- confirmed by a first real run, see RS-PORT.md).
        anchor: Anchor::Top | Anchor::Left | Anchor::Right,
        size: LayerSize::fill_width(PANEL_HEIGHT),
        // Python default is "bottom" (LAYERS["bottom"]); hardcoded to Top
        // here for visibility during development of the spike.
        layer: Layer::Top,
        // GtkLayerShell.auto_exclusive_zone_enable() computes a zone from
        // the surface's actual size; iced_exwlshell's exclusive_zone is a
        // plain pixel count (protocol-level -1 = "don't care", not
        // "auto-size to content" -- see RS-PORT.md's uncertain-points
        // list). A fixed pixel value matching the bar height is an
        // acceptable stand-in for this spike.
        exclusive_zone: PANEL_HEIGHT as i32,
        // mirrors AriaWindow.KeyboardMode.NONE
        keyboard_interactivity: KeyboardInteractivity::None,
        // mirrors AriaWindow margins=(0, 0, 0, 0)
        margin: (0, 0, 0, 0),
        ..Default::default()
    }
}

/// Mirrors `AriaPanel`'s 3-box (start/center/end) content, hardcoded to a
/// single centered Clock for this spike -- no `[panel]`/`PanelConfig`
/// parsing yet, no `items_start`/`items_center`/`items_end` config
/// wiring (see RS-PORT.md's "explicitly out of scope" list).
pub struct PanelState {
    start: Vec<GadgetSlot>,
    center: Vec<GadgetSlot>,
    end: Vec<GadgetSlot>,
}

impl PanelState {
    /// Mirrors `AriaPanel.populate()`'s default-empty-config behavior
    /// (`self.conf.items_center = ['Clock']`) -- only the *result* is
    /// mirrored here, not the mechanism.
    pub fn new_default(output_name: &str) -> Self {
        Self {
            start: Vec::new(),
            center: request_gadget("Clock", output_name).into_iter().collect(),
            end: Vec::new(),
        }
    }

    /// Mirrors `ClockModule.timer_cb()` broadcasting to every
    /// `self.gadgets` instance -- walks every gadget slot currently on
    /// the panel and lets the top-level `update()` dispatch into it.
    pub fn all_slots_mut(&mut self) -> impl Iterator<Item = &mut GadgetSlot> {
        self.start
            .iter_mut()
            .chain(self.center.iter_mut())
            .chain(self.end.iter_mut())
    }

    /// Mirrors `AriaPanel.setup_window()`'s `Gtk.CenterBox` with 3 child
    /// `Gtk.Box`: three equal-width sections, aligned start/center/end.
    pub fn view(&self) -> Element<'_, Message> {
        let start = row(self.start.iter().map(GadgetSlot::view));
        let center = row(self.center.iter().map(GadgetSlot::view));
        let end = row(self.end.iter().map(GadgetSlot::view));

        row![
            container(start).width(Length::Fill),
            container(center).width(Length::Fill).center_x(Length::Fill),
            container(end)
                .width(Length::Fill)
                .align_right(Length::Fill),
        ]
        .into()
    }
}
