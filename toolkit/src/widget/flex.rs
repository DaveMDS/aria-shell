//! The box: a row or a column of children, sized by the box model and
//! the flex subset (`layout.rs`), painted by `paint.rs`. With an
//! `on_press` it's a button: `:active` while pressed, its message on a
//! release over it, or on Enter or Space while it has the focus.
//!
//! Its states live in the widget tree, which survives every rebuild: a
//! hover shows on the frame it happens. Only the states some rule can
//! see are tracked (`Node::sensitivity`), so a pointer moving over an
//! unthemed box asks no frame. A disabled box is never `:hover` or
//! `:active`, unlike CSS: there's nothing to press.

use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use iced::advanced::Renderer as _;
use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer;
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::keyboard::{self, key};
use iced::{Event, Length, Rectangle, Size, Transformation, touch};

use crate::layout::{flex, length, snap};
use crate::paint::{draw_box, draw_outline, fade};
use crate::style::{Ancestors, Computed, Direction, Node, States};
use crate::{Element, Renderer, Theme};

pub struct Flex<'a, Message> {
    node: Node<'a>,
    style: Arc<Computed>,
    /// The constructor's; the theme's `flex-direction` wins.
    direction: Direction,
    children: Vec<Element<'a, Message>>,
    on_press: Option<Message>,
}

/// A box laying `children` out in a row.
pub fn row<'a, Message>(
    node: Node<'a>,
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> Flex<'a, Message> {
    Flex::new(node, Direction::Row, children)
}

/// A box laying `children` out in a column.
pub fn column<'a, Message>(
    node: Node<'a>,
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> Flex<'a, Message> {
    Flex::new(node, Direction::Column, children)
}

/// A row to press: give it an [`Flex::on_press`]. Its variants are
/// classes of its node (`button.primary`), its look the theme's.
pub fn button<'a, Message>(
    node: Node<'a>,
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> Flex<'a, Message> {
    Flex::new(node, Direction::Row, children)
}

impl<'a, Message> Flex<'a, Message> {
    fn new(
        node: Node<'a>,
        direction: Direction,
        children: impl IntoIterator<Item = Element<'a, Message>>,
    ) -> Self {
        Self {
            style: node.base(),
            node,
            direction,
            children: children.into_iter().collect(),
            on_press: None,
        }
    }

    /// What a press sends: the box is a button, unless its node is
    /// `:disabled`.
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    fn pressable(&self) -> bool {
        self.on_press.is_some() && !self.node.is_disabled()
    }

    /// The style its layout follows: its own states can change it,
    /// its ancestors' can't (they reach it only at draw).
    fn layout_style(&self, states: States) -> Arc<Computed> {
        if states.is_empty() {
            self.style.clone()
        } else {
            self.node.resolve(states, &Ancestors::default())
        }
    }
}

impl<Message: Clone> Flex<'_, Message> {
    /// Its box, its children, its outline.
    #[allow(clippy::too_many_arguments)]
    fn paint(
        &self,
        tree: &Tree,
        s: &Computed,
        bounds: Rectangle,
        renderer: &mut Renderer,
        theme: &Theme,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let opacity = theme.opacity * s.opacity;
        draw_box(renderer, s, bounds, opacity);

        // What its children get: its states, where a descendant's style
        // can see them; its opacity.
        let seen = state.states & self.node.sensitivity().descendants;
        let derived;
        let theme = if !seen.is_empty() || theme.changed < state.since || opacity != theme.opacity {
            derived = theme
                .under(self.node.depth(), seen, state.since)
                .with_opacity(opacity);
            &derived
        } else {
            theme
        };
        let style = renderer::Style {
            text_color: fade(s.color, opacity),
        };
        for ((child, tree), layout) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            child
                .as_widget()
                .draw(tree, renderer, theme, &style, layout, cursor, viewport);
        }
        draw_outline(renderer, s, bounds, opacity);
    }

    #[allow(clippy::too_many_arguments)]
    fn handle(
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

        let pressable = self.pressable();
        let sensitivity = self.node.sensitivity();
        let tracked = sensitivity.any()
            | if pressable {
                States::HOVER | States::ACTIVE
            } else {
                States::NONE
            };
        let state = tree.state.downcast_mut::<State>();
        let before = state.states;
        let bounds = border_box(layout.bounds(), &self.style, self.node.scale());
        let over = cursor.is_over(bounds);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. })
                if pressable && over && !shell.is_event_captured() =>
            {
                state.states = state.states.with(States::ACTIVE, true);
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. })
                if state.states.contains(States::ACTIVE) =>
            {
                state.states = state.states.with(States::ACTIVE, false);
                if over && let Some(message) = &self.on_press {
                    shell.publish(message.clone());
                }
                shell.capture_event();
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key::Named::Enter | key::Named::Space),
                ..
            }) if pressable && state.is_focused() && !shell.is_event_captured() => {
                if let Some(message) = &self.on_press {
                    shell.publish(message.clone());
                }
                shell.capture_event();
            }
            _ => {}
        }
        if tracked.contains(States::HOVER) {
            state.states = state
                .states
                .with(States::HOVER, over && !self.node.is_disabled());
        }

        let changed = before != state.states;
        if changed {
            state.since = Some(Instant::now());
        }
        if changed && tracked.intersects(before ^ state.states) {
            shell.request_redraw();
            // A state its own style sees may change its layout too.
            if sensitivity.own.intersects(before ^ state.states) {
                let after = self.layout_style(state.states);
                if !self.layout_style(before).same_layout(&after) {
                    shell.invalidate_layout();
                }
            }
        }
    }
}

