//! Where a widget sits in the element tree, for selector matching: its
//! type, classes, id, attributes, position among siblings, interaction
//! state, and its ancestors.
//!
//! A `Node` is an immutable linked list (`Arc` to the parent), so deriving
//! a child or a state variant is one small allocation and cloning is a
//! refcount bump. Style closures handed to iced must own everything they
//! use, so they own a `Node`.
//!
//! The `Debug` form is the element path as a selector would spell it
//! (`panel.top[output="DP-1"] > slot.end > gadget.clock:nth-child(2)`);
//! the theme helpers use it as the `widget::Id` of the widgets they
//! build, which is what `debug widgets` reports.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use iced::widget::{button, slider, text_input};

#[derive(Clone)]
pub struct Node(Arc<Inner>);

#[derive(Clone)]
struct Inner {
    parent: Option<Node>,
    kind: Cow<'static, str>,
    classes: Vec<Cow<'static, str>>,
    id: Option<String>,
    attrs: Vec<(&'static str, String)>,
    /// `(index, count)` among siblings, when the parent tells us.
    index: Option<(usize, usize)>,
    hover: bool,
    pressed: bool,
    focused: bool,
    disabled: bool,
}

impl Node {
    /// A root element (a surface: `panel`, `popup`).
    pub fn root(kind: impl Into<Cow<'static, str>>) -> Self {
        Self::build(None, kind.into())
    }

    pub fn child(&self, kind: impl Into<Cow<'static, str>>) -> Self {
        Self::build(Some(self.clone()), kind.into())
    }

    fn build(parent: Option<Node>, kind: Cow<'static, str>) -> Self {
        Self(Arc::new(Inner {
            parent,
            kind,
            classes: Vec::new(),
            id: None,
            attrs: Vec::new(),
            index: None,
            hover: false,
            pressed: false,
            focused: false,
            disabled: false,
        }))
    }

    fn with(&self, f: impl FnOnce(&mut Inner)) -> Self {
        let mut inner = (*self.0).clone();
        f(&mut inner);
        Self(Arc::new(inner))
    }

    pub fn class(&self, class: impl Into<Cow<'static, str>>) -> Self {
        self.with(|n| n.classes.push(class.into()))
    }

    pub fn class_if(&self, class: impl Into<Cow<'static, str>>, on: bool) -> Self {
        if on { self.class(class) } else { self.clone() }
    }

    pub fn id(&self, id: impl Into<String>) -> Self {
        self.with(|n| n.id = Some(id.into()))
    }

    /// `Some(id)` sets it, `None` leaves the node without one.
    pub fn id_opt(&self, id: Option<impl Into<String>>) -> Self {
        match id {
            Some(id) => self.id(id),
            None => self.clone(),
        }
    }

    pub fn attr(&self, name: &'static str, value: impl Into<String>) -> Self {
        self.with(|n| n.attrs.push((name, value.into())))
    }

    /// Position among siblings, for `:first-child` / `:last-child`.
    pub fn nth(&self, index: usize, count: usize) -> Self {
        self.with(|n| n.index = Some((index, count)))
    }

    /// The interaction state iced reports for a button.
    pub fn status(&self, status: button::Status) -> Self {
        self.with(|n| {
            n.hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
            n.pressed = status == button::Status::Pressed;
            n.disabled = status == button::Status::Disabled;
        })
    }

    /// The interaction state iced reports for a slider: dragged is
    /// `:active`.
    pub fn slider_status(&self, status: slider::Status) -> Self {
        self.with(|n| {
            n.hover = matches!(status, slider::Status::Hovered | slider::Status::Dragged);
            n.pressed = status == slider::Status::Dragged;
        })
    }

    /// The interaction state iced reports for a text input.
    pub fn input_status(&self, status: text_input::Status) -> Self {
        self.with(|n| {
            n.hover = matches!(
                status,
                text_input::Status::Hovered | text_input::Status::Focused { is_hovered: true }
            );
            n.focused = matches!(status, text_input::Status::Focused { .. });
            n.disabled = status == text_input::Status::Disabled;
        })
    }

    pub fn parent(&self) -> Option<&Node> {
        self.0.parent.as_ref()
    }

    pub fn kind(&self) -> &str {
        &self.0.kind
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.0.classes.iter().any(|c| c == class)
    }

    pub fn has_id(&self, id: &str) -> bool {
        self.0.id.as_deref() == Some(id)
    }

    pub fn attr_is(&self, name: &str, value: &str) -> bool {
        self.0.attrs.iter().any(|(n, v)| *n == name && v == value)
    }

    pub fn is_root(&self) -> bool {
        self.0.parent.is_none()
    }

    pub fn is_first_child(&self) -> bool {
        self.0.index.is_some_and(|(i, _)| i == 0)
    }

    pub fn is_last_child(&self) -> bool {
        self.0.index.is_some_and(|(i, n)| i + 1 == n)
    }

    /// Position among siblings, 0-based, when known.
    pub fn child_index(&self) -> Option<usize> {
        self.0.index.map(|(i, _)| i)
    }

    pub fn is_hover(&self) -> bool {
        self.0.hover
    }

    pub fn is_pressed(&self) -> bool {
        self.0.pressed
    }

    pub fn is_focused(&self) -> bool {
        self.0.focused
    }

    pub fn is_disabled(&self) -> bool {
        self.0.disabled
    }

    /// Root first, `self` last.
    pub fn path(&self) -> Vec<&Node> {
        let mut path = vec![self];
        let mut cur = self;
        while let Some(p) = cur.parent() {
            path.push(p);
            cur = p;
        }
        path.reverse();
        path
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(p) = self.parent() {
            write!(f, "{p:?} > ")?;
        }
        f.write_str(&self.0.kind)?;
        if let Some(id) = &self.0.id {
            write!(f, "#{id}")?;
        }
        for c in &self.0.classes {
            write!(f, ".{c}")?;
        }
        for (k, v) in &self.0.attrs {
            write!(f, "[{k}={v:?}]")?;
        }
        if let Some((i, n)) = self.0.index {
            write!(f, ":nth-child({})", i + 1)?;
            if i + 1 == n {
                f.write_str(":last-child")?;
            }
        }
        if self.0.hover {
            f.write_str(":hover")?;
        }
        if self.0.pressed {
            f.write_str(":active")?;
        }
        if self.0.focused {
            f.write_str(":focus")?;
        }
        if self.0.disabled {
            f.write_str(":disabled")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn building_and_querying() {
        let panel = Node::root("panel")
            .class("top")
            .id("2")
            .attr("output", "DP-1");
        let gadget = panel
            .child("slot")
            .class("end")
            .child("gadget")
            .class("clock")
            .nth(1, 2);
        assert_eq!(gadget.kind(), "gadget");
        assert!(gadget.has_class("clock"));
        assert!(!gadget.has_class("top"));
        assert!(gadget.is_last_child() && !gadget.is_first_child());
        assert!(panel.is_root() && !gadget.is_root());
        assert!(panel.has_id("2") && panel.attr_is("output", "DP-1"));
        assert_eq!(gadget.path().len(), 3);
        assert_eq!(
            format!("{gadget:?}"),
            "panel#2.top[output=\"DP-1\"] > slot.end > gadget.clock:nth-child(2):last-child"
        );

        let hovered = gadget.status(button::Status::Hovered);
        assert!(hovered.is_hover() && !hovered.is_pressed());
        assert!(!gadget.is_hover(), "status() doesn't touch the original");
        let pressed = gadget.status(button::Status::Pressed);
        assert!(pressed.is_hover() && pressed.is_pressed());
    }
}
