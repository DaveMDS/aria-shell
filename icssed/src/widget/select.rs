//! A select: one of a list of options, shown in a box with an arrow.
//! Pressed (or Enter, Space while it has the focus), its options open
//! under it (over it, where there's no room under) as a menu, the one
//! chosen checked; picking one sends the app's message and closes them,
//! as Escape and a press elsewhere do. Up and Down on the closed select,
//! focused, choose the option before or after. `:active` while its
//! options are open: a class is the view's, this state the widget's.
//!
//! In a window the options are an overlay; an app with surfaces of its
//! own (a layer surface would cut them) is told the rectangle to show
//! them by instead (`on_open`), says when they're open, and shows a
//! `icssed::menu` of them there.

use std::borrow::Cow;

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::keyboard::{self, key};
use iced::{Event, Length, Point, Rectangle, Size, Vector, touch};

use super::boxed::{BoxState, Boxed};
use super::menu::{MenuItem, menu};
use super::place::{self, placed};
use super::text::{mark, text};
use crate::layout::{flex, snap};
use crate::query::Reported;
use crate::style::{Direction, States, name};
use crate::{Element, Renderer, Theme};

pub struct Select<'a, Message> {
    boxed: Boxed<'a>,
    /// The choice shown, its arrow.
    children: Vec<Element<'a, Message>>,
    /// The options, a menu.
    list: Element<'a, Message>,
    count: usize,
    selected: Option<usize>,
    on_select: Box<dyn Fn(usize) -> Message + 'a>,
    /// The app's way: told where, it says whether they're open.
    on_open: Option<Box<dyn Fn(Option<Rectangle>) -> Message + 'a>>,
    opened: bool,
}

/// One of `options`, `selected` if any: a `select`, its `label` and
/// `arrow`, its options a `menu`. Picking one sends `on_select` with
/// its index.
pub fn select<'a, Message: Clone + 'a, S: Into<Cow<'a, str>>>(
    options: impl IntoIterator<Item = S>,
    selected: Option<usize>,
    on_select: impl Fn(usize) -> Message + 'a,
) -> Select<'a, Message> {
    let options: Vec<Cow<'a, str>> = options.into_iter().map(Into::into).collect();
    let count = options.len();
    let selected = selected.filter(|&i| i < count);
    let shown = selected.map(|i| options[i].clone()).unwrap_or_default();
    let items = options.into_iter().enumerate().map(|(i, option)| {
        MenuItem::new(option)
            .check(Some(i) == selected)
            .on_press(on_select(i))
    });
    Select {
        boxed: Boxed::of(name!("select")),
        children: vec![text(shown).into(), mark().kind_of(name!("arrow")).into()],
        list: menu(items).into(),
        count,
        selected,
        on_select: Box::new(on_select),
        on_open: None,
        opened: false,
    }
}

impl<'a, Message: Clone + 'a> Select<'a, Message> {
    placed!(boxed.place);

    /// What it shows while nothing's chosen: its `label.placeholder`.
    pub fn placeholder(mut self, placeholder: impl Into<Cow<'a, str>>) -> Self {
        if self.selected.is_none() {
            self.children[0] = text(placeholder).class("placeholder").into();
        }
        self
    }

    /// Its options shown by the app: a press sends the rectangle to show
    /// them by (`Some`, in the surface's coordinates) or that they're to
    /// close (`None`); `opened` says whether they are.
    pub fn on_open(
        mut self,
        opened: bool,
        on_open: impl Fn(Option<Rectangle>) -> Message + 'a,
    ) -> Self {
        self.on_open = Some(Box::new(on_open));
        self.opened = opened;
        self
    }
}

impl<Message> Select<'_, Message> {
    fn enabled(&self) -> bool {
        !self.boxed.node().is_disabled() && self.count > 0
    }
}

#[derive(Debug)]
struct State {
    boxed: BoxState,
    /// Where the scrolls around it move it, as the last overlay saw.
    translation: Vector,
}

impl State {
    fn open(&self) -> bool {
        self.boxed.states.contains(States::ACTIVE)
    }