#[derive(Debug, Default)]
struct State {
    states: States,
    /// When its states last changed.
    since: Option<Instant>,
    /// The `transform` it was last drawn with (an ancestor's state may
    /// set it, which only draw sees): the pointer is taken back through
    /// it, so that hit testing follows what's drawn.
    drawn: Cell<Option<Transformation>>,
}

impl State {
    /// `cursor` where the box's layout is, before its transform.
    fn local(&self, cursor: mouse::Cursor) -> mouse::Cursor {
        match self.drawn.get() {
            Some(transformation) => cursor * transformation.inverse(),
            None => cursor,
        }
    }
}

impl Focusable for State {
    fn is_focused(&self) -> bool {
        self.states.contains(States::FOCUS)
    }

    fn focus(&mut self) {
        self.states = self.states.with(States::FOCUS, true);
        self.since = Some(Instant::now());
    }

    fn unfocus(&mut self) {
        if self.is_focused() {
            self.states = self.states.with(States::FOCUS, false);
            self.since = Some(Instant::now());
        }
    }
}

/// The border box of a margin box.
pub(crate) fn border_box(bounds: Rectangle, s: &Computed, scale: f32) -> Rectangle {
    let m = s.margin;
    let (left, top) = (snap(m.left, scale), snap(m.top, scale));
    Rectangle {
        x: bounds.x + left,
        y: bounds.y + top,
        width: (bounds.width - left - snap(m.right, scale)).max(0.0),
        height: (bounds.height - top - snap(m.bottom, scale)).max(0.0),
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for Flex<'_, Message> {
    fn size(&self) -> Size<Length> {
        let m = self.style.margin;
        Size::new(
            length(self.style.width, m.horizontal()),
            length(self.style.height, m.vertical()),
        )
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let style = self.layout_style(tree.state.downcast_ref::<State>().states);
        let direction = style.flex_direction.unwrap_or(self.direction);
        flex(
            &style,
            direction,
            self.node.scale(),
            &mut self.children,
            &mut tree.children,
            renderer,
            limits,
        )
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
        let state = tree.state.downcast_ref::<State>();
        let s = self.node.resolve(state.states, &theme.ancestors);
        let bounds = border_box(layout.bounds(), &s, self.node.scale());
        let transformation = s.transformation(bounds);
        state.drawn.set(transformation);
        let cursor = state.local(cursor);
        match transformation {
            Some(transformation) => renderer.with_transformation(transformation, |renderer| {
                self.paint(tree, &s, bounds, renderer, theme, layout, cursor, viewport);
            }),
            None => self.paint(tree, &s, bounds, renderer, theme, layout, cursor, viewport),
        }
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
        let cursor = tree.state.downcast_ref::<State>().local(cursor);
        self.handle(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let cursor = tree.state.downcast_ref::<State>().local(cursor);
        let inner = self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, tree), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default();
        let bounds = border_box(layout.bounds(), &self.style, self.node.scale());
        if inner == mouse::Interaction::None && self.pressable() && cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            inner
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let bounds = border_box(layout.bounds(), &self.style, self.node.scale());
        operation.container(None, bounds);
        if self.pressable() {
            operation.focusable(None, bounds, tree.state.downcast_mut::<State>());
        }
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

impl<'a, Message: Clone + 'a> From<Flex<'a, Message>> for Element<'a, Message> {
    fn from(flex: Flex<'a, Message>) -> Self {
        Element::new(flex)
    }
}
