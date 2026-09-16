//! Styling: a CSS-like theme file, resolved per widget at view time.
//!
//! The built-in `assets/base.css` is always loaded; `[general] style`
//! names a user theme loaded on top of it. Both are parsed and
//! type-checked once, at load ([`css`], [`selector`], [`value`]); a
//! mistake is logged with `file:line:col` and the offending rule or
//! declaration is skipped, so a theme with a typo still mostly works.
//!
//! In `view`, a widget describes where it is in the element tree with a
//! [`Node`] (`panel > slot.start > gadget.clock > button`), and
//! [`Theme::resolve`] gives it the cascaded [`Style`]: every matching
//! rule applied in specificity then source order, walking the path from
//! the root so `color` and the `font-*` properties inherit. The helpers
//! ([`Theme::container`], [`Theme::button`], [`Theme::text`],
//! [`Theme::row`]) do the resolving and return plain iced widgets.
//!
//! Interaction state is part of the node: a button's style closure
//! re-resolves with `node.status(status)`, which is what makes `:hover`
//! and `:active` rules apply.
//!
//! The element tree and the supported properties are documented for
//! theme authors in `assets/base.css`.

mod css;
mod node;
mod selector;
mod value;
mod watch;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use iced::border::Radius;
use iced::font::{Family, Weight};
use iced::widget::{Button, Container, Row, Text, button, container, row, text};
use iced::{Border, Color, Element, Font, Padding, Shadow};

use crate::config::{self, Config, GeneralConfig};
use selector::Selector;
use value::Property;

pub use node::Node;
pub use value::Length;
pub use watch::{Event, watch};

/// Always loaded first; the neutral defaults every theme builds on.
const BASE: &str = include_str!("../../assets/base.css");

/// Bar thickness when no rule sets `min-height` on `panel`.
pub const DEFAULT_PANEL_HEIGHT: f32 = 32.0;

pub struct Theme {
    /// In cascade order: later rules win.
    rules: Vec<Rule>,
    /// User files loaded, to watch for changes.
    files: Vec<PathBuf>,
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
    /// The base stylesheet plus the theme named by `[general] style`, if
    /// any. Never fails: a missing or broken user theme is logged and
    /// the base alone is used (its file still watched).
    pub fn load(config: &Config) -> Self {
        Self::try_load(config).unwrap_or_else(|e| {
            log::error!("{}, using the base theme", e.message);
            Self {
                files: e.files,
                ..Self::default()
            }
        })
    }

    /// Like [`Theme::load`], but a user theme that can't be read or has a
    /// syntax error is an error, so a reload can keep the last good one.
    pub fn try_load(config: &Config) -> Result<Self, LoadError> {
        let general: GeneralConfig = config.section(None);
        let Some(style) = &general.style else {
            return Ok(Self::default());
        };
        let Some(path) = locate(style, config.dir()) else {
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
        log::info!("loading theme {}", path.display());
        let name = path.display().to_string();
        let mut theme =
            Self::from_sources(&[("base.css", BASE), (&name, &text)]).map_err(|message| {
                LoadError {
                    message,
                    files: files.clone(),
                }
            })?;
        theme.files = files;
        Ok(theme)
    }

    /// Build from `(name, text)` sources in cascade order. A scanner
    /// error in any source is an error; everything else (bad selector,
    /// unknown property, bad value) is logged and skipped.
    fn from_sources(sources: &[(&str, &str)]) -> Result<Self, String> {
        let mut sheets = Vec::new();
        for (name, text) in sources {
            let sheet = css::parse(text).map_err(|e| format!("{name}:{e}"))?;
            sheets.push((*name, sheet));
        }
        // Later files override earlier variables, and every file sees
        // the final set.
        let vars = sheets
            .iter()
            .flat_map(|(_, s)| s.vars.iter())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

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
        Ok(Self {
            rules,
            files: Vec::new(),
        })
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    pub fn resolve(&self, node: &Node) -> Style {
        let mut style = Style::default();
        for n in node.path() {
            let mut own = style.inherited();
            for rule in &self.rules {
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

    /// A `container` styled as `node`: padding, size, background,
    /// border, shadow, text colour.
    pub fn container<'a, M: 'a>(
        &self,
        node: &Node,
        content: impl Into<Element<'a, M>>,
    ) -> Container<'a, M> {
        let s = self.resolve(node);
        let mut c = container(content).padding(s.padding);
        if let Some(w) = s.width {
            c = c.width(w);
        }
        if let Some(h) = s.height {
            c = c.height(h);
        }
        c.style(move |_| s.container())
    }

    /// A `button` styled as `node`, re-resolved with `:hover`/`:active`
    /// as iced reports them.
    pub fn button<'a, M: 'a>(
        &'a self,
        node: &Node,
        content: impl Into<Element<'a, M>>,
    ) -> Button<'a, M> {
        let s = self.resolve(node);
        let node = node.clone();
        let mut b = button(content).padding(s.padding);
        if let Some(w) = s.width {
            b = b.width(w);
        }
        if let Some(h) = s.height {
            b = b.height(h);
        }
        b.style(move |theme: &iced::Theme, status| {
            self.resolve(&node.status(status))
                .button(theme.palette().text)
        })
    }

    /// A `text` styled as `node`: font, size, and colour when a rule sets
    /// it on the text itself.
    pub fn text<'a>(&self, node: &Node, content: impl text::IntoFragment<'a>) -> Text<'a> {
        let s = self.resolve(node);
        let mut t = text(content);
        if let Some(font) = s.font() {
            t = t.font(font);
        }
        if let Some(size) = s.font_size {
            t = t.size(size);
        }
        t.style(move |_| s.text())
    }

    /// A `row` styled as `node`: `gap` and padding.
    pub fn row<'a, M: 'a>(
        &self,
        node: &Node,
        children: impl IntoIterator<Item = Element<'a, M>>,
    ) -> Row<'a, M> {
        let s = self.resolve(node);
        row(children).spacing(s.gap).padding(s.padding)
    }
}

