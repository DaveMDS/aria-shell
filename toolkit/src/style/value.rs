//! Property values: what a declaration may say, parsed from its tokens
//! once its `var()`s are substituted. Shorthands expand to longhands
//! here, so the cascade only ever sees longhands.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cssparser::{BasicParseErrorKind, ParseError, ParseErrorKind, Parser, ToCss, Token, color};
use iced::Color;
use rustc_hash::FxHashMap;

use super::images::Picture;

pub(crate) type Error = ParseError<String>;

fn fail<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(ParseError::custom(message.into()))
}

/// What went wrong, in words.
pub(crate) fn describe(error: Error) -> String {
    match error.kind {
        ParseErrorKind::Custom(message) => message,
        ParseErrorKind::Basic(BasicParseErrorKind::EndOfInput) => "the value ends too soon".into(),
        ParseErrorKind::Basic(BasicParseErrorKind::UnexpectedToken) => "unexpected token".into(),
        ParseErrorKind::Basic(kind) => kind.to_string(),
    }
}

/// `width`, `height`: the border box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    /// The content's.
    Auto,
    Px(f32),
    /// The free room on the main axis, stretched on the cross one.
    Fill,
}

/// `min-width`, `min-height`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MinSize {
    /// CSS's automatic minimum: the content's own.
    Auto,
    Px(f32),
}

/// A side of a box, in CSS's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

const SIDES: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

/// A corner of a box, in CSS's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

const CORNERS: [Corner; 4] = [
    Corner::TopLeft,
    Corner::TopRight,
    Corner::BottomRight,
    Corner::BottomLeft,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineStyle {
    None,
    Solid,
}

/// A colour as declared: `currentcolor` is the element's own `color`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ColorValue {
    Rgba(Color),
    Current,
}

/// `flex-direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Row,
    Column,
}

/// `align-items`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Stretch,
    Start,
    Center,
    End,
}

/// `justify-content`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Justify {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ShadowValue {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub color: ColorValue,
}

/// `box-shadow`: one outer shadow and one inset at most.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Shadows {
    pub outer: Option<ShadowValue>,
    /// Unblurred: `blur` is 0.
    pub inset: Option<ShadowValue>,
}

/// One name of a `font-family` list.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FamilyName {
    Named(Arc<str>),
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
}

/// `font-style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WeightValue {
    Absolute(u16),
    Bolder,
    Lighter,
}

/// `line-height`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    Normal,
    /// Times the font size.
    Number(f32),
    Px(f32),
}

/// `white-space`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteSpace {
    /// Wraps at words.
    Normal,
    Nowrap,
}

/// `text-overflow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOverflow {
    Clip,
    Ellipsis,
}

/// `text-align`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Start,
    End,
    Left,
    Right,
    Center,
}

/// `text-transform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTransform {
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

/// `text-box-trim`: which ends of a text's box are cut to its letters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextBoxTrim {
    /// The first line's top.
    pub start: bool,
    /// The last line's bottom.
    pub end: bool,
}

/// `text-box-edge`'s over edge: where a trimmed top is cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeOver {
    /// The font's ascent.
    Text,
    /// The capitals' top.
    Cap,
    /// The lowercase letters' top.
    Ex,
}

/// `text-box-edge`'s under edge: where a trimmed bottom is cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeUnder {
    /// The font's descent.
    Text,
    /// The baseline.
    Alphabetic,
}

/// `text-box-edge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextBoxEdge {
    pub over: EdgeOver,
    pub under: EdgeUnder,
}

impl TextBoxEdge {
    /// `auto`: the font's ascent and descent.
    pub const AUTO: Self = Self {
        over: EdgeOver::Text,
        under: EdgeUnder::Text,
    };
}

/// `transform`: a uniform scale, then a translation, about the
/// `transform-origin` (iced's `Transformation` has nothing else).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub scale: f32,
    pub x: f32,
    pub y: f32,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        scale: 1.0,
        x: 0.0,
        y: 0.0,
    };

    /// `self` applied after `inner`: CSS composes a list's functions
    /// that way, the last one applied first.
    fn then(self, inner: Self) -> Self {
        Self {
            scale: self.scale * inner.scale,
            x: self.scale * inner.x + self.x,
            y: self.scale * inner.y + self.y,
        }
    }
}

/// One coordinate of `transform-origin`: a length from the border
/// box's left or top, or a fraction of its size (the keywords).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Origin {
    Px(f32),
    Fraction(f32),
}

/// `background-size`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackgroundSize {
    /// The image's own: a pixel of it a logical pixel.
    Auto,
    Cover,
    Contain,
    /// A width and a height, `None` for `auto` (the image's ratio kept).
    Px(Option<f32>, Option<f32>),
}

/// `background-repeat`: whether the image tiles across, down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Repeat {
    pub x: bool,
    pub y: bool,
}

/// `object-fit`: how an image fills its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectFit {
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

/// What `url()`s are loaded from: the sheet's directory, and what's
/// been loaded already (each file once a load).
pub(crate) struct Assets {
    pub dir: Option<PathBuf>,
    pub loaded: FxHashMap<PathBuf, Picture>,
}

/// A longhand property, as a CSS-wide keyword (`inherit`, `initial`,
/// `unset`) names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Prop {
    Width,
    Height,
    MinWidth,
    MinHeight,
    MaxWidth,
    MaxHeight,
    Margin(Side),
    Padding(Side),
    BorderWidth(Side),
    BorderStyle(Side),
    BorderColor(Side),
    Radius(Corner),
    RowGap,
    ColumnGap,
    FlexDirection,
    AlignItems,
    JustifyContent,
    Background,
    Color,
    BoxShadow,
    Opacity,
    FontFamily,
    FontSize,
    FontWeight,
    FontStyle,
    LineHeight,
    WhiteSpace,
    TextOverflow,
    TextAlign,
    TextTransform,
    TextBoxTrim,
    TextBoxEdge,
    OutlineWidth,
    OutlineStyle,
    OutlineColor,
    OutlineOffset,
    Transform,
    TransformOrigin,
    BackgroundImage,
    BackgroundSize,
    BackgroundPosition,
    BackgroundRepeat,
    BorderImageSource,
    BorderImageSlice,
    ObjectFit,
}

