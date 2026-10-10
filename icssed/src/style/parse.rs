//! Stylesheets, from CSS text: their rules, their declarations, their
//! variables. A value stays text until its `var()`s are substituted
//! (see `Sheet::load`), as variables may come from a later file.
//!
//! cssparser recovers from every error as CSS does; what's still a
//! syntax error here, rejecting the whole file, is what recovery would
//! swallow the rest of the file for (a bracket never closed, a comment
//! never ended) and a rule with no block. Everything else is a warning,
//! and the rule or the declaration is skipped.

use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, ParseErrorKind, Parser, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, SourceLocation, StyleSheetParser,
    Token,
};
use rustc_hash::FxHashMap;

use super::node::Name;
use super::selector::{self, Ctx, Selector};
use super::value::{Error, describe};

/// A place in a source: line and column, both from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Loc {
    pub line: u32,
    pub column: u32,
}

impl From<SourceLocation> for Loc {
    fn from(l: SourceLocation) -> Self {
        Self {
            line: l.line + 1,
            column: l.column,
        }
    }
}

pub(crate) struct RawDecl {
    pub name: String,
    pub value: String,
    pub location: Loc,
}

pub(crate) struct RawRule {
    pub selectors: Vec<Selector>,
    pub decls: Vec<RawDecl>,
}

/// `@keyframes name { from { } 50% { } to { } }`: the steps' offsets (0 to
/// 1) and their declarations.
pub(crate) struct RawKeyframes {
    pub name: String,
    pub frames: Vec<(Vec<f32>, Vec<RawDecl>)>,
}

pub(crate) enum Item {
    Rule(RawRule),
    Keyframes(RawKeyframes),
}

#[derive(Default)]
pub(crate) struct File {
    /// The variables of `:root`, `:root.light` and `:root.dark`.
    pub vars: [Vec<(String, String)>; 3],
    pub rules: Vec<RawRule>,
    pub keyframes: Vec<RawKeyframes>,
}

/// A file's rules and variables, the warnings into `warnings`; a syntax
/// error is an `Err`.
pub(crate) fn parse(
    text: &str,
    ctx: &mut Ctx,
    light: Name,
    dark: Name,
    warnings: &mut Vec<(Loc, String)>,
) -> Result<File, (Loc, String)> {
    balanced(text)?;
    let mut p = Parser::new(text);
    let mut top = Top {
        ctx,
        warnings: Vec::new(),
    };
    let items: Vec<_> = StyleSheetParser::new(&mut p, &mut top).collect();
    warnings.append(&mut top.warnings);
    let mut file = File::default();
    for item in items {
        let mut rule = match item {
            Ok(Item::Rule(rule)) => rule,
            Ok(Item::Keyframes(keyframes)) => {
                file.keyframes.push(keyframes);
                continue;
            }
            Err((error, slice, location)) => match error.kind {
                ParseErrorKind::Custom(message) => {
                    warnings.push((
                        location.into(),
                        format!("{message}: `{}` skipped", head(slice)),
                    ));
                    continue;
                }
                ParseErrorKind::Basic(_) => {
                    return Err((
                        location.into(),
                        format!("`{}` has no `{{ }}` block", head(slice)),
                    ));
                }
            },
        };
        let scope = match &rule.selectors[..] {
            [selector] => selector.variable_scope(light, dark),
            _ => None,
        };
        rule.decls.retain_mut(|decl| {
            if !decl.name.starts_with("--") {
                return true;
            }
            match scope {
                Some(scope) => file.vars[scope].push((
                    std::mem::take(&mut decl.name),
                    std::mem::take(&mut decl.value),
                )),
                None => warnings.push((
                    decl.location,
                    format!(
                        "`{}`: variables are set in `:root`, `:root.light` or `:root.dark` only, skipped",
                        decl.name
                    ),
                )),
            }
            false
        });
        if !rule.decls.is_empty() {
            file.rules.push(rule);
        }
    }
    warnings.sort_by_key(|(at, _)| (at.line, at.column));
    Ok(file)
}

/// The first line of a skipped rule, for its warning.
fn head(text: &str) -> &str {
    let line = text.lines().next().unwrap_or("").trim();
    line.strip_suffix('{').unwrap_or(line).trim()
}

struct Top<'c, 'n> {
    ctx: &'c mut Ctx<'n>,
    warnings: Vec<(Loc, String)>,
}

impl<'i> QualifiedRuleParser<'i> for Top<'_, '_> {
    type Prelude = Vec<Selector>;
    type QualifiedRule = Item;
    type Error = String;

    fn parse_prelude(&mut self, input: &mut Parser<'i>) -> Result<Vec<Selector>, Error> {
        selector::parse_list(input, self.ctx)
    }

