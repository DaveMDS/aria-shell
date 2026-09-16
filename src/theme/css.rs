//! The CSS-subset scanner: text -> rules with selector text and untyped
//! declarations. Knows nothing about what a selector or a value means;
//! that's `selector.rs` and `value.rs`. Errors carry a line:column so
//! the user can find them in the theme file.

use std::collections::HashMap;
use std::fmt;

use super::Scheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub pos: Pos,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.pos, self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub value: String,
    pub pos: Pos,
}

/// One `selectors { declarations }` block, selectors still as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRule {
    pub selectors: Vec<String>,
    pub declarations: Vec<Declaration>,
    pub pos: Pos,
}

/// A parsed file. Variables (`--name: value` inside `:root { }`) are
/// pulled out; the remaining `:root` declarations stay as a normal rule.
/// `:root.light { }` / `:root.dark { }` hold the variables of one colour
/// scheme, applied over the plain ones when that scheme is active.
/// `var()` references are left in place: substitution happens after all
/// files are loaded, so a user theme can override a variable the base
/// stylesheet uses.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Sheet {
    pub rules: Vec<RawRule>,
    pub vars: HashMap<String, String>,
    pub scheme_vars: HashMap<Scheme, HashMap<String, String>>,
}

pub fn parse(src: &str) -> Result<Sheet, Error> {
    Scanner::new(src).sheet()
}

/// Replace every `var(--name[, fallback])` in `value` with the variable's
/// value (itself substituted). Unknown names without a fallback, and
/// reference cycles, are errors.
pub fn substitute_vars(value: &str, vars: &HashMap<String, String>) -> Result<String, String> {
    substitute(value, vars, 0)
}

fn substitute(value: &str, vars: &HashMap<String, String>, depth: usize) -> Result<String, String> {
    const MAX_DEPTH: usize = 16;
    if depth > MAX_DEPTH {
        return Err("variable references nest too deep (cycle?)".to_owned());
    }
    let Some(start) = value.find("var(") else {
        return Ok(value.to_owned());
    };
    let mut out = String::from(&value[..start]);
    let rest = &value[start + 4..];
    let end = matching_paren(rest).ok_or("unclosed var(")?;
    let inner = &rest[..end];
    let (name, fallback) = match inner.find(',') {
        Some(i) => (inner[..i].trim(), Some(inner[i + 1..].trim())),
        None => (inner.trim(), None),
    };
    if !name.starts_with("--") {
        return Err(format!("variable names start with --, got {name:?}"));
    }
    let replacement = match (vars.get(name), fallback) {
        (Some(v), _) => substitute(v, vars, depth + 1)?,
        (None, Some(fb)) => substitute(fb, vars, depth + 1)?,
        (None, None) => return Err(format!("undefined variable {name}")),
    };
    out.push_str(&replacement);
    out.push_str(&substitute(&rest[end + 1..], vars, depth)?);
    Ok(out)
}

