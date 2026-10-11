//! Where a widget is in the element tree, found by the toolkit: a widget
//! is built with its type and its classes only, and takes its node from
//! its parent the first time the runtime reaches it (making its state,
//! diffing it), as a box in a document gets its place from the one it's
//! in. A container hands its node, and each child's position, to its
//! children as it reaches them; a widget at the top of a view is given
//! its node (`at`).

use std::borrow::Borrow;
use std::cell::{Cell, OnceCell};
use std::sync::Arc;

use iced::advanced::widget::Tree;

use crate::Element;
use crate::style::{Computed, Name, Node, NodeId, Sheet, Step};

/// What a container hands the child it's reaching: where it is, in the
/// sheet of the container's node.
#[derive(Clone, Copy)]
struct Parent {
    /// Borrowed by the container for as long as it lives, which is as
    /// long as its children do.
    sheet: *const Sheet,
    at: At,
}

#[derive(Clone, Copy)]
enum At {
    /// Under `node`, at `index` of `count`: a container's child.
    Under {
        node: NodeId,
        index: usize,
        count: usize,
    },
    /// It's `node` itself: a surface's content, its root box.
    Is(NodeId),
    /// A child of `node` beside its children, at no position among
    /// them: a tip, which floats over the rest.
    Aside(NodeId),
}

thread_local! {
    static PARENT: Cell<Option<Parent>> = const { Cell::new(None) };
}

fn reach<R>(parent: Parent, f: impl FnOnce() -> R) -> R {
    let outer = PARENT.replace(Some(parent));
    let out = f();
    PARENT.set(outer);
    out
}

/// `f`, with the child at `index` of `count` reached under `parent`.
fn within<R>(parent: Node<'_>, index: usize, count: usize, f: impl FnOnce() -> R) -> R {
    let (sheet, node) = parent.parts();
    let at = At::Under { node, index, count };
    reach(Parent { sheet, at }, f)
}

/// `f`, with the only child reached under `parent`.
pub(crate) fn under<R>(parent: Node<'_>, f: impl FnOnce() -> R) -> R {
    within(parent, 0, 1, f)
}

/// `f`, with a widget reached as `node` itself.
pub(crate) fn being<R>(node: Node<'_>, f: impl FnOnce() -> R) -> R {
    let (sheet, node) = node.parts();
    reach(
        Parent {
            sheet,
            at: At::Is(node),
        },
        f,
    )
}

/// `f`, with a widget reached beside the children of the element the
/// caller is in (the caller has no node: a tooltip, reached itself),
/// given that element's scale.
pub(crate) fn aside<R>(f: impl FnOnce(f32) -> R) -> R {
    let Some(here) = PARENT.get() else {
        panic!("a tooltip reached outside a container of the toolkit");
    };
    let (At::Under { node, .. } | At::Is(node) | At::Aside(node)) = here.at;
    // SAFETY: as `Place::node`'s.
    let scale = Node::new(unsafe { &*here.sheet }, node).scale();
    reach(
        Parent {
            sheet: here.sheet,
            at: At::Aside(node),
        },
        || f(scale),
    )
}

/// `f` for each of `count` children, the child at `index` reached under
/// `parent`.
fn each(parent: Node<'_>, count: usize, mut f: impl FnMut(usize)) {
    let (sheet, node) = parent.parts();
    let outer = PARENT.get();
    for index in 0..count {
        let at = At::Under { node, index, count };
        PARENT.set(Some(Parent { sheet, at }));
        f(index);
    }
    PARENT.set(outer);
}

