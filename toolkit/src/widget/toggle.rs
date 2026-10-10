//! A switch: a button holding its knob, `.on` when it is; a press, or
//! Enter or Space while it has the focus, says the other state. The
//! knob slides as the theme says: the toolkit's base moves it with a
//! transform, by the toggle's width less its padding and the knob's.

use super::flex::{Flex, button, row};
use crate::style::Node;

/// A switch `on` or off: `node` (`.on` when it is), its `knob`.
/// `on_toggle` makes what a press sends from the state it asks for.
pub fn toggle<'a, Message: Clone + 'a>(
    node: Node<'a>,
    on: bool,
    on_toggle: impl Fn(bool) -> Message,
) -> Flex<'a, Message> {
    let node = node.class_if("on", on);
    button(node, [row(node.child("knob"), []).into()]).on_press(on_toggle(!on))
}
