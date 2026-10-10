//! The element tree: interned names, nodes as `Copy` handles to interned
//! paths, the states an element can be in.

use std::fmt;
use std::ops::{BitAnd, BitOr, BitOrAssign, BitXor};
use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::computed::Computed;
use super::sheet::Sheet;

/// An interned name: an element type, a class, an id, an attribute's
/// name or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Name(u32);

#[derive(Default)]
pub(crate) struct Names {
    by_text: FxHashMap<Box<str>, Name>,
    texts: Vec<Box<str>>,
}

impl Names {
    pub fn intern(&mut self, text: &str) -> Name {
        if let Some(&name) = self.by_text.get(text) {
            return name;
        }
        let name = Name(self.texts.len() as u32);
        self.texts.push(text.into());
        self.by_text.insert(text.into(), name);
        name
    }

    pub fn text(&self, name: Name) -> &str {
        &self.texts[name.0 as usize]
    }
}

/// The interaction states of an element, as `:hover`, `:active` and
/// `:focus` match them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct States(u8);

impl States {
    pub const NONE: Self = Self(0);
    pub const HOVER: Self = Self(1);
    pub const ACTIVE: Self = Self(2);
    pub const FOCUS: Self = Self(4);

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Every state of `other` is in `self`.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// `self` with `other` set or cleared.
    pub fn with(self, other: Self, on: bool) -> Self {
        if on {
            Self(self.0 | other.0)
        } else {
            Self(self.0 & !other.0)
        }
    }

    pub(crate) fn count(self) -> u32 {
        self.0.count_ones()
    }
}

impl BitOr for States {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitAnd for States {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

/// The states in one and not the other: what changed.
impl BitXor for States {
    type Output = Self;

    fn bitxor(self, other: Self) -> Self {
        Self(self.0 ^ other.0)
    }
}

impl BitOrAssign for States {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// The states of a node's ancestors that aren't at rest, by element
/// depth, outermost first: what a descendant's style may depend on.
/// Inline, so that a copy allocates nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Ancestors {
    len: u8,
    /// Unused slots stay at their default, so that equal sets hash
    /// alike.
    items: [(u16, States); Ancestors::MAX],
}

impl Ancestors {
    /// Ancestors not at rest at once, at most: past it the outermost
    /// goes.
    pub const MAX: usize = 8;

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn items(&self) -> &[(u16, States)] {
        &self.items[..self.len as usize]
    }

    /// The states of the ancestor at `depth`.
    pub fn get(&self, depth: u16) -> States {
        self.items()
            .iter()
            .find(|(d, _)| *d == depth)
            .map_or(States::NONE, |(_, s)| *s)
    }

    /// With the element at `depth` in `states`, below every one there:
    /// what a parent hands its children. An element at rest isn't kept.
    pub fn push(&self, depth: u16, states: States) -> Self {
        let mut out = self.above(depth);
        if states.is_empty() {
            return out;
        }
        if out.len as usize == Self::MAX {
            out.items.copy_within(1.., 0);
            out.len -= 1;
        }
        out.items[out.len as usize] = (depth, states);
        out.len += 1;
        out
    }

