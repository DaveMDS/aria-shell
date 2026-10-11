//! A slider: a value in a range, dragged, clicked, scrolled, or moved
//! with the keys while it has the focus (the arrows a step, Page Up and
//! Down ten, Home and End to the ends). Its track is a meter across its
//! content box, filled up to the value; its knob is centred on the
//! value, from the track's start to its end, so the slider's padding is
//! the room the knob overhangs into. `:active` while dragged.

use std::ops::RangeInclusive;

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer;
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse, overlay};
use iced::keyboard::{self, key};
use iced::{Event, Length, Point, Rectangle, Size, Vector, touch};

use super::boxed::{BoxState, Boxed};
use super::flex::row;
use super::meter::meter;
use super::place::{self, placed};
use crate::layout::{Sizing, snap};
use crate::query::Reported;
use crate::style::{States, name};
use crate::{Element, Renderer, Theme};

pub struct Slider<'a, Message> {
    boxed: Boxed<'a>,
    start: f32,
    end: f32,
    value: f32,
    step: Option<f32>,
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
    on_release: Option<Message>,
    /// The track (a meter), the knob.
    children: [Element<'a, Message>; 2],
}

/// `value` in `range`: a `slider`, its `track > fill`, its `knob`. A
/// move sends `on_change` with the new value.
pub fn slider<'a, Message: Clone + 'a>(
    range: RangeInclusive<f32>,
    value: f32,
    on_change: impl Fn(f32) -> Message + 'a,
) -> Slider<'a, Message> {
    let (start, end) = range.into_inner();
    let value = if start <= end {
        value.clamp(start, end)
    } else {
        start
    };
    let fraction = if end > start {
        (value - start) / (end - start)
    } else {
        0.0
    };
    Slider {
        boxed: Boxed::of(name!("slider")),
        start,
        end,
        value,
        step: None,
        on_change: Box::new(on_change),
        on_release: None,
        children: [
            meter(fraction).kind_of(name!("track")).into(),
            row([]).kind_of(name!("knob")).into(),
        ],
    }
}

impl<'a, Message> Slider<'a, Message> {
    placed!(boxed.place);

    /// The values it takes: the range's start and its multiples of
    /// `step` on (a hundredth of the range if not given).
    pub fn step(mut self, step: f32) -> Self {
        self.step = (step > 0.0).then_some(step);
        self
    }

    /// What it sends when the pointer lets it go: the value settled.
    pub fn on_release(mut self, message: Message) -> Self {
        self.on_release = Some(message);
        self
    }

    fn enabled(&self) -> bool {
        !self.boxed.node().is_disabled() && self.end > self.start
    }

    fn step_size(&self) -> f32 {
        self.step.unwrap_or((self.end - self.start) / 100.0)
    }

    /// `value` on a step, in the range.
    fn stepped(&self, value: f32) -> f32 {
        let step = self.step_size();
        let steps = ((value - self.start) / step).round();
        (self.start + steps * step).clamp(self.start, self.end)
    }