/// Index of the `)` closing the paren opened just before `s`.
fn matching_paren(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

struct Scanner<'a> {
    src: &'a str,
    /// Byte offset.
    at: usize,
    line: usize,
    col: usize,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            at: 0,
            line: 1,
            col: 1,
        }
    }

    fn pos(&self) -> Pos {
        Pos {
            line: self.line,
            col: self.col,
        }
    }

    fn error(&self, message: impl Into<String>) -> Error {
        Error {
            pos: self.pos(),
            message: message.into(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.at..].chars().next()
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.at..].starts_with(s)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn skip_ws_and_comments(&mut self) -> Result<(), Error> {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.starts_with("/*") => {
                    let start = self.pos();
                    self.bump();
                    self.bump();
                    loop {
                        if self.starts_with("*/") {
                            self.bump();
                            self.bump();
                            break;
                        }
                        if self.bump().is_none() {
                            return Err(Error {
                                pos: start,
                                message: "unterminated comment".to_owned(),
                            });
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// Consume a quoted string (opening quote already peeked), appending
    /// it verbatim, quotes included.
    fn string(&mut self, out: &mut String) -> Result<(), Error> {
        let start = self.pos();
        let quote = self.bump().expect("caller peeked the quote");
        out.push(quote);
        loop {
            match self.bump() {
                Some('\n') | None => {
                    return Err(Error {
                        pos: start,
                        message: "unterminated string".to_owned(),
                    });
                }
                Some('\\') => {
                    out.push('\\');
                    if let Some(c) = self.bump() {
                        out.push(c);
                    }
                }
                Some(c) => {
                    out.push(c);
                    if c == quote {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// Text up to (not including) one of `stops` at nesting depth zero,
    /// with strings, parens and brackets kept intact. Comments are
    /// dropped. Returns the trimmed text and which stop was hit (`None`
    /// at end of input).
    fn until(&mut self, stops: &[char]) -> Result<(String, Option<char>), Error> {
        let mut out = String::new();
        let mut depth = 0usize;
        loop {
            let Some(c) = self.peek() else {
                return Ok((out.trim().to_owned(), None));
            };
            if depth == 0 && stops.contains(&c) {
                return Ok((out.trim().to_owned(), Some(c)));
            }
            match c {
                '"' | '\'' => self.string(&mut out)?,
                '/' if self.starts_with("/*") => self.skip_ws_and_comments()?,
                '(' | '[' => {
                    depth += 1;
                    out.push(c);
                    self.bump();
                }
                ')' | ']' => {
                    depth = depth.saturating_sub(1);
                    out.push(c);
                    self.bump();
                }
                _ => {
                    out.push(c);
                    self.bump();
                }
            }
        }
    }

    fn sheet(&mut self) -> Result<Sheet, Error> {
        let mut sheet = Sheet::default();
        loop {
            self.skip_ws_and_comments()?;
            match self.peek() {
                None => return Ok(sheet),
                Some('@') => self.skip_at_rule()?,
                Some('}') => return Err(self.error("unexpected `}`")),
                Some(_) => self.rule(&mut sheet)?,
            }
        }
    }

    /// `@something ... ;` or `@something ... { ... }`: not supported,
    /// skipped whole.
    fn skip_at_rule(&mut self) -> Result<(), Error> {
        let pos = self.pos();
        self.bump();
        let (name, stop) = self.until(&[';', '{'])?;
        let name = name
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        log::warn!("{pos}: @{name} rules are not supported, skipped");
        match stop {
            Some(';') => {
                self.bump();
            }
            Some(_) => {
                self.bump();
                let mut depth = 1;
                loop {
                    match self.bump() {
                        Some('{') => depth += 1,
                        Some('}') => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Some(_) => {}
                        None => {
                            return Err(Error {
                                pos,
                                message: format!("unterminated @{name} block"),
                            });
                        }
                    }
                }
            }
            None => {}
        }
        Ok(())
    }

    fn rule(&mut self, sheet: &mut Sheet) -> Result<(), Error> {
        let pos = self.pos();
        let (selectors, stop) = self.until(&['{', ';', '}'])?;
        match stop {
            Some('{') => {}
            Some(c) => return Err(self.error(format!("expected `{{` after selector, got `{c}`"))),
            None => return Err(self.error("expected `{` after selector, got end of file")),
        }
        self.bump();
        let selectors: Vec<String> = selectors
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if selectors.is_empty() {
            return Err(Error {
                pos,
                message: "rule without a selector".to_owned(),
            });
        }
        // `:root` takes variables; `:root.light` / `:root.dark` take the
        // variables of that scheme (and nothing else).
        let is_root = selectors.iter().all(|s| s == ":root");
        let scheme = match selectors.as_slice() {
            [s] => s
                .strip_prefix(":root.")
                .and_then(|name| name.parse::<Scheme>().ok()),
            _ => None,
        };
        let mut declarations = Vec::new();
        loop {
            self.skip_ws_and_comments()?;
            match self.peek() {
                Some('}') => {
                    self.bump();
                    break;
                }
                Some(';') => {
                    self.bump();
                    continue;
                }
                None => {
                    return Err(Error {
                        pos,
                        message: "unterminated rule, missing `}`".to_owned(),
                    });
                }
                Some(_) => {}
            }
            let decl_pos = self.pos();
            let (name, stop) = self.until(&[':', ';', '}'])?;
            if stop != Some(':') {
                return Err(Error {
                    pos: decl_pos,
                    message: format!("expected `property: value`, got {name:?}"),
                });
            }
            self.bump();
            let (value, _) = self.until(&[';', '}'])?;
            if name.is_empty() || name.contains(char::is_whitespace) {
                return Err(Error {
                    pos: decl_pos,
                    message: format!("invalid property name {name:?}"),
                });
            }
            if value.is_empty() {
                return Err(Error {
                    pos: decl_pos,
                    message: format!("empty value for {name}"),
                });
            }
            if name.starts_with("--") {
                if let Some(scheme) = scheme {
                    sheet
                        .scheme_vars
                        .entry(scheme)
                        .or_default()
                        .insert(name, value);
                } else if is_root {
                    sheet.vars.insert(name, value);
                } else {
                    return Err(Error {
                        pos: decl_pos,
                        message: "variables can only be declared in `:root { }` \
                                  (or `:root.light` / `:root.dark`)"
                            .to_owned(),
                    });
                }
            } else if scheme.is_some() {
                return Err(Error {
                    pos: decl_pos,
                    message: format!(
                        "`{}` takes only variables; style the root elements \
                         with `panel.{}`, `popup.{}`, ...",
                        selectors[0],
                        scheme.unwrap().name(),
                        scheme.unwrap().name()
                    ),
                });
            } else {
                declarations.push(Declaration {
                    name,
                    value,
                    pos: decl_pos,
                });
            }
        }
        if !declarations.is_empty() {
            sheet.rules.push(RawRule {
                selectors,
                declarations,
                pos,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decls(rule: &RawRule) -> Vec<(&str, &str)> {
        rule.declarations
            .iter()
            .map(|d| (d.name.as_str(), d.value.as_str()))
            .collect()
    }

    #[test]
    fn rules_and_declarations() {
        let sheet = parse(
            "/* header */\n\
             panel, popup { color: red; padding: 1 2 }\n\
             workspace.active:hover > text { background: rgba(1, 2, 3, 0.5) ; }",
        )
        .unwrap();
        assert_eq!(sheet.rules.len(), 2);
        assert_eq!(sheet.rules[0].selectors, ["panel", "popup"]);
        assert_eq!(
            decls(&sheet.rules[0]),
            [("color", "red"), ("padding", "1 2")]
        );
        assert_eq!(sheet.rules[0].pos, Pos { line: 2, col: 1 });
        assert_eq!(sheet.rules[1].selectors, ["workspace.active:hover > text"]);
        assert_eq!(
            decls(&sheet.rules[1]),
            [("background", "rgba(1, 2, 3, 0.5)")]
        );
    }

    #[test]
    fn root_variables() {
        let sheet = parse(":root { --bg: #fff; color: var(--fg, black); --fg: red }").unwrap();
        assert_eq!(sheet.vars["--bg"], "#fff");
        assert_eq!(sheet.vars["--fg"], "red");
        assert_eq!(sheet.rules.len(), 1);
        assert_eq!(sheet.rules[0].selectors, [":root"]);
        assert_eq!(decls(&sheet.rules[0]), [("color", "var(--fg, black)")]);

        let err = parse("panel { --x: 1 }").unwrap_err();
        assert!(err.message.contains(":root"), "{err}");
    }

    #[test]
    fn scheme_variables() {
        let sheet = parse(
            ":root { --bg: grey }\n\
             :root.dark { --bg: black; --fg: white }\n\
             :root.light { --bg: white }",
        )
        .unwrap();
        assert_eq!(sheet.vars["--bg"], "grey");
        assert_eq!(sheet.scheme_vars[&Scheme::Dark]["--bg"], "black");
        assert_eq!(sheet.scheme_vars[&Scheme::Dark]["--fg"], "white");
        assert_eq!(sheet.scheme_vars[&Scheme::Light]["--bg"], "white");
        assert!(sheet.rules.is_empty());

        let err = parse(":root.dark { color: red }").unwrap_err();
        assert!(err.message.contains("only variables"), "{err}");
        // Not a scheme: an ordinary (unknown) selector, no variables.
        let err = parse(":root.blue { --x: 1 }").unwrap_err();
        assert!(err.message.contains(":root"), "{err}");
    }

    #[test]
    fn strings_and_comments_in_values() {
        let sheet = parse("text { font-family: \"Fira; Code\" /* ; */; }").unwrap();
        assert_eq!(decls(&sheet.rules[0]), [("font-family", "\"Fira; Code\"")]);
    }

    #[test]
    fn at_rules_are_skipped() {
        let sheet =
            parse("@import \"x.css\"; @media (x) { a { b: c } } panel { color: red }").unwrap();
        assert_eq!(sheet.rules.len(), 1);
        assert_eq!(sheet.rules[0].selectors, ["panel"]);
    }

    #[test]
    fn errors_have_positions() {
        let err = parse("panel {\n  color red;\n}").unwrap_err();
        assert_eq!(err.pos, Pos { line: 2, col: 3 });
        assert_eq!(
            err.to_string(),
            "2:3: expected `property: value`, got \"color red\""
        );

        let err = parse("panel { color: red ").unwrap_err();
        assert!(err.message.contains("missing `}`"), "{err}");

        let err = parse("/* never closed").unwrap_err();
        assert_eq!(err.message, "unterminated comment");

        let err = parse("panel").unwrap_err();
        assert!(err.message.contains("end of file"), "{err}");
    }

    #[test]
    fn variable_substitution() {
        let vars: HashMap<String, String> = [
            ("--a", "red"),
            ("--b", "var(--a)"),
            ("--loop", "var(--loop)"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        assert_eq!(
            substitute_vars("1px solid var(--b)", &vars).unwrap(),
            "1px solid red"
        );
        assert_eq!(
            substitute_vars("var(--missing, blue)", &vars).unwrap(),
            "blue"
        );
        assert_eq!(
            substitute_vars("var(--missing, var(--a))", &vars).unwrap(),
            "red"
        );
        assert_eq!(
            substitute_vars("no vars here", &vars).unwrap(),
            "no vars here"
        );
        assert!(substitute_vars("var(--missing)", &vars).is_err());
        assert!(substitute_vars("var(--loop)", &vars).is_err());
        assert!(substitute_vars("var(x)", &vars).is_err());
    }
}
