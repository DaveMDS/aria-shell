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
//! [`Theme::row`]) do the resolving and return plain iced widgets.
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

mod css;
mod node;
mod selector;
mod value;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use iced::border::Radius;
use iced::font::{Family, Weight};
use iced::widget::{
    Button, Column, Container, Row, Text, TextInput, button, column, container, row, slider, text,
    text_input,
};
use iced::{Alignment, Border, Color, Element, Font, Padding, Shadow, Size};

use crate::config::{self, Config};
use value::Property;

pub use node::Node;
pub use selector::{Selector, node_from_path};
pub use value::Length;

use iced::Length as IcedLength;

/// A slider's rail thickness and handle diameter when the theme doesn't
/// say.
const DEFAULT_RAIL: f32 = 4.0;
const DEFAULT_HANDLE: f32 = 12.0;

/// Always loaded first; the neutral defaults every theme builds on.
const BASE: &str = include_str!("../../assets/base.css");

/// Bar thickness when no rule sets `min-height` on `panel`.
pub const DEFAULT_PANEL_HEIGHT: f32 = 32.0;

/// iced's text size when no rule sets `font-size`.
const DEFAULT_FONT_SIZE: f32 = 16.0;

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
        Ok(Self {
            rules,
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
    /// border, shadow, text colour. Content is centred on an axis the
    /// theme sizes (`height: fill` shouldn't leave the content at the
    /// top).
    pub fn container<'a, M: 'a>(
        &self,
        node: &Node,
        content: impl Into<Element<'a, M>>,
    ) -> Container<'a, M> {
        let s = self.resolve(node);
        let mut c = container(content).id(widget_id(node)).padding(s.padding);
        if let Some(w) = s.width {
            c = c.width(w).align_x(Alignment::Center);
        }
        if let Some(h) = s.height {
            c = c.height(h).align_y(Alignment::Center);
        }
        c.style(move |_| s.container())
    }

    /// A plain `container` tagged with `node`'s path and nothing else,
    /// for a widget that takes no id of its own (a `text_input`, a
    /// `canvas`) to be found by `debug widgets`.
    pub fn tag<'a, M: 'a>(
        &self,
        node: &Node,
        content: impl Into<Element<'a, M>>,
    ) -> Container<'a, M> {
        container(content).id(widget_id(node))
    }

    /// A `button` styled as `node`, re-resolved with `:hover`/`:active`
    /// as iced reports them. As for [`Theme::container`], content is
    /// centred on a themed axis (iced's button lays it out top-left).
    /// The content sits in a container tagged with the node path (iced
    /// buttons take no id), so `debug widgets` reports the button's
    /// content area.
    pub fn button<'a, M: 'a>(
        &'a self,
        node: &Node,
        content: impl Into<Element<'a, M>>,
    ) -> Button<'a, M> {
        let s = self.resolve(node);
        let mut inner = container(content).id(widget_id(node));
        if s.width.is_some() {
            inner = inner.width(IcedLength::Fill).align_x(Alignment::Center);
        }
        if s.height.is_some() {
            inner = inner.height(IcedLength::Fill).align_y(Alignment::Center);
        }
        let node = node.clone();
        let mut b = button(inner).padding(s.padding);
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
    /// it on the text itself. Shaped with per-glyph font fallback, so a
    /// theme font missing a glyph (an icon font, a nerd font without
    /// some script) doesn't show boxes.
    pub fn text<'a>(&self, node: &Node, content: impl text::IntoFragment<'a>) -> Text<'a> {
        let s = self.resolve(node);
        let mut t = text(content).shaping(text::Shaping::Advanced);
        if let Some(font) = s.font() {
            t = t.font(font);
        }
        if let Some(size) = s.font_size {
            t = t.size(size);
        }
        t.style(move |_| s.text())
    }

    /// A `text_input` styled as `node`: background, border, font and
    /// colours (`color` is the value, the placeholder is it faded, the
    /// selection is the border colour), re-resolved with `:hover` /
    /// `:focus` as iced reports them.
    pub fn text_input<'a, M: Clone + 'a>(
        &'a self,
        node: &Node,
        placeholder: &str,
        value: &str,
    ) -> TextInput<'a, M> {
        let s = self.resolve(node);
        let node = node.clone();
        let mut t = text_input(placeholder, value).padding(s.padding);
        if let Some(font) = s.font() {
            t = t.font(font);
        }
        if let Some(size) = s.font_size {
            t = t.size(size);
        }
        if let Some(w) = s.width {
            t = t.width(w);
        }
        t.style(move |theme: &iced::Theme, status| {
            self.resolve(&node.input_status(status))
                .text_input(theme.palette().text)
        })
    }

    /// A `slider` styled as `node`: the rail is `height` thick, its
    /// filled part `color`, the rest `background`, with the node's
    /// border; the handle is the `handle` child: a circle of its
    /// `width`, its `background` (the rail's `color` when unset) and
    /// border. Re-resolved with `:hover` / `:active` (dragged). Comes
    /// in a container tagged with the node path (iced sliders take no
    /// id), filling the width.
    pub fn slider<'a, M: Clone + 'a>(
        &'a self,
        node: &Node,
        range: std::ops::RangeInclusive<f32>,
        value: f32,
        step: f32,
        on_change: impl Fn(f32) -> M + 'a,
    ) -> Container<'a, M> {
        let s = self.resolve(node);
        let handle_node = node.child("handle");
        let rail = match s.height {
            Some(Length::Px(px)) => px,
            _ => DEFAULT_RAIL,
        };
        let diameter = match self.resolve(&handle_node).width {
            Some(Length::Px(px)) => px,
            _ => DEFAULT_HANDLE,
        };
        let id = widget_id(node);
        let node = node.clone();
        let slider = slider(range, value, on_change)
            .step(step)
            .width(IcedLength::Fill)
            .height(rail.max(diameter))
            .style(move |theme: &iced::Theme, status| {
                let n = node.slider_status(status);
                let s = self.resolve(&n);
                let h = self.resolve(&n.child("handle"));
                let filled = s.color.unwrap_or(theme.palette().primary);
                slider::Style {
                    rail: slider::Rail {
                        backgrounds: (
                            filled.into(),
                            s.background.unwrap_or(Color { a: 0.2, ..filled }).into(),
                        ),
                        width: rail,
                        border: s.border(),
                    },
                    handle: slider::Handle {
                        shape: slider::HandleShape::Circle {
                            radius: diameter / 2.0,
                        },
                        background: h.background.unwrap_or(filled).into(),
                        border_width: h.border_width,
                        border_color: h.border_color.unwrap_or(Color::TRANSPARENT),
                    },
                }
            });
        container(slider).id(id).width(IcedLength::Fill)
    }

    /// The size `text` takes as a [`Theme::text`] of `node` (font and
    /// size from the theme; iced's default 16px and 1.3 line height
    /// otherwise), for surfaces that must be sized before layout.
    pub fn measure(&self, node: &Node, content: &str) -> Size {
        use iced::advanced::text::Paragraph as _;
        let s = self.resolve(node);
        let size = s.font_size.unwrap_or(DEFAULT_FONT_SIZE);
        let paragraph =
            iced::advanced::graphics::text::Paragraph::with_text(iced::advanced::Text {
                content,
                bounds: Size::INFINITE,
                size: size.into(),
                line_height: text::LineHeight::default(),
                font: s.font().unwrap_or_default(),
                align_x: text::Alignment::Left,
                align_y: iced::alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::None,
            });
        paragraph.min_bounds()
    }

    /// The size `content` takes as a [`Theme::text`] of `node` wrapped
    /// at words within `width` (as `.wrapping(Wrapping::Word)` in a
    /// `width`-wide container lays it out).
    pub fn measure_in(&self, node: &Node, content: &str, width: f32) -> Size {
        use iced::advanced::text::Paragraph as _;
        let s = self.resolve(node);
        let size = s.font_size.unwrap_or(DEFAULT_FONT_SIZE);
        let paragraph =
            iced::advanced::graphics::text::Paragraph::with_text(iced::advanced::Text {
                content,
                bounds: Size::new(width, f32::INFINITY),
                size: size.into(),
                line_height: text::LineHeight::default(),
                font: s.font().unwrap_or_default(),
                align_x: text::Alignment::Left,
                align_y: iced::alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::Word,
            });
        paragraph.min_bounds()
    }

    /// The height of one line of text of `node`, as [`Theme::measure`]
    /// sees it.
    pub fn line_height(&self, node: &Node) -> f32 {
        let size = self.resolve(node).font_size.unwrap_or(DEFAULT_FONT_SIZE);
        text::LineHeight::default().to_absolute(size.into()).0
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

    /// A `column` styled as `node`: `gap` and padding.
    pub fn column<'a, M: 'a>(
        &self,
        node: &Node,
        children: impl IntoIterator<Item = Element<'a, M>>,
    ) -> Column<'a, M> {
        let s = self.resolve(node);
        column(children).spacing(s.gap).padding(s.padding)
    }
}

