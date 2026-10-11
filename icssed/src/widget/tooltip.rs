//! A tip shown by what the pointer rests on: after a delay, under it (or
//! over it, where there's no room under), centred on it; hidden when the
//! pointer leaves or presses. In a window it's an overlay of iced's; an
//! app showing it in a surface of its own (a bar's tips, which a 30px
//! layer surface would cut) is told the rectangle to show it by instead
//! (`on_tip`), and draws [`tip`] there. The tip is made anew each time it
//! shows, so the theme's entry animation runs each time.

use std::borrow::Cow;
use std::cell::Cell;
use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::overlay::{self, Group};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::{Event, Length, Point, Rectangle, Size, Vector, touch, window};

use super::flex::row;
use super::place;
use super::surface::{Surface, surface};
use super::text::text;
use crate::layout::snap;
use crate::style::name;
use crate::{Element, Renderer, Theme};

/// How long the pointer rests before a tip shows.
const DELAY: Duration = Duration::from_millis(600);

pub struct Tooltip<'a, Message> {
    content: Element<'a, Message>,
    tip: Element<'a, Message>,
    /// The scale of where it is, once reached.
    scale: Cell<f32>,
    delay: Duration,
    on_tip: Option<Box<dyn Fn(Option<Rectangle>) -> Message + 'a>>,
}

/// `content`, its tip `text` shown after the pointer rests on it: a
/// `tooltip` and its `label`, beside the children of the element the
/// tooltip is in.
pub fn tooltip<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    text: impl Into<Cow<'a, str>>,
) -> Tooltip<'a, Message> {
    Tooltip {
        content: content.into(),
        tip: tip_box(text),
        scale: Cell::new(1.0),
        delay: DELAY,
        on_tip: None,
    }
}

/// The tip's box and its `label`.
fn tip_box<'a, Message: Clone + 'a>(label: impl Into<Cow<'a, str>>) -> Element<'a, Message> {
    row([text(label).into()]).kind_of(name!("tooltip")).into()
}

/// A tip as a surface's content, for an app showing it in a surface of
/// its own: its box and its `label`, with the room its shadow needs; at
/// a root `tooltip` the app gives (`at`).
pub fn tip<'a, Message: Clone + 'a>(label: impl Into<Cow<'a, str>>) -> Surface<'a, Message> {
    surface(tip_box(label))
}

impl<'a, Message> Tooltip<'a, Message> {
    /// How long the pointer rests before the tip shows (600ms if not
    /// said).
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// The tip shown by the app instead: it's told the rectangle of what
    /// the tip is about (`Some`, in the surface's coordinates), then that
    /// it's gone (`None`).
    pub fn on_tip(mut self, on_tip: impl Fn(Option<Rectangle>) -> Message + 'a) -> Self {
        self.on_tip = Some(Box::new(on_tip));
        self
    }
}

#[derive(Default)]
struct State {
    phase: Phase,
    /// Where the scrolls around it move it, as the last overlay saw.
    translation: Vector,
}

#[derive(Clone, Copy, Default)]
enum Phase {
    #[default]
    Idle,
    /// The pointer resting on it since then.
    Resting(Instant),
    Shown,
    /// Pressed: no tip until the pointer leaves.
    Dismissed,
}

impl<Message> Widget<Message, Theme, Renderer> for Tooltip<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        // Its content is where the tooltip is; the tip floats over it.
        vec![
            Tree::new(&self.content),
            place::aside(|scale| {
                self.scale.set(scale);
                Tree::new(&self.tip)
            }),
        ]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.children.truncate(2);
        match tree.children.as_mut_slice() {
            [content, tip] => {
                content.diff(&self.content);
                place::aside(|scale| {
                    self.scale.set(scale);
                    tip.diff(&self.tip);
                });
            }
            _ => tree.children = self.children(),
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        let now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => *now,
            Event::Mouse(_) | Event::Touch(_) => Instant::now(),
            _ => return,
        };
        let over = cursor.is_over(layout.bounds());
        let pressed = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(_))
                | Event::Touch(touch::Event::FingerPressed { .. })
        );
        let state = tree.state.downcast_mut::<State>();
        let shown = matches!(state.phase, Phase::Shown);
        state.phase = match state.phase {
            _ if pressed && over => Phase::Dismissed,
            _ if !over => Phase::Idle,
            Phase::Idle => {
                shell.request_redraw_at(now + self.delay);
                Phase::Resting(now)
            }
            Phase::Resting(since) if now >= since + self.delay => Phase::Shown,
            Phase::Resting(since) => {
                shell.request_redraw_at(since + self.delay);
                Phase::Resting(since)
            }
            phase @ (Phase::Shown | Phase::Dismissed) => phase,
        };
        match (shown, matches!(state.phase, Phase::Shown)) {
            (false, true) => {
                // Anew, its entry animation from the start.
                tree.children[1] = Tree::new(&self.tip);
                match &self.on_tip {
                    Some(on_tip) => {
                        shell.publish(on_tip(Some(layout.bounds() + state.translation)));
                    }
                    None => {
                        shell.invalidate_layout();
                        shell.request_redraw();
                    }
                }
            }
            (true, false) => match &self.on_tip {
                Some(on_tip) => shell.publish(on_tip(None)),
                None => {
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
            },
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        state.translation = translation;
        let shown = matches!(state.phase, Phase::Shown) && self.on_tip.is_none();
        let [content_tree, tip_tree] = &mut tree.children[..] else {
            unreachable!("the content and the tip")
        };
        let content = self.content.as_widget_mut().overlay(
            content_tree,
            layout,
            renderer,
            viewport,
            translation,
        );
        let tip = shown.then(|| {
            overlay::Element::new(Box::new(Tip {
                tip: &mut self.tip,
                tree: tip_tree,
                about: layout.bounds() + translation,
                scale: self.scale.get(),
            }))
        });
        match (content, tip) {
            (Some(content), Some(tip)) => Some(Group::with_children(vec![content, tip]).overlay()),
            (content, tip) => content.or(tip),
        }
    }
}

/// The tip, over everything.
struct Tip<'a, 'b, Message> {
    tip: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    /// What it's about, where it's shown.
    about: Rectangle,
    scale: f32,
}

impl<Message> overlay::Overlay<Message, Theme, Renderer> for Tip<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let node =
            self.tip
                .as_widget_mut()
                .layout(self.tree, renderer, &Limits::new(Size::ZERO, bounds));
        let size = node.size();
        // Centred under it, or over it when there's no room under; in the
        // window either way.
        let x = (self.about.center_x() - size.width / 2.0)
            .min(bounds.width - size.width)
            .max(0.0);
        let under = self.about.y + self.about.height;
        let y = if under + size.height <= bounds.height {
            under
        } else {
            (self.about.y - size.height).max(0.0)
        };
        node.move_to(Point::new(snap(x, self.scale), snap(y, self.scale)))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.tip.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &Rectangle::with_size(Size::INFINITE),
        );
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.tip
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        // Its frames, for its animations.
        self.tip.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &Rectangle::with_size(Size::INFINITE),
        );
    }
}

impl<'a, Message: 'a> From<Tooltip<'a, Message>> for Element<'a, Message> {
    fn from(tooltip: Tooltip<'a, Message>) -> Self {
        Element::new(tooltip)
    }
}