impl Prop {
    /// Inherited by default, as in CSS.
    pub fn inherited(self) -> bool {
        matches!(
            self,
            Prop::Color
                | Prop::FontFamily
                | Prop::FontSize
                | Prop::FontWeight
                | Prop::FontStyle
                | Prop::LineHeight
                | Prop::WhiteSpace
                | Prop::TextAlign
                | Prop::TextTransform
                | Prop::TextBoxEdge
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wide {
    Inherit,
    Initial,
    Unset,
}

/// One longhand declaration.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Decl {
    Width(Size),
    Height(Size),
    MinWidth(MinSize),
    MinHeight(MinSize),
    /// `f32::INFINITY` for `none`.
    MaxWidth(f32),
    MaxHeight(f32),
    Margin(Side, f32),
    Padding(Side, f32),
    BorderWidth(Side, f32),
    BorderStyle(Side, LineStyle),
    BorderColor(Side, ColorValue),
    Radius(Corner, f32),
    RowGap(f32),
    ColumnGap(f32),
    FlexDirection(Direction),
    AlignItems(Align),
    JustifyContent(Justify),
    Background(ColorValue),
    Color(ColorValue),
    BoxShadow(Shadows),
    Opacity(f32),
    FontFamily(Arc<[FamilyName]>),
    FontSize(f32),
    FontWeight(WeightValue),
    FontStyle(FontStyle),
    LineHeight(LineHeight),
    WhiteSpace(WhiteSpace),
    TextOverflow(TextOverflow),
    TextAlign(TextAlign),
    TextTransform(TextTransform),
    TextBoxTrim(TextBoxTrim),
    TextBoxEdge(TextBoxEdge),
    OutlineWidth(f32),
    OutlineStyle(LineStyle),
    OutlineColor(ColorValue),
    OutlineOffset(f32),
    Transform(Option<Transform>),
    TransformOrigin(Origin, Origin),
    BackgroundImage(Option<Picture>),
    BackgroundSize(BackgroundSize),
    BackgroundPosition(Origin, Origin),
    BackgroundRepeat(Repeat),
    BorderImageSource(Option<Picture>),
    /// Top, right, bottom, left, in image pixels; `fill` keeps the middle.
    BorderImageSlice([f32; 4], bool),
    ObjectFit(ObjectFit),
    Wide(Prop, Wide),
}

/// The longhands a property name sets, `None` for an unknown name.
fn longhands(name: &str) -> Option<Vec<Prop>> {
    let sides = |f: fn(Side) -> Prop| SIDES.map(f).to_vec();
    let side = |s: &str| match s {
        "top" => Some(Side::Top),
        "right" => Some(Side::Right),
        "bottom" => Some(Side::Bottom),
        "left" => Some(Side::Left),
        _ => None,
    };
    let border = || {
        [
            sides(Prop::BorderWidth),
            sides(Prop::BorderStyle),
            sides(Prop::BorderColor),
        ]
        .concat()
    };
    Some(match name {
        "width" => vec![Prop::Width],
        "height" => vec![Prop::Height],
        "min-width" => vec![Prop::MinWidth],
        "min-height" => vec![Prop::MinHeight],
        "max-width" => vec![Prop::MaxWidth],
        "max-height" => vec![Prop::MaxHeight],
        "margin" => sides(Prop::Margin),
        "padding" => sides(Prop::Padding),
        "border" => border(),
        "border-width" => sides(Prop::BorderWidth),
        "border-style" => sides(Prop::BorderStyle),
        "border-color" => sides(Prop::BorderColor),
        "border-radius" => CORNERS.map(Prop::Radius).to_vec(),
        "border-top-left-radius" => vec![Prop::Radius(Corner::TopLeft)],
        "border-top-right-radius" => vec![Prop::Radius(Corner::TopRight)],
        "border-bottom-right-radius" => vec![Prop::Radius(Corner::BottomRight)],
        "border-bottom-left-radius" => vec![Prop::Radius(Corner::BottomLeft)],
        "gap" => vec![Prop::RowGap, Prop::ColumnGap],
        "row-gap" => vec![Prop::RowGap],
        "column-gap" => vec![Prop::ColumnGap],
        "flex-direction" => vec![Prop::FlexDirection],
        "align-items" => vec![Prop::AlignItems],
        "justify-content" => vec![Prop::JustifyContent],
        "background" => vec![
            Prop::Background,
            Prop::BackgroundImage,
            Prop::BackgroundSize,
            Prop::BackgroundPosition,
            Prop::BackgroundRepeat,
        ],
        "background-color" => vec![Prop::Background],
        "background-image" => vec![Prop::BackgroundImage],
        "background-size" => vec![Prop::BackgroundSize],
        "background-position" => vec![Prop::BackgroundPosition],
        "background-repeat" => vec![Prop::BackgroundRepeat],
        "border-image" => vec![Prop::BorderImageSource, Prop::BorderImageSlice],
        "border-image-source" => vec![Prop::BorderImageSource],
        "border-image-slice" => vec![Prop::BorderImageSlice],
        "object-fit" => vec![Prop::ObjectFit],
        "color" => vec![Prop::Color],
        "box-shadow" => vec![Prop::BoxShadow],
        "opacity" => vec![Prop::Opacity],
        "font-family" => vec![Prop::FontFamily],
        "font-size" => vec![Prop::FontSize],
        "font-weight" => vec![Prop::FontWeight],
        "font-style" => vec![Prop::FontStyle],
        "line-height" => vec![Prop::LineHeight],
        "white-space" => vec![Prop::WhiteSpace],
        "text-overflow" => vec![Prop::TextOverflow],
        "text-align" => vec![Prop::TextAlign],
        "text-transform" => vec![Prop::TextTransform],
        "text-box" => vec![Prop::TextBoxTrim, Prop::TextBoxEdge],
        "text-box-trim" => vec![Prop::TextBoxTrim],
        "text-box-edge" => vec![Prop::TextBoxEdge],
        "outline" => vec![Prop::OutlineWidth, Prop::OutlineStyle, Prop::OutlineColor],
        "outline-width" => vec![Prop::OutlineWidth],
        "outline-style" => vec![Prop::OutlineStyle],
        "outline-color" => vec![Prop::OutlineColor],
        "outline-offset" => vec![Prop::OutlineOffset],
        "transform" => vec![Prop::Transform],
        "transform-origin" => vec![Prop::TransformOrigin],
        _ => {
            // margin-top, padding-left, border-bottom, border-left-width, ...
            let (head, rest) = name.split_once('-')?;
            let (s, tail) = rest
                .split_once('-')
                .map_or((rest, None), |(s, t)| (s, Some(t)));
            let s = side(s)?;
            match (head, tail) {
                ("margin", None) => vec![Prop::Margin(s)],
                ("padding", None) => vec![Prop::Padding(s)],
                ("border", None) => vec![
                    Prop::BorderWidth(s),
                    Prop::BorderStyle(s),
                    Prop::BorderColor(s),
                ],
                ("border", Some("width")) => vec![Prop::BorderWidth(s)],
                ("border", Some("style")) => vec![Prop::BorderStyle(s)],
                ("border", Some("color")) => vec![Prop::BorderColor(s)],
                _ => return None,
            }
        }
    })
}

/// The longhand declarations of `name: value`; `url()`s loaded from
/// `assets`.
pub(crate) fn parse(name: &str, value: &str, assets: &mut Assets) -> Result<Vec<Decl>, String> {
    let name = name.to_ascii_lowercase();
    let Some(props) = longhands(&name) else {
        return Err(format!("unknown property `{name}`"));
    };
    let mut p = Parser::new(value);
    let decls = match p.try_parse(wide) {
        Ok(wide) => props.iter().map(|&prop| Decl::Wide(prop, wide)).collect(),
        Err(_) => value_of(&name, &props, &mut p, assets).map_err(describe)?,
    };
    let rest = p.position();
    while p.next_including_whitespace_and_comments().is_ok() {}
    let rest = p.slice_from(rest).trim();
    if !rest.is_empty() {
        return Err(format!("`{rest}` after the value"));
    }
    Ok(decls)
}

fn wide(p: &mut Parser) -> Result<Wide, Error> {
    keyword(
        p,
        "a CSS-wide keyword",
        &[
            ("inherit", Wide::Inherit),
            ("initial", Wide::Initial),
            ("unset", Wide::Unset),
        ],
    )
}

fn value_of(
    name: &str,
    props: &[Prop],
    p: &mut Parser,
    assets: &mut Assets,
) -> Result<Vec<Decl>, Error> {
    // One value, for each of the props: margin-top, border-left-width, ...
    let each = |p: &mut Parser, prop: Prop| -> Result<Decl, Error> {
        Ok(match prop {
            Prop::Margin(s) => Decl::Margin(s, length(p)?),
            Prop::Padding(s) => Decl::Padding(s, non_negative(p)?),
            Prop::BorderWidth(s) => Decl::BorderWidth(s, border_width(p)?),
            Prop::BorderStyle(s) => Decl::BorderStyle(s, line_style(p)?),
            Prop::BorderColor(s) => Decl::BorderColor(s, color_value(p)?),
            Prop::Radius(c) => Decl::Radius(c, non_negative(p)?),
            _ => unreachable!("not a one-value longhand: {prop:?}"),
        })
    };
    Ok(match name {
        "width" => vec![Decl::Width(size(p)?)],
        "height" => vec![Decl::Height(size(p)?)],
        "min-width" => vec![Decl::MinWidth(min_size(p)?)],
        "min-height" => vec![Decl::MinHeight(min_size(p)?)],
        "max-width" => vec![Decl::MaxWidth(max_size(p)?)],
        "max-height" => vec![Decl::MaxHeight(max_size(p)?)],
        "margin" => four(p, length)?
            .into_iter()
            .zip(SIDES)
            .map(|(v, s)| Decl::Margin(s, v))
            .collect(),
        "padding" => four(p, non_negative)?
            .into_iter()
            .zip(SIDES)
            .map(|(v, s)| Decl::Padding(s, v))
            .collect(),
        "border-width" => four(p, border_width)?
            .into_iter()
            .zip(SIDES)
            .map(|(v, s)| Decl::BorderWidth(s, v))
            .collect(),
        "border-style" => four(p, line_style)?
            .into_iter()
            .zip(SIDES)
            .map(|(v, s)| Decl::BorderStyle(s, v))
            .collect(),
        "border-color" => four(p, color_value)?
            .into_iter()
            .zip(SIDES)
            .map(|(v, s)| Decl::BorderColor(s, v))
            .collect(),
        "border-radius" => {
            let radii = four(p, non_negative)?;
            if p.try_parse(|p| p.expect_delim('/')).is_ok() {
                return fail("elliptical corners (`/`) aren't supported");
            }
            radii
                .into_iter()
                .zip(CORNERS)
                .map(|(v, c)| Decl::Radius(c, v))
                .collect()
        }
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let (width, style, color) = border(p)?;
            let sides: Vec<Side> = props
                .iter()
                .filter_map(|prop| match prop {
                    Prop::BorderWidth(s) => Some(*s),
                    _ => None,
                })
                .collect();
            sides
                .into_iter()
                .flat_map(|s| {
                    [
                        Decl::BorderWidth(s, width),
                        Decl::BorderStyle(s, style),
                        Decl::BorderColor(s, color),
                    ]
                })
                .collect()
        }
        "gap" => {
            let row = non_negative(p)?;
            let column = p.try_parse(non_negative).unwrap_or(row);
            vec![Decl::RowGap(row), Decl::ColumnGap(column)]
        }
        "row-gap" => vec![Decl::RowGap(non_negative(p)?)],
        "column-gap" => vec![Decl::ColumnGap(non_negative(p)?)],
        "flex-direction" => vec![Decl::FlexDirection(keyword(
            p,
            "a direction",
            &[("row", Direction::Row), ("column", Direction::Column)],
        )?)],
        "align-items" => vec![Decl::AlignItems(keyword(
            p,
            "an alignment",
            &[
                ("stretch", Align::Stretch),
                ("normal", Align::Stretch),
                ("start", Align::Start),
                ("flex-start", Align::Start),
                ("center", Align::Center),
                ("end", Align::End),
                ("flex-end", Align::End),
            ],
        )?)],
        "justify-content" => vec![Decl::JustifyContent(keyword(
            p,
            "a justification",
            &[
                ("normal", Justify::Start),
                ("start", Justify::Start),
                ("flex-start", Justify::Start),
                ("center", Justify::Center),
                ("end", Justify::End),
                ("flex-end", Justify::End),
            ],
        )?)],
        "background" => {
            // `<color> || <image> || <repeat>`: what's left out is its
            // initial value, as a CSS shorthand has it.
            let (mut color, mut image, mut repeat) = (None, None, None);
            for _ in 0..3 {
                if image.is_none()
                    && let Ok(i) = p.try_parse(|p| url(p, assets))
                {
                    image = Some(i);
                } else if color.is_none()
                    && let Ok(c) = p.try_parse(color_value)
                {
                    color = Some(c);
                } else if repeat.is_none()
                    && let Ok(r) = p.try_parse(background_repeat)
                {
                    repeat = Some(r);
                } else {
                    break;
                }
            }
            if color.is_none() && image.is_none() && repeat.is_none() {
                color_value(p)?;
            }
            vec![
                Decl::Background(color.unwrap_or(ColorValue::Rgba(Color::TRANSPARENT))),
                Decl::BackgroundImage(image.flatten()),
                Decl::BackgroundSize(BackgroundSize::Auto),
                Decl::BackgroundPosition(Origin::Fraction(0.0), Origin::Fraction(0.0)),
                Decl::BackgroundRepeat(repeat.unwrap_or(Repeat { x: true, y: true })),
            ]
        }
        "background-image" => vec![Decl::BackgroundImage(url(p, assets)?)],
        "background-size" => vec![Decl::BackgroundSize(background_size(p)?)],
        "background-position" => {
            let (x, y) = transform_origin(p)?;
            vec![Decl::BackgroundPosition(x, y)]
        }
        "background-repeat" => vec![Decl::BackgroundRepeat(background_repeat(p)?)],
        "border-image" => {
            let mut source = None;
            let mut slice = None;
            for _ in 0..2 {
                if source.is_none()
                    && let Ok(s) = p.try_parse(|p| url(p, assets))
                {
                    source = Some(s);
                } else if slice.is_none()
                    && let Ok(s) = p.try_parse(border_image_slice)
                {
                    slice = Some(s);
                } else {
                    break;
                }
            }
            if source.is_none() && slice.is_none() {
                url(p, assets)?;
            }
            let (slices, fill) = slice.unwrap_or(FULL_SLICE);
            vec![
                Decl::BorderImageSource(source.flatten()),
                Decl::BorderImageSlice(slices, fill),
            ]
        }
        "border-image-source" => vec![Decl::BorderImageSource(url(p, assets)?)],
        "border-image-slice" => {
            let (slices, fill) = border_image_slice(p)?;
            vec![Decl::BorderImageSlice(slices, fill)]
        }
        "object-fit" => vec![Decl::ObjectFit(keyword(
            p,
            "an object fit",
            &[
                ("fill", ObjectFit::Fill),
                ("contain", ObjectFit::Contain),
                ("cover", ObjectFit::Cover),
                ("none", ObjectFit::None),
                ("scale-down", ObjectFit::ScaleDown),
            ],
        )?)],
        "background-color" => vec![Decl::Background(color_value(p)?)],
        "color" => vec![Decl::Color(color_value(p)?)],
        "box-shadow" => vec![Decl::BoxShadow(shadows(p)?)],
        "opacity" => vec![Decl::Opacity(match p.next()?.clone() {
            Token::Number { value, .. } => value.clamp(0.0, 1.0),
            Token::Percentage { unit_value, .. } => unit_value.clamp(0.0, 1.0),
            t => return fail(format!("expected a number, found `{}`", t.to_css_string())),
        })],
        "font-family" => vec![Decl::FontFamily(p.parse_comma_separated(family)?.into())],
        "font-size" => vec![Decl::FontSize(non_negative(p)?)],
        "font-weight" => vec![Decl::FontWeight(font_weight(p)?)],
        "font-style" => vec![Decl::FontStyle(keyword(
            p,
            "a font style",
            &[
                ("normal", FontStyle::Normal),
                ("italic", FontStyle::Italic),
                ("oblique", FontStyle::Oblique),
            ],
        )?)],
        "line-height" => vec![Decl::LineHeight(match p.next()?.clone() {
            Token::Ident(name) if name.eq_ignore_ascii_case("normal") => LineHeight::Normal,
            Token::Number { value, .. } if value >= 0.0 => LineHeight::Number(value),
            Token::Dimension { value, unit, .. }
                if unit.eq_ignore_ascii_case("px") && value >= 0.0 =>
            {
                LineHeight::Px(value)
            }
            t => {
                return fail(format!(
                    "expected `normal`, a number or a length in px, found `{}`",
                    t.to_css_string()
                ));
            }
        })],
        "white-space" => vec![Decl::WhiteSpace(keyword(
            p,
            "a white-space mode",
            &[
                ("normal", WhiteSpace::Normal),
                ("nowrap", WhiteSpace::Nowrap),
            ],
        )?)],
        "text-overflow" => vec![Decl::TextOverflow(keyword(
            p,
            "an overflow mode",
            &[
                ("clip", TextOverflow::Clip),
                ("ellipsis", TextOverflow::Ellipsis),
            ],
        )?)],
        "text-align" => vec![Decl::TextAlign(keyword(
            p,
            "an alignment",
            &[
                ("start", TextAlign::Start),
                ("end", TextAlign::End),
                ("left", TextAlign::Left),
                ("right", TextAlign::Right),
                ("center", TextAlign::Center),
            ],
        )?)],
        "text-transform" => vec![Decl::TextTransform(keyword(
            p,
            "a text transform",
            &[
                ("none", TextTransform::None),
                ("uppercase", TextTransform::Uppercase),
                ("lowercase", TextTransform::Lowercase),
                ("capitalize", TextTransform::Capitalize),
            ],
        )?)],
        "text-box" => {
            if p.try_parse(|p| p.expect_ident_matching("normal")).is_ok() {
                vec![
                    Decl::TextBoxTrim(TextBoxTrim::default()),
                    Decl::TextBoxEdge(TextBoxEdge::AUTO),
                ]
            } else {
                // `<trim> || <edge>`: a trim left out is trim-both (not
                // its initial value), an edge left out is auto.
                let mut trim = p.try_parse(text_box_trim).ok();
                let edge = p.try_parse(text_box_edge).ok();
                if trim.is_none() {
                    trim = p.try_parse(text_box_trim).ok();
                }
                if trim.is_none() && edge.is_none() {
                    text_box_trim(p)?;
                }
                vec![
                    Decl::TextBoxTrim(trim.unwrap_or(TextBoxTrim {
                        start: true,
                        end: true,
                    })),
                    Decl::TextBoxEdge(edge.unwrap_or(TextBoxEdge::AUTO)),
                ]
            }
        }
        "outline" => {
            let (width, style, color) = border(p)?;
            vec![
                Decl::OutlineWidth(width),
                Decl::OutlineStyle(style),
                Decl::OutlineColor(color),
            ]
        }
        "outline-width" => vec![Decl::OutlineWidth(border_width(p)?)],
        "outline-style" => vec![Decl::OutlineStyle(line_style(p)?)],
        "outline-color" => vec![Decl::OutlineColor(color_value(p)?)],
        "outline-offset" => vec![Decl::OutlineOffset(length(p)?)],
        "transform" => vec![Decl::Transform(transform(p)?)],
        "transform-origin" => {
            let (x, y) = transform_origin(p)?;
            vec![Decl::TransformOrigin(x, y)]
        }
        "text-box-trim" => vec![Decl::TextBoxTrim(text_box_trim(p)?)],
        "text-box-edge" => vec![Decl::TextBoxEdge(text_box_edge(p)?)],
        _ => vec![each(p, props[0])?],
    })
}

/// CSS's initial `border-image-slice`, 100%: the whole image in the
/// corners. Infinite here, cut to the image's size.
pub(crate) const FULL_SLICE: ([f32; 4], bool) = ([f32::INFINITY; 4], false);

/// `none`, or `url(path)`: the image loaded, from the sheet's directory
/// when the path is relative.
fn url(p: &mut Parser, assets: &mut Assets) -> Result<Option<Picture>, Error> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(None);
    }
    let text = match p.next()?.clone() {
        Token::UnquotedUrl(url) => url.to_string(),
        Token::Function(name) if name.eq_ignore_ascii_case("url") => {
            p.parse_nested_block(|p| Ok(p.expect_string()?.to_string()))?
        }
        t => {
            return fail(format!(
                "expected `url()` or `none`, found `{}`",
                t.to_css_string()
            ));
        }
    };
    let path = Path::new(&text);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match &assets.dir {
            Some(dir) => dir.join(path),
            None => {
                return fail(format!(
                    "`{text}`: a relative url() needs its sheet's directory"
                ));
            }
        }
    };
    if let Some(picture) = assets.loaded.get(&path) {
        return Ok(Some(picture.clone()));
    }
    let picture = Picture::load(&path).map_err(ParseError::custom)?;
    assets.loaded.insert(path, picture.clone());
    Ok(Some(picture))
}

