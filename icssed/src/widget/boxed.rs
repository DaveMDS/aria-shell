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
use iced::{Event, Length, Rectangle, Size, Transformation, Vector, touch};

use super::place::Place;
use crate::anim::Animated;
use crate::layout::{HIDDEN, content_box, length, snap};
use crate::paint::{draw_box, draw_debug, draw_outline, fade};
use crate::style::{Ancestors, Computed, Node, States};
use crate::{Renderer, Theme};

/// A styled box: its place, and through it its node and its style at
/// rest.
pub(crate) struct Boxed<'a> {
    pub place: Place<'a>,
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
    /// Drawn without `Boxed::draw`, so with no transform: the pointer
    /// is where the layout is.
    pub fn drawn_untransformed(&self) {
        self.drawn.set(None);
    }

    /// `cursor` where the box's layout is, before its transform.
    pub fn local(&self, cursor: mouse::Cursor) -> mouse::Cursor {
        match self.drawn.get() {
            Some(transformation) => cursor * transformation.inverse(),
            None => cursor,
        }
    }
}

impl BoxState {
    fn set_focus(&mut self, states: States) {
        let after = self
            .states
            .with(States::FOCUS | States::FOCUS_VISIBLE, false)
            .with(states, true);
        if after != self.states {
            self.states = after;
            self.animated.changed();
        }
    }

    /// A press of the pointer, `here` on the box: it takes the focus
    /// without showing it (`:focus`, not `:focus-visible`), as a click
    /// does in a page; anywhere else it loses it.
    pub fn pressed(&mut self, here: bool) {
        self.set_focus(if here { States::FOCUS } else { States::NONE });
    }
}

/// The keyboard's focus and the app's (iced's operations): shown.
impl Focusable for BoxState {
    fn is_focused(&self) -> bool {
        self.states.contains(States::FOCUS)
    }

    fn focus(&mut self) {
        self.set_focus(States::FOCUS | States::FOCUS_VISIBLE);
    }

    fn unfocus(&mut self) {
        self.set_focus(States::NONE);
    }
}