    fn parse_block(
        &mut self,
        selectors: Vec<Selector>,
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<Item, Error> {
        let decls = declarations(input, &mut self.warnings);
        Ok(Item::Rule(RawRule { selectors, decls }))
    }
}

/// A block's declarations, what can't be read a warning.
fn declarations(input: &mut Parser, warnings: &mut Vec<(Loc, String)>) -> Vec<RawDecl> {
    let mut decls = Vec::new();
    for item in RuleBodyParser::new(input, &mut Body) {
        match item {
            Ok(decl) => decls.push(decl),
            Err((error, slice, location)) => {
                let why = if slice.contains('{') {
                    "nested rules aren't supported".to_string()
                } else {
                    describe(error)
                };
                warnings.push((
                    location.into(),
                    format!("`{}`: {why}, skipped", slice.trim()),
                ));
            }
        }
    }
    decls
}

impl<'i> AtRuleParser<'i> for Top<'_, '_> {
    /// A `@keyframes`' name.
    type Prelude = String;
    type AtRule = Item;
    type Error = String;

    fn parse_prelude(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
    ) -> Result<String, Error> {
        if !name.eq_ignore_ascii_case("keyframes") {
            return Err(ParseError::custom(format!("`@{name}` isn't supported")));
        }
        match input.next() {
            Ok(Token::Ident(name) | Token::QuotedString(name)) => Ok(name.to_string()),
            _ => Err(ParseError::custom("`@keyframes` takes a name".to_string())),
        }
    }

    fn parse_block(
        &mut self,
        name: String,
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<Item, Error> {
        let mut steps = Steps {
            warnings: &mut self.warnings,
        };
        let mut frames = Vec::new();
        let mut failures = Vec::new();
        for item in RuleBodyParser::new(input, &mut steps) {
            match item {
                Ok(frame) => frames.push(frame),
                Err((error, slice, location)) => failures.push((
                    Loc::from(location),
                    format!("`{}`: {}, skipped", head(slice), describe(error)),
                )),
            }
        }
        self.warnings.extend(failures);
        Ok(Item::Keyframes(RawKeyframes { name, frames }))
    }
}

/// A `@keyframes`' body: its steps.
struct Steps<'w> {
    warnings: &'w mut Vec<(Loc, String)>,
}

impl<'i> QualifiedRuleParser<'i> for Steps<'_> {
    type Prelude = Vec<f32>;
    type QualifiedRule = (Vec<f32>, Vec<RawDecl>);
    type Error = String;

    /// `from`, `to`, percentages: offsets from 0 to 1.
    fn parse_prelude(&mut self, input: &mut Parser<'i>) -> Result<Vec<f32>, Error> {
        input.parse_comma_separated(|p| match p.next()?.clone() {
            Token::Ident(word) if word.eq_ignore_ascii_case("from") => Ok(0.0),
            Token::Ident(word) if word.eq_ignore_ascii_case("to") => Ok(1.0),
            Token::Percentage { unit_value, .. } if (0.0..=1.0).contains(&unit_value) => {
                Ok(unit_value)
            }
            _ => Err(ParseError::custom(
                "a step is `from`, `to` or a percentage".to_string(),
            )),
        })
    }

    fn parse_block(
        &mut self,
        offsets: Vec<f32>,
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<(Vec<f32>, Vec<RawDecl>), Error> {
        Ok((offsets, declarations(input, self.warnings)))
    }
}

impl<'i> DeclarationParser<'i> for Steps<'_> {
    type Declaration = (Vec<f32>, Vec<RawDecl>);
    type Error = String;
}

impl<'i> AtRuleParser<'i> for Steps<'_> {
    type Prelude = ();
    type AtRule = (Vec<f32>, Vec<RawDecl>);
    type Error = String;
}

impl<'i> RuleBodyItemParser<'i, (Vec<f32>, Vec<RawDecl>), String> for Steps<'_> {
    fn parse_declarations(&self) -> bool {
        false
    }

    fn parse_qualified(&self) -> bool {
        true
    }
}

/// A rule's body: declarations only, no nested rules.
struct Body;

impl<'i> DeclarationParser<'i> for Body {
    type Declaration = RawDecl;
    type Error = String;

    fn parse_value(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
        start: &ParserState,
    ) -> Result<RawDecl, Error> {
        let from = input.position();
        let (mut bang, mut important) = (false, false);
        while let Ok(token) = input.next_including_whitespace_and_comments() {
            match token {
                Token::WhiteSpace(_) | Token::Comment(_) => {}
                Token::Delim('!') => bang = true,
                Token::Ident(word) if bang && word.eq_ignore_ascii_case("important") => {
                    important = true
                }
                _ => bang = false,
            }
        }
        if important {
            return Err(ParseError::custom(
                "`!important` isn't supported".to_string(),
            ));
        }
        Ok(RawDecl {
            name: name.to_string(),
            value: input.slice_from(from).trim().to_string(),
            location: start.source_location().into(),
        })
    }
}

impl<'i> QualifiedRuleParser<'i> for Body {
    type Prelude = ();
    type QualifiedRule = RawDecl;
    type Error = String;
}

