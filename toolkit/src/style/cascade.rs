//! The cascade: a node's computed style, resolved once per distinct
//! (node, own states, ancestors' states) and shared; and which states of
//! a node any rule can see.

use std::sync::Arc;

use super::computed::{Cascade, Computed};
use super::node::{Ancestors, Name, NodeId, Sensitivity, States};
use super::selector::StateCheck;
use super::sheet::{Rule, Sheet};

impl Sheet {
    /// The style at rest.
    pub(crate) fn base(&self, id: NodeId) -> Arc<Computed> {
        if let Some(style) = self.tree.borrow().base.get(&id) {
            return style.clone();
        }
        let style = self.compute(id, States::NONE, &Ancestors::default());
        self.tree.borrow_mut().base.insert(id, style.clone());
        style
    }

    pub(crate) fn resolve(&self, id: NodeId, own: States, ancestors: &Ancestors) -> Arc<Computed> {
        if own.is_empty() && ancestors.is_empty() {
            return self.base(id);
        }
        let key = (id, own, *ancestors);
        if let Some(style) = self.tree.borrow().styles.get(&key) {
            return style.clone();
        }
        let style = self.compute(id, own, ancestors);
        self.tree.borrow_mut().styles.insert(key, style.clone());
        style
    }

    fn compute(&self, id: NodeId, own: States, ancestors: &Ancestors) -> Arc<Computed> {
        let node = self.node(id);
        let parent = match node.parent {
            // The parent, as it is: in its own states, under the
            // ancestors above it.
            Some(p) => {
                let depth = node.depth - 1;
                self.resolve(p, ancestors.get(depth), &ancestors.above(depth))
            }
            None => self.initial.clone(),
        };
        let chain = self.tree.borrow().nodes.chain(id, self.scheme_class);
        let kind = chain.last().expect("a node is an element").kind;
        let check = StateCheck::Check { own, ancestors };
        let mut cascade = Cascade::new(&parent);
        for rule in self.candidates(kind) {
            if rule.selector.matches(&chain, check) {
                for decl in rule.decls.iter() {
                    cascade.apply(decl);
                }
            }
        }
        let mut style = cascade.finish(node.scale);
        if style.animations.names.iter().any(Option::is_some) {
            style.animations.keyframes = style
                .animations
                .names
                .iter()
                .map(|name| name.as_ref().and_then(|n| self.keyframes.get(n).cloned()))
                .collect();
        }
        Arc::new(style)
    }

    /// The rules that may match an element of type `kind`, in cascade
    /// order: its type's, merged with those naming no type.
    fn candidates(&self, kind: Name) -> impl Iterator<Item = &Rule> {
        let typed = self.by_kind.get(&kind).map_or(&[][..], Vec::as_slice);
        let untyped = &self.any_kind[..];
        let (mut i, mut j) = (0, 0);
        std::iter::from_fn(move || {
            let next = match (typed.get(i), untyped.get(j)) {
                (Some(&a), Some(&b)) if a < b => {
                    i += 1;
                    a
                }
                (_, Some(&b)) => {
                    j += 1;
                    b
                }
                (Some(&a), None) => {
                    i += 1;
                    a
                }
                (None, None) => return None,
            };
            Some(&self.rules[next as usize])
        })
    }

    pub(crate) fn sensitivity(&self, id: NodeId) -> Sensitivity {
        if let Some(&s) = self.tree.borrow().sensitivity.get(&id) {
            return s;
        }
        let chain = self.tree.borrow().nodes.chain(id, self.scheme_class);
        let mut s = Sensitivity::default();
        for &i in &self.stateful {
            let selector = &self.rules[i as usize].selector;
            let subject = selector.compounds().len() - 1;
            for (k, compound) in selector.compounds().iter().enumerate() {
                if compound.states.is_empty() || !selector.matches_prefix(k, &chain) {
                    continue;
                }
                if k == subject {
                    s.own |= compound.states;
                } else {
                    s.descendants |= compound.states;
                }
            }
        }
        self.tree.borrow_mut().sensitivity.insert(id, s);
        s
    }
}
