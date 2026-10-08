//! The daemon beyond its router (`main.rs`: `Message`, `update`,
//! `perform`): more `impl AriaShell` blocks, by subject. Child modules
//! of the crate root, they see its private fields.

mod debug;
mod outputs;
pub(crate) mod showcase;
mod surfaces;
mod view;

pub(crate) use surfaces::{surface_tasks, wallpaper_tasks};