/// `auto`, `cover`, `contain`, or a width and an optional height
/// (lengths or `auto`).
fn background_size(p: &mut Parser) -> Result<BackgroundSize, Error> {
    if let Ok(size) = p.try_parse(|p| {
        keyword(
            p,
            "a size",
            &[
                ("cover", BackgroundSize::Cover),
                ("contain", BackgroundSize::Contain),
            ],
        )
    }) {
        return Ok(size);
    }
    let side = |p: &mut Parser| -> Result<Option<f32>, Error> {
        if p.try_parse(|p| p.expect_ident_matching("auto")).is_ok() {
            return Ok(None);
        }
        Ok(Some(non_negative(p)?))
    };
    let width = side(p)?;
    let height = p.try_parse(side).unwrap_or(None);
    Ok(match (width, height) {
        (None, None) => BackgroundSize::Auto,
        (w, h) => BackgroundSize::Px(w, h),
    })
}

fn background_repeat(p: &mut Parser) -> Result<Repeat, Error> {
    let first = keyword(
        p,
        "a repeat",
        &[
            ("repeat", (true, true)),
            ("no-repeat", (false, false)),
            ("repeat-x", (true, false)),
            ("repeat-y", (false, true)),
        ],
    )?;
    // Two values: one per axis, `repeat` or `no-repeat`.
    if let Ok(y) =
        p.try_parse(|p| keyword(p, "a repeat", &[("repeat", true), ("no-repeat", false)]))
    {
        return Ok(Repeat { x: first.0, y });
    }
    Ok(Repeat {
        x: first.0,
        y: first.1,
    })
}