impl Default for Theme {
    /// The base stylesheet alone.
    fn default() -> Self {
        Self::from_sources(&[("base.css", BASE)]).expect("base.css is valid")
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
            Property::FontFamily(f) => self.font_family = Some(intern(f)),
            Property::FontSize(s) => self.font_size = Some(*s),
            Property::FontWeight(w) => self.font_weight = Some(*w),
        }
    }

    pub fn border(&self) -> Border {
        Border {
            color: self.border_color.unwrap_or(Color::TRANSPARENT),
            width: self.border_width,
            radius: self.border_radius,
        }
    }

    pub fn container(&self) -> container::Style {
        container::Style {
            text_color: self.color,
            background: self.background.map(Into::into),
            border: self.border(),
            shadow: self.shadow.unwrap_or_default(),
            snap: false,
        }
    }

    /// `fallback` is the text colour when no rule set one.
    pub fn button(&self, fallback: Color) -> button::Style {
        button::Style {
            background: self.background.map(Into::into),
            text_color: self.color.unwrap_or(fallback),
            border: self.border(),
            shadow: self.shadow.unwrap_or_default(),
            snap: false,
        }
    }

    pub fn text(&self) -> text::Style {
        text::Style {
            color: self.color.filter(|_| self.own_color),
        }
    }

    /// `None` when neither family nor weight is set (keep iced's default
    /// font, whatever the surface was created with).
    pub fn font(&self) -> Option<Font> {
        if self.font_family.is_none() && self.font_weight.is_none() {
            return None;
        }
        Some(Font {
            family: self.font_family.map_or(Family::SansSerif, family),
            weight: self.font_weight.unwrap_or_default(),
            ..Font::DEFAULT
        })
    }
}

/// CSS generic families map to iced's; anything else is a font name.
fn family(name: &'static str) -> Family {
    match name {
        "serif" => Family::Serif,
        "sans-serif" => Family::SansSerif,
        "monospace" => Family::Monospace,
        "cursive" => Family::Cursive,
        "fantasy" => Family::Fantasy,
        _ => Family::Name(name),
    }
}

/// `iced::font::Family::Name` wants a `&'static str`; theme font names
/// are leaked once each and reused across reloads.
fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(n) = names.iter().find(|n| **n == name) {
        return n;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.push(leaked);
    leaked
}

/// Where `[general] style = <value>` points: a path if it looks like
/// one (has a `/` or ends in `.css`, relative to the config file's
/// directory), else `themes/<name>.css` under the config dirs, the data
/// dirs, then the source tree's `assets/`.
fn locate(style: &str, config_dir: Option<&Path>) -> Option<PathBuf> {
    if style.contains('/') || style.ends_with(".css") {
        let path = Path::new(style);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            config_dir.unwrap_or(Path::new(".")).join(path)
        };
        return path.is_file().then_some(path);
    }
    config::config_dirs()
        .into_iter()
        .chain(config::data_dirs())
        .chain(std::iter::once(config::dev_assets_dir()))
        .map(|dir| dir.join("themes").join(format!("{style}.css")))
        .find(|f| f.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::button::Status;

    fn theme(css: &str) -> Theme {
        Theme::from_sources(&[("test.css", css)]).expect("valid css")
    }

    fn red() -> Color {
        Color::from_rgb(1.0, 0.0, 0.0)
    }

    #[test]
    fn base_stylesheet_is_valid() {
        let base = css::parse(BASE).expect("base.css parses");
        for rule in &base.rules {
            for s in &rule.selectors {
                Selector::parse(s).unwrap_or_else(|e| panic!("base.css {s:?}: {e}"));
            }
            for d in &rule.declarations {
                let v = css::substitute_vars(&d.value, &base.vars)
                    .unwrap_or_else(|e| panic!("base.css {}: {e}", d.pos));
                value::parse(&d.name, &v).unwrap_or_else(|e| panic!("base.css {}: {e}", d.pos));
            }
        }
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
        let t = Theme::from_sources(&[
            (
                "a.css",
                ":root { --accent: blue } workspace { color: var(--accent) }",
            ),
            ("b.css", ":root { --accent: red }"),
        ])
        .unwrap();
        let s = t.resolve(&Node::root("panel").child("workspace"));
        assert_eq!(
            s.color,
            Some(red()),
            "the later file's variable wins in earlier rules"
        );
    }

    #[test]
    fn bad_declarations_are_skipped_not_fatal() {
        let t = theme("panel { colour: red; color: red; margin: 1 } :nope { color: red }");
        assert_eq!(t.resolve(&Node::root("panel")).color, Some(red()));
        let err = Theme::from_sources(&[("t.css", "panel { color: red ")])
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
    fn interned_font_names_are_reused() {
        let a = intern("Same Font");
        let b = intern("Same Font");
        assert!(std::ptr::eq(a, b));
    }
}