    /// Only those above `depth`: what the ancestor at `depth` itself is
    /// under.
    pub fn above(&self, depth: u16) -> Self {
        let mut out = Self::default();
        for &(d, s) in self.items() {
            if d < depth {
                out.items[out.len as usize] = (d, s);
                out.len += 1;
            }
        }
        out
    }
}

/// Which states of an element some rule can see: those its own style
/// depends on, and those its descendants' do. An element tracks only
/// these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sensitivity {
    pub own: States,
    pub descendants: States,
}

impl Sensitivity {
    pub fn any(self) -> States {
        self.own | self.descendants
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NodeId(u32);

/// One step of a path: an element, or something about the last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Step {
    /// A surface's root element, and its scale (`f32` bits).
    Root(Name, u32),
    Kind(Name),
    Class(Name),
    Id(Name),
    Attr(Name, Name),
    /// Which of the sheet's positional pseudo-classes the element
    /// matches, as bits.
    Position(u32),
    Disabled,
}

#[derive(Clone, Copy)]
pub(crate) struct NodeData {
    prev: Option<NodeId>,
    step: Step,
    /// The node of the parent element, `None` for a root.
    pub parent: Option<NodeId>,
    /// The element's depth, 0 for a root.
    pub depth: u16,
    pub scale: f32,
    /// The element is `:disabled`.
    pub disabled: bool,
}

/// An element of a path, as selectors match it.
#[derive(Debug, Clone)]
pub(crate) struct Element {
    pub kind: Name,
    pub id: Option<Name>,
    pub classes: Vec<Name>,
    pub attrs: Vec<(Name, Name)>,
    pub position: u32,
    pub disabled: bool,
}

impl Element {
    fn new(kind: Name) -> Self {
        Self {
            kind,
            id: None,
            classes: Vec::new(),
            attrs: Vec::new(),
            position: 0,
            disabled: false,
        }
    }
}

/// The interned paths: a node is a chain of steps with a canonical id.
#[derive(Default)]
pub(crate) struct Nodes {
    data: Vec<NodeData>,
    index: FxHashMap<(u32, Step), NodeId>,
}

impl Nodes {
    pub fn intern(&mut self, prev: Option<NodeId>, step: Step) -> NodeId {
        let key = (prev.map_or(u32::MAX, |p| p.0), step);
        if let Some(&id) = self.index.get(&key) {
            return id;
        }
        let (parent, depth, scale, disabled) = match (prev, step) {
            (None, Step::Root(_, scale)) => (None, 0, f32::from_bits(scale), false),
            (Some(p), Step::Kind(_)) => {
                let d = self.get(p);
                (Some(p), d.depth + 1, d.scale, false)
            }
            (Some(_), Step::Root(..)) | (None, _) => {
                unreachable!("a path starts at a root, and only there")
            }
            (Some(p), _) => {
                let d = self.get(p);
                (
                    d.parent,
                    d.depth,
                    d.scale,
                    d.disabled || step == Step::Disabled,
                )
            }
        };
        let id = NodeId(self.data.len() as u32);
        self.data.push(NodeData {
            prev,
            step,
            parent,
            depth,
            scale,
            disabled,
        });
        self.index.insert(key, id);
        id
    }

    pub fn get(&self, id: NodeId) -> &NodeData {
        &self.data[id.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    fn steps(&self, id: NodeId) -> Vec<Step> {
        let mut steps = Vec::new();
        let mut cur = Some(id);
        while let Some(i) = cur {
            let d = self.get(i);
            steps.push(d.step);
            cur = d.prev;
        }
        steps.reverse();
        steps
    }

    /// The elements of a node's path, root first; the root carries the
    /// scheme as a class.
    pub fn chain(&self, id: NodeId, scheme: Name) -> Vec<Element> {
        let mut chain: Vec<Element> = Vec::new();
        for step in self.steps(id) {
            match step {
                Step::Root(kind, _) => {
                    let mut root = Element::new(kind);
                    root.classes.push(scheme);
                    chain.push(root);
                }
                Step::Kind(kind) => chain.push(Element::new(kind)),
                step => {
                    let last = chain.last_mut().expect("a path starts at a root");
                    match step {
                        Step::Class(c) => last.classes.push(c),
                        Step::Id(i) => last.id = Some(i),
                        Step::Attr(n, v) => last.attrs.push((n, v)),
                        Step::Position(bits) => last.position = bits,
                        Step::Disabled => last.disabled = true,
                        Step::Root(..) | Step::Kind(_) => unreachable!(),
                    }
                }
            }
        }
        chain
    }

    /// The path in selector syntax, for messages and debugging.
    pub fn path(&self, id: NodeId, names: &Names) -> String {
        let mut out = String::new();
        for step in self.steps(id) {
            match step {
                Step::Root(kind, _) => out.push_str(names.text(kind)),
                Step::Kind(kind) => {
                    out.push_str(" > ");
                    out.push_str(names.text(kind));
                }
                Step::Class(c) => {
                    out.push('.');
                    out.push_str(names.text(c));
                }
                Step::Id(i) => {
                    out.push('#');
                    out.push_str(names.text(i));
                }
                Step::Attr(n, v) => {
                    out.push_str(&format!("[{}=\"{}\"]", names.text(n), names.text(v)));
                }
                Step::Position(_) => {}
                Step::Disabled => out.push_str(":disabled"),
            }
        }
        out
    }
}

/// A widget's place in the element tree: `Copy`, a reference to the
/// sheet and an interned path. A view builds them top-down, from a
/// surface's root: `root.child("menu").child("item").class_if("active",
/// on)`.
#[derive(Clone, Copy)]
pub struct Node<'a> {
    sheet: &'a Sheet,
    id: NodeId,
}

impl<'a> Node<'a> {
    pub(crate) fn new(sheet: &'a Sheet, id: NodeId) -> Self {
        Self { sheet, id }
    }