    /// The value under `x`, by the track's `layout`.
    fn value_at(&self, layout: Layout<'_>, x: f32) -> f32 {
        let track = layout.children().next().expect("a track").bounds();
        let fraction = if track.width > 0.0 {
            ((x - track.x) / track.width).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.stepped(self.start + fraction * (self.end - self.start))
    }

    /// `steps` steps on from the value.
    fn moved(&self, steps: f32) -> f32 {
        self.stepped(self.value + steps * self.step_size())
    }
}

#[derive(Debug)]
struct State {
    boxed: BoxState,
    /// What the wheel scrolled, in steps, not yet a whole one (a
    /// touchpad's pixels, a fine wheel's fractions of a line).
    scrolled: f32,
}

/// The pixels a touchpad scrolls for a step; a wheel's line is one.
const SCROLL_STEP: f32 = 10.0;

impl<Message: Clone> Widget<Message, Theme, Renderer> for Slider<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.boxed.size()
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            boxed: self.boxed.state(),
            scrolled: 0.0,
        })
    }

    fn children(&self) -> Vec<Tree> {
        place::trees(self.boxed.node(), &self.children)
    }

    fn diff(&self, tree: &mut Tree) {
        tree.state
            .downcast_mut::<State>()
            .boxed
            .animated
            .diff(self.boxed.node());
        place::diff(self.boxed.node(), tree, &self.children);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let s = self
            .boxed
            .layout_style(tree.state.downcast_ref::<State>().boxed.states);
        let scale = self.boxed.node().scale();
        let sizing = Sizing::new(&s, scale, limits);
        let room = sizing.room();
        let [track, knob] = &mut self.children;
        let [track_tree, knob_tree] = &mut tree.children[..] else {
            unreachable!("a track and a knob")
        };
        let own = Limits::with_compression(Size::ZERO, room, Size::new(true, true));
        let across = |width: f32| Limits::new(Size::new(width, 0.0), Size::new(width, room.height));
        let mut knob_node = knob.as_widget_mut().layout(knob_tree, renderer, &own);
        // The track across the content box: its width when the slider
        // has one of its own, else the track's.
        let at = if sizing.definite().0 {
            across(room.width)
        } else {
            own
        };
        let mut track_node = track.as_widget_mut().layout(track_tree, renderer, &at);
        let (track_size, knob_size) = (track_node.size(), knob_node.size());
        let (size, content) = sizing.around(Size::new(
            track_size.width.max(knob_size.width),
            track_size.height.max(knob_size.height),
        ));
        if (track_size.width - content.width).abs() > 0.01 {
            track_node = track
                .as_widget_mut()
                .layout(track_tree, renderer, &across(content.width));
        }
        // Both centred across, the knob on the value.
        let track_size = track_node.size();
        let centred = |height: f32| content.y + snap((content.height - height) / 2.0, scale);
        track_node.move_to_mut(Point::new(content.x, centred(track_size.height)));
        let fraction = (self.value - self.start) / (self.end - self.start).max(f32::EPSILON);
        let x = content.x + fraction * track_size.width - knob_size.width / 2.0;
        knob_node.move_to_mut(Point::new(snap(x, scale), centred(knob_size.height)));
        layout::Node::with_children(size, vec![track_node, knob_node])
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
        self.boxed.draw(
            &state.boxed,
            renderer,
            theme,
            layout.bounds(),
            *viewport,
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
        let cursor = tree.state.downcast_ref::<State>().boxed.local(cursor);
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

        let enabled = self.enabled();
        let tracked = self.boxed.tracked(if enabled {
            States::HOVER | States::ACTIVE
        } else {
            States::NONE
        });
        let state = tree.state.downcast_mut::<State>();
        state.boxed.animated.update(event, shell);
        let before = state.boxed.states;
        let over = cursor.is_over(self.boxed.border_box(layout.bounds()));
        let dragged = state.boxed.states.contains(States::ACTIVE);
        let free = !shell.is_event_captured();

        let value = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. })
                if enabled && over && free =>
            {
                state.boxed.states = state.boxed.states.with(States::ACTIVE, true);
                shell.capture_event();
                cursor.position().map(|p| self.value_at(layout, p.x))
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. })
                if dragged =>
            {
                shell.capture_event();
                cursor.land().position().map(|p| self.value_at(layout, p.x))
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. })
                if dragged =>
            {
                state.boxed.states = state.boxed.states.with(States::ACTIVE, false);
                if let Some(message) = &self.on_release {
                    shell.publish(message.clone());
                }
                shell.capture_event();
                None
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if enabled && over && free => {
                shell.capture_event();
                state.scrolled += match *delta {
                    mouse::ScrollDelta::Lines { y, .. } => y,
                    mouse::ScrollDelta::Pixels { y, .. } => y / SCROLL_STEP,
                };
                let steps = state.scrolled.trunc();
                state.scrolled -= steps;
                (steps != 0.0).then(|| self.moved(steps))
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(named),
                ..
            }) if enabled && state.boxed.is_focused() && free => {
                let value = match named {
                    key::Named::ArrowLeft | key::Named::ArrowDown => Some(self.moved(-1.0)),
                    key::Named::ArrowRight | key::Named::ArrowUp => Some(self.moved(1.0)),
                    key::Named::PageDown => Some(self.moved(-10.0)),
                    key::Named::PageUp => Some(self.moved(10.0)),
                    key::Named::Home => Some(self.start),
                    key::Named::End => Some(self.end),
                    _ => None,
                };
                if value.is_some() {
                    shell.capture_event();
                }
                value
            }
            _ => None,
        };
        if let Some(value) = value
            && value != self.value
        {
            self.value = value;
            shell.publish((self.on_change)(value));
        }
        self.boxed
            .settle(&mut state.boxed, before, tracked, over, shell);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        let cursor = state.boxed.local(cursor);
        if !self.enabled() {
            mouse::Interaction::None
        } else if state.boxed.states.contains(States::ACTIVE) {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(self.boxed.border_box(layout.bounds())) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::None
        }
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
        operation.custom(None, bounds, &mut Reported::new(self.boxed.node()));
        if self.enabled() {
            operation.focusable(None, bounds, &mut tree.state.downcast_mut::<State>().boxed);
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

impl<'a, Message: Clone + 'a> From<Slider<'a, Message>> for Element<'a, Message> {
    fn from(slider: Slider<'a, Message>) -> Self {
        Element::new(slider)
    }
}