/// The `widget::Id` of the widget built for `node`: its element path.
pub(crate) fn widget_id(node: &Node) -> iced::widget::Id {
    iced::widget::Id::from(format!("{node:?}"))
}

/// The element path back from a [`widget_id`], `None` for other ids.
/// `Id` keeps its string private; its `Debug` form is
/// `Id(Custom("..."))` with `str` escaping, undone here.
pub fn widget_path(id: &iced::widget::Id) -> Option<String> {
    let debug = format!("{id:?}");
    let inner = debug.strip_prefix("Id(Custom(\"")?.strip_suffix("\"))")?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        out.push(match c {
            '\\' => chars.next()?,
            c => c,
        });
    }
    Some(out)
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

    /// `fallback` is the text colour when no rule set one.
    pub fn text_input(&self, fallback: Color) -> text_input::Style {
        let value = self.color.unwrap_or(fallback);
        text_input::Style {
            background: self.background.unwrap_or(Color::TRANSPARENT).into(),
            border: self.border(),
            icon: value,
            placeholder: Color {
                a: value.a * 0.5,
                ..value
            },
            value,
            selection: self.border_color.unwrap_or(Color { a: 0.3, ..value }),
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

const GENERIC_FAMILIES: [&str; 5] = ["serif", "sans-serif", "monospace", "cursive", "fantasy"];

/// The first of `names` that is a generic family or an installed font,
/// else the first one (and the font system falls back on its own).
/// Only asks the font database when there's a choice to make.
fn pick_family(names: &[String]) -> &str {
    if names.len() == 1 {
        return &names[0];
    }
    let picked = names
        .iter()
        .find(|n| GENERIC_FAMILIES.contains(&n.as_str()) || font_installed(n));
    match picked {
        Some(n) => n,
        None => {
            log::warn!("none of the fonts {names:?} is installed");
            &names[0]
        }
    }
}

/// Is a font family with this name installed? Uses the renderer's font
/// database (it loads the system fonts on first use, which iced would
/// do at the first text draw anyway).
fn font_installed(name: &str) -> bool {
    let mut system = iced::advanced::graphics::text::font_system()
        .write()
        .unwrap_or_else(|e| e.into_inner());
    system
        .raw()
        .db()
        .faces()
        .any(|face| face.families.iter().any(|(family, _)| family == name))
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
    theme_dirs()
        .into_iter()
        .map(|dir| dir.join("themes").join(format!("{style}.css")))
        .find(|f| f.is_file())
}

#[cfg(test)]
mod tests {
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
    fn interned_font_names_are_reused() {
        let a = intern("Same Font");
        let b = intern("Same Font");
        assert!(std::ptr::eq(a, b));
    }
}