/// 1 to 4 numbers (image pixels) and an optional `fill`, either side.
fn border_image_slice(p: &mut Parser) -> Result<([f32; 4], bool), Error> {
    let fill_first = p.try_parse(|p| p.expect_ident_matching("fill")).is_ok();
    let number = |p: &mut Parser| -> Result<f32, Error> {
        match p.next()?.clone() {
            Token::Number { value, .. } if value >= 0.0 => Ok(value),
            Token::Percentage { .. } => fail("slices are numbers here: image pixels"),
            t => fail(format!("expected a slice, found `{}`", t.to_css_string())),
        }
    };
    let slices = four(p, number)?;
    let fill = fill_first || p.try_parse(|p| p.expect_ident_matching("fill")).is_ok();
    Ok((slices, fill))
}

/// `none`, or `translate()`, `translateX()`, `translateY()` and
/// `scale()` (one factor: no `scaleX`, `scaleY`, `rotate`: iced draws
/// neither).
fn transform(p: &mut Parser) -> Result<Option<Transform>, Error> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(None);
    }
    let mut total = Transform::IDENTITY;
    loop {
        let name = match p.next() {
            Ok(Token::Function(name)) => name.to_ascii_lowercase(),
            Ok(t) => {
                let t = t.clone();
                return fail(format!(
                    "expected a transform function, found `{}`",
                    t.to_css_string()
                ));
            }
            Err(_) => break,
        };
        let step = p.parse_nested_block(|p| match &*name {
            "translate" => {
                let x = length(p)?;
                let y = if p.try_parse(|p| p.expect_comma()).is_ok() {
                    length(p)?
                } else {
                    0.0
                };
                Ok(Transform { scale: 1.0, x, y })
            }
            "translatex" => Ok(Transform {
                x: length(p)?,
                ..Transform::IDENTITY
            }),
            "translatey" => Ok(Transform {
                y: length(p)?,
                ..Transform::IDENTITY
            }),
            "scale" => {
                let factor = p.expect_number()?;
                if p.try_parse(|p| p.expect_comma()).is_ok() && p.expect_number()? != factor {
                    return fail("a scale is uniform here: one factor for both axes");
                }
                if factor <= 0.0 {
                    return fail("a scale is a positive number");
                }
                Ok(Transform {
                    scale: factor,
                    ..Transform::IDENTITY
                })
            }
            "scalex" | "scaley" | "rotate" | "skew" | "skewx" | "skewy" | "matrix" => fail(
                format!("`{name}()` isn't supported: translate() and scale() only"),
            ),
            _ => fail(format!("`{name}()` isn't a transform function")),
        })?;
        total = total.then(step);
    }
    Ok(Some(total))
}

