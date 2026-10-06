//! Styling: a CSS-like theme file, resolved per widget at view time.
//!
//! The built-in `assets/base.css` is always loaded; `[general] style`
//! names a user theme loaded on top of it. Both are parsed and
//! type-checked once, at load ([`css`], [`selector`], [`value`]); a
//! mistake is logged with `file:line:col` and the offending rule or
//! declaration is skipped, so a theme with a typo still mostly works.
//!
//! A theme is loaded for one colour [`Scheme`]: the variables of
//! `:root.light { }` / `:root.dark { }` go over the plain `:root` ones,
//! and every root element carries the scheme as a class while matching
//! (`panel.dark { }`). Changing the scheme is a reload.
//!
//! In `view`, a widget describes where it is in the element tree with a
//! [`Node`] (`panel > slot.start > gadget.clock > button`), and
//! [`Theme::resolve`] gives it the cascaded [`Style`]: every matching
//! rule applied in specificity then source order, walking the path from
//! the root so `color` and the `font-*` properties inherit. The helpers
//! ([`Theme::container`], [`Theme::button`], [`Theme::text`],
//! [`Theme::row`], in `build.rs`) do the resolving and return plain
//! iced widgets; `fonts.rs` picks the font family.
//!
//! The theme doesn't watch its own files: the daemon does (`watch.rs`)
//! and calls [`Theme::try_load`] again.
//!
//! Interaction state is part of the node: a button's style closure
//! re-resolves with `node.status(status)`, which is what makes `:hover`
//! and `:active` rules apply.
//!
//! The element tree and the supported properties are documented for
//! theme authors in `assets/base.css`.

mod build;
mod css;
mod fonts;
mod node;
mod selector;
mod value;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use iced::border::Radius;
use iced::font::Weight;
use iced::{Color, Padding, Shadow};

use crate::config::{self, Config};
use value::Property;

pub use build::widget_path;
pub use node::Node;
pub use selector::{Selector, node_from_path};
pub use value::Length;

pub(crate) use build::widget_id;
use fonts::{intern, pick_family};

/// Always loaded first; the neutral defaults every theme builds on.
const BASE: &str = include_str!("../../../assets/base.css");

/// Bar thickness when no rule sets `min-height` on `panel`.
pub const DEFAULT_PANEL_HEIGHT: f32 = 32.0;

/// The colour scheme a theme is loaded for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Scheme {
    #[default]
    Light,
    Dark,
}

impl Scheme {
    /// The class root elements get, and the config value.
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

impl std::str::FromStr for Scheme {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(()),
        }
    }
}

/// What a gadget asks the daemon to change about the theme.
#[derive(Debug, Clone)]
pub enum Command {
    ToggleScheme,
    SetScheme(Scheme),
    /// A theme name (as `[general] style`), or `None` for the base alone.
    SetStyle(Option<String>),
}

pub struct Theme {
    /// In cascade order: later rules win.
    rules: Vec<Rule>,
    /// Indices into `rules`, ascending, of those whose selector names
    /// an element type, by that type: a node is only tried against its
    /// own type's and `any_kind`'s (most of the time in a rebuild went
    /// to trying every rule on every node and its ancestors).
    by_kind: HashMap<String, Vec<usize>>,
    /// Indices of the rules naming no type (`.active`, `*`), ascending.
    any_kind: Vec<usize>,
    /// User files loaded, for the daemon to watch (see `watch.rs`).
    files: Vec<PathBuf>,
    scheme: Scheme,
    /// The user theme loaded, as named in the config or picked by the
    /// user; `None` for the base alone.
    name: Option<String>,
}

struct Rule {
    selector: Selector,
    props: Vec<Property>,
}

/// The cascaded style of one node. `None`/zero means "not set": the
/// widget keeps iced's default for it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Style {
    pub color: Option<Color>,
    /// `color` was set by a rule matching this very node, not inherited.
    /// Only then does a `text` widget get an explicit colour; otherwise
    /// it takes its parent's at draw time (which is how `:hover` on a
    /// button reaches its label).
    pub own_color: bool,
    pub background: Option<Color>,
    pub border_width: f32,
    pub border_color: Option<Color>,
    pub border_radius: Radius,
    pub shadow: Option<Shadow>,
    pub padding: Padding,
    pub gap: f32,
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub min_height: Option<f32>,
    pub font_family: Option<&'static str>,
    pub font_size: Option<f32>,
    pub font_weight: Option<Weight>,
}