impl<'i> AtRuleParser<'i> for Body {
    type Prelude = ();
    type AtRule = RawDecl;
    type Error = String;
}

impl<'i> RuleBodyItemParser<'i, RawDecl, String> for Body {
    fn parse_declarations(&self) -> bool {
        true
    }

    fn parse_qualified(&self) -> bool {
        false
    }
}

/// Brackets closed in order, comments ended: what CSS's error recovery
/// would otherwise take the rest of the file for.
fn balanced(text: &str) -> Result<(), (Loc, String)> {
    let mut open: Vec<(char, Loc)> = Vec::new();
    let mut here = Loc { line: 1, column: 1 };
    let mut chars = text.chars().peekable();
    let advance = |here: &mut Loc, c: char| {
        if c == '\n' {
            here.line += 1;
            here.column = 1;
        } else {
            here.column += 1;
        }
    };
    while let Some(c) = chars.next() {
        let at = here;
        advance(&mut here, c);
        match c {
            '/' if chars.peek() == Some(&'*') => {
                advance(&mut here, chars.next().unwrap());
                let mut star = false;
                loop {
                    let Some(c) = chars.next() else {
                        return Err((at, "a comment is never ended".into()));
                    };
                    advance(&mut here, c);
                    if star && c == '/' {
                        break;
                    }
                    star = c == '*';
                }
            }
            '"' | '\'' => {
                // A string ends at its quote, or (a bad string) at the
                // end of the line.
                while let Some(s) = chars.next() {
                    advance(&mut here, s);
                    match s {
                        '\\' => {
                            if let Some(e) = chars.next() {
                                advance(&mut here, e);
                            }
                        }
                        '\n' => break,
                        _ if s == c => break,
                        _ => {}
                    }
                }
            }
            '\\' => {
                if let Some(e) = chars.next() {
                    advance(&mut here, e);
                }
            }
            '{' | '(' | '[' => open.push((c, at)),
            '}' | ')' | ']' => {
                let opener = match c {
                    '}' => '{',
                    ')' => '(',
                    _ => '[',
                };
                match open.pop() {
                    Some((o, _)) if o == opener => {}
                    Some((o, from)) => {
                        return Err((
                            at,
                            format!(
                                "`{c}` where the `{o}` opened at {}:{} is still open",
                                from.line, from.column
                            ),
                        ));
                    }
                    None => return Err((at, format!("`{c}` closes nothing"))),
                }
            }
            _ => {}
        }
    }
    match open.pop() {
        Some((o, at)) => Err((at, format!("`{o}` is never closed"))),
        None => Ok(()),
    }
}

/// `text` with its `var()`s replaced by the variables' values, or their
/// fallbacks.
pub(crate) fn substitute(text: &str, vars: &FxHashMap<String, String>) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut p = Parser::new(text);
    substitute_into(&mut p, vars, &mut out, 0).map_err(describe)?;
    Ok(out)
}

fn substitute_into(
    p: &mut Parser,
    vars: &FxHashMap<String, String>,
    out: &mut String,
    depth: u8,
) -> Result<(), Error> {
    loop {
        let start = p.position();
        let Ok(token) = p.next_including_whitespace_and_comments() else {
            return Ok(());
        };
        let closing = match token {
            Token::Function(name) if name.eq_ignore_ascii_case("var") => {
                p.parse_nested_block(|p| var(p, vars, out, depth))?;
                continue;
            }
            Token::Function(_) | Token::ParenthesisBlock => ')',
            Token::SquareBracketBlock => ']',
            Token::CurlyBracketBlock => '}',
            _ => {
                out.push_str(p.slice_from(start));
                continue;
            }
        };
        out.push_str(p.slice_from(start));
        p.parse_nested_block(|p| substitute_into(p, vars, out, depth))?;
        out.push(closing);
    }
}

/// Inside `var(`: `--name`, then an optional fallback.
fn var(
    p: &mut Parser,
    vars: &FxHashMap<String, String>,
    out: &mut String,
    depth: u8,
) -> Result<(), Error> {
    p.skip_whitespace();
    let name = match p.next()?.clone() {
        Token::Ident(name) if name.starts_with("--") => name,
        _ => return Err(ParseError::custom("`var()` takes a `--name`".to_string())),
    };
    p.skip_whitespace();
    let fallback = p.try_parse(|p| p.expect_comma()).is_ok();
    match vars.get(&*name) {
        Some(value) => {
            if depth >= 16 {
                return Err(ParseError::custom(format!(
                    "`{name}` refers back to itself"
                )));
            }
            substitute_into(&mut Parser::new(value), vars, out, depth + 1)?;
            // The fallback, unused.
            while p.next_including_whitespace_and_comments().is_ok() {}
            Ok(())
        }
        None if fallback => substitute_into(p, vars, out, depth),
        None => Err(ParseError::custom(format!("`{name}` isn't set"))),
    }
}
