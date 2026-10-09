//! A box drawn by hand. iced's own container and button styles draw a
//! background and a border all around; what a theme asks for can be
//! more (a border on one side only, `border.rs`). [`Painted`] wraps the
//! container or button [`Theme`] builds, styled to draw its content
//! only, and draws the box under it in CSS order: shadow and
//! background, then the border, then the content. The border takes
//! room, as in CSS: the widget's padding is the theme's plus the
//! border ([`Style::inset`]). The box is snapped to the pixel grid, so
//! a 1 px border is one sharp line wherever the layout puts the box.
//!
//! Everything else (layout, events, `operate` for the widget ids,
//! overlays) is the wrapped widget's; `Painted` forwards the builder
//! methods the views chain on it (`on_press`, `width`, ...).
//!
//! A button's box follows its status (`:hover`, `:active`), worked out
//! as iced's button does (iced keeps its own private, and asks for the
//! style only while drawing, after the box).

mod border;

use std::collections::BTreeSet;
use std::sync::Mutex;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay, renderer};
use iced::border::Radius;
use iced::widget::{Button, Container, button};
use iced::{
    Border, Color, Element, Event, Length, Padding, Rectangle, Renderer, Size, Vector, mouse, touch,
};

use super::{Node, Style, Theme};

/// A [`Theme::container`] or [`Theme::button`]: the iced widget `W`
/// and the box drawn under its content.
pub struct Painted<'a, W> {
    inner: W,
    node: Node,
    look: Look<'a>,
}

enum Look<'a> {
    /// A container's box: the style it was built with.
    Fixed(Box<Style>),
    /// A button's: resolved for its status at every draw.
    Button { theme: &'a Theme, enabled: bool },
}

/// Whether the button is held down, as iced's button tracks it.
#[derive(Default)]
struct State {
    pressed: bool,
}

impl<'a, W> Painted<'a, W> {
    pub(super) fn fixed(inner: W, node: Node, style: Style) -> Self {
        Self {
            inner,
            node,
            look: Look::Fixed(Box::new(style)),
        }
    }

    /// Disabled until an `on_press`, as an iced button.
    pub(super) fn button(inner: W, node: Node, theme: &'a Theme) -> Self {
        Self {
            inner,
            node,
            look: Look::Button {
                theme,
                enabled: false,
            },
        }
    }

    fn map(mut self, f: impl FnOnce(W) -> W) -> Self {
        self.inner = f(self.inner);
        self
    }

    fn enable(mut self, on: bool) -> Self {
        if let Look::Button { enabled, .. } = &mut self.look {
            *enabled = on;
        }
        self
    }
}

impl<'a, M: Clone + 'a> Painted<'a, Button<'a, M>> {
    pub fn on_press(self, message: M) -> Self {
        self.map(|b| b.on_press(message)).enable(true)
    }

    pub fn on_press_maybe(self, message: Option<M>) -> Self {
        let on = message.is_some();
        self.map(|b| b.on_press_maybe(message)).enable(on)
    }

    pub fn width(self, width: impl Into<Length>) -> Self {
        self.map(|b| b.width(width))
    }

    pub fn height(self, height: impl Into<Length>) -> Self {
        self.map(|b| b.height(height))
    }
}

impl<'a, M: 'a> Painted<'a, Container<'a, M>> {
    pub fn width(self, width: impl Into<Length>) -> Self {
        self.map(|c| c.width(width))
    }

    pub fn height(self, height: impl Into<Length>) -> Self {
        self.map(|c| c.height(height))
    }

    /// The padding inside the border, in place of the theme's.
    pub fn padding(self, padding: impl Into<Padding>) -> Self {
        let padding = padding.into();
        let inset = match &self.look {
            Look::Fixed(style) => style.inset_with(padding),
            Look::Button { .. } => padding,
        };
        self.map(|c| c.padding(inset))
    }

    pub fn align_x(self, align: impl Into<iced::alignment::Horizontal>) -> Self {
        self.map(|c| c.align_x(align))
    }

    pub fn align_y(self, align: impl Into<iced::alignment::Vertical>) -> Self {
        self.map(|c| c.align_y(align))
    }

    pub fn center_x(self, width: impl Into<Length>) -> Self {
        self.map(|c| c.center_x(width))
    }

    pub fn center_y(self, height: impl Into<Length>) -> Self {
        self.map(|c| c.center_y(height))
    }

    pub fn max_width(self, width: impl Into<iced::Pixels>) -> Self {
        self.map(|c| c.max_width(width))
    }

    pub fn clip(self, clip: bool) -> Self {
        self.map(|c| c.clip(clip))
    }
}

/// iced's button status from what it's built with and the pointer.
fn status(enabled: bool, pressed: bool, over: bool) -> button::Status {
    match (enabled, over, pressed) {
        (false, _, _) => button::Status::Disabled,
        (true, true, true) => button::Status::Pressed,
        (true, true, false) => button::Status::Hovered,
        (true, false, _) => button::Status::Active,
    }
}

