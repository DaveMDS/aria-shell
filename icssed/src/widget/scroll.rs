//! Content taller or wider than its room, scrolled: iced's scrollable
//! under the toolkit's `Catalog`. It's no box: its node gives its sizes
//! (`width`, `height`, `min-*`, `max-*`: a list grows with its items up
//! to its `max-height`, then scrolls) and its scrollbars' look. A
//! scrollbar floats over the content, at its end; iced draws it, so its
//! colours don't transition, its sizes follow no state, and the query
//! doesn't find it. Scrolled only sideways, it takes the wheel's turns
//! sideways too, as GTK's do.

use std::sync::Arc;

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer;
use iced::advanced::widget::{Id, Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::widget::scrollable::{Direction as Bars, Scrollable, Scrollbar, Viewport};
use iced::{Event, Length, Rectangle, Size, Vector};

use crate::layout::snap;
use crate::query::Reported;
use crate::style::{Computed, MinSize, Node, Size as Len};
use crate::theme::ScrollClass;
use crate::{Element, Renderer, Theme};

pub struct Scroll<'a, Message> {
    node: Node<'a>,
    style: Arc<Computed>,
    class: ScrollClass<'a>,
    /// Scrolled only sideways.
    sideways: bool,
    scrollable: Scrollable<'a, Message, Theme, Renderer>,
}

/// `content` scrolled up and down: `node`, its `scrollbar.vertical`.
pub fn scroll<'a, Message: 'a>(
    node: Node<'a>,
    content: impl Into<Element<'a, Message>>,
) -> Scroll<'a, Message> {
    let style = node.base();
    let class = ScrollClass::new(node);
    let mut scrollable =
        Scrollable::with_direction(content, Bars::Vertical(bar(class.vertical, true)))
            .class(Some(class));
    let length = |len: Len, scale: f32| match len {
        Len::Px(v) => Some(Length::Fixed(snap(v, scale))),
        Len::Fill => Some(Length::Fill),
        // As iced has it: the content's, or the room's when the content
        // fills across.
        Len::Auto => None,
    };
    if let Some(width) = length(style.width, node.scale()) {
        scrollable = scrollable.width(width);
    }
    if let Some(height) = length(style.height, node.scale()) {
        scrollable = scrollable.height(height);
    }
    Scroll {
        node,
        style,
        class,
        sideways: false,
        scrollable,
    }
}

/// A scrollbar as its node says: its thickness its `width` (a vertical
/// one's) or `height`, its thumb's the same way (the bar's when not
/// set), the room on both of its sides across its `margin-left` (or
/// `margin-top`).
fn bar(node: Node<'_>, vertical: bool) -> Scrollbar {
    let s = node.base();
    let thumb = node.child("thumb").base();
    let scale = node.scale();
    let px = |len: Len| match len {
        Len::Px(v) => Some(snap(v, scale)),
        Len::Auto | Len::Fill => None,
    };
    let (thickness, thumb, margin) = if vertical {
        (px(s.width), px(thumb.width), s.margin.left)
    } else {
        (px(s.height), px(thumb.height), s.margin.top)
    };
    let thickness = thickness.unwrap_or(0.0);
    Scrollbar::new()
        .width(thickness)
        .scroller_width(thumb.unwrap_or(thickness))
        .margin(snap(margin, scale))
}

impl<'a, Message: 'a> Scroll<'a, Message> {
    /// Scrolled sideways instead, its `scrollbar.horizontal`.
    pub fn horizontal(mut self) -> Self {
        self.sideways = true;
        self.scrollable = self
            .scrollable
            .direction(Bars::Horizontal(bar(self.class.horizontal, false)));
        self
    }

    /// Scrolled both ways, both its scrollbars.
    pub fn both(mut self) -> Self {
        self.sideways = false;
        self.scrollable = self.scrollable.direction(Bars::Both {
            vertical: bar(self.class.vertical, true),
            horizontal: bar(self.class.horizontal, false),
        });
        self
    }

    /// Its id, for the app to scroll it (`iced::widget::operation`).
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.scrollable = self.scrollable.id(id);
        self
    }

    /// What a scroll sends: where it is now.
    pub fn on_scroll(mut self, on_scroll: impl Fn(Viewport) -> Message + 'a) -> Self {
        self.scrollable = self.scrollable.on_scroll(on_scroll);
        self
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Scroll<'_, Message> {
    fn size(&self) -> Size<Length> {
        Widget::<Message, Theme, Renderer>::size(&self.scrollable)
    }

    fn size_hint(&self) -> Size<Length> {
        Widget::<Message, Theme, Renderer>::size_hint(&self.scrollable)
    }

    fn tag(&self) -> tree::Tag {
        Widget::<Message, Theme, Renderer>::tag(&self.scrollable)
    }

    fn state(&self) -> tree::State {
        Widget::<Message, Theme, Renderer>::state(&self.scrollable)
    }

    fn children(&self) -> Vec<Tree> {
        Widget::<Message, Theme, Renderer>::children(&self.scrollable)
    }

    fn diff(&self, tree: &mut Tree) {
        Widget::<Message, Theme, Renderer>::diff(&self.scrollable, tree);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let s = &self.style;
        let scale = self.node.scale();
        let min = |min: MinSize| match min {
            MinSize::Px(v) => snap(v, scale),
            MinSize::Auto => 0.0,
        };
        let max = |max: f32| {
            if max.is_finite() {
                snap(max, scale)
            } else {
                max
            }
        };
        let limits = limits
            .max_width(max(s.max_width))
            .max_height(max(s.max_height))
            .min_width(min(s.min_width))
            .min_height(min(s.min_height));
        Widget::<Message, Theme, Renderer>::layout(&mut self.scrollable, tree, renderer, &limits)
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
        Widget::<Message, Theme, Renderer>::draw(
            &self.scrollable,
            tree,
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
        // Sideways, the wheel's turns go sideways too.
        let turned;
        let event = match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if self.sideways => {
                let delta = match *delta {
                    mouse::ScrollDelta::Lines { x: 0.0, y } => {
                        mouse::ScrollDelta::Lines { x: y, y: 0.0 }
                    }
                    mouse::ScrollDelta::Pixels { x: 0.0, y } => {
                        mouse::ScrollDelta::Pixels { x: y, y: 0.0 }
                    }
                    delta => delta,
                };
                turned = Event::Mouse(mouse::Event::WheelScrolled { delta });
                &turned
            }
            event => event,
        };
        Widget::<Message, Theme, Renderer>::update(
            &mut self.scrollable,
            tree,
            event,
            layout,
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
        Widget::<Message, Theme, Renderer>::mouse_interaction(
            &self.scrollable,
            tree,
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
        operation.custom(None, layout.bounds(), &mut Reported::new(self.node));
        Widget::<Message, Theme, Renderer>::operate(
            &mut self.scrollable,
            tree,
            layout,
            renderer,
            operation,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        Widget::<Message, Theme, Renderer>::overlay(
            &mut self.scrollable,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: 'a> From<Scroll<'a, Message>> for Element<'a, Message> {
    fn from(scroll: Scroll<'a, Message>) -> Self {
        Element::new(scroll)
    }
}
