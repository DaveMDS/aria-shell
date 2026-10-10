//! Keyboard focus within a scope: Tab and Shift+Tab move it among the
//! buttons, sliders and inputs inside; a press of the pointer takes it
//! away before what it presses sees it (`:focus` shows for the
//! keyboard's sake, but a text field pressed takes it back). Per scope,
//! so per surface: an iced task would operate on every window an app
//! has. A scope of the arrows moves it with Up and Down (a menu's) or
//! Left and Right (tabs') instead, among its own, once one of them has
//! it, round from one end to the other (Tab leaves the scope at its
//! ends, as iced has it).

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer;
use iced::advanced::widget::Id;
use iced::advanced::widget::operation::focusable::{self, Count, Focusable};
use iced::advanced::widget::operation::{self, Operation, Outcome};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, overlay};
use iced::keyboard::{self, key};
use iced::{Event, Length, Rectangle, Size, Vector};

use crate::{Element, Renderer, Theme};

pub struct FocusScope<'a, Message> {
    content: Element<'a, Message>,
    /// The keys that move the focus: Tab, or a pair of arrows (and then
    /// the pointer leaves it alone).
    keys: Keys,
}

/// What moves the focus in a scope: the keys back, the keys on.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Keys {
    Tab,
    UpDown,
    LeftRight,
}

pub fn focus_scope<'a, Message>(
    content: impl Into<Element<'a, Message>>,
) -> FocusScope<'a, Message> {
    FocusScope {
        content: content.into(),
        keys: Keys::Tab,
    }
}

/// A scope of the arrows: `keys` move the focus among what's inside,
/// once one of them has it.
pub(crate) fn arrow_scope<'a, Message>(
    keys: Keys,
    content: impl Into<Element<'a, Message>>,
) -> FocusScope<'a, Message> {
    FocusScope {
        content: content.into(),
        keys,
    }
}

impl<Message> FocusScope<'_, Message> {
    /// Whether something inside has the focus.
    fn focused(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer) -> bool {
        let mut count = focusable::count();
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout,
            renderer,
            &mut operation::black_box(&mut count),
        );
        matches!(count.finish(), Outcome::Some(count) if count.focused.is_some())
    }

    /// Runs `operation` on the content, its chained ones too.
    fn run(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: impl Operation<()> + 'static,
    ) {
        let mut operation: Box<dyn Operation<()>> = Box::new(operation);
        loop {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation.as_mut(),
            );
            match operation.finish() {
                Outcome::Chain(next) => operation = next,
                Outcome::None | Outcome::Some(()) => break,
            }
        }
    }
}

impl<Message> Widget<Message, Theme, Renderer> for FocusScope<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
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
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event
            && self.keys == Keys::Tab
            && self.focused(tree, layout, renderer)
        {
            self.run(tree, layout, renderer, focusable::unfocus());
            shell.request_redraw();
        }
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
        let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(named),
            modifiers,
            ..
        }) = event
        else {
            return;
        };
        let back = match (self.keys, named) {
            (Keys::Tab, key::Named::Tab) => modifiers.shift(),
            (Keys::UpDown, key::Named::ArrowUp) | (Keys::LeftRight, key::Named::ArrowLeft) => true,
            (Keys::UpDown, key::Named::ArrowDown) | (Keys::LeftRight, key::Named::ArrowRight) => {
                false
            }
            _ => return,
        };
        let arrows = self.keys != Keys::Tab;
        if shell.is_event_captured() || (arrows && !self.focused(tree, layout, renderer)) {
            return;
        }
        match (arrows, back) {
            (false, true) => self.run(tree, layout, renderer, focusable::focus_previous()),
            (false, false) => self.run(tree, layout, renderer, focusable::focus_next()),
            (true, true) => self.run(
                tree,
                layout,
                renderer,
                operation::then(focusable::count(), Round::back),
            ),
            (true, false) => self.run(
                tree,
                layout,
                renderer,
                operation::then(focusable::count(), Round::on),
            ),
        }
        shell.capture_event();
        shell.request_redraw();
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

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
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
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
}

impl<'a, Message: 'a> From<FocusScope<'a, Message>> for Element<'a, Message> {
    fn from(scope: FocusScope<'a, Message>) -> Self {
        Element::new(scope)
    }
}

/// The focus a step on or back, round from one end to the other.
struct Round {
    focused: Option<usize>,
    target: Option<usize>,
    current: usize,
}

impl Round {
    fn on(count: Count) -> Self {
        let target = match count.focused {
            Some(i) => Some((i + 1) % count.total),
            None => (count.total > 0).then_some(0),
        };
        Self {
            focused: count.focused,
            target,
            current: 0,
        }
    }

    fn back(count: Count) -> Self {
        let target = match count.focused {
            Some(i) => Some((i + count.total - 1) % count.total),
            None => count.total.checked_sub(1),
        };
        Self {
            focused: count.focused,
            target,
            current: 0,
        }
    }
}

impl Operation<()> for Round {
    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if Some(self.current) == self.target {
            state.focus();
        } else if Some(self.current) == self.focused {
            state.unfocus();
        }
        self.current += 1;
    }

    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
        operate(self);
    }
}
