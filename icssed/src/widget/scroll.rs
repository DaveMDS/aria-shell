//! Content taller or wider than its room, scrolled: iced's scrollable
//! under the toolkit's `Catalog`. It's no box: its node gives its sizes
//! (`width`, `height`, `min-*`, `max-*`: a list grows with its items up
//! to its `max-height`, then scrolls) and its scrollbars' look. A
//! scrollbar floats over the content, at its end; iced draws it, so its
//! colours don't transition, its sizes follow no state, and the query
//! doesn't find it. Scrolled only sideways, it takes the wheel's turns
//! sideways too, as GTK's do.

use std::cell::{Cell, Ref, RefCell};

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer;
use iced::advanced::widget::{Id, Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::widget::scrollable::{Direction, Scrollable, Scrollbar, Viewport};
use iced::{Event, Length, Rectangle, Size, Vector};

use super::place::{self, Place, placed};
use crate::layout::snap;
use crate::query::Reported;
use crate::style::{Computed, MinSize, Node, Size as Len, name};
use crate::theme::ScrollClass;
use crate::{Element, Renderer, Theme};

pub struct Scroll<'a, Message> {
    place: Place<'a>,
    bars: Bars,
    id: Option<Id>,
    /// What the scrollable is made of, until the scroll has its node.
    pending: Cell<Option<Pending<'a, Message>>>,
    scrollable: RefCell<Option<Scrollable<'a, Message, Theme, Renderer>>>,
}

/// Which ways it scrolls.
#[derive(Clone, Copy, PartialEq)]
enum Bars {
    Vertical,
    Horizontal,
    Both,
}

struct Pending<'a, Message> {
    content: Element<'a, Message>,
    on_scroll: Option<Box<dyn Fn(Viewport) -> Message + 'a>>,
}

/// `content` scrolled up and down: a `scroll`, its
/// `scrollbar.vertical`.
pub fn scroll<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Scroll<'a, Message> {
    Scroll {
        place: Place::new(name!("scroll")),
        bars: Bars::Vertical,
        id: None,
        pending: Cell::new(Some(Pending {
            content: content.into(),
            on_scroll: None,
        })),
        scrollable: RefCell::new(None),
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
    let px = |s: &Computed| thickness(s, vertical).map(|v| snap(v, scale));
    let margin = if vertical {
        s.margin.left
    } else {
        s.margin.top
    };
    let (thickness, thumb) = (px(&s).unwrap_or(0.0), px(&thumb));
    Scrollbar::new()
        .width(thickness)
        .scroller_width(thumb.unwrap_or(thickness))
        .margin(snap(margin, scale))
}

/// A scrollbar's or its thumb's thickness, as its style says: a
/// vertical one's `width`, a horizontal one's `height`.
pub(crate) fn thickness(s: &Computed, vertical: bool) -> Option<f32> {
    match if vertical { s.width } else { s.height } {
        Len::Px(v) => Some(v),
        Len::Auto | Len::Fill => None,
    }
}

impl<'a, Message: 'a> Scroll<'a, Message> {
    placed!(place);

    /// Scrolled sideways instead, its `scrollbar.horizontal`.
    pub fn horizontal(mut self) -> Self {
        self.bars = Bars::Horizontal;
        self
    }

    /// Scrolled both ways, both its scrollbars.
    pub fn both(mut self) -> Self {
        self.bars = Bars::Both;
        self
    }

    /// Its id, for the app to scroll it (`iced::widget::operation`).
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// What a scroll sends: where it is now.
    pub fn on_scroll(mut self, on_scroll: impl Fn(Viewport) -> Message + 'a) -> Self {
        if let Some(pending) = self.pending.get_mut() {
            pending.on_scroll = Some(Box::new(on_scroll));
        }
        self
    }

    /// iced's scrollable, made the first time: it asks its content's
    /// size as it's made, so the content is reached under the scroll.
    fn scrollable(&self) -> Ref<'_, Scrollable<'a, Message, Theme, Renderer>> {
        if let Some(Pending { content, on_scroll }) = self.pending.take() {
            let node = self.place.node();
            let style = self.place.style();
            let class = ScrollClass::new(node);
            let bars = match self.bars {
                Bars::Vertical => Direction::Vertical(bar(class.vertical, true)),
                Bars::Horizontal => Direction::Horizontal(bar(class.horizontal, false)),
                Bars::Both => Direction::Both {
                    vertical: bar(class.vertical, true),
                    horizontal: bar(class.horizontal, false),
                },
            };
            let mut scrollable =
                place::under(node, || Scrollable::with_direction(content, bars)).class(Some(class));
            let length = |len: Len| match len {
                Len::Px(v) => Some(Length::Fixed(snap(v, node.scale()))),
                Len::Fill => Some(Length::Fill),
                // As iced has it: the content's, or the room's when the
                // content fills across.
                Len::Auto => None,
            };
            if let Some(width) = length(style.width) {
                scrollable = scrollable.width(width);
            }
            if let Some(height) = length(style.height) {
                scrollable = scrollable.height(height);
            }
            if let Some(id) = self.id.clone() {
                scrollable = scrollable.id(id);
            }
            if let Some(on_scroll) = on_scroll {
                scrollable = scrollable.on_scroll(on_scroll);
            }
            *self.scrollable.borrow_mut() = Some(scrollable);
        }
        Ref::map(self.scrollable.borrow(), |s| s.as_ref().expect("made"))
    }

    fn scrollable_mut(&mut self) -> &mut Scrollable<'a, Message, Theme, Renderer> {
        drop(self.scrollable());
        self.scrollable.get_mut().as_mut().expect("made")
    }
}

impl<'a, Message: 'a> Widget<Message, Theme, Renderer> for Scroll<'a, Message> {
    fn size(&self) -> Size<Length> {
        Widget::<Message, Theme, Renderer>::size(&*self.scrollable())
    }

    fn size_hint(&self) -> Size<Length> {
        Widget::<Message, Theme, Renderer>::size_hint(&*self.scrollable())
    }

    fn tag(&self) -> tree::Tag {
        Widget::<Message, Theme, Renderer>::tag(&*self.scrollable())
    }

    fn state(&self) -> tree::State {
        Widget::<Message, Theme, Renderer>::state(&*self.scrollable())
    }

    fn children(&self) -> Vec<Tree> {
        let scrollable = self.scrollable();
        place::under(self.place.node(), || {
            Widget::<Message, Theme, Renderer>::children(&*scrollable)
        })
    }

    fn diff(&self, tree: &mut Tree) {
        let scrollable = self.scrollable();
        place::under(self.place.node(), || {
            Widget::<Message, Theme, Renderer>::diff(&*scrollable, tree);
        });
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let s = self.place.style().clone();
        let scale = self.place.node().scale();
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
        Widget::<Message, Theme, Renderer>::layout(self.scrollable_mut(), tree, renderer, &limits)
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
            &*self.scrollable(),
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
            Event::Mouse(mouse::Event::WheelScrolled { delta })
                if self.bars == Bars::Horizontal =>
            {
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
            self.scrollable_mut(),
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
            &*self.scrollable(),
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
        operation.custom(None, layout.bounds(), &mut Reported::new(self.place.node()));
        Widget::<Message, Theme, Renderer>::operate(
            self.scrollable_mut(),
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
            self.scrollable_mut(),
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
