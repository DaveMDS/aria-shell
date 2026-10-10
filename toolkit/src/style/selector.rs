//! Selectors: parsed from a rule's prelude, matched against the elements
//! of a node's path.
//!
//! Type, `.class`, `#id`, `[attr]`, `[attr="value"]`, `*`, descendant
//! and child (`>`) combinators; `:hover :active :focus :disabled :root
//! :first-child :last-child :nth-child() :nth-last-child()`.

use cssparser::{ParseError, Parser, ToCss, Token};

use super::node::{Ancestors, Element, Name, Names, States};
use super::value::{Error, describe};

fn fail<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(ParseError::custom(message.into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Combinator {
    Descendant,
    Child,
}

#[derive(Debug, Clone)]
pub(crate) struct Attr {
    name: Name,
    /// `None`: present, whatever its value.
    value: Option<Name>,
}

/// The simple selectors of one element.
#[derive(Debug, Clone, Default)]
pub(crate) struct Compound {
    /// `None` for `*`, or no type at all.
    pub kind: Option<Name>,
    ids: Vec<Name>,
    classes: Vec<Name>,
    attrs: Vec<Attr>,
    pub states: States,
    disabled: bool,
    root: bool,
    /// Bits of the sheet's positional pseudo-classes it asks for.
    positions: u32,
}

impl Compound {
    fn matches(&self, element: &Element, depth: usize, states: Option<States>) -> bool {
        self.kind.is_none_or(|k| k == element.kind)
            && self.ids.iter().all(|i| element.id == Some(*i))
            && self.classes.iter().all(|c| element.classes.contains(c))
            && self.attrs.iter().all(|a| {
                element
                    .attrs
                    .iter()
                    .any(|(n, v)| *n == a.name && a.value.is_none_or(|a| a == *v))
            })
            && (!self.root || depth == 0)
            && (!self.disabled || element.disabled)
            && self.positions & !element.position == 0
            && states.is_none_or(|s| s.contains(self.states))
    }

    fn specificity(&self) -> u32 {
        let ids = self.ids.len() as u32;
        let classes = self.classes.len() as u32
            + self.attrs.len() as u32
            + self.states.count()
            + self.disabled as u32
            + self.root as u32
            + self.positions.count_ones();
        let types = self.kind.is_some() as u32;
        (ids << 20) | (classes << 10) | types
    }
}

/// The states selectors are matched with.
#[derive(Clone, Copy)]
pub(crate) enum StateCheck<'a> {
    /// Structure only.
    Ignore,
    Check {
        own: States,
        ancestors: &'a Ancestors,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Selector {
    /// Left to right: the subject last.
    compounds: Vec<Compound>,
    /// `combinators[i]` joins `compounds[i]` and `compounds[i + 1]`.
    combinators: Vec<Combinator>,
}

impl Selector {
    /// Ids, then classes (attributes and pseudo-classes alike), then
    /// types, packed so that a plain comparison orders them.
    pub fn specificity(&self) -> u32 {
        self.compounds.iter().map(Compound::specificity).sum()
    }

    pub fn compounds(&self) -> &[Compound] {
        &self.compounds
    }

    /// The type its subject names, if it names one.
    pub fn subject_kind(&self) -> Option<Name> {
        self.compounds.last().and_then(|c| c.kind)
    }

    /// Whether the last element of `chain` (root first) is its subject.
    pub fn matches(&self, chain: &[Element], check: StateCheck) -> bool {
        !chain.is_empty()
            && self.matches_at(self.compounds.len() - 1, chain, chain.len() - 1, check)
    }

    /// Whether compound `k`, with those before it, matches the last
    /// element of `chain`, states aside: whether some rule's state there
    /// can matter to that element or below it.
    pub fn matches_prefix(&self, k: usize, chain: &[Element]) -> bool {
        !chain.is_empty() && self.matches_at(k, chain, chain.len() - 1, StateCheck::Ignore)
    }

    fn matches_at(&self, k: usize, chain: &[Element], i: usize, check: StateCheck) -> bool {
        let states = match check {
            StateCheck::Ignore => None,
            StateCheck::Check { own, ancestors } => Some(if i == chain.len() - 1 {
                own
            } else {
                ancestors.get(i as u16)
            }),
        };
        if !self.compounds[k].matches(&chain[i], i, states) {
            return false;
        }
        if k == 0 {
            return true;
        }
        match self.combinators[k - 1] {
            Combinator::Child => i > 0 && self.matches_at(k - 1, chain, i - 1, check),
            Combinator::Descendant => (0..i)
                .rev()
                .any(|j| self.matches_at(k - 1, chain, j, check)),
        }
    }

    /// `:root`, `:root.light` or `:root.dark` and nothing else: where
    /// variables are declared. 0 for the plain one, 1 for light, 2 for
    /// dark.
    pub fn variable_scope(&self, light: Name, dark: Name) -> Option<usize> {
        let [c] = &self.compounds[..] else {
            return None;
        };
        let plain_root = c.root
            && c.kind.is_none()
            && c.ids.is_empty()
            && c.attrs.is_empty()
            && c.states.is_empty()
            && !c.disabled
            && c.positions == 0;
        if !plain_root {
            return None;
        }
        match &c.classes[..] {
            [] => Some(0),
            [class] if *class == light => Some(1),
            [class] if *class == dark => Some(2),
            _ => None,
        }
    }
}

/// `:nth-child(an+b)` and its kin: positions counted from 1, from the
/// end for `last`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Nth {
    a: i32,
    b: i32,
    last: bool,
}

impl Nth {
    pub fn matches(&self, index: usize, count: usize) -> bool {
        let position = if self.last { count - index } else { index + 1 } as i32;
        if self.a == 0 {
            return position == self.b;
        }
        let n = position - self.b;
        n % self.a == 0 && n / self.a >= 0
    }
}

/// What parsing selectors adds to: the names, and the positional
/// pseudo-classes the sheet uses.
pub(crate) struct Ctx<'a> {
    pub names: &'a mut Names,
    pub positional: &'a mut Vec<Nth>,
}

