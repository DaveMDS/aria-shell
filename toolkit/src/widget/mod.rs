//! The widgets: each a real iced widget on the advanced API, styled by
//! its node. See TOOLKIT.md, "Widgets".

mod boxed;
mod calendar;
mod flex;
mod focus;
mod graph;
mod icon;
mod image;
mod input;
mod menu;
mod meter;
mod scroll;
mod slider;
mod surface;
mod tabs;
#[cfg(test)]
mod tests;
mod text;
mod toggle;

pub use calendar::{Calendar, Month, calendar};
pub use flex::{Flex, button, column, row};
pub use focus::{FocusScope, focus_scope};
pub use graph::{Graph, graph};
pub use icon::{Icon, IconFile, IconSource, Icons, icon};
pub use image::{Image, image};
pub use input::{Input, input};
pub use menu::{MenuItem, menu};
pub use meter::{Meter, meter};
pub use scroll::{Scroll, scroll};
pub use slider::{Slider, slider};
pub use surface::{Surface, surface};
pub use tabs::{Tab, tabs};
pub use text::{Text, text};
pub use toggle::toggle;
