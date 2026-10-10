//! A toolkit of CSS-styled widgets on iced, knowing nothing of any app.
//! See TOOLKIT.md for its design, UI.md for what it's for.

mod anim;
mod layout;
mod measure;
mod paint;
pub mod style;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod theme;
pub mod widget;

pub use measure::Measure;
pub use style::{Node, Scheme, Sheet, Source, Warning};
pub use theme::Theme;
pub use widget::*;

/// The renderer every widget draws with: iced's, wgpu with tiny-skia
/// behind it.
pub type Renderer = iced::Renderer;

/// A widget of the toolkit, or an iced one that takes its `Theme`.
pub type Element<'a, Message> = iced::Element<'a, Message, Theme, Renderer>;