impl Ctx<'_> {
    fn nth(&mut self, nth: Nth) -> Result<u32, Error> {
        let index = match self.positional.iter().position(|n| *n == nth) {
            Some(i) => i,
            None if self.positional.len() < 32 => {
                self.positional.push(nth);
                self.positional.len() - 1
            }
            None => return fail("more than 32 different positional pseudo-classes"),
        };
        Ok(1 << index)
    }
}

/// A rule's selector list. Any error is a custom one: a bad selector is
/// the rule's to skip, not a syntax error of the sheet.
pub(crate) fn parse_list(p: &mut Parser, ctx: &mut Ctx) -> Result<Vec<Selector>, Error> {
    p.parse_comma_separated(|p| selector(p, ctx))
        .map_err(|e| ParseError::custom(describe(e)))
}

fn selector(p: &mut Parser, ctx: &mut Ctx) -> Result<Selector, Error> {
    p.skip_whitespace();
    let mut compounds = vec![compound(p, ctx)?];
    let mut combinators = Vec::new();
    loop {
        let mut space = false;
        let combinator = loop {
            let state = p.state();
            match p.next_including_whitespace() {
                Err(_) => break None,
                Ok(Token::WhiteSpace(_)) => space = true,
                Ok(Token::Delim('>')) => break Some(Combinator::Child),
                Ok(Token::Delim(c @ ('+' | '~'))) => {
                    let c = *c;
                    return fail(format!("the `{c}` combinator isn't supported"));
                }
                Ok(token) => {
                    if !space {
                        return fail(format!("unexpected `{}`", token.to_css_string()));
                    }
                    p.reset(&state);
                    break Some(Combinator::Descendant);
                }
            }
        };
        let Some(combinator) = combinator else {
            break;
        };
        p.skip_whitespace();
        combinators.push(combinator);
        compounds.push(compound(p, ctx)?);
    }
    Ok(Selector {
        compounds,
        combinators,
    })
}