/// `<x> [<y>]`: `left`, `center`, `right` or a length; `top`, `center`,
/// `bottom` or a length.
fn transform_origin(p: &mut Parser) -> Result<(Origin, Origin), Error> {
    let coordinate = |p: &mut Parser| -> Result<(Origin, Option<bool>), Error> {
        if let Ok(word) = p.try_parse(|p| p.expect_ident_cloned()) {
            let at = |f: f32, horizontal: Option<bool>| Ok((Origin::Fraction(f), horizontal));
            return match &*word.to_ascii_lowercase() {
                "left" => at(0.0, Some(true)),
                "right" => at(1.0, Some(true)),
                "top" => at(0.0, Some(false)),
                "bottom" => at(1.0, Some(false)),
                "center" => at(0.5, None),
                other => fail(format!("`{other}` isn't an origin")),
            };
        }
        Ok((Origin::Px(length(p)?), None))
    };
    let (first, first_horizontal) = coordinate(p)?;
    let Ok((second, _)) = p.try_parse(coordinate) else {
        // One value: `top` and `bottom` are vertical, the rest horizontal.
        return Ok(match first_horizontal {
            Some(false) => (Origin::Fraction(0.5), first),
            _ => (first, Origin::Fraction(0.5)),
        });
    };
    // `top left` names them the other way round.
    Ok(match first_horizontal {
        Some(false) => (second, first),
        _ => (first, second),
    })
}

