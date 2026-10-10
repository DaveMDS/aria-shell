//! A meter: a bar filled to a fraction (a battery's charge, a disk's
//! use); with a text over it, a gauge. Its fill is a box of its own,
//! as wide as the fraction of the meter's content box and as tall,
//! under the label; the label is laid out as in a row, so the meter's
//! `justify-content` and `align-items` place it.

use std::borrow::Cow;

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse, overlay};
use iced::{Event, Length, Rectangle, Size, Vector};

use super::boxed::{BoxState, Boxed};
use super::flex::row;
use super::text::text;
use crate::layout::{content_box, flex, snap};
use crate::query::Reported;
use crate::style::{Direction, Node, States};
use crate::{Element, Renderer, Theme};

pub struct Meter<'a, Message> {
    boxed: Boxed<'a>,
    fraction: f32,
    /// The fill, then the label if there's one.
    children: Vec<Element<'a, Message>>,
}

/// A bar filled to `fraction`, from 0 to 1: `node`, and its `fill`.
pub fn meter<'a, Message: Clone + 'a>(node: Node<'a>, fraction: f32) -> Meter<'a, Message> {
    Meter {
        boxed: Boxed::new(node),
        fraction: if fraction.is_nan() {
            0.0
        } else {
            fraction.clamp(0.0, 1.0)
        },
        children: vec![row(node.child("fill"), []).into()],
    }
}

impl<'a, Message: Clone + 'a> Meter<'a, Message> {
    /// A text over it, its `label`: a gauge.
    pub fn label(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.children.truncate(1);
        self.children
            .push(text(self.boxed.node.child("label"), label).into());
        self
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for Meter<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.boxed.size()
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<BoxState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(self.boxed.state())
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.state
            .downcast_mut::<BoxState>()
            .animated
            .diff(self.boxed.node);
        tree.diff_children(&self.children);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let s = self
            .boxed
            .layout_style(tree.state.downcast_ref::<BoxState>().states);
        let scale = self.boxed.node.scale();
        // The meter around its label, as a row.
        let (fill, label) = self.children.split_at_mut(1);
        let (fill_tree, label_tree) = tree.children.split_at_mut(1);
        let around = flex(
            &s,
            Direction::Row,
            scale,
            label,
            label_tree,
            renderer,
            limits,
        );
        // The fill under it, from the content box's start.
        let content = content_box(around.size(), &s, scale);
        let width = snap(self.fraction * content.width, scale);
        let exact = Size::new(width, content.height);
        let fill = fill[0]
            .as_widget_mut()
            .layout(&mut fill_tree[0], renderer, &Limits::new(exact, exact))
            .move_to(content.position());
        let mut children = Vec::with_capacity(self.children.len());
        children.push(fill);
        children.extend(around.children().iter().cloned());
        layout::Node::with_children(around.size(), children)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<BoxState>();
        self.boxed.draw(
            state,
            renderer,
            theme,
            layout.bounds(),
            cursor,
            |renderer, theme, style, cursor| {
                for ((child, tree), layout) in self
                    .children
                    .iter()
                    .zip(&tree.children)
                    .zip(layout.children())
                {
                    child
                        .as_widget()
                        .draw(tree, renderer, theme, style, layout, cursor, viewport);
                }
            },
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let cursor = tree.state.downcast_ref::<BoxState>().local(cursor);
        for ((child, tree), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
        let state = tree.state.downcast_mut::<BoxState>();
        state.animated.update(event, shell);
        let before = state.states;
        let over = cursor.is_over(self.boxed.border_box(layout.bounds()));
        let tracked = self.boxed.tracked(States::NONE);
        self.boxed.settle(state, before, tracked, over, shell);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let bounds = self.boxed.border_box(layout.bounds());
        operation.container(None, bounds);
        operation.custom(None, bounds, &mut Reported::new(self.boxed.node));
        operation.traverse(&mut |operation| {
            for ((child, tree), layout) in self
                .children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                child
                    .as_widget_mut()
                    .operate(tree, layout, renderer, operation);
            }
        });
    }
}

impl<'a, Message: Clone + 'a> From<Meter<'a, Message>> for Element<'a, Message> {
    fn from(meter: Meter<'a, Message>) -> Self {
        Element::new(meter)
    }
}
