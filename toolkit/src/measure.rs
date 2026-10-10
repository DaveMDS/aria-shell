//! A tree laid out with no surface: the real widgets and the real
//! layout, on a headless tiny-skia renderer (CPU, no window). What a
//! layer surface opens at, the one source of truth for its size.

use std::hash::{Hash, Hasher};

use iced::advanced::layout::Limits;
use iced::advanced::renderer::Headless;
use iced::advanced::widget::Tree;
use iced::{Font, Pixels, Size};
use rustc_hash::{FxHashMap, FxHasher};

use crate::{Element, Renderer};

pub struct Measure {
    renderer: Renderer,
    /// Kept per surface, so that text is shaped once.
    trees: FxHashMap<u64, Tree>,
}

/// The tiny-skia renderer, headless: CPU, no window.
pub(crate) fn headless() -> Renderer {
    iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("tiny-skia"),
    ))
    .expect("the tiny-skia renderer needs no device")
}

impl Measure {
    pub fn new() -> Self {
        Self {
            renderer: headless(),
            trees: FxHashMap::default(),
        }
    }

    /// The size `element` takes within `max`, rounded up to whole
    /// logical pixels as a layer surface's size is. `surface` names the
    /// tree kept for it: built at the scale its root node was made for,
    /// the element is laid out as the window will lay it out.
    pub fn size<'a, Message>(
        &mut self,
        surface: impl Hash,
        element: impl Into<Element<'a, Message>>,
        max: Size,
    ) -> Size {
        let mut element = element.into();
        let mut hasher = FxHasher::default();
        surface.hash(&mut hasher);
        let tree = self
            .trees
            .entry(hasher.finish())
            .or_insert_with(|| Tree::new(element.as_widget()));
        tree.diff(element.as_widget());
        let node =
            element
                .as_widget_mut()
                .layout(tree, &self.renderer, &Limits::new(Size::ZERO, max));
        let size = node.size();
        Size::new((size.width - 1e-3).ceil(), (size.height - 1e-3).ceil())
    }

    /// Drops the tree kept for `surface`, closed.
    pub fn forget(&mut self, surface: impl Hash) {
        let mut hasher = FxHasher::default();
        surface.hash(&mut hasher);
        self.trees.remove(&hasher.finish());
    }
}

impl Default for Measure {
    fn default() -> Self {
        Self::new()
    }
}
