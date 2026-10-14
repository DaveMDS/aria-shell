//! A line of text to edit: iced's text input in a box of the toolkit,
//! the app's children before and after it (an icon, a toggle to peek at
//! a password). The box is `:focus` while its field has the focus (CSS's
//! `:focus-within`), and a press on the box puts the focus in the field.
//! The field takes the free room, its text centred on its letters as a
//! label's is (`text-box`). iced draws the field: its colours are its
//! nodes', but they don't transition.

use std::cell::{Cell, Ref, RefCell};

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer;
use iced::advanced::text;
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Id, Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse, overlay};
use iced::widget::text::LineHeight;
use iced::widget::text_input::{self, TextInput};
use iced::{Event, Length, Pixels, Point, Rectangle, Size, Vector, touch};

use super::boxed::{BoxState, Boxed};
use super::place::{self, Place, placed};
use crate::layout::{flex, snap, snap_up};
use crate::query::Reported;
use crate::style::{Direction, EdgeOver, EdgeUnder, Node, States, name};
use crate::theme::FieldClass;
use crate::{Element, Renderer, Theme};

type Paragraph = <Renderer as text::Renderer>::Paragraph;

pub struct Input<'a, Message> {
    place: Place<'a>,
    field: Field<'a, Message>,
    leading: Vec<Element<'a, Message>>,
    trailing: Vec<Element<'a, Message>>,
}

/// `value` to edit: an `input`, its `field` (`placeholder` shown while
/// it's empty). Without [`Input::on_input`] it can't be edited.
pub fn input<'a, Message: Clone + 'a>(placeholder: &str, value: &str) -> Input<'a, Message> {
    let input = TextInput::new(placeholder, value)
        .padding(0)
        .width(Length::Fill);
    Input {
        place: Place::new(name!("input")),
        field: Field {
            place: Place::new(name!("field")),
            input: RefCell::new(Some(input)),
            on_input: Cell::new(None),
            ready: Cell::new(false),
        },
        leading: Vec::new(),
        trailing: Vec::new(),
    }
}

impl<'a, Message: Clone + 'a> Input<'a, Message> {
    placed!(place);

    fn edit(
        mut self,
        f: impl FnOnce(
            TextInput<'a, Message, Theme, Renderer>,
        ) -> TextInput<'a, Message, Theme, Renderer>,
    ) -> Self {
        let slot = self.field.input.get_mut();
        *slot = slot.take().map(f);
        self
    }

    /// What an edit sends: the new text. Ignored when it's `:disabled`.
    pub fn on_input(self, on_input: impl Fn(String) -> Message + 'a) -> Self {
        self.field.on_input.set(Some(Box::new(on_input)));
        self
    }

    /// What Enter sends.
    pub fn on_submit(self, message: Message) -> Self {
        self.edit(|input| input.on_submit(message))
    }

    /// Its text shown as dots: a password's.
    pub fn secure(self, secure: bool) -> Self {
        self.edit(|input| input.secure(secure))
    }

    /// Its field's id, for the app to put the focus in it
    /// (`iced::widget::operation::focus`).
    pub fn id(self, id: impl Into<Id>) -> Self {
        self.edit(|input| input.id(id))
    }

    /// A child before the field: an icon.
    pub fn leading(mut self, child: impl Into<Element<'a, Message>>) -> Self {
        self.leading.push(child.into());
        self
    }

    /// A child after the field: a toggle to peek at a password.
    pub fn trailing(mut self, child: impl Into<Element<'a, Message>>) -> Self {
        self.trailing.push(child.into());
        self
    }
}

impl<'a, Message: Clone + 'a> From<Input<'a, Message>> for Element<'a, Message> {
    fn from(input: Input<'a, Message>) -> Self {
        let field = input.leading.len();
        let mut children = input.leading;
        children.push(Element::new(input.field));
        children.extend(input.trailing);
        Element::new(InputBox {
            boxed: Boxed::placed(input.place),
            children,
            field,
        })
    }
}

type OnInput<'a, Message> = Box<dyn Fn(String) -> Message + 'a>;

/// iced's text input in its box: laid out as a label is, its box from
/// its capitals' top to its baseline, where iced centres a line box.
/// Disabled, it's out of reach: no pointer or keys, no focus (iced's
/// would still take it).
struct Field<'a, Message> {
    place: Place<'a>,
    /// iced's input, given its style once the field has its node.
    input: RefCell<Option<TextInput<'a, Message, Theme, Renderer>>>,
    /// What an edit sends, given to the input unless it's disabled.
    on_input: Cell<Option<OnInput<'a, Message>>>,
    ready: Cell<bool>,
}

impl<'a, Message: Clone + 'a> Field<'a, Message> {
    fn node(&self) -> Node<'a> {
        self.place.node()
    }

    /// Its input is `:disabled`.
    fn disabled(&self) -> bool {
        self.node().parent().is_some_and(Node::is_disabled)
    }

    /// The input, its style given the first time.
    fn input(&self) -> Ref<'_, TextInput<'a, Message, Theme, Renderer>> {
        if !self.ready.replace(true) {
            let node = self.node();
            let s = self.place.style();
            let mut slot = self.input.borrow_mut();
            let mut input = slot
                .take()
                .expect("an input")
                .font(s.font)
                .size(s.font_size)
                .line_height(LineHeight::Absolute(Pixels(s.line_height_px)))
                .class(FieldClass::new(node));
            if let Some(on_input) = self.on_input.take()
                && !self.disabled()
            {
                input = input.on_input(on_input);
            }
            *slot = Some(input);
        }
        Ref::map(self.input.borrow(), |input| {
            input.as_ref().expect("an input")
        })
    }

    fn input_mut(&mut self) -> &mut TextInput<'a, Message, Theme, Renderer> {
        drop(self.input());
        self.input.get_mut().as_mut().expect("an input")
    }
}