/// Whether `event` is a press of the pointer, and then whether of its
/// primary button (the one that focuses).
pub(crate) fn press(event: &Event) -> Option<bool> {
    match event {
        Event::Mouse(mouse::Event::ButtonPressed(button)) => Some(*button == mouse::Button::Left),
        Event::Touch(touch::Event::FingerPressed { .. }) => Some(true),
        _ => None,
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

/// The room its padding and border take around its content, both
/// axes.
pub(crate) fn frame(s: &Computed, scale: f32) -> Size {
    let (b, p) = (s.border_width, s.padding);
    let sn = |v: f32| snap(v, scale);
    Size::new(
        b.left + b.right + sn(p.left) + sn(p.right),
        b.top + b.bottom + sn(p.top) + sn(p.bottom),
    )
}

/// Its margin, border, padding and content boxes, from its margin box
/// `bounds`: what `debug: box` paints.
fn debug_boxes(bounds: Rectangle, s: &Computed, scale: f32) -> [Rectangle; 4] {
    let border = border_box(bounds, s, scale);
    let b = s.border_width;
    let padding = Rectangle {
        x: border.x + b.left,
        y: border.y + b.top,
        width: (border.width - b.left - b.right).max(0.0),
        height: (border.height - b.top - b.bottom).max(0.0),
    };
    let content = content_box(bounds.size(), s, scale) + Vector::new(bounds.x, bounds.y);
    [bounds, border, padding, content]
}

/// The corners of its content box, inside its border box `bounds`: its
/// own radii less the border and the padding beside them, as CSS
/// rounds a content box (a picture drawn there).
pub(crate) fn inner_radius(s: &Computed, bounds: Rectangle, scale: f32) -> iced::border::Radius {
    let [tl, tr, br, bl] = s.radii(bounds.size());
    let (b, p) = (s.border_width, s.padding);
    let sn = |v: f32| snap(v, scale);
    let (left, right) = (b.left + sn(p.left), b.right + sn(p.right));
    let (top, bottom) = (b.top + sn(p.top), b.bottom + sn(p.bottom));
    let less = |r: f32, x: f32, y: f32| (r - x.max(y)).max(0.0);
    iced::border::Radius {
        top_left: less(tl, left, top),
        top_right: less(tr, right, top),
        bottom_right: less(br, right, bottom),
        bottom_left: less(bl, left, bottom),
    }
}

/// Whether a box styled `s` paints nothing of its own (no background,
/// border, shadow, outline, picture or transform): a leaf (an icon, an
/// image) then draws its content alone, as cheaply as before it was a
/// box.
pub(crate) fn bare(s: &Computed) -> bool {
    let w = s.border_width;
    s.background.a == 0.0
        && w.top + w.right + w.bottom + w.left == 0.0
        && s.shadow.is_none()
        && s.inset_shadow.is_none()
        && s.outline_width == 0.0
        && s.backgrounds.is_empty()
        && s.border_image.is_none()
        && s.transform.is_none()
        && !s.debug_box
}

impl<'a> Boxed<'a> {
    /// A `kind` box, wherever it's put.
    pub fn of(kind: crate::style::Name) -> Self {
        Self::placed(Place::new(kind))
    }

    pub fn placed(place: Place<'a>) -> Self {
        Self { place }
    }

    #[inline]
    pub fn node(&self) -> Node<'a> {
        self.place.node()
    }

    /// Its style at rest.
    #[inline]
    pub fn style(&self) -> &Arc<Computed> {
        self.place.style()
    }

    pub fn state(&self) -> BoxState {
        BoxState {
            states: States::NONE,
            animated: Animated::new(self.node()),
            drawn: Cell::new(None),
        }
    }

    /// The style its layout follows: its own states can change it,
    /// its ancestors' can't (they reach it only at draw).
    pub fn layout_style(&self, states: States) -> Arc<Computed> {
        if states.is_empty() {
            self.style().clone()
        } else {
            self.node().resolve(states, &Ancestors::default())
        }
    }

    /// Its size as iced sees it: its margin box, or `HIDDEN`.
    pub fn size(&self) -> Size<Length> {
        if self.place.hidden() {
            return HIDDEN;
        }
        let m = self.style().margin;
        Size::new(
            length(self.style().width, m.horizontal()),
            length(self.style().height, m.vertical()),
        )
    }

    /// Its border box at rest, in its margin box `bounds`: where the
    /// pointer counts as over it.
    pub fn border_box(&self, bounds: Rectangle) -> Rectangle {
        border_box(bounds, self.style(), self.node().scale())
    }

    /// The states it follows: those some rule sees, and `own` (a
    /// button's hover and press, whatever the theme says).
    pub fn tracked(&self, own: States) -> States {
        self.node().sensitivity().any() | own
    }

    /// Draws it in its margin box `bounds`, shown within `viewport`: its
    /// box, then `content` (its children, given the theme and the text
    /// style they're drawn with, the pointer where they are, and the
    /// box's style as drawn: its states, its transitions), then its
    /// outline, and its boxes if `debug: box`. Its transform moves all
    /// of it.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        state: &BoxState,
        renderer: &mut Renderer,
        theme: &Theme,
        bounds: Rectangle,
        viewport: Rectangle,
        cursor: mouse::Cursor,
        content: impl FnOnce(&mut Renderer, &Theme, &renderer::Style, mouse::Cursor, &Computed),
    ) {
        let node = self.node();
        let resolved = node.resolve(state.states, &theme.ancestors);
        let moving = state.animated.style(&resolved, theme);
        let s = moving.as_ref().unwrap_or(&resolved);
        let margin_box = bounds;
        let bounds = border_box(bounds, s, node.scale());
        let transformation = s.transformation(bounds);
        state.drawn.set(transformation);
        let cursor = state.local(cursor);
        let paint = |renderer: &mut Renderer| {
            let opacity = theme.opacity * s.opacity;
            let before = theme.debugged.replace(false);
            draw_box(renderer, s, bounds, viewport, opacity, |renderer| {
                // What its children get: its states, where a descendant's
                // style can see them or inherits what they change (a
                // `color`); its opacity.
                let seen = state.states & node.sensitivity().any();
                let since = state.animated.since;
                let derived;
                let inner = if !seen.is_empty() || theme.changed < since || opacity != theme.opacity
                {
                    derived = theme.under(node.depth(), seen, since).with_opacity(opacity);
                    &derived
                } else {
                    theme
                };
                let style = renderer::Style {
                    text_color: fade(s.color, opacity),
                };
                content(renderer, inner, &style, cursor, s);
                draw_outline(renderer, s, bounds, opacity);
                // The innermost shows its boxes, as an inspector does
                // (`* :hover` matches every ancestor too).
                let below = inner.debugged.get();
                if s.debug_box && !below {
                    draw_debug(renderer, debug_boxes(margin_box, s, node.scale()));
                }
                theme.debugged.set(before || below || s.debug_box);
            });
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
                .with(States::HOVER, over && !self.node().is_disabled());
        }
        let changed = before ^ state.states;
        if changed.is_empty() {
            return;
        }
        state.animated.changed();
        if tracked.intersects(changed) {
            shell.request_redraw();
            // A state its own style sees may change its layout too.
            if self.node().sensitivity().own.intersects(changed) {
                let after = self.layout_style(state.states);
                if !self.layout_style(before).same_layout(&after) {
                    shell.invalidate_layout();
                }
            }
        }
    }
}
