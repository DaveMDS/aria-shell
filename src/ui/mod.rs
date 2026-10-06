//! The building blocks of every surface: the theme (the CSS-like files,
//! resolved per widget, and the themed widgets built from them), and
//! the reusable pieces a gadget or a component embeds: plain Elm
//! components whose messages the host maps (the menu, the calendar),
//! drawn ones (the graphs), and the shapes the shell shows things in (a
//! notification's toast).

pub mod calendar;
pub mod graph;
pub mod menu;
pub mod theme;
pub mod toast;

use iced::advanced::widget::operation::{Operation, Outcome};
use iced::{Rectangle, Task, widget};

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