impl<'a, Message: Clone + 'a> Widget<Message, Theme, Renderer> for Field<'a, Message> {
    fn size(&self) -> Size<Length> {
        Widget::<Message, Theme, Renderer>::size(&*self.input())
    }

    fn tag(&self) -> tree::Tag {
        let input = self.input.borrow();
        Widget::<Message, Theme, Renderer>::tag(input.as_ref().expect("an input"))
    }

    fn state(&self) -> tree::State {
        Widget::<Message, Theme, Renderer>::state(&*self.input())
    }

    fn diff(&self, tree: &mut Tree) {
        Widget::<Message, Theme, Renderer>::diff(&*self.input(), tree);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let s = self.place.style().clone();
        let scale = self.node().scale();
        let inner =
            Widget::<Message, Theme, Renderer>::layout(self.input_mut(), tree, renderer, limits);
        // One line box, its baseline where cosmic-text puts it, trimmed
        // as `text-box` says; the baseline on a whole device pixel.
        let line = inner.size().height;
        let baseline = (line - (s.ascent + s.descent)) / 2.0 + s.ascent;
        let over = match s.text_box_edge.over {
            EdgeOver::Text => s.ascent,
            EdgeOver::Cap => s.cap_height,
            EdgeOver::Ex => s.x_height,
        };
        let under = match s.text_box_edge.under {
            EdgeUnder::Text => s.descent,
            EdgeUnder::Alphabetic => 0.0,
        };
        let top = if s.text_box_trim.start {
            (baseline - over).max(0.0)
        } else {
            0.0
        };
        let below = if s.text_box_trim.end {
            under.min(line - baseline)
        } else {
            line - baseline
        };
        let at = snap(baseline - top, scale);
        let height = snap_up(at + below.max(0.0), scale);
        let size = Size::new(inner.size().width, height);
        layout::Node::with_children(size, vec![inner.move_to(Point::new(0.0, at - baseline))])
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
        let inner = layout.children().next().expect("the input");
        Widget::<Message, Theme, Renderer>::draw(
            &*self.input(),
            tree,
            renderer,
            theme,
            style,
            inner,
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
        if self.disabled() && !matches!(event, Event::Window(_)) {
            return;
        }
        let inner = layout.children().next().expect("the input");
        Widget::<Message, Theme, Renderer>::update(
            self.input_mut(),
            tree,
            event,
            inner,
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
        let inner = layout.children().next().expect("the input");
        Widget::<Message, Theme, Renderer>::mouse_interaction(
            &*self.input(),
            tree,
            inner,
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
        operation.custom(None, layout.bounds(), &mut Reported::new(self.node()));
        if self.disabled() {
            return;
        }
        let inner = layout.children().next().expect("the input");
        Widget::<Message, Theme, Renderer>::operate(
            self.input_mut(),
            tree,
            inner,
            renderer,
            operation,
        );
    }
}

/// The input's box, its field and the app's children in a row.
struct InputBox<'a, Message> {
    boxed: Boxed<'a>,
    children: Vec<Element<'a, Message>>,
    /// Where the field is among them.
    field: usize,
}

/// The box `:focus` while its field has the focus.
fn follow(state: &mut BoxState, field: &Tree) {
    let focused = field
        .state
        .downcast_ref::<text_input::State<Paragraph>>()
        .is_focused();
    if focused {
        state.focus();
    } else {
        state.unfocus();
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for InputBox<'_, Message> {
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
        flex(
            &style,
            style.flex_direction.unwrap_or(Direction::Row),
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
            |renderer, theme, style, cursor, _| {
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
        let over = cursor.is_over(self.boxed.border_box(layout.bounds()));
        // A press on the box, around its field: the field takes the focus.
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        | Event::Touch(touch::Event::FingerPressed { .. }) = event
            && over
            && !self.boxed.node().is_disabled()
            && !shell.is_event_captured()
        {
            tree.children[self.field]
                .state
                .downcast_mut::<text_input::State<Paragraph>>()
                .focus();
            shell.capture_event();
            shell.request_redraw();
        }
        let state = tree.state.downcast_mut::<BoxState>();
        state.animated.update(event, shell);
        let before = state.states;
        follow(state, &tree.children[self.field]);
        let tracked = self.boxed.tracked(States::NONE);
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
        if inner == mouse::Interaction::None && over && !self.boxed.node().is_disabled() {
            mouse::Interaction::Text
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
        operation.container(None, bounds);
        operation.custom(None, bounds, &mut Reported::new(self.boxed.node()));
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
        // An operation may have moved the focus (Tab, the app's).
        follow(
            tree.state.downcast_mut::<BoxState>(),
            &tree.children[self.field],
        );
    }
}