    fn step(self, make: impl FnOnce(&mut Names) -> Step) -> Self {
        Self {
            sheet: self.sheet,
            id: self.sheet.extend(Some(self.id), make),
        }
    }

    /// A child element of type `kind`.
    pub fn child(self, kind: &str) -> Self {
        self.step(|names| Step::Kind(names.intern(kind)))
    }

    /// The child of type `kind` at `index` of `count`, for
    /// `:first-child`, `:last-child` and `:nth-child()`. Where the sheet
    /// has none of them, the same as [`Node::child`]: rows that look the
    /// same share one node.
    pub fn child_at(self, kind: &str, index: usize, count: usize) -> Self {
        let child = self.child(kind);
        match self.sheet.position(index, count) {
            0 => child,
            bits => child.step(|_| Step::Position(bits)),
        }
    }

    pub fn class(self, class: &str) -> Self {
        self.step(|names| Step::Class(names.intern(class)))
    }

    pub fn class_if(self, class: &str, on: bool) -> Self {
        if on { self.class(class) } else { self }
    }

    pub fn id(self, id: &str) -> Self {
        self.step(|names| Step::Id(names.intern(id)))
    }

    pub fn attr(self, name: &str, value: &str) -> Self {
        self.step(|names| Step::Attr(names.intern(name), names.intern(value)))
    }

    /// `:disabled`: known where the widget is built, so part of the path
    /// (and of its descendants').
    pub fn disabled_if(self, disabled: bool) -> Self {
        if disabled {
            self.step(|_| Step::Disabled)
        } else {
            self
        }
    }

    /// The style at rest: no state of its own, none of its ancestors'.
    pub fn base(self) -> Arc<Computed> {
        self.sheet.base(self.id)
    }

    /// The style in `own` states, under `ancestors`.
    pub fn resolve(self, own: States, ancestors: &Ancestors) -> Arc<Computed> {
        self.sheet.resolve(self.id, own, ancestors)
    }

    pub fn sensitivity(self) -> Sensitivity {
        self.sheet.sensitivity(self.id)
    }

    /// The element's depth: 0 for a root.
    pub fn depth(self) -> u16 {
        self.sheet.node(self.id).depth
    }

    /// The surface's device pixels per logical one.
    pub fn scale(self) -> f32 {
        self.sheet.node(self.id).scale
    }

    /// The element is `:disabled`.
    pub fn is_disabled(self) -> bool {
        self.sheet.node(self.id).disabled
    }

    pub fn sheet(self) -> &'a Sheet {
        self.sheet
    }

    /// Who it is, for a widget to tell when its node changed: its
    /// sheet, its path.
    pub(crate) fn key(self) -> (usize, u32) {
        (std::ptr::from_ref(self.sheet) as usize, self.id.0)
    }

    /// The path in selector syntax: `menu > item.active > label`.
    pub fn path(self) -> String {
        self.sheet.path(self.id)
    }
}

impl PartialEq for Node<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.sheet, other.sheet) && self.id == other.id
    }
}

impl fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.path())
    }
}