fn text_box_trim(p: &mut Parser) -> Result<TextBoxTrim, Error> {
    let (start, end) = keyword(
        p,
        "a trim",
        &[
            ("none", (false, false)),
            ("trim-start", (true, false)),
            ("trim-end", (false, true)),
            ("trim-both", (true, true)),
        ],
    )?;
    Ok(TextBoxTrim { start, end })
}

/// `auto`, or an over edge (`text`, `cap`, `ex`) and an optional under
/// one (`text`, `alphabetic`).
fn text_box_edge(p: &mut Parser) -> Result<TextBoxEdge, Error> {
    if p.try_parse(|p| p.expect_ident_matching("auto")).is_ok() {
        return Ok(TextBoxEdge::AUTO);
    }
    let over = keyword(
        p,
        "an over edge",
        &[
            ("text", EdgeOver::Text),
            ("cap", EdgeOver::Cap),
            ("ex", EdgeOver::Ex),
        ],
    )?;
    let under = p
        .try_parse(|p| {
            keyword(
                p,
                "an under edge",
                &[
                    ("text", EdgeUnder::Text),
                    ("alphabetic", EdgeUnder::Alphabetic),
                ],
            )
        })
        .unwrap_or(EdgeUnder::Text);
    Ok(TextBoxEdge { over, under })
}

/// One of `options`, by its keyword.
fn keyword<T: Copy>(p: &mut Parser, what: &str, options: &[(&str, T)]) -> Result<T, Error> {
    let token = p.next()?.clone();
    if let Token::Ident(name) = &token
        && let Some((_, value)) = options.iter().find(|(k, _)| name.eq_ignore_ascii_case(k))
    {
        return Ok(*value);
    }
    let names: Vec<String> = options.iter().map(|(k, _)| format!("`{k}`")).collect();
    fail(format!(
        "expected {what} ({}), found `{}`",
        names.join(", "),
        token.to_css_string()
    ))
}

/// 1 to 4 values, top-right-bottom-left as CSS spreads them.
fn four<T: Copy>(
    p: &mut Parser,
    mut one: impl FnMut(&mut Parser) -> Result<T, Error>,
) -> Result<[T; 4], Error> {
    let mut values = vec![one(p)?];
    while values.len() < 4 {
        match p.try_parse(&mut one) {
            Ok(v) => values.push(v),
            Err(_) => break,
        }
    }
    Ok(match values[..] {
        [a] => [a, a, a, a],
        [a, b] => [a, b, a, b],
        [a, b, c] => [a, b, c, b],
        [a, b, c, d] => [a, b, c, d],
        _ => unreachable!(),
    })
}

