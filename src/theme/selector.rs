//! Selectors: parsing, specificity and matching against a [`Node`].
//!
//! Grammar (a CSS subset): a selector is compounds joined by ` `
//! (descendant) or `>` (child); a compound is an optional type or `*`,
//! then any of `.class`, `#id`, `[attr="value"]`, `:pseudo`. Supported
//! pseudo-classes: `:root :hover :active :disabled :first-child
//! :last-child`.

use super::node::Node;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    /// Left to right; the first compound's combinator is ignored.
    parts: Vec<(Combinator, Compound)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Compound {
    /// `None` is `*` or no type.
    kind: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, String)>,
    pseudo: Vec<Pseudo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pseudo {
    Root,
    Hover,
    Active,
    Disabled,
    FirstChild,
    LastChild,
}

impl Selector {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut parts = Vec::new();
        let mut combinator = Combinator::Descendant;
        let mut chars = text.chars().peekable();
        loop {
            // Whitespace between compounds is the descendant combinator
            // unless a `>` follows.
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    chars.next();
                } else if c == '>' {
                    if parts.is_empty() {
                        return Err("selector can't start with `>`".to_owned());
                    }
                    combinator = Combinator::Child;
                    chars.next();
                } else {
                    break;
                }
            }
            if chars.peek().is_none() {
                break;
            }
            let compound = parse_compound(&mut chars)?;
            parts.push((combinator, compound));
            combinator = Combinator::Descendant;
        }
        if parts.is_empty() {
            return Err("empty selector".to_owned());
        }
        Ok(Self { parts })
    }

    /// `(ids, classes+attrs+pseudo, types)`, CSS order.
    pub fn specificity(&self) -> (u32, u32, u32) {
        self.parts.iter().fold((0, 0, 0), |(a, b, c), (_, p)| {
            (
                a + u32::from(p.id.is_some()),
                b + (p.classes.len() + p.attrs.len() + p.pseudo.len()) as u32,
                c + u32::from(p.kind.is_some()),
            )
        })
    }

    pub fn matches(&self, node: &Node) -> bool {
        matches_from(&self.parts, node)
    }
}

/// Right-to-left: the last compound must match `node`, the rest its
/// ancestors as the combinators say. Descendant combinators backtrack.
fn matches_from(parts: &[(Combinator, Compound)], node: &Node) -> bool {
    let Some(((combinator, compound), rest)) = parts.split_last() else {
        return true;
    };
    if !compound.matches(node) {
        return false;
    }
    if rest.is_empty() {
        return true;
    }
    match combinator {
        Combinator::Child => node.parent().is_some_and(|p| matches_from(rest, p)),
        Combinator::Descendant => {
            let mut cur = node.parent();
            while let Some(p) = cur {
                if matches_from(rest, p) {
                    return true;
                }
                cur = p.parent();
            }
            false
        }
    }
}

