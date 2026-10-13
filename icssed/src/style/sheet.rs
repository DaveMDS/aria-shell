//! A sheet: the toolkit's base stylesheet, then the app's sources (its
//! own base, a user theme), one cascade compiled for one colour scheme,
//! and the caches of what it has resolved so far.

use std::cell::RefCell;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::computed::Computed;
use super::keyframes::{Frame, Keyframes};
use super::node::{
    Ancestors, Name, Node, NodeData, NodeId, Nodes, Sensitivity, States, Step, name,
};
use super::parse::{self, Loc};
use super::selector::{Ctx, Nth, Selector};
use super::value::{self, Assets, Decl, Moving};

/// The toolkit's base stylesheet: every element, class and property,
/// documented, with neutral defaults. Always first.
const BASE: &str = include_str!("../../assets/base.css");

/// A stylesheet to load: a name for the messages, its text, and the
/// directory its relative `url()`s are found in.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    pub name: &'a str,
    pub text: &'a str,
    pub dir: Option<&'a Path>,
}

/// The colour scheme a sheet is compiled for: the variables of
/// `:root.light { }` or `:root.dark { }` go over the plain `:root` ones,
/// and every root element carries it as a class (`menu.dark { }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Scheme {
    #[default]
    Light,
    Dark,
}

impl Scheme {
    /// Its class, and its name in a config.
    pub fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }
}

/// Something a sheet skipped: a selector or a declaration it can't use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub source: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.source, self.line, self.column, self.message
        )
    }
}

/// A source that couldn't be loaded at all: a syntax error. The caller
/// keeps its last good sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub source: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.source, self.line, self.column, self.message
        )
    }
}

impl std::error::Error for Error {}

pub(crate) struct Rule {
    /// 0 for the toolkit's base stylesheet, 1 for the app's sources: a
    /// rule of theirs wins over every rule of the base, whatever its
    /// specificity (CSS's user-agent and author origins).
    pub origin: u8,
    pub selector: Selector,
    pub decls: Arc<[Decl]>,
}

/// What grows as views ask: the names and nodes interned, the styles
/// resolved. Bounded by the shapes of the UI, not by its data.
pub(crate) struct Tree {
    pub nodes: Nodes,
    pub base: FxHashMap<NodeId, Arc<Computed>>,
    pub styles: FxHashMap<(NodeId, States, Ancestors), Arc<Computed>>,
    pub sensitivity: FxHashMap<NodeId, Sensitivity>,
}

pub struct Sheet {
    /// In cascade order: origin, specificity, then source order.
    pub(crate) rules: Vec<Rule>,
    /// Indices into `rules`, ascending, of those whose subject names a
    /// type, by that type: a node is only tried against its own type's
    /// and `any_kind`'s.
    pub(crate) by_kind: FxHashMap<super::node::Name, Vec<u32>>,
    /// Indices of the rules whose subject names no type.
    pub(crate) any_kind: Vec<u32>,
    /// Indices of the rules with a state somewhere: what makes a node
    /// sensitive.
    pub(crate) stateful: Vec<u32>,
    pub(crate) positional: Vec<Nth>,
    pub(crate) scheme: Scheme,
    pub(crate) scheme_class: super::node::Name,
    /// What a root inherits from.
    pub(crate) initial: Arc<Computed>,
    /// `@keyframes`, by name: the last one of a name wins.
    pub(crate) keyframes: FxHashMap<Arc<str>, Arc<Keyframes>>,
    pub(crate) tree: RefCell<Tree>,
}

