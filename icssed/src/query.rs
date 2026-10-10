//! Widgets by selector: every widget reports its node and its border box
//! through `operate`, a CSS selector picks among them. For tests, and an
//! app's debugging (Aria's `debug widgets 'launcher item:nth-child(2)'`).
//!
//! The report is the tree in pre-order with each widget's depth: a
//! widget's parent is the last one before it a level up, whatever stock
//! widgets lie between; its position among its siblings comes from there,
//! so `:nth-child()` matches even where the sheet has none (its nodes
//! then share no position). A widget's box is where it's shown: moved as
//! the scrolls around it are scrolled.

use std::any::Any;

use iced::advanced::widget::Id;
use iced::advanced::widget::operation::{Operation, Outcome, Scrollable};
use iced::{Rectangle, Vector};

use crate::style::Node;

/// What a widget gives a [`Report`]: who it is.
pub(crate) struct Reported {
    sheet: usize,
    node: u32,
    depth: u16,
}

impl Reported {
    pub fn new(node: Node) -> Self {
        let (sheet, node_id) = node.key();
        Self {
            sheet,
            node: node_id,
            depth: node.depth(),
        }
    }
}

/// A widget found: its node, where it is in the tree, its border box.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub(crate) sheet: usize,
    pub(crate) node: u32,
    pub(crate) depth: u16,
    pub bounds: Rectangle,
}

/// The operation that collects every widget, in pre-order.
#[derive(Debug, Default)]
pub struct Report {
    pub found: Vec<Found>,
    /// How far the scrolls around the widgets reported now move them.
    offset: Vector,
    /// A scroll's own move, for the content it traverses next.
    scrolled: Option<Vector>,
}

impl Operation<Vec<Found>> for Report {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Vec<Found>>)) {
        let outside = self.offset;
        if let Some(translation) = self.scrolled.take() {
            self.offset -= translation;
        }
        operate(self);
        self.offset = outside;
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        _bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        self.scrolled = Some(translation);
    }

    fn custom(&mut self, _id: Option<&Id>, bounds: Rectangle, state: &mut dyn Any) {
        if let Some(reported) = state.downcast_mut::<Reported>() {
            self.found.push(Found {
                sheet: reported.sheet,
                node: reported.node,
                depth: reported.depth,
                bounds: bounds + self.offset,
            });
        }
    }

    fn finish(&self) -> Outcome<Vec<Found>> {
        Outcome::Some(self.found.clone())
    }
}

/// Every widget's parent, index among its siblings and their count, from
/// the pre-order and the depths.
pub(crate) fn family(found: &[Found]) -> Vec<(Option<usize>, usize, usize)> {
    let mut out: Vec<(Option<usize>, usize, usize)> = Vec::with_capacity(found.len());
    // The last widget seen at each depth.
    let mut last: Vec<Option<usize>> = Vec::new();
    let mut children: Vec<usize> = vec![0; found.len() + 1];
    for (i, f) in found.iter().enumerate() {
        let depth = f.depth as usize;
        let parent = depth
            .checked_sub(1)
            .and_then(|d| last.get(d).copied().flatten());
        let slot = parent.unwrap_or(found.len());
        out.push((parent, children[slot], 0));
        children[slot] += 1;
        if last.len() <= depth {
            last.resize(depth + 1, None);
        }
        last[depth] = Some(i);
        last.truncate(depth + 1);
    }
    for entry in &mut out {
        entry.2 = children[entry.0.unwrap_or(found.len())];
    }
    out
}
