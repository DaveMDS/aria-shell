//! What every styled box shares, whatever lays its children out (a row,
//! a meter's fill, a slider's knob): its node and its style; in the
//! widget tree, its states as far as some rule can see them, its
//! transitions and animations, the transform it was drawn with; how it's
//! drawn, and what its children get from it: the states their styles may
//! follow, its opacity, when it last changed.
//!
//! Its states live in the widget tree, which survives every rebuild: a
//! hover shows on the frame it happens. Only the states some rule can
//! see are tracked (`Node::sensitivity`), so a pointer moving over an
//! unthemed box asks no frame. A disabled box is never `:hover`, unlike
//! CSS: there's nothing to press.

use std::cell::Cell;
use std::sync::Arc;

use iced::advanced::Renderer as _;
use iced::advanced::renderer;
use iced::advanced::widget::operation::Focusable;
use iced::advanced::{Shell, mouse};
use iced::{Length, Rectangle, Size, Transformation};

use crate::anim::Animated;
use crate::layout::{length, snap};
use crate::paint::{draw_box, draw_outline, fade};
use crate::style::{Ancestors, Computed, Node, States};
use crate::{Renderer, Theme};

/// A styled box: its node, its style at rest.
pub(crate) struct Boxed<'a> {
    pub node: Node<'a>,
    pub style: Arc<Computed>,
}

/// What a box keeps in the widget tree.
#[derive(Debug)]
pub(crate) struct BoxState {
    pub states: States,
    /// Its transitions and animations, and when it last changed.
    pub animated: Animated,
    /// The `transform` it was last drawn with (an ancestor's state may
    /// set it, which only draw sees): the pointer is taken back through
    /// it, so that hit testing follows what's drawn.
    drawn: Cell<Option<Transformation>>,
}

impl BoxState {
    /// `cursor` where the box's layout is, before its transform.
    pub fn local(&self, cursor: mouse::Cursor) -> mouse::Cursor {
        match self.drawn.get() {
            Some(transformation) => cursor * transformation.inverse(),
            None => cursor,
        }
    }
}

impl Focusable for BoxState {
    fn is_focused(&self) -> bool {
        self.states.contains(States::FOCUS)
    }

    fn focus(&mut self) {
        if !self.is_focused() {
            self.states = self.states.with(States::FOCUS, true);
            self.animated.changed();
        }
    }

    fn unfocus(&mut self) {
        if self.is_focused() {
            self.states = self.states.with(States::FOCUS, false);
            self.animated.changed();
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

impl<'a> Boxed<'a> {
    pub fn new(node: Node<'a>) -> Self {
        Self {
            style: node.base(),
            node,
        }
    }

    pub fn state(&self) -> BoxState {
        BoxState {
            states: States::NONE,
            animated: Animated::new(self.node),
            drawn: Cell::new(None),
        }
    }

    /// The style its layout follows: its own states can change it,
    /// its ancestors' can't (they reach it only at draw).
    pub fn layout_style(&self, states: States) -> Arc<Computed> {
        if states.is_empty() {
            self.style.clone()
        } else {
            self.node.resolve(states, &Ancestors::default())
        }
    }

    /// Its size as iced sees it: its margin box.
    pub fn size(&self) -> Size<Length> {
        let m = self.style.margin;
        Size::new(
            length(self.style.width, m.horizontal()),
            length(self.style.height, m.vertical()),
        )
    }

    /// Its border box at rest, in its margin box `bounds`: where the
    /// pointer counts as over it.
    pub fn border_box(&self, bounds: Rectangle) -> Rectangle {
        border_box(bounds, &self.style, self.node.scale())
    }

    /// The states it follows: those some rule sees, and `own` (a
    /// button's hover and press, whatever the theme says).
    pub fn tracked(&self, own: States) -> States {
        self.node.sensitivity().any() | own
    }

    /// Draws it in its margin box `bounds`: its box, then `content` (its
    /// children, given the theme and the text style they're drawn with,
    /// and the pointer where they are), then its outline. Its transform
    /// moves all of it.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        state: &BoxState,
        renderer: &mut Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        content: impl FnOnce(&mut Renderer, &Theme, &renderer::Style, mouse::Cursor),
    ) {
        let resolved = self.node.resolve(state.states, &theme.ancestors);
        let moving = state.animated.style(&resolved, theme);
        let s = moving.as_ref().unwrap_or(&resolved);
        let bounds = border_box(bounds, s, self.node.scale());
        let transformation = s.transformation(bounds);
        state.drawn.set(transformation);
        let cursor = state.local(cursor);
        let paint = |renderer: &mut Renderer| {
            let opacity = theme.opacity * s.opacity;
            draw_box(renderer, s, bounds, opacity);
            // What its children get: its states, where a descendant's
            // style can see them; its opacity.
            let seen = state.states & self.node.sensitivity().descendants;
            let since = state.animated.since;
            let derived;
            let theme = if !seen.is_empty() || theme.changed < since || opacity != theme.opacity {
                derived = theme
                    .under(self.node.depth(), seen, since)
                    .with_opacity(opacity);
                &derived
            } else {
                theme
            };
            let style = renderer::Style {
                text_color: fade(s.color, opacity),
            };
            content(renderer, theme, &style, cursor);
            draw_outline(renderer, s, bounds, opacity);
        };
        match transformation {
            Some(transformation) => renderer.with_transformation(transformation, paint),
            None => paint(renderer),
        }
    }

    /// `update`'s part after the widget's own, its states `before` it:
    /// the hover followed where `tracked`, a frame asked for a change
    /// some rule sees, a new layout for one that changes its own sizes.
    pub fn settle<Message>(
        &self,
        state: &mut BoxState,
        before: States,
        tracked: States,
        over: bool,
        shell: &mut Shell<'_, Message>,
    ) {
        if tracked.contains(States::HOVER) {
            state.states = state
                .states
                .with(States::HOVER, over && !self.node.is_disabled());
        }
        let changed = before ^ state.states;
        if changed.is_empty() {
            return;
        }
        state.animated.changed();
        if tracked.intersects(changed) {
            shell.request_redraw();
            // A state its own style sees may change its layout too.
            if self.node.sensitivity().own.intersects(changed) {
                let after = self.layout_style(state.states);
                if !self.layout_style(before).same_layout(&after) {
                    shell.invalidate_layout();
                }
            }
        }
    }
}