/// A length: px, or a unitless 0.
fn length(p: &mut Parser) -> Result<f32, Error> {
    match p.next()?.clone() {
        Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("px") => Ok(value),
        Token::Dimension { unit, .. } => {
            fail(format!("`{unit}` isn't a unit here: lengths are px"))
        }
        Token::Number { value: 0.0, .. } => Ok(0.0),
        Token::Percentage { .. } => fail("percentages aren't supported: lengths are px"),
        Token::Ident(name) if name.eq_ignore_ascii_case("auto") => {
            fail("`auto` isn't a length here")
        }
        t => fail(format!("expected a length, found `{}`", t.to_css_string())),
    }
}

fn non_negative(p: &mut Parser) -> Result<f32, Error> {
    let value = length(p)?;
    if value < 0.0 {
        return fail("a negative length isn't allowed here");
    }
    Ok(value)
}

fn size(p: &mut Parser) -> Result<Size, Error> {
    if let Ok(size) =
        p.try_parse(|p| keyword(p, "a size", &[("auto", Size::Auto), ("fill", Size::Fill)]))
    {
        return Ok(size);
    }
    Ok(Size::Px(non_negative(p)?))
}

fn min_size(p: &mut Parser) -> Result<MinSize, Error> {
    if p.try_parse(|p| p.expect_ident_matching("auto")).is_ok() {
        return Ok(MinSize::Auto);
    }
    Ok(MinSize::Px(non_negative(p)?))
}

fn max_size(p: &mut Parser) -> Result<f32, Error> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(f32::INFINITY);
    }
    non_negative(p)
}

fn border_width(p: &mut Parser) -> Result<f32, Error> {
    if let Ok(width) = p.try_parse(|p| {
        keyword(
            p,
            "a border width",
            &[("thin", 1.0), ("medium", 3.0), ("thick", 5.0)],
        )
    }) {
        return Ok(width);
    }
    non_negative(p)
}

fn line_style(p: &mut Parser) -> Result<LineStyle, Error> {
    let token = p.next()?.clone();
    if let Token::Ident(name) = &token {
        match &*name.to_ascii_lowercase() {
            "none" | "hidden" => return Ok(LineStyle::None),
            "solid" => return Ok(LineStyle::Solid),
            "dotted" | "dashed" | "double" | "groove" | "ridge" | "inset" | "outset" => {
                return fail(format!(
                    "`{name}` borders aren't supported: `solid` or `none`"
                ));
            }
            _ => {}
        }
    }
    fail(format!(
        "expected a border style (`solid`, `none`), found `{}`",
        token.to_css_string()
    ))
}

/// `<width> || <style> || <color>`, in any order; the ones left out
/// are their initial values, as a CSS shorthand has them.
fn border(p: &mut Parser) -> Result<(f32, LineStyle, ColorValue), Error> {
    let (mut width, mut style, mut color) = (None, None, None);
    for _ in 0..3 {
        if width.is_none()
            && let Ok(w) = p.try_parse(border_width)
        {
            width = Some(w);
        } else if style.is_none()
            && let Ok(s) = p.try_parse(line_style)
        {
            style = Some(s);
        } else if color.is_none()
            && let Ok(c) = p.try_parse(color_value)
        {
            color = Some(c);
        } else {
            break;
        }
    }
    if width.is_none() && style.is_none() && color.is_none() {
        // The token that's none of them, for the message.
        line_style(p)?;
    }
    Ok((
        width.unwrap_or(3.0),
        style.unwrap_or(LineStyle::None),
        color.unwrap_or(ColorValue::Current),
    ))
}

/// `none`, or a list of shadows: one outer and one inset at most.
fn shadows(p: &mut Parser) -> Result<Shadows, Error> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(Shadows::default());
    }
    let list = p.parse_comma_separated(shadow)?;
    let mut shadows = Shadows::default();
    for (inset, value) in list {
        let slot = if inset {
            &mut shadows.inset
        } else {
            &mut shadows.outer
        };
        if slot.is_some() {
            return fail(if inset {
                "one inset shadow at most"
            } else {
                "one outer shadow at most"
            });
        }
        *slot = Some(value);
    }
    Ok(shadows)
}

/// `inset? && <x> <y> <blur>? && <color>?`
fn shadow(p: &mut Parser) -> Result<(bool, ShadowValue), Error> {
    let mut inset = false;
    let mut color = None;
    let mut lengths = Vec::new();
    loop {
        if !inset && p.try_parse(|p| p.expect_ident_matching("inset")).is_ok() {
            inset = true;
        } else if color.is_none()
            && let Ok(c) = p.try_parse(color_value)
        {
            color = Some(c);
        } else if lengths.is_empty() {
            lengths.push(length(p)?);
            while lengths.len() < 4 {
                match p.try_parse(length) {
                    Ok(l) => lengths.push(l),
                    Err(_) => break,
                }
            }
        } else {
            break;
        }
        if p.is_exhausted() {
            break;
        }
    }
    let (x, y, blur, spread) = match lengths[..] {
        [x, y] => (x, y, 0.0, 0.0),
        [x, y, blur] => (x, y, blur, 0.0),
        [x, y, blur, spread] => (x, y, blur, spread),
        _ => return fail("a shadow needs an x and a y offset"),
    };
    if blur < 0.0 {
        return fail("a negative blur isn't allowed");
    }
    if spread != 0.0 {
        return fail("a shadow's spread isn't supported");
    }
    if inset && blur != 0.0 {
        return fail("an inset shadow isn't blurred here: leave its blur out");
    }
    Ok((
        inset,
        ShadowValue {
            x,
            y,
            blur,
            color: color.unwrap_or(ColorValue::Current),
        },
    ))
}