impl Compound {
    fn matches(&self, node: &Node) -> bool {
        self.kind.as_deref().is_none_or(|k| k == node.kind())
            && self.id.as_deref().is_none_or(|id| node.has_id(id))
            && self.classes.iter().all(|c| node.has_class(c))
            && self.attrs.iter().all(|(k, v)| node.attr_is(k, v))
            && self.pseudo.iter().all(|p| match p {
                Pseudo::Root => node.is_root(),
                Pseudo::Hover => node.is_hover(),
                Pseudo::Active => node.is_pressed(),
                Pseudo::Disabled => node.is_disabled(),
                Pseudo::FirstChild => node.is_first_child(),
                Pseudo::LastChild => node.is_last_child(),
            })
    }
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

fn take_name(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut name = String::new();
    while let Some(&c) = chars.peek() {
        if is_name_char(c) {
            name.push(c);
            chars.next();
        } else {
            break;
        }
    }
    name
}

fn parse_compound(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<Compound, String> {
    let mut compound = Compound::default();
    let mut empty = true;
    if chars.peek() == Some(&'*') {
        chars.next();
        empty = false;
    } else if chars.peek().is_some_and(|&c| is_name_char(c)) {
        compound.kind = Some(take_name(chars));
        empty = false;
    }
    while let Some(&c) = chars.peek() {
        match c {
            '.' => {
                chars.next();
                let name = take_name(chars);
                if name.is_empty() {
                    return Err("expected a class name after `.`".to_owned());
                }
                compound.classes.push(name);
            }
            '#' => {
                chars.next();
                let name = take_name(chars);
                if name.is_empty() {
                    return Err("expected an id after `#`".to_owned());
                }
                if compound.id.replace(name).is_some() {
                    return Err("more than one #id in a compound".to_owned());
                }
            }
            ':' => {
                chars.next();
                let name = take_name(chars);
                compound.pseudo.push(match name.as_str() {
                    "root" => Pseudo::Root,
                    "hover" => Pseudo::Hover,
                    "active" => Pseudo::Active,
                    "disabled" => Pseudo::Disabled,
                    "first-child" => Pseudo::FirstChild,
                    "last-child" => Pseudo::LastChild,
                    _ => return Err(format!("unsupported pseudo-class :{name}")),
                });
            }
            '[' => {
                chars.next();
                let name = take_name(chars);
                if name.is_empty() || chars.next() != Some('=') {
                    return Err("expected `[name=\"value\"]`".to_owned());
                }
                let mut value = String::new();
                let quote = chars.peek().copied().filter(|q| *q == '"' || *q == '\'');
                if quote.is_some() {
                    chars.next();
                }
                loop {
                    match chars.next() {
                        Some(c) if Some(c) == quote => break,
                        Some(']') if quote.is_none() => break,
                        Some(c) => value.push(c),
                        None => return Err("unterminated attribute selector".to_owned()),
                    }
                }
                if quote.is_some() && chars.next() != Some(']') {
                    return Err("expected `]`".to_owned());
                }
                compound.attrs.push((name, value));
            }
            _ => break,
        }
        empty = false;
    }
    if empty {
        return Err(format!(
            "unexpected `{}` in selector",
            chars.peek().copied().unwrap_or(' ')
        ));
    }
    Ok(compound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::button;

    fn sel(s: &str) -> Selector {
        Selector::parse(s).unwrap_or_else(|e| panic!("{s:?}: {e}"))
    }

    fn tree() -> (Node, Node, Node) {
        let panel = Node::root("panel")
            .class("top")
            .id("2")
            .attr("output", "DP-1");
        let slot = panel.child("slot").class("start");
        let ws = slot
            .child("gadget")
            .class("workspaces")
            .nth(0, 2)
            .child("workspace")
            .class("active");
        (panel, slot, ws)
    }

    #[test]
    fn simple_and_compound() {
        let (panel, slot, ws) = tree();
        assert!(sel("panel").matches(&panel));
        assert!(sel("*").matches(&slot));
        assert!(sel(".top").matches(&panel));
        assert!(sel("panel.top#2").matches(&panel));
        assert!(!sel("panel.bottom").matches(&panel));
        assert!(!sel("panel#3").matches(&panel));
        assert!(sel("workspace.active").matches(&ws));
        assert!(!sel("workspace.urgent").matches(&ws));
        assert!(sel("[output=\"DP-1\"]").matches(&panel));
        assert!(sel("panel[output=DP-1]").matches(&panel));
        assert!(!sel("panel[output='HDMI-A-1']").matches(&panel));
    }

    #[test]
    fn combinators() {
        let (panel, slot, ws) = tree();
        assert!(sel("panel workspace").matches(&ws));
        assert!(sel("panel > slot").matches(&slot));
        assert!(!sel("panel > workspace").matches(&ws));
        assert!(sel("slot.start gadget.workspaces > workspace").matches(&ws));
        assert!(!sel("slot.end workspace").matches(&ws));
        assert!(!sel("slot").matches(&panel));
        assert!(sel("panel>slot").matches(&slot), "no spaces around >");
    }

    #[test]
    fn pseudo_classes() {
        let (panel, _, ws) = tree();
        let gadget = ws.parent().unwrap();
        assert!(sel(":root").matches(&panel));
        assert!(!sel(":root").matches(&ws));
        assert!(sel("gadget:first-child").matches(gadget));
        assert!(!sel("gadget:last-child").matches(gadget));
        assert!(!sel("workspace:hover").matches(&ws));
        assert!(sel("workspace:hover").matches(&ws.status(button::Status::Hovered)));
        assert!(sel("workspace:active").matches(&ws.status(button::Status::Pressed)));
        assert!(sel("workspace:hover").matches(&ws.status(button::Status::Pressed)));
        assert!(sel("workspace:disabled").matches(&ws.status(button::Status::Disabled)));
        assert!(!sel("workspace:hover").matches(&ws.status(button::Status::Active)));
    }

    #[test]
    fn specificity() {
        assert_eq!(sel("*").specificity(), (0, 0, 0));
        assert_eq!(sel("panel").specificity(), (0, 0, 1));
        assert_eq!(sel(".a.b").specificity(), (0, 2, 0));
        assert_eq!(sel("#x").specificity(), (1, 0, 0));
        assert_eq!(
            sel("panel > slot.start gadget:hover").specificity(),
            (0, 2, 3)
        );
        assert!(sel("#x").specificity() > sel(".a.b.c.d").specificity());
    }

    #[test]
    fn parse_errors() {
        for bad in ["", "> a", "a.", "a#", "a:nope", "a[b", "a[b=\"c\"", "a b {"] {
            assert!(Selector::parse(bad).is_err(), "{bad:?} should fail");
        }
    }
}
