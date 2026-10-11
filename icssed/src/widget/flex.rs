//! The box: a row or a column of children, sized by the box model and
//! the flex subset (`layout.rs`), painted as `boxed.rs` says. With an
//! `on_press` it's a button: `:active` while pressed, its message on a
//! release over it, or on Enter or Space while it has the focus. A
//! disabled box is never `:active`. Its transitions and keyframe
//! animations are `anim.rs`'s; when its animations end it can say so
//! (`on_animation_end`): an app closes a surface after its exit
//! animation that way.

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer;
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{self, Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse, overlay};
use iced::keyboard::{self, key};
use iced::{Event, Length, Rectangle, Size, Vector, touch};

use super::boxed::{BoxState, Boxed};
use super::place::{self, Place, placed};
use crate::layout::flex;
use crate::query::Reported;
use crate::style::{Direction, Name, States, name};
use crate::{Element, Renderer, Theme};

pub struct Flex<'a, Message> {
    pub(crate) boxed: Boxed<'a>,
    /// The constructor's; the theme's `flex-direction` wins.
    direction: Direction,
    children: Vec<Element<'a, Message>>,
    on_press: Option<Message>,
    on_animation_end: Option<Message>,
    id: Option<widget::Id>,
}

/// A `row` box laying `children` out in a row.
pub fn row<'a, Message>(
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> Flex<'a, Message> {
    Flex::of(name!("row"), Direction::Row, children)
}

/// A `column` box laying `children` out in a column.
pub fn column<'a, Message>(
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> Flex<'a, Message> {
    Flex::of(name!("column"), Direction::Column, children)
}

/// A `button`: a row to press, given an [`Flex::on_press`]. Its
/// variants are its classes (`button.primary`), its look the theme's.
pub fn button<'a, Message>(
    children: impl IntoIterator<Item = Element<'a, Message>>,
) -> Flex<'a, Message> {
    Flex::of(name!("button"), Direction::Row, children)
}

/// A row of any widgets: `row![icon(star), text("Save")]`.
#[macro_export]
macro_rules! row {
    ($($child:expr),* $(,)?) => {
        $crate::row([$(::core::convert::Into::<$crate::Element<'_, _>>::into($child)),*])
    };
}

/// A column of any widgets.
#[macro_export]
macro_rules! column {
    ($($child:expr),* $(,)?) => {
        $crate::column([$(::core::convert::Into::<$crate::Element<'_, _>>::into($child)),*])
    };
}

/// A button of any widgets.
#[macro_export]
macro_rules! button {
    ($($child:expr),* $(,)?) => {
        $crate::button([$(::core::convert::Into::<$crate::Element<'_, _>>::into($child)),*])
    };
}

impl<'a, Message> Flex<'a, Message> {
    /// A `kind` box.
    pub(crate) fn of(
        kind: Name,
        direction: Direction,
        children: impl IntoIterator<Item = Element<'a, Message>>,
    ) -> Self {
        Self::placed(Place::new(kind), direction, children)
    }

    /// A box in `place`: a widget made of boxes gives its own.
    pub(crate) fn placed(
        place: Place<'a>,
        direction: Direction,
        children: impl IntoIterator<Item = Element<'a, Message>>,
    ) -> Self {
        Self {
            boxed: Boxed::placed(place),
            direction,
            children: children.into_iter().collect(),
            on_press: None,
            on_animation_end: None,
            id: None,
        }
    }

    placed!(boxed.place);

    /// Its iced id, for operations: where it is (an app opening a
    /// surface by it), focusing a button. Unique across windows: an
    /// operation runs on every window.
    pub fn id(mut self, id: widget::Id) -> Self {
        self.id = Some(id);
        self
    }

    /// What a press sends: the box is a button, unless its node is
    /// `:disabled`.
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    /// What it sends when its keyframe animations end (none of them
    /// infinite): an exit animation over, its surface can go.
    pub fn on_animation_end(mut self, message: Message) -> Self {
        self.on_animation_end = Some(message);
        self
    }

    fn pressable(&self) -> bool {
        self.on_press.is_some() && !self.boxed.node().is_disabled()
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for Flex<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.boxed.size()
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<BoxState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(self.boxed.state())
    }

    fn children(&self) -> Vec<Tree> {
        place::trees(self.boxed.node(), &self.children)
    }

    fn diff(&self, tree: &mut Tree) {
        tree.state
            .downcast_mut::<BoxState>()
            .animated
            .diff(self.boxed.node());
        place::diff(self.boxed.node(), tree, &self.children);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let style = self
            .boxed
            .layout_style(tree.state.downcast_ref::<BoxState>().states);
        let direction = style.flex_direction.unwrap_or(self.direction);
        flex(
            &style,
            direction,
            self.boxed.node().scale(),
            &mut self.children,
            &mut tree.children,
            renderer,
            limits,
        )
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
        let state = tree.state.downcast_ref::<BoxState>();
        self.boxed.draw(
            state,
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
        let cursor = tree.state.downcast_ref::<BoxState>().local(cursor);
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

        let pressable = self.pressable();
        let tracked = self.boxed.tracked(if pressable {
            States::HOVER | States::ACTIVE
        } else {
            States::NONE
        });
        let state = tree.state.downcast_mut::<BoxState>();
        if state.animated.update(event, shell)
            && let Some(message) = &self.on_animation_end
        {
            shell.publish(message.clone());
        }
        let before = state.states;
        let over = cursor.is_over(self.boxed.border_box(layout.bounds()));

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. })
                if pressable && over && !shell.is_event_captured() =>
            {
                state.states = state.states.with(States::ACTIVE, true);
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. })
                if state.states.contains(States::ACTIVE) =>
            {
                state.states = state.states.with(States::ACTIVE, false);
                if over && let Some(message) = &self.on_press {
                    shell.publish(message.clone());
                }
                shell.capture_event();
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key::Named::Enter | key::Named::Space),
                ..
            }) if pressable && state.is_focused() && !shell.is_event_captured() => {
                if let Some(message) = &self.on_press {
                    shell.publish(message.clone());
                }
                shell.capture_event();
            }
            _ => {}
        }
        self.boxed.settle(state, before, tracked, over, shell);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let cursor = tree.state.downcast_ref::<BoxState>().local(cursor);
        let inner = self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, tree), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default();
        let over = cursor.is_over(self.boxed.border_box(layout.bounds()));
        if inner == mouse::Interaction::None && self.pressable() && over {
            mouse::Interaction::Pointer
        } else {
            inner
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
        operation.container(self.id.as_ref(), bounds);
        operation.custom(None, bounds, &mut Reported::new(self.boxed.node()));
        if self.pressable() {
            operation.focusable(
                self.id.as_ref(),
                bounds,
                tree.state.downcast_mut::<BoxState>(),
            );
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

impl<'a, Message: Clone + 'a> From<Flex<'a, Message>> for Element<'a, Message> {
    fn from(flex: Flex<'a, Message>) -> Self {
        Element::new(flex)
    }
}