/// Why a user theme couldn't be loaded. `files` are still the ones to
/// watch, so fixing the file triggers a reload.
#[derive(Debug)]
pub struct LoadError {
    pub message: String,
    pub files: Vec<PathBuf>,
}

impl Theme {
    /// The base stylesheet plus the user theme `style` (a name looked up
    /// in the theme directories, or a path), if any, for `scheme`. Never
    /// fails: a missing or broken user theme is logged and the base
    /// alone is used (its file still watched).
    pub fn load(config: &Config, style: Option<&str>, scheme: Scheme) -> Self {
        Self::try_load(config, style, scheme).unwrap_or_else(|e| {
            log::error!("{}, using the base theme", e.message);
            Self {
                files: e.files,
                ..Self::base(scheme)
            }
        })
    }

    /// Like [`Theme::load`], but a user theme that can't be read or has a
    /// syntax error is an error, so a reload can keep the last good one.
    pub fn try_load(
        config: &Config,
        style: Option<&str>,
        scheme: Scheme,
    ) -> Result<Self, LoadError> {
        let Some(style) = style else {
            return Ok(Self::base(scheme));
        };
        let Some(path) = locate(style, config) else {
            return Err(LoadError {
                message: format!("theme {style:?} not found"),
                files: Vec::new(),
            });
        };
        let files = vec![path.clone()];
        let text = fs::read_to_string(&path).map_err(|e| LoadError {
            message: format!("cannot read theme {}: {e}", path.display()),
            files: files.clone(),
        })?;
        log::info!("loading theme {} ({})", path.display(), scheme.name());
        let name = path.display().to_string();
        let mut theme = Self::from_sources(&[("base.css", BASE), (&name, &text)], scheme).map_err(
            |message| LoadError {
                message,
                files: files.clone(),
            },
        )?;
        theme.files = files;
        theme.name = Some(style.to_owned());
        Ok(theme)
    }