fn font_weight(p: &mut Parser) -> Result<WeightValue, Error> {
    match p.next()?.clone() {
        Token::Number { value, .. } if (1.0..=1000.0).contains(&value) => {
            Ok(WeightValue::Absolute(value as u16))
        }
        Token::Ident(name) => match &*name.to_ascii_lowercase() {
            "normal" => Ok(WeightValue::Absolute(400)),
            "bold" => Ok(WeightValue::Absolute(700)),
            "bolder" => Ok(WeightValue::Bolder),
            "lighter" => Ok(WeightValue::Lighter),
            _ => fail(format!("`{name}` isn't a font weight")),
        },
        t => fail(format!(
            "expected a font weight (`normal`, `bold`, 1-1000), found `{}`",
            t.to_css_string()
        )),
    }
}

/// A quoted name, a name in plain words, or a generic family.
fn family(p: &mut Parser) -> Result<FamilyName, Error> {
    if let Ok(name) = p.try_parse(|p| p.expect_string_cloned()) {
        return Ok(FamilyName::Named(Arc::from(&*name)));
    }
    let mut words = vec![p.expect_ident_cloned()?.to_string()];
    while let Ok(word) = p.try_parse(|p| p.expect_ident_cloned()) {
        words.push(word.to_string());
    }
    if let [word] = &words[..] {
        match &*word.to_ascii_lowercase() {
            "serif" => return Ok(FamilyName::Serif),
            "sans-serif" | "system-ui" => return Ok(FamilyName::SansSerif),
            "monospace" => return Ok(FamilyName::Monospace),
            "cursive" => return Ok(FamilyName::Cursive),
            "fantasy" => return Ok(FamilyName::Fantasy),
            _ => {}
        }
    }
    Ok(FamilyName::Named(Arc::from(words.join(" "))))
}

/// `#rgb[a]`, `#rrggbb[aa]`, `rgb()`, `rgba()`, `hsl()`, `hsla()`, a
/// name, `transparent` or `currentcolor`.
pub(crate) fn color_value(p: &mut Parser) -> Result<ColorValue, Error> {
    let token = p.next()?.clone();
    match &token {
        Token::Hash(hash) | Token::IDHash(hash) => match color::parse_hash_color(hash.as_bytes()) {
            Ok((r, g, b, a)) => Ok(ColorValue::Rgba(Color::from_rgba8(r, g, b, a))),
            Err(()) => fail(format!("`#{hash}` isn't a colour")),
        },
        Token::Ident(name) => {
            let name = name.to_ascii_lowercase();
            match &*name {
                "currentcolor" => Ok(ColorValue::Current),
                "transparent" => Ok(ColorValue::Rgba(Color::TRANSPARENT)),
                _ => match color::parse_named_color(&name) {
                    Ok((r, g, b)) => Ok(ColorValue::Rgba(Color::from_rgb8(r, g, b))),
                    Err(()) => fail(format!("`{name}` isn't a colour")),
                },
            }
        }
        Token::Function(name) => {
            let name = name.to_ascii_lowercase();
            p.parse_nested_block(|p| match &*name {
                "rgb" | "rgba" => rgb(p),
                "hsl" | "hsla" => hsl(p),
                _ => fail(format!("`{name}()` isn't a colour function here")),
            })
        }
        t => fail(format!("expected a colour, found `{}`", t.to_css_string())),
    }
}

/// `r, g, b[, a]` or `r g b[ / a]`: numbers 0-255 or percentages.
fn rgb(p: &mut Parser) -> Result<ColorValue, Error> {
    let channel = |p: &mut Parser| -> Result<f32, Error> {
        match p.next()?.clone() {
            Token::Number { value, .. } => Ok(value.clamp(0.0, 255.0) / 255.0),
            Token::Percentage { unit_value, .. } => Ok(unit_value.clamp(0.0, 1.0)),
            t => fail(format!("expected a channel, found `{}`", t.to_css_string())),
        }
    };
    let r = channel(p)?;
    let legacy = p.try_parse(|p| p.expect_comma()).is_ok();
    let g = channel(p)?;
    if legacy {
        p.expect_comma()?;
    }
    let b = channel(p)?;
    let a = alpha(p, legacy)?;
    Ok(ColorValue::Rgba(Color::from_rgba(r, g, b, a)))
}

/// `h, s%, l%[, a]` or `h s% l%[ / a]`; the hue in degrees (a number,
/// `deg` or `turn`).
fn hsl(p: &mut Parser) -> Result<ColorValue, Error> {
    let hue = match p.next()?.clone() {
        Token::Number { value, .. } => value,
        Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("deg") => value,
        Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("turn") => value * 360.0,
        t => return fail(format!("expected a hue, found `{}`", t.to_css_string())),
    };
    let percent = |p: &mut Parser| -> Result<f32, Error> {
        match p.next()?.clone() {
            Token::Percentage { unit_value, .. } => Ok(unit_value.clamp(0.0, 1.0)),
            Token::Number { value, .. } => Ok((value / 100.0).clamp(0.0, 1.0)),
            t => fail(format!(
                "expected a percentage, found `{}`",
                t.to_css_string()
            )),
        }
    };
    let legacy = p.try_parse(|p| p.expect_comma()).is_ok();
    let s = percent(p)?;
    if legacy {
        p.expect_comma()?;
    }
    let l = percent(p)?;
    let a = alpha(p, legacy)?;
    // CSS Color 4's hsl-to-rgb.
    let h = hue.rem_euclid(360.0);
    let f = |n: f32| {
        let k = (n + h / 30.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    Ok(ColorValue::Rgba(Color::from_rgba(
        f(0.0),
        f(8.0),
        f(4.0),
        a,
    )))
}

/// The optional alpha of a colour function: after a comma in the
/// legacy syntax, after a `/` in the modern one.
fn alpha(p: &mut Parser, legacy: bool) -> Result<f32, Error> {
    let separator = if legacy {
        p.try_parse(|p| p.expect_comma())
    } else {
        p.try_parse(|p| p.expect_delim('/'))
    };
    if separator.is_err() {
        return Ok(1.0);
    }
    match p.next()?.clone() {
        Token::Number { value, .. } => Ok(value.clamp(0.0, 1.0)),
        Token::Percentage { unit_value, .. } => Ok(unit_value.clamp(0.0, 1.0)),
        t => fail(format!("expected an alpha, found `{}`", t.to_css_string())),
    }
}