/// The trees of a container's `children`, made under `parent`.
pub(crate) fn trees<'b, Message: 'b>(
    parent: Node<'_>,
    children: &[impl Borrow<Element<'b, Message>>],
) -> Vec<Tree> {
    let mut trees = Vec::with_capacity(children.len());
    each(parent, children.len(), |i| {
        trees.push(Tree::new(children[i].borrow().as_widget()));
    });
    trees
}

/// `tree`'s children diffed with a container's `children`, under
/// `parent`: iced's `Tree::diff_children`, each child knowing where it
/// is.
pub(crate) fn diff<'b, Message: 'b>(
    parent: Node<'_>,
    tree: &mut Tree,
    children: &[impl Borrow<Element<'b, Message>>],
) {
    tree.children.truncate(children.len());
    each(parent, children.len(), |i| {
        let child = children[i].borrow().as_widget();
        match tree.children.get_mut(i) {
            Some(tree) => tree.diff(child),
            None => tree.children.push(Tree::new(child)),
        }
    });
}

/// A widget's place: its type and the rest until it's reached, its node
/// and its style at rest after.
pub(crate) struct Place<'a> {
    kind: Name,
    /// Its classes, id, attributes, `:disabled`: the steps after its
    /// type.
    extras: Vec<Step>,
    node: Cell<Option<Node<'a>>>,
    style: OnceCell<Arc<Computed>>,
}

impl<'a> Place<'a> {
    /// A `kind` element, wherever it's put.
    pub fn new(kind: Name) -> Self {
        Self {
            kind,
            extras: Vec::new(),
            node: Cell::new(None),
            style: OnceCell::new(),
        }
    }

    /// It's `node`, with the classes it was given so far.
    pub fn pin(&mut self, node: Node<'a>) {
        let node = self.extras.drain(..).fold(node, Node::step);
        self.node.set(Some(node));
    }

    pub fn kind(&mut self, kind: Name) {
        debug_assert!(
            self.node.get().is_none(),
            "a type for a widget given its node"
        );
        self.kind = kind;
    }

    pub fn class(&mut self, class: &str) {
        self.extra(Step::Class(Name::new(class)));
    }

    pub fn id(&mut self, id: &str) {
        self.extra(Step::Id(Name::new(id)));
    }

    pub fn attr(&mut self, name: &str, value: &str) {
        self.extra(Step::Attr(Name::new(name), Name::new(value)));
    }

    pub fn disabled(&mut self) {
        self.extra(Step::Disabled);
    }

    fn extra(&mut self, step: Step) {
        match self.node.get_mut() {
            Some(node) => *node = node.step(step),
            None => self.extras.push(step),
        }
    }

    /// Its node: found under the parent reaching it, the first time.
    #[inline]
    pub fn node(&self) -> Node<'a> {
        match self.node.get() {
            Some(node) => node,
            None => self.find(),
        }
    }

    #[cold]
    fn find(&self) -> Node<'a> {
        let Some(parent) = PARENT.get() else {
            panic!(
                "`{}` reached outside a container of the toolkit: put it in one, or give it its node (`at`)",
                self.kind.text()
            );
        };
        // SAFETY: the sheet is the parent's node's, borrowed by the
        // parent for as long as it lives; the widget is the parent's
        // child, reached only through it, so it never outlives it.
        let sheet: &'a Sheet = unsafe { &*parent.sheet };
        let node = match parent.at {
            At::Under { node, index, count } => {
                Node::new(sheet, node).child_named(self.kind, Some((index, count)))
            }
            At::Is(node) => Node::new(sheet, node),
            At::Aside(node) => Node::new(sheet, node).child_named(self.kind, None),
        };
        let node = self.extras.iter().copied().fold(node, Node::step);
        self.node.set(Some(node));
        node
    }

    /// Its style at rest.
    #[inline]
    pub fn style(&self) -> &Arc<Computed> {
        self.style.get_or_init(|| self.node().base())
    }
}

/// The builder methods of a widget with a place: `$($path).+` reaches
/// its `Place`.
macro_rules! placed {
    ($($path:ident).+) => {
        /// Its element type, instead of the widget's own.
        pub fn kind(self, kind: &str) -> Self {
            self.kind_of($crate::style::Name::new(kind))
        }

        #[allow(dead_code)]
        pub(crate) fn kind_of(mut self, kind: $crate::style::Name) -> Self {
            self.$($path).+.kind(kind);
            self
        }

        pub fn class(mut self, class: &str) -> Self {
            self.$($path).+.class(class);
            self
        }

        pub fn class_if(self, class: &str, on: bool) -> Self {
            if on { self.class(class) } else { self }
        }

        /// Its `#id`.
        pub fn named(mut self, id: &str) -> Self {
            self.$($path).+.id(id);
            self
        }

        pub fn attr(mut self, name: &str, value: &str) -> Self {
            self.$($path).+.attr(name, value);
            self
        }

        /// `:disabled`.
        pub fn disabled_if(mut self, disabled: bool) -> Self {
            if disabled {
                self.$($path).+.disabled();
            }
            self
        }

        /// It's `node`, wherever it's put: the top of a view.
        pub fn at(mut self, node: $crate::Node<'a>) -> Self {
            self.$($path).+.pin(node);
            self
        }
    };
}

pub(crate) use placed;
