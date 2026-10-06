//! The shell's surfaces: Elm components with their own `Message`,
//! `update` and `view` on layer (or lock) surfaces the daemon opens,
//! owns and routes to.

pub mod dialog;
pub mod exiter;
pub mod launcher;
pub mod locker;
pub mod osd;
pub mod panel;
pub mod picker;
pub mod wallpaper;