impl Sheet {
    /// The toolkit's base stylesheet, then `sources` in cascade order,
    /// compiled for `scheme`. What can't be used is a warning and is
    /// skipped; a syntax error in any source is an error.
    pub fn load(sources: &[Source], scheme: Scheme) -> Result<(Sheet, Vec<Warning>), Error> {
        let light = Name::new(Scheme::Light.name());
        let dark = Name::new(Scheme::Dark.name());
        let mut positional = Vec::new();
        let mut warnings = Vec::new();
        let warning = |source: &str, at: Loc, message: String| Warning {
            source: source.to_string(),
            line: at.line,
            column: at.column,
            message,
        };

        let base = Source {
            name: "icssed:base.css",
            text: BASE,
            dir: None,
        };
        let mut files = Vec::new();
        for source in std::iter::once(&base).chain(sources) {
            let mut ctx = Ctx {
                positional: &mut positional,
            };
            let mut found = Vec::new();
            let file = parse::parse(source.text, &mut ctx, light, dark, &mut found).map_err(
                |(at, message)| Error {
                    source: source.name.to_string(),
                    line: at.line,
                    column: at.column,
                    message,
                },
            )?;
            warnings.extend(found.into_iter().map(|(at, m)| warning(source.name, at, m)));
            files.push((source.name, source.dir, file));
        }

        // Later files override earlier variables, and every file sees
        // the final set. Within a file the scheme's go over the plain
        // ones; a later file's plain ones still win over an earlier
        // file's scheme ones.
        let slot = match scheme {
            Scheme::Light => 1,
            Scheme::Dark => 2,
        };
        let mut vars: FxHashMap<String, String> = FxHashMap::default();
        for (_, _, file) in &files {
            for (name, value) in file.vars[0].iter().chain(&file.vars[slot]) {
                vars.insert(name.clone(), value.clone());
            }
        }

        let mut rules = Vec::new();
        let mut keyframes = FxHashMap::default();
        let mut assets = Assets {
            dir: None,
            loaded: FxHashMap::default(),
        };
        for (index, (source, dir, file)) in files.into_iter().enumerate() {
            let origin = u8::from(index > 0);
            assets.dir = dir.map(Path::to_path_buf);
            for raw in file.keyframes {
                let mut frames = Vec::new();
                for (offsets, raw_decls) in raw.frames {
                    let mut decls = Vec::new();
                    for d in raw_decls {
                        let parsed = parse::substitute(&d.value, &vars)
                            .and_then(|value| value::parse(&d.name, &value, &mut assets));
                        match parsed {
                            Ok(longhands) => {
                                if longhands.iter().any(|l| Moving::of(l) == 0) {
                                    warnings.push(warning(
                                        source,
                                        d.location,
                                        format!(
                                            "`{}` doesn't move in `@keyframes`: colours, opacity, shadows, transforms only, skipped",
                                            d.name
                                        ),
                                    ));
                                }
                                decls.extend(longhands.into_iter().filter(|l| Moving::of(l) != 0));
                            }
                            Err(message) => warnings.push(warning(
                                source,
                                d.location,
                                format!("`{}`: {message}, skipped", d.name),
                            )),
                        }
                    }
                    let sets = decls.iter().fold(0, |set, d| set | Moving::of(d));
                    let decls: Arc<[Decl]> = decls.into();
                    for offset in offsets {
                        frames.push(Frame {
                            offset,
                            decls: decls.clone(),
                            sets,
                        });
                    }
                }
                frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
                let name: Arc<str> = raw.name.into();
                keyframes.insert(
                    name.clone(),
                    Arc::new(Keyframes {
                        sets: frames.iter().fold(0, |set, f| set | f.sets),
                        name,
                        frames,
                    }),
                );
            }
            for raw in file.rules {
                let mut decls = Vec::new();
                for d in raw.decls {
                    let parsed = parse::substitute(&d.value, &vars)
                        .and_then(|value| value::parse(&d.name, &value, &mut assets));
                    match parsed {
                        Ok(longhands) => decls.extend(longhands),
                        Err(message) => warnings.push(warning(
                            source,
                            d.location,
                            format!("`{}`: {message}, skipped", d.name),
                        )),
                    }
                }
                if decls.is_empty() {
                    continue;
                }
                let decls: Arc<[Decl]> = decls.into();
                for selector in raw.selectors {
                    rules.push(Rule {
                        origin,
                        selector,
                        decls: decls.clone(),
                    });
                }
            }
        }
        // Stable: equal origin and specificity keep source order.
        rules.sort_by_key(|r| (r.origin, r.selector.specificity()));
        // Each source's in its own order, sources in theirs.
        let order: Vec<&str> = std::iter::once(base.name)
            .chain(sources.iter().map(|s| s.name))
            .collect();
        warnings.sort_by_key(|w| {
            let source = order.iter().position(|s| *s == w.source);
            (source, w.line, w.column)
        });

        let mut by_kind: FxHashMap<_, Vec<u32>> = FxHashMap::default();
        let mut any_kind = Vec::new();
        let mut stateful = Vec::new();
        for (i, rule) in rules.iter().enumerate() {
            let i = i as u32;
            match rule.selector.subject_kind() {
                Some(kind) => by_kind.entry(kind).or_default().push(i),
                None => any_kind.push(i),
            }
            if rule
                .selector
                .compounds()
                .iter()
                .any(|c| !c.states.is_empty())
            {
                stateful.push(i);
            }
        }

        let sheet = Sheet {
            rules,
            by_kind,
            any_kind,
            stateful,
            positional,
            scheme,
            scheme_class: match scheme {
                Scheme::Light => light,
                Scheme::Dark => dark,
            },
            initial: Arc::new(Computed::initial()),
            keyframes,
            tree: RefCell::new(Tree {
                nodes: Nodes::default(),
                base: FxHashMap::default(),
                styles: FxHashMap::default(),
                sensitivity: FxHashMap::default(),
            }),
        };
        Ok((sheet, warnings))
    }

    /// The root element of a surface, `surface.<name>`, at `scale`
    /// device pixels per logical one (the output's scale times the
    /// UI's): every surface the same type, told apart by its class.
    pub fn root(&self, name: &str, scale: f32) -> Node<'_> {
        let id = self.extend(None, Step::Root(name!("surface"), scale.to_bits()));
        Node::new(self, id).class(name)
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// How many rules it has, and how many nodes and styles it has
    /// interned and resolved so far.
    pub fn stats(&self) -> Stats {
        let tree = self.tree.borrow();
        Stats {
            rules: self.rules.len(),
            nodes: tree.nodes.len(),
            styles: tree.base.len() + tree.styles.len(),
        }
    }

    pub(crate) fn extend(&self, prev: Option<NodeId>, step: Step) -> NodeId {
        self.tree.borrow_mut().nodes.intern(prev, step)
    }

    pub(crate) fn node(&self, id: NodeId) -> NodeData {
        *self.tree.borrow().nodes.get(id)
    }

    pub(crate) fn path(&self, id: NodeId) -> String {
        self.tree.borrow().nodes.path(id)
    }

    /// Which of the positional pseudo-classes the child at `index` of
    /// `count` matches, as bits.
    pub(crate) fn position(&self, index: usize, count: usize) -> u32 {
        self.positional
            .iter()
            .enumerate()
            .filter(|(_, nth)| nth.matches(index, count))
            .fold(0, |bits, (i, _)| bits | 1 << i)
    }
}

/// What a sheet holds, in numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub rules: usize,
    pub nodes: usize,
    pub styles: usize,
}