impl<'a, M, W> Widget<M, iced::Theme, Renderer> for Painted<'a, W>
where
    W: Widget<M, iced::Theme, Renderer>,
{
    fn size(&self) -> Size<Length> {
        self.inner.size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.inner.size_hint()
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(
            &self.inner as &dyn Widget<M, iced::Theme, Renderer>,
        )]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.inner as &dyn Widget<M, iced::Theme, Renderer>]);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.inner.layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.inner
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        // A press iced's button takes: not taken by anyone before it,
        // on it, while it can be pressed.
        let free = !shell.is_event_captured();
        let enabled = matches!(self.look, Look::Button { enabled: true, .. });
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if free && enabled && cursor.is_over(layout.bounds()) {
                    state.pressed = true;
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. }) => {
                state.pressed = false;
            }
            _ => {}
        }
        self.inner.update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let resolved;
        let look = match &self.look {
            Look::Fixed(style) => style,
            Look::Button { theme, enabled } => {
                let pressed = tree.state.downcast_ref::<State>().pressed;
                let status = status(*enabled, pressed, cursor.is_over(bounds));
                resolved = theme.resolve(&self.node.status(status));
                &resolved
            }
        };
        draw_box(renderer, bounds, look, &self.node);
        self.inner.draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
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
        self.inner
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, Renderer>> {
        self.inner.overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M, W> From<Painted<'a, W>> for Element<'a, M>
where
    M: 'a,
    W: Widget<M, iced::Theme, Renderer> + 'a,
{
    fn from(painted: Painted<'a, W>) -> Self {
        Element::new(painted)
    }
}

/// The box of `style` in `bounds`: shadow, background and border.
fn draw_box(renderer: &mut Renderer, bounds: Rectangle, style: &Style, node: &Node) {
    use iced::advanced::Renderer as _;

    let (quad, background) = box_quad(bounds, style, node);
    if background.a > 0.0 || quad.border.width > 0.0 || quad.shadow.color.a > 0.0 {
        renderer.fill_quad(quad, background);
    }
    if border::uniform(&style.border).is_none() {
        for (bounds, color) in border::rects(bounds, &style.border) {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    snap: true,
                    ..Default::default()
                },
                color,
            );
        }
    }
}

/// The quad iced draws for the box, and its background colour: with
/// the border when its sides look the same, square when they don't.
fn box_quad(bounds: Rectangle, style: &Style, node: &Node) -> (renderer::Quad, Color) {
    let border = match border::uniform(&style.border) {
        Some(side) => Border {
            width: side.width,
            color: side.color.unwrap_or(Color::TRANSPARENT),
            radius: style.border_radius,
        },
        None => {
            if style.border_radius != Radius::default() {
                warn_square(node);
            }
            Border::default()
        }
    };
    let quad = renderer::Quad {
        bounds,
        border,
        shadow: style.shadow.unwrap_or_default(),
        snap: true,
    };
    (quad, style.background.unwrap_or(Color::TRANSPARENT))
}

/// Rounded corners need the same border all around: said once per
/// element.
fn warn_square(node: &Node) {
    static WARNED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    let path = format!("{node:?}");
    let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    if !warned.contains(&path) {
        log::warn!(
            "theme: {path}: border-radius ignored, the sides of the border differ \
             (rounded corners take the same border all around)"
        );
        warned.insert(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{BorderSide, Sides};

    fn red() -> Color {
        Color::from_rgb(1.0, 0.0, 0.0)
    }

    #[test]
    fn uniform_border_is_rounded() {
        let style = Style {
            background: Some(Color::WHITE),
            border: Sides::all(BorderSide {
                width: 2.0,
                color: Some(red()),
            }),
            border_radius: Radius::new(4.0),
            ..Style::default()
        };
        let (quad, background) = box_quad(Rectangle::default(), &style, &Node::root("a"));
        assert_eq!(background, Color::WHITE);
        assert_eq!(quad.border.width, 2.0);
        assert_eq!(quad.border.color, red());
        assert_eq!(quad.border.radius, Radius::new(4.0));
    }

    #[test]
    fn sides_that_differ_are_square() {
        let style = Style {
            border: Sides {
                bottom: BorderSide {
                    width: 3.0,
                    color: Some(red()),
                },
                ..Sides::default()
            },
            border_radius: Radius::new(4.0),
            ..Style::default()
        };
        let (quad, _) = box_quad(Rectangle::default(), &style, &Node::root("b"));
        assert_eq!(quad.border.width, 0.0);
        assert_eq!(quad.border.radius, Radius::default());
    }

    #[test]
    fn button_status() {
        assert_eq!(status(false, true, true), button::Status::Disabled);
        assert_eq!(status(true, true, true), button::Status::Pressed);
        assert_eq!(status(true, false, true), button::Status::Hovered);
        assert_eq!(status(true, true, false), button::Status::Active);
    }
}