fn compound(p: &mut Parser, ctx: &mut Ctx) -> Result<Compound, Error> {
    let mut c = Compound::default();
    let mut any = false;
    loop {
        let state = p.state();
        let Ok(token) = p.next_including_whitespace() else {
            break;
        };
        match token.clone() {
            Token::Ident(kind) if !any => c.kind = Some(ctx.names.intern(&kind)),
            Token::Delim('*') if !any => {}
            Token::Delim('.') => match p.next_including_whitespace()?.clone() {
                Token::Ident(class) => c.classes.push(ctx.names.intern(&class)),
                t => {
                    return fail(format!(
                        "expected a class name, found `{}`",
                        t.to_css_string()
                    ));
                }
            },
            Token::IDHash(id) => c.ids.push(ctx.names.intern(&id)),
            Token::Hash(id) => {
                return fail(format!(
                    "`#{id}` isn't an id selector: an id starts with a letter (CSS escapes the rest: `#\\3{}`)",
                    id.chars().next().map(String::from).unwrap_or_default()
                ));
            }
            Token::SquareBracketBlock => {
                let attr = p.parse_nested_block(|p| attribute(p, ctx))?;
                c.attrs.push(attr);
            }
            Token::Colon => pseudo_class(p, ctx, &mut c)?,
            _ => {
                p.reset(&state);
                break;
            }
        }
        any = true;
    }
    if !any {
        return match p.next() {
            Ok(t) => fail(format!(
                "expected a selector, found `{}`",
                t.to_css_string()
            )),
            Err(_) => fail("expected a selector"),
        };
    }
    Ok(c)
}

/// `[name]` or `[name="value"]`.
fn attribute(p: &mut Parser, ctx: &mut Ctx) -> Result<Attr, Error> {
    p.skip_whitespace();
    let name = ctx.names.intern(&p.expect_ident_cloned()?);
    p.skip_whitespace();
    if p.is_exhausted() {
        return Ok(Attr { name, value: None });
    }
    match p.next()?.clone() {
        Token::Delim('=') => {}
        t @ (Token::IncludeMatch
        | Token::DashMatch
        | Token::PrefixMatch
        | Token::SuffixMatch
        | Token::SubstringMatch) => {
            return fail(format!(
                "`{}` isn't supported in an attribute selector: `=` only",
                t.to_css_string()
            ));
        }
        t => return fail(format!("expected `=`, found `{}`", t.to_css_string())),
    }
    let value = p.expect_ident_or_string()?.clone();
    p.skip_whitespace();
    Ok(Attr {
        name,
        value: Some(ctx.names.intern(&value)),
    })
}

fn pseudo_class(p: &mut Parser, ctx: &mut Ctx, c: &mut Compound) -> Result<(), Error> {
    match p.next_including_whitespace()?.clone() {
        Token::Ident(name) => match &*name.to_ascii_lowercase() {
            "hover" => c.states |= States::HOVER,
            "active" => c.states |= States::ACTIVE,
            "focus" => c.states |= States::FOCUS,
            "disabled" => c.disabled = true,
            "root" => c.root = true,
            "first-child" => {
                c.positions |= ctx.nth(Nth {
                    a: 0,
                    b: 1,
                    last: false,
                })?
            }
            "last-child" => {
                c.positions |= ctx.nth(Nth {
                    a: 0,
                    b: 1,
                    last: true,
                })?
            }
            other => return fail(format!("`:{other}` isn't supported")),
        },
        Token::Function(name) => {
            let last = match &*name.to_ascii_lowercase() {
                "nth-child" => false,
                "nth-last-child" => true,
                other => return fail(format!("`:{other}()` isn't supported")),
            };
            let (a, b) = p.parse_nested_block(|p| {
                cssparser::parse_nth(p).map_err(|_| ParseError::custom(format!("bad `:{name}()`")))
            })?;
            c.positions |= ctx.nth(Nth { a, b, last })?;
        }
        Token::Colon => return fail("pseudo-elements (`::`) aren't supported"),
        t => {
            return fail(format!(
                "expected a pseudo-class, found `{}`",
                t.to_css_string()
            ));
        }
    }
    Ok(())
}