    fn set_open(&mut self, open: bool) {
        if self.open() != open {
            self.boxed.states = self.boxed.states.with(States::ACTIVE, open);
            self.boxed.animated.changed();
        }
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for Select<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.boxed.size()
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            boxed: self.boxed.state(),
            translation: Vector::ZERO,
        })
    }

    fn children(&self) -> Vec<Tree> {
        let mut children: Vec<&Element<'_, Message>> = self.children.iter().collect();
        children.push(&self.list);
        place::trees(self.boxed.node(), &children)
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        state.boxed.animated.diff(self.boxed.node());
        if self.on_open.is_some() {
            state.set_open(self.opened);
        }
        let mut children: Vec<&Element<'_, Message>> = self.children.iter().collect();
        children.push(&self.list);
        place::diff(self.boxed.node(), tree, &children);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let style = self
            .boxed
            .layout_style(tree.state.downcast_ref::<State>().boxed.states);
        let shown = self.children.len();
        flex(
            &style,
            Direction::Row,
            self.boxed.node().scale(),
            &mut self.children,
            &mut tree.children[..shown],
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
        let bounds = self.boxed.border_box(layout.bounds());
        let over = cursor.is_over(bounds);
        let free = enabled && !shell.is_event_captured();
        let toggle = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => free && over,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key::Named::Enter | key::Named::Space),
                ..
            }) => free && state.boxed.is_focused(),
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(named @ (key::Named::ArrowUp | key::Named::ArrowDown)),
                ..
            }) if free && state.boxed.is_focused() && !state.open() => {
                let next = match (named, self.selected) {
                    (key::Named::ArrowUp, Some(i)) => i.saturating_sub(1),
                    (key::Named::ArrowUp, None) => 0,
                    (_, Some(i)) => (i + 1).min(self.count - 1),
                    (_, None) => 0,
                };
                if Some(next) != self.selected {
                    self.selected = Some(next);
                    shell.publish((self.on_select)(next));
                }
                shell.capture_event();
                false
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key::Named::Escape),
                ..
            }) if state.open() && self.on_open.is_none() => {
                state.set_open(false);
                shell.capture_event();
                false
            }
            _ => false,
        };
        if toggle {
            shell.capture_event();
            match &self.on_open {
                Some(on_open) => shell.publish(on_open(
                    (!state.open()).then_some(bounds + state.translation),
                )),
                None => {
                    let open = !state.open();
                    state.set_open(open);
                    shell.invalidate_layout();
                }
            }
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
        let cursor = tree.state.downcast_ref::<State>().boxed.local(cursor);
        if self.enabled() && cursor.is_over(self.boxed.border_box(layout.bounds())) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
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

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        state.translation = translation;
        if !state.open() || self.on_open.is_some() {
            return None;
        }
        let about = self.boxed.border_box(layout.bounds()) + translation;
        let list = Options {
            list: &mut self.list,
            tree: tree.children.last_mut().expect("the options' tree"),
            state,
            about,
            scale: self.boxed.node().scale(),
        };
        Some(overlay::Element::new(Box::new(list)))
    }
}

/// The options, open over everything.
struct Options<'a, 'b, Message> {
    list: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    state: &'b mut State,
    /// The select, where it's shown.
    about: Rectangle,
    scale: f32,
}

impl<Message> overlay::Overlay<Message, Theme, Renderer> for Options<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        // At least as wide as the select.
        let node = self.list.as_widget_mut().layout(
            self.tree,
            renderer,
            &Limits::new(Size::new(self.about.width.min(bounds.width), 0.0), bounds),
        );
        let size = node.size();
        let x = self.about.x.min(bounds.width - size.width).max(0.0);
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
        self.list.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &Rectangle::with_size(Size::INFINITE),
        );
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
        // Its items' messages seen apart: one sent is an option picked.
        let mut messages = Vec::new();
        let mut local = Shell::new(&mut messages);
        self.list.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut local,
            &Rectangle::with_size(Size::INFINITE),
        );
        let picked = !local.is_empty();
        shell.merge(local, |message| message);
        let pressed_elsewhere = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(_))
                | Event::Touch(touch::Event::FingerPressed { .. })
        ) && !cursor.is_over(layout.bounds())
            && !cursor.is_over(self.about);
        let escape = matches!(
            event,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key::Named::Escape),
                ..
            })
        );
        if picked || pressed_elsewhere || escape {
            self.state.set_open(false);
            shell.invalidate_layout();
            shell.request_redraw();
            if escape {
                shell.capture_event();
            }
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.list.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &Rectangle::with_size(Size::INFINITE),
            renderer,
        )
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.list
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation);
    }
}

impl<'a, Message: Clone + 'a> From<Select<'a, Message>> for Element<'a, Message> {
    fn from(select: Select<'a, Message>) -> Self {
        Element::new(select)
    }
}
