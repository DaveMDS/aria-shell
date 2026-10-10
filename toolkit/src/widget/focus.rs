//! Keyboard focus within a scope: Tab and Shift+Tab move it among the
//! buttons, sliders and inputs inside; a press of the pointer takes it
//! away before what it presses sees it (`:focus` shows for the
//! keyboard's sake, but a text field pressed takes it back). Per scope,
//! so per surface: an iced task would operate on every window an app
//! has.

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer;
use iced::advanced::widget::operation::{self, Operation, Outcome, focusable};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, overlay};
use iced::keyboard::{self, key};
use iced::{Event, Length, Rectangle, Size, Vector};

use crate::{Element, Renderer, Theme};

pub struct FocusScope<'a, Message> {
    content: Element<'a, Message>,
}

pub fn focus_scope<'a, Message>(
    content: impl Into<Element<'a, Message>>,
) -> FocusScope<'a, Message> {
    FocusScope {
        content: content.into(),
    }
}

impl<Message> FocusScope<'_, Message> {
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
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event {
            let mut count = focusable::count();
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                &mut operation::black_box(&mut count),
            );
            if let Outcome::Some(count) = count.finish()
                && count.focused.is_some()
            {
                self.run(tree, layout, renderer, focusable::unfocus());
                shell.request_redraw();
            }
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
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key::Named::Tab),
            modifiers,
            ..
        }) = event
            && !shell.is_event_captured()
        {
            if modifiers.shift() {
                self.run(tree, layout, renderer, focusable::focus_previous());
            } else {
                self.run(tree, layout, renderer, focusable::focus_next());
            }
            shell.capture_event();
            shell.request_redraw();
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
