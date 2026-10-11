//! A slot: what a view is, said by whoever puts it somewhere. Its
//! content's top widget is the slot's element (its type, its classes),
//! with the content's own classes too: a frame says a page is
//! `demo#buttons`, a bar says a gadget is `gadget.clock`, and the page,
//! the gadget, needn't know. It's nothing of its own: no box, no state,
//! the content's in all.

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse, overlay};
use iced::{Event, Length, Rectangle, Size, Vector};

use super::place::{self, Place, placed};
use crate::style::name;
use crate::{Element, Renderer, Theme};

pub struct Slot<'a, Message> {
    place: Place<'a>,
    content: Element<'a, Message>,
}

/// `content` as a `slot`, or the element `kind` and the classes say.
pub fn slot<'a, Message>(content: impl Into<Element<'a, Message>>) -> Slot<'a, Message> {
    Slot {
        place: Place::new(name!("slot")),
        content: content.into(),
    }
}

impl<'a, Message> Slot<'a, Message> {
    placed!(place);
}

impl<Message> Widget<Message, Theme, Renderer> for Slot<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        place::being(self.place.node(), || self.content.as_widget().state())
    }

    fn children(&self) -> Vec<Tree> {
        place::being(self.place.node(), || self.content.as_widget().children())
    }

    fn diff(&self, tree: &mut Tree) {
        place::being(self.place.node(), || self.content.as_widget().diff(tree));
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
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
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
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
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
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
            .operate(tree, layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

impl<'a, Message: 'a> From<Slot<'a, Message>> for Element<'a, Message> {
    fn from(slot: Slot<'a, Message>) -> Self {
        Element::new(slot)
    }
}
