//! The widgets: each a real iced widget on the advanced API, styled by
//! its node. See TOOLKIT.md, "Widgets".

mod boxed;
mod flex;
mod focus;
mod icon;
mod image;
mod meter;
mod slider;
mod surface;
#[cfg(test)]
mod tests;
mod text;
mod toggle;

pub use flex::{Flex, button, column, row};
pub use focus::{FocusScope, focus_scope};
pub use icon::{Icon, IconFile, IconSource, Icons, icon};
pub use image::{Image, image};
pub use meter::{Meter, meter};
pub use slider::{Slider, slider};
pub use surface::{Surface, surface};
pub use text::{Text, text};
pub use toggle::toggle;