    /// The base stylesheet alone.
    fn base(scheme: Scheme) -> Self {
        Self::from_sources(&[("base.css", BASE)], scheme).expect("base.css is valid")
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// The user theme loaded, `None` for the base alone.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Build from `(name, text)` sources in cascade order. A scanner
    /// error in any source is an error; everything else (bad selector,
    /// unknown property, bad value) is logged and skipped.
    fn from_sources(sources: &[(&str, &str)], scheme: Scheme) -> Result<Self, String> {
        let mut sheets = Vec::new();
        for (name, text) in sources {
            let sheet = css::parse(text).map_err(|e| format!("{name}:{e}"))?;
            sheets.push((*name, sheet));
        }
        // Later files override earlier variables, and every file sees
        // the final set. Within a file the scheme's variables go over
        // the plain ones; a later file's plain ones still win over an
        // earlier file's scheme ones.
        let mut vars: HashMap<String, String> = HashMap::new();
        for (_, sheet) in &sheets {
            vars.extend(sheet.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
            if let Some(scheme_vars) = sheet.scheme_vars.get(&scheme) {
                vars.extend(scheme_vars.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        }

        let mut rules = Vec::new();
        for (name, sheet) in &sheets {
            for raw in &sheet.rules {
                let mut props = Vec::new();
                for decl in &raw.declarations {
                    let value = match css::substitute_vars(&decl.value, &vars) {
                        Ok(v) => v,
                        Err(e) => {
                            log::warn!("{name}:{}: {}: {e}, skipped", decl.pos, decl.name);
                            continue;
                        }
                    };
                    match value::parse(&decl.name, &value) {
                        Ok(p) => props.extend(p),
                        Err(e) => log::warn!("{name}:{}: {e}, skipped", decl.pos),
                    }
                }
                for text in &raw.selectors {
                    match Selector::parse(text) {
                        Ok(selector) => rules.push(Rule {
                            selector,
                            props: props.clone(),
                        }),
                        Err(e) => {
                            log::warn!("{name}:{}: selector {text:?}: {e}, skipped", raw.pos);
                        }
                    }
                }
            }
        }
        // Stable sort: equal specificity keeps source order.
        rules.sort_by_key(|r| r.selector.specificity());
        let mut by_kind: HashMap<String, Vec<usize>> = HashMap::new();
        let mut any_kind = Vec::new();
        for (i, rule) in rules.iter().enumerate() {
            match rule.selector.subject_kind() {
                Some(kind) => by_kind.entry(kind.to_owned()).or_default().push(i),
                None => any_kind.push(i),
            }
        }
        Ok(Self {
            rules,
            by_kind,
            any_kind,
            files: Vec::new(),
            scheme,
            name: None,
        })
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    pub fn resolve(&self, node: &Node) -> Style {
        let mut style = Style::default();
        for n in node.path() {
            let mut own = style.inherited();
            // The root element carries the scheme: `panel.dark { }`.
            let with_scheme;
            let n = if n.is_root() {
                with_scheme = n.class(self.scheme.name());
                &with_scheme
            } else {
                n
            };
            for rule in self.candidates(n.kind()) {
                if rule.selector.matches(n) {
                    for p in &rule.props {
                        own.apply(p);
                    }
                }
            }
            style = own;
        }
        style
    }

    /// The rules a node of type `kind` may match, in cascade order: its
    /// type's merged with those naming none.
    fn candidates(&self, kind: &str) -> impl Iterator<Item = &Rule> {
        let typed = self.by_kind.get(kind).map_or(&[][..], Vec::as_slice);
        let any = self.any_kind.as_slice();
        let (mut i, mut j) = (0, 0);
        std::iter::from_fn(move || {
            let next = match (typed.get(i), any.get(j)) {
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
            Some(&self.rules[next])
        })
    }
}

impl Default for Theme {
    /// The base stylesheet alone, light.
    fn default() -> Self {
        Self::base(Scheme::default())
    }
}

impl Style {
    /// What a child starts from.
    fn inherited(&self) -> Self {
        Self {
            color: self.color,
            font_family: self.font_family,
            font_size: self.font_size,
            font_weight: self.font_weight,
            ..Self::default()
        }
    }

    fn apply(&mut self, p: &Property) {
        match p {
            Property::Color(c) => {
                self.color = Some(*c);
                self.own_color = true;
            }
            Property::Background(b) => self.background = *b,
            Property::BorderWidth(w) => self.border_width = *w,
            Property::BorderColor(c) => self.border_color = Some(*c),
            Property::BorderRadius(r) => self.border_radius = *r,
            Property::Shadow(s) => self.shadow = *s,
            Property::Padding(p) => self.padding = *p,
            Property::Gap(g) => self.gap = *g,
            Property::Width(l) => self.width = Some(*l),
            Property::Height(l) => self.height = Some(*l),
            Property::MinHeight(h) => self.min_height = Some(*h),
            Property::FontFamily(names) => self.font_family = Some(intern(pick_family(names))),
            Property::FontSize(s) => self.font_size = Some(*s),
            Property::FontWeight(w) => self.font_weight = Some(*w),
        }
    }
}

/// The directories a theme name is looked up in, in order.
fn theme_dirs() -> Vec<PathBuf> {
    config::config_dirs()
        .into_iter()
        .chain(config::data_dirs())
        .chain(std::iter::once(config::dev_assets_dir()))
        .collect()
}

/// Every theme a name resolves to: `(name, path)`, sorted by name; a
/// name found in an earlier directory hides the later ones, as
/// [`locate`] would pick it.
pub fn available() -> Vec<(String, PathBuf)> {
    available_in(&theme_dirs())
}

fn available_in(dirs: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir.join("themes")) else {
            continue;
        };
        for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
            let Some(name) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|_| path.extension().is_some_and(|e| e == "css") && path.is_file())
            else {
                continue;
            };
            if !found.iter().any(|(n, _)| n == name) {
                found.push((name.to_owned(), path.clone()));
            }
        }
    }
    found.sort();
    found
}

/// Where `[general] style = <value>` points: a path if it looks like
/// one (has a `/` or ends in `.css`, relative to the config file's
/// directory), else `themes/<name>.css` under the config dirs, the data
/// dirs, then the source tree's `assets/`.
fn locate(style: &str, config: &Config) -> Option<PathBuf> {
    if style.contains('/') || style.ends_with(".css") {
        let path = config.resolve_path(style);
        return path.is_file().then_some(path);
    }
    theme_dirs()
        .into_iter()
        .map(|dir| dir.join("themes").join(format!("{style}.css")))
        .find(|f| f.is_file())
}

#[cfg(test)]
mod tests {
    use iced::font::Family;

    use super::*;

    #[test]
    fn widget_id_round_trips_the_node_path() {
        let node = Node::root("panel")
            .attr("output", "DP-1")
            .child("slot")
            .class("end")
            .nth(1, 2);
        let path = format!("{node:?}");
        assert_eq!(
            path,
            "panel[output=\"DP-1\"] > slot.end:nth-child(2):last-child"
        );
        assert_eq!(
            widget_path(&widget_id(&node)).as_deref(),
            Some(path.as_str())
        );
        assert_eq!(widget_path(&iced::widget::Id::unique()), None);
    }
    use iced::widget::button::Status;

    fn theme(css: &str) -> Theme {
        Theme::from_sources(&[("test.css", css)], Scheme::Light).expect("valid css")
    }

    fn red() -> Color {
        Color::from_rgb(1.0, 0.0, 0.0)
    }

    #[test]
    fn base_stylesheet_is_valid() {
        let base = css::parse(BASE).expect("base.css parses");
        // Both schemes must define every variable the rules use.
        for scheme in [Scheme::Light, Scheme::Dark] {
            let mut vars = base.vars.clone();
            vars.extend(base.scheme_vars[&scheme].clone());
            for rule in &base.rules {
                for s in &rule.selectors {
                    Selector::parse(s).unwrap_or_else(|e| panic!("base.css {s:?}: {e}"));
                }
                for d in &rule.declarations {
                    let v = css::substitute_vars(&d.value, &vars)
                        .unwrap_or_else(|e| panic!("base.css {}: {e}", d.pos));
                    value::parse(&d.name, &v).unwrap_or_else(|e| panic!("base.css {}: {e}", d.pos));
                }
            }
        }
        assert_eq!(
            base.scheme_vars[&Scheme::Light]
                .keys()
                .collect::<std::collections::BTreeSet<_>>(),
            base.scheme_vars[&Scheme::Dark]
                .keys()
                .collect::<std::collections::BTreeSet<_>>(),
            "the two palettes define the same variables"
        );
    }

    #[test]
    fn cascade_order() {
        let t = theme(
            "gadget { color: blue; padding: 1 }\n\
             .clock { color: red }\n\
             gadget { color: green }",
        );
        let node = Node::root("panel").child("gadget").class("clock");
        let s = t.resolve(&node);
        // .clock beats both `gadget` rules by specificity; the second
        // `gadget` rule beats the first by order.
        assert_eq!(s.color, Some(red()));
        assert_eq!(s.padding, Padding::new(1.0));
        assert_eq!(
            t.resolve(&Node::root("panel").child("gadget")).color,
            Some(Color::from_rgb(0.0, 0.5019608, 0.0))
        );
    }

    #[test]
    fn cascade_order_across_the_rule_index() {
        // Same specificity (0,1,1), one rule indexed under `gadget`, the
        // other under no type: source order decides, either way round.
        let node = Node::root("panel").child("gadget").class("a");
        let blue = Color::from_rgb(0.0, 0.0, 1.0);
        let t = theme("gadget.a { color: red } panel .a { color: blue }");
        assert_eq!(t.resolve(&node).color, Some(blue));
        let t = theme("panel .a { color: blue } gadget.a { color: red }");
        assert_eq!(t.resolve(&node).color, Some(red()));
    }

    #[test]
    fn inheritance() {
        let t = theme("panel { color: red; font-size: 13px; padding: 4 } text { padding: 1 }");
        let node = Node::root("panel").child("slot").child("text");
        let s = t.resolve(&node);
        assert_eq!(s.color, Some(red()));
        assert!(!s.own_color);
        assert_eq!(
            s.text().color,
            None,
            "inherited colour is left to iced's cascade"
        );
        assert_eq!(s.font_size, Some(13.0));
        assert_eq!(s.padding, Padding::new(1.0), "padding doesn't inherit");
        assert_eq!(
            t.resolve(&Node::root("panel").child("slot")).padding,
            Padding::ZERO
        );

        let t = theme("text { color: red }");
        let s = t.resolve(&node);
        assert!(s.own_color);
        assert_eq!(s.text().color, Some(red()));
    }

    #[test]
    fn hover_state() {
        let t = theme("workspace { background: black } workspace:hover { background: red }");
        let node = Node::root("panel").child("workspace");
        assert_eq!(t.resolve(&node).background, Some(Color::BLACK));
        assert_eq!(
            t.resolve(&node.status(Status::Hovered)).background,
            Some(red())
        );
        assert_eq!(
            t.resolve(&node.status(Status::Pressed)).background,
            Some(red())
        );
    }

    #[test]
    fn variables_across_sources() {
        let t = Theme::from_sources(
            &[
                (
                    "a.css",
                    ":root { --accent: blue } workspace { color: var(--accent) }",
                ),
                ("b.css", ":root { --accent: red }"),
            ],
            Scheme::Light,
        )
        .unwrap();
        let s = t.resolve(&Node::root("panel").child("workspace"));
        assert_eq!(
            s.color,
            Some(red()),
            "the later file's variable wins in earlier rules"
        );
    }

    #[test]
    fn scheme_variables_and_root_class() {
        let css = ":root { --fg: blue } :root.dark { --fg: red } \
                   panel { color: var(--fg) } panel.dark { padding: 3px }";
        let light = Theme::from_sources(&[("t.css", css)], Scheme::Light).unwrap();
        let dark = Theme::from_sources(&[("t.css", css)], Scheme::Dark).unwrap();
        let panel = Node::root("panel");
        assert_eq!(
            light.resolve(&panel).color,
            Some(Color::from_rgb(0.0, 0.0, 1.0))
        );
        assert_eq!(light.resolve(&panel).padding.top, 0.0);
        assert_eq!(dark.resolve(&panel).color, Some(red()));
        assert_eq!(dark.resolve(&panel).padding.top, 3.0);
        assert_eq!(dark.scheme(), Scheme::Dark);

        // A later file's plain variable beats an earlier file's scheme one.
        let t = Theme::from_sources(
            &[
                (
                    "base.css",
                    ":root.dark { --fg: blue } panel { color: var(--fg) }",
                ),
                ("user.css", ":root { --fg: red }"),
            ],
            Scheme::Dark,
        )
        .unwrap();
        assert_eq!(t.resolve(&panel).color, Some(red()));
    }

    #[test]
    fn available_themes_dedup_and_sort() {
        let dir = std::env::temp_dir().join(format!("aria-themes-{}", std::process::id()));
        let (a, b) = (dir.join("a/themes"), dir.join("b/themes"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("zeta.css"), "").unwrap();
        fs::write(a.join("alpha.css"), "").unwrap();
        fs::write(b.join("alpha.css"), "").unwrap();
        fs::write(b.join("beta.css"), "").unwrap();
        fs::write(b.join("notes.txt"), "").unwrap();
        let list = available_in(&[dir.join("a"), dir.join("b")]);
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["alpha", "beta", "zeta"]);
        assert_eq!(list[0].1, a.join("alpha.css"), "the first directory wins");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bad_declarations_are_skipped_not_fatal() {
        let t = theme("panel { colour: red; color: red; margin: 1 } :nope { color: red }");
        assert_eq!(t.resolve(&Node::root("panel")).color, Some(red()));
        let err = Theme::from_sources(&[("t.css", "panel { color: red ")], Scheme::Light)
            .err()
            .expect("scanner error");
        assert!(
            err.starts_with("t.css:1:1: "),
            "scanner error is fatal: {err}"
        );
    }

    #[test]
    fn conversions() {
        let t = theme(
            "b { background: #fff; color: black; border: 2px solid red; border-radius: 3; \
             box-shadow: 1 2 3 black; font-family: \"Fira Code\"; font-weight: bold }",
        );
        let s = t.resolve(&Node::root("b"));
        let c = s.container();
        assert_eq!(c.background, Some(Color::WHITE.into()));
        assert_eq!(c.text_color, Some(Color::BLACK));
        assert_eq!(c.border.width, 2.0);
        assert_eq!(c.border.color, red());
        assert_eq!(c.border.radius, Radius::new(3.0));
        assert_eq!(c.shadow.blur_radius, 3.0);
        let b = s.button(red());
        assert_eq!(b.text_color, Color::BLACK);
        let f = s.font().unwrap();
        assert_eq!(f.family, Family::Name("Fira Code"));
        assert_eq!(f.weight, Weight::Bold);

        let s = t.resolve(&Node::root("other"));
        assert_eq!(s.font(), None);
        assert_eq!(s.button(red()).text_color, red());
        assert_eq!(s.container().border.color, Color::TRANSPARENT);
    }

    #[test]
    fn room_for_the_shadow() {
        let t = theme(
            "a { box-shadow: 0 2px 8px black } b { box-shadow: -3 0 2.5 black } \
             c { box-shadow: 0 2px 8px transparent } d { box-shadow: none }",
        );
        let room = |name| t.shadow_room(&Node::root(name));
        assert_eq!(
            room("a"),
            Padding {
                top: 6.0,
                right: 8.0,
                bottom: 10.0,
                left: 8.0
            }
        );
        assert_eq!(
            room("b"),
            Padding {
                top: 3.0,
                right: 0.0,
                bottom: 3.0,
                left: 6.0
            }
        );
        assert_eq!(room("c"), Padding::ZERO);
        assert_eq!(room("d"), Padding::ZERO);
        assert_eq!(room("none"), Padding::ZERO);
    }

    #[test]
    fn interned_font_names_are_reused() {
        let a = intern("Same Font");
        let b = intern("Same Font");
        assert!(std::ptr::eq(a, b));
    }
}
