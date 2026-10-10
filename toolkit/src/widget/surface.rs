//! A surface's root: its box, and the room its shadow needs around it,
//! so the shadow isn't cut where the surface ends. Popups, toasts and
//! dialogs are a `surface` and their content; the surfaces are the
//! app's to open.

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::{Event, Length, Point, Rectangle, Size};

use crate::layout::snap_up;
use crate::style::Node;
use crate::{Element, Renderer, Theme};

pub struct Surface<'a, Message> {
    /// The room around the content: left, top, right, bottom.
    room: [f32; 4],
    content: Element<'a, Message>,
}

/// `content`, whose root box is `node`, with the room its shadow needs.
pub fn surface<'a, Message>(
    node: Node<'a>,
    content: impl Into<Element<'a, Message>>,
) -> Surface<'a, Message> {
    let style = node.base();
    let scale = node.scale();
    let room = style.shadow.map_or([0.0; 4], |s| {
        let side = |offset: f32| snap_up((s.blur - offset).max(0.0), scale);
        [side(s.x), side(s.y), side(-s.x), side(-s.y)]
    });
    Surface {
        room,
        content: content.into(),
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Surface<'_, Message> {
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
        let [left, top, right, bottom] = self.room;
        let room = Size::new(left + right, top + bottom);
        let content = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &limits.shrink(room))
            .move_to(Point::new(left, top));
        let size = content.size();
        layout::Node::with_children(
            Size::new(size.width + room.width, size.height + room.height),
            vec![content],
        )
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
        let content = layout.children().next().expect("one child");
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            content,
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
        let content = layout.children().next().expect("one child");
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            content,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
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
        let content = layout.children().next().expect("one child");
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            content,
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
        let content = layout.children().next().expect("one child");
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], content, renderer, operation);
    }
}

impl<'a, Message: 'a> From<Surface<'a, Message>> for Element<'a, Message> {
    fn from(surface: Surface<'a, Message>) -> Self {
        Element::new(surface)
    }
}
