//! A bar's tip: the text of what the pointer rests on (an app's name in
//! the dock), shown off the bar on a layer surface of its own. Not a
//! popup: an xdg popup opened with no click wants a grab Hyprland
//! refuses (the pointer's enter serial), closing it at once; nor an
//! overlay of the bar's, which its surface would cut. The toolkit's
//! `tooltip(..).on_tip(..)` says when (after its delay, until the
//! pointer leaves or presses) and about what; a gadget passes that on
//! as [`Tip`], the daemon shows it with [`Tips::show`]. Its end says
//! what it was about: the pointer moving from one widget to the next
//! may tell the next one's start first.
//!
//! On the overlay layer, taking neither the keyboard nor the pointer,
//! anchored to the bar's edge of the output past the bar (its height;
//! the tip's margin, in its surface, keeps it away), centred on what
//! it's about, inside the output.
//! One at a time.
//!
//! ```text
//! surface.tooltip[output="<connector>"]   the toolkit's tip, its box
//! ╰─ label
//! ```

use iced::window::Id;
use iced::{Point, Rectangle, Size};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};
use icssed::{Element, Node};

use crate::ui::popup::{self, Side};
use crate::ui::theme::Theme;
use crate::ui::{self, Measuring, Surfaces};

/// What a gadget wants shown: `text` by `about`, a rectangle in its
/// bar's surface, in the UI's units (as the toolkit lays it out).
#[derive(Debug, Clone, PartialEq)]
pub struct Tip {
    pub text: String,
    pub about: Rectangle,
}

/// Where the bar a tip comes from is: its rectangle in its output, in
/// the compositor's units, and the side of it tips go.
#[derive(Debug, Clone, Copy)]
pub struct Bar {
    pub window: Id,
    pub rect: Rectangle,
    pub side: Side,
}

#[derive(Default)]
pub struct Tips {
    shown: Option<Shown>,
}

struct Shown {
    window: Id,
    /// The bar it's about.
    bar: Id,
    /// What it's about, in the bar.
    about: Rectangle,
    output: OutputId,
    text: String,
    /// Where it is in its output, in the compositor's units.
    rect: Rectangle,
}

impl Tips {
    /// `tip`, from the bar `bar` on output `info`, in place of the one
    /// shown: a new surface, so the theme's entry animation runs again.
    pub fn show(&mut self, tip: Tip, bar: Bar, info: &OutputInfo, m: Measuring<'_>) -> Surfaces {
        let mut surfaces = self.close();
        let output = OutputId::from(info);
        let window = Id::unique();
        let ui = m.scales.ui();
        let name = info.name.as_deref().unwrap_or_default();
        let root = root(m.shared.theme, m.scales.of(output), name);
        // Its margin, its distance from the bar, is in the surface.
        let room = popup::room(root, ui);
        let (w, h) = ui::size(
            m,
            window,
            view::<()>(root, &tip.text),
            ui::room_on(Some(info)),
        );
        let out = ui::room_on(Some(info));
        let (w, h) = (w as f32, h as f32);
        // Centred on what it's about, inside the output; its box, not
        // its surface, meets the bar.
        let center = bar.rect.x + (tip.about.x + tip.about.width / 2.0) * ui;
        let x = (center - w / 2.0).min(out.width - w).max(0.0).round();
        let (anchor, margin, y) = match bar.side {
            Side::Below => {
                let edge = bar.rect.y + bar.rect.height - room.top;
                (
                    Anchor::Top | Anchor::Left,
                    (edge as i32, 0, 0, x as i32),
                    edge,
                )
            }
            Side::Above => {
                let edge = out.height - bar.rect.y - room.bottom;
                let y = out.height - edge - h;
                (
                    Anchor::Bottom | Anchor::Left,
                    (0, 0, edge as i32, x as i32),
                    y,
                )
            }
        };
        self.shown = Some(Shown {
            window,
            bar: bar.window,
            about: tip.about,
            output,
            text: tip.text,
            rect: Rectangle::new(Point::new(x, y), Size::new(w, h)),
        });
        surfaces.open.push((
            window,
            NewLayerShellSettings {
                anchor,
                size: LayerSize::px(w as u32, h as u32),
                layer: Layer::Overlay,
                // At the output's edge whatever reserves room there: the
                // margin is the bar's.
                exclusive_zone: Some(-1),
                margin: Some(margin),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: OutputOption::GlobalName(info.id),
                // Over the windows near the bar: their clicks are theirs.
                events_transparent: true,
                namespace: Some("aria-tip".to_owned()),
                ..Default::default()
            },
        ));
        surfaces
    }

    /// Take it off the screen.
    pub fn close(&mut self) -> Surfaces {
        Surfaces {
            close: self.shown.take().map(|s| s.window).into_iter().collect(),
            ..Surfaces::default()
        }
    }

    /// Take it off if it's about `about` on bar `bar`: the pointer left.
    pub fn hide(&mut self, bar: Id, about: Rectangle) -> Surfaces {
        match &self.shown {
            Some(shown) if shown.bar == bar && shown.about == about => self.close(),
            _ => Surfaces::default(),
        }
    }

    /// Take it off if it's on bar `bar` (gone) or output `output`.
    pub fn close_on(&mut self, bar: Option<Id>, output: Option<OutputId>) -> Surfaces {
        match &self.shown {
            Some(s) if Some(s.bar) == bar || Some(s.output) == output => self.close(),
            _ => Surfaces::default(),
        }
    }

    /// Surface `window` was closed: whether it was the tip's.
    pub fn closed(&mut self, window: Id) -> bool {
        if self.shown.as_ref().is_some_and(|s| s.window == window) {
            self.shown = None;
            return true;
        }
        false
    }

    pub fn output_of(&self, window: Id) -> Option<OutputId> {
        self.shown
            .as_ref()
            .filter(|s| s.window == window)
            .map(|s| s.output)
    }

    /// Its surface, its output, where it is there.
    pub fn placed(&self) -> Option<(Id, OutputId, Rectangle)> {
        self.shown.as_ref().map(|s| (s.window, s.output, s.rect))
    }

    /// What its surface shows on output `output` (its name), at `scale`.
    pub fn view<'a, M: Clone + 'a>(
        &'a self,
        theme: &'a Theme,
        output: &str,
        scale: f32,
    ) -> Element<'a, M> {
        let root = root(theme, scale, output);
        match &self.shown {
            Some(shown) => view(root, &shown.text),
            None => icssed::column![].at(root).into(),
        }
    }
}

/// `surface.tooltip[output=..]`: the toolkit styles it as the tip.
fn root<'a>(theme: &'a Theme, scale: f32, output: &str) -> Node<'a> {
    theme.root("tooltip", scale).attr("output", output)
}

fn view<'a, M: Clone + 'a>(root: Node<'a>, text: &str) -> Element<'a, M> {
    icssed::tip(text.to_owned()).at(root).into()
}
