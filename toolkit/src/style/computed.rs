//! The computed style of a node: every property, cascaded, inherited and
//! made absolute for the node's scale. What a widget lays out and draws
//! with.

use std::sync::Arc;

use iced::{Color, Font};

use super::fonts;
use super::images::{Nine, Picture};
use super::keyframes::Keyframes;
use super::value::{
    Align, AnimationDirection, BackgroundSize, ColorValue, Decl, Direction, Easing, FULL_SLICE,
    FamilyName, FillMode, FontStyle, Justify, LineHeight, LineStyle, MinSize, Moving, ObjectFit,
    Origin, Prop, Repeat, Shadows, Side, Size, TextAlign, TextBoxEdge, TextBoxTrim, TextOverflow,
    TextTransform, Transform, WeightValue, WhiteSpace, Wide,
};

/// Four lengths, one per side.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sides {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Sides {
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }

    fn get(&self, side: Side) -> f32 {
        match side {
            Side::Top => self.top,
            Side::Right => self.right,
            Side::Bottom => self.bottom,
            Side::Left => self.left,
        }
    }

    fn set(&mut self, side: Side, value: f32) {
        match side {
            Side::Top => self.top = value,
            Side::Right => self.right = value,
            Side::Bottom => self.bottom = value,
            Side::Left => self.left = value,
        }
    }
}

/// `transition`'s lists, as CSS keeps them: the i-th transition takes
/// the i-th of each, a shorter list repeating.
#[derive(Debug, Clone, PartialEq)]
pub struct Transitions {
    /// Sets of `Moving` bits.
    pub property: Arc<[u16]>,
    pub duration: Arc<[f32]>,
    pub easing: Arc<[Easing]>,
    pub delay: Arc<[f32]>,
}

impl Transitions {
    fn initial() -> Self {
        Self {
            property: Arc::from([Moving::ALL.iter().fold(0, |set, m| set | m.bit())]),
            duration: Arc::from([0.0]),
            easing: Arc::from([Easing::EASE]),
            delay: Arc::from([0.0]),
        }
    }

    /// Whether any of them takes time.
    pub fn any(&self) -> bool {
        !self.property.is_empty() && self.duration.iter().any(|d| *d > 0.0)
    }
}

/// `animation`'s lists, as CSS keeps them.
#[derive(Debug, Clone, PartialEq)]
pub struct Animations {
    pub names: Arc<[Option<Arc<str>>]>,
    /// The names' `@keyframes`, found in the sheet.
    pub keyframes: Arc<[Option<Arc<Keyframes>>]>,
    pub duration: Arc<[f32]>,
    pub easing: Arc<[Easing]>,
    pub delay: Arc<[f32]>,
    /// `f32::INFINITY` for `infinite`.
    pub count: Arc<[f32]>,
    pub direction: Arc<[AnimationDirection]>,
    pub fill: Arc<[FillMode]>,
}

impl Animations {
    fn initial() -> Self {
        Self {
            names: Arc::from([None]),
            keyframes: Arc::from([None]),
            duration: Arc::from([0.0]),
            easing: Arc::from([Easing::EASE]),
            delay: Arc::from([0.0]),
            count: Arc::from([1.0]),
            direction: Arc::from([AnimationDirection::Normal]),
            fill: Arc::from([FillMode::None]),
        }
    }

    /// Whether one of them has its `@keyframes`.
    pub fn any(&self) -> bool {
        self.keyframes.iter().any(Option::is_some)
    }
}

/// An outer shadow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub color: Color,
}

/// An inset shadow: an unblurred band inside the border, along the sides
/// its offset points away from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InsetShadow {
    pub x: f32,
    pub y: f32,
    pub color: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Computed {
    pub width: Size,
    pub height: Size,
    pub min_width: MinSize,
    pub min_height: MinSize,
    /// `f32::INFINITY` for `none`.
    pub max_width: f32,
    pub max_height: f32,
    pub margin: Sides,
    pub padding: Sides,
    /// Whole device pixels at the node's scale; 0 where the style is
    /// `none`.
    pub border_width: Sides,
    /// Top, right, bottom, left.
    pub border_color: [Color; 4],
    /// Top-left, top-right, bottom-right, bottom-left.
    pub border_radius: [f32; 4],
    pub row_gap: f32,
    pub column_gap: f32,
    /// `None`: the widget's own (a row's, a column's).
    pub flex_direction: Option<Direction>,
    pub align_items: Align,
    pub justify_content: Justify,
    pub background: Color,
    pub color: Color,
    pub shadow: Option<Shadow>,
    pub inset_shadow: Option<InsetShadow>,
    pub opacity: f32,
    pub font_family: Arc<[FamilyName]>,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_style: FontStyle,
    pub line_height: LineHeight,
    pub white_space: WhiteSpace,
    pub text_overflow: TextOverflow,
    pub text_align: TextAlign,
    pub text_transform: TextTransform,
    pub text_box_trim: TextBoxTrim,
    pub text_box_edge: TextBoxEdge,
    /// Whole device pixels; 0 where the style is `none`.
    pub outline_width: f32,
    pub outline_color: Color,
    pub outline_offset: f32,
    pub transform: Option<Transform>,
    pub transform_origin: (Origin, Origin),
    pub background_image: Option<Picture>,
    pub background_size: BackgroundSize,
    pub background_position: (Origin, Origin),
    pub background_repeat: Repeat,
    pub border_image_source: Option<Picture>,
    pub border_image_slice: ([f32; 4], bool),
    /// The source cut at the slices: drawn in place of the border.
    pub border_image: Option<Arc<Nine>>,
    pub object_fit: ObjectFit,
    /// Degrees: an icon's or an image's turn.
    pub rotate: f32,
    pub transitions: Transitions,
    pub animations: Animations,
    /// What a mark shows: a menu item's check, its arrow.
    pub content: Option<Arc<str>>,
    /// A path's paint, as SVG's: a graph's series.
    pub fill: Color,
    pub stroke: Color,
    pub stroke_width: f32,
    /// The family picked from `font_family`, with the weight and style.
    pub font: Font,
    /// `line-height`, in logical px: whole device pixels.
    pub line_height_px: f32,
    /// The first available font's, in px at `font_size`.
    pub ascent: f32,
    pub descent: f32,
    pub cap_height: f32,
    pub x_height: f32,
}

impl Computed {
    /// CSS's initial values.
    pub fn initial() -> Self {
        Self {
            width: Size::Auto,
            height: Size::Auto,
            min_width: MinSize::Auto,
            min_height: MinSize::Auto,
            max_width: f32::INFINITY,
            max_height: f32::INFINITY,
            margin: Sides::default(),
            padding: Sides::default(),
            border_width: Sides::default(),
            border_color: [Color::BLACK; 4],
            border_radius: [0.0; 4],
            row_gap: 0.0,
            column_gap: 0.0,
            flex_direction: None,
            align_items: Align::Stretch,
            justify_content: Justify::Start,
            background: Color::TRANSPARENT,
            color: Color::BLACK,
            shadow: None,
            inset_shadow: None,
            opacity: 1.0,
            font_family: Arc::from([FamilyName::SansSerif]),
            font_size: 16.0,
            font_weight: 400,
            font_style: FontStyle::Normal,
            line_height: LineHeight::Normal,
            white_space: WhiteSpace::Normal,
            text_overflow: TextOverflow::Clip,
            text_align: TextAlign::Start,
            text_transform: TextTransform::None,
            text_box_trim: TextBoxTrim::default(),
            text_box_edge: TextBoxEdge::AUTO,
            outline_width: 0.0,
            outline_color: Color::BLACK,
            outline_offset: 0.0,
            transform: None,
            transform_origin: (Origin::Fraction(0.5), Origin::Fraction(0.5)),
            background_image: None,
            background_size: BackgroundSize::Auto,
            background_position: (Origin::Fraction(0.0), Origin::Fraction(0.0)),
            background_repeat: Repeat { x: true, y: true },
            border_image_source: None,
            border_image_slice: FULL_SLICE,
            border_image: None,
            object_fit: ObjectFit::Fill,
            rotate: 0.0,
            transitions: Transitions::initial(),
            animations: Animations::initial(),
            content: None,
            fill: Color::BLACK,
            stroke: Color::TRANSPARENT,
            stroke_width: 1.0,
            font: Font::DEFAULT,
            line_height_px: 0.0,
            ascent: 0.0,
            descent: 0.0,
            cap_height: 0.0,
            x_height: 0.0,
        }
    }

    /// Whether a box styled `other` is laid out as one styled `self`:
    /// what a state may change without a new layout is paint.
    pub fn same_layout(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.min_width == other.min_width
            && self.min_height == other.min_height
            && self.max_width == other.max_width
            && self.max_height == other.max_height
            && self.margin == other.margin
            && self.padding == other.padding
            && self.border_width == other.border_width
            && self.row_gap == other.row_gap
            && self.column_gap == other.column_gap
            && self.flex_direction == other.flex_direction
            && self.align_items == other.align_items
            && self.justify_content == other.justify_content
            && self.font == other.font
            && self.font_size == other.font_size
            && self.line_height_px == other.line_height_px
            && self.white_space == other.white_space
            && self.text_overflow == other.text_overflow
            && self.text_transform == other.text_transform
            && self.text_box_trim == other.text_box_trim
            && self.text_box_edge == other.text_box_edge
    }

    /// Its `transform` as iced draws it, about its origin in the border
    /// box `bounds`.
    pub fn transformation(&self, bounds: iced::Rectangle) -> Option<iced::Transformation> {
        let t = self.transform?;
        let at = |origin: Origin, start: f32, size: f32| match origin {
            Origin::Px(v) => start + v,
            Origin::Fraction(f) => start + f * size,
        };
        let (ox, oy) = (
            at(self.transform_origin.0, bounds.x, bounds.width),
            at(self.transform_origin.1, bounds.y, bounds.height),
        );
        Some(
            iced::Transformation::translate(ox + t.x, oy + t.y)
                * iced::Transformation::scale(t.scale)
                * iced::Transformation::translate(-ox, -oy),
        )
    }

    /// The gap between the children laid out along `direction`.
    pub fn gap(&self, direction: Direction) -> f32 {
        match direction {
            Direction::Row => self.column_gap,
            Direction::Column => self.row_gap,
        }
    }
}

/// A node's style while the cascade runs: the declared values that are
/// only resolved at the end (border styles, `currentcolor`).
pub(crate) struct Cascade<'p> {
    out: Computed,
    parent: &'p Computed,
    background: ColorValue,
    border_style: [LineStyle; 4],
    border_color: [ColorValue; 4],
    shadows: Shadows,
    outline_style: LineStyle,
    outline_color: ColorValue,
    fill: ColorValue,
    stroke: ColorValue,
}

impl<'p> Cascade<'p> {
    /// A node under `parent`, with what it inherits from it.
    pub fn new(parent: &'p Computed) -> Self {
        let mut out = Computed::initial();
        for prop in INHERITED {
            copy(prop, parent, &mut out);
        }
        Self {
            out,
            parent,
            background: ColorValue::Rgba(Color::TRANSPARENT),
            border_style: [LineStyle::None; 4],
            border_color: [ColorValue::Current; 4],
            shadows: Shadows::default(),
            outline_style: LineStyle::None,
            outline_color: ColorValue::Current,
            fill: ColorValue::Rgba(parent.fill),
            stroke: ColorValue::Rgba(parent.stroke),
        }
    }

    pub fn apply(&mut self, decl: &Decl) {
        let out = &mut self.out;
        match decl {
            Decl::Width(v) => out.width = *v,
            Decl::Height(v) => out.height = *v,
            Decl::MinWidth(v) => out.min_width = *v,
            Decl::MinHeight(v) => out.min_height = *v,
            Decl::MaxWidth(v) => out.max_width = *v,
            Decl::MaxHeight(v) => out.max_height = *v,
            Decl::Margin(s, v) => out.margin.set(*s, *v),
            Decl::Padding(s, v) => out.padding.set(*s, *v),
            Decl::BorderWidth(s, v) => out.border_width.set(*s, *v),
            Decl::BorderStyle(s, v) => self.border_style[*s as usize] = *v,
            Decl::BorderColor(s, v) => self.border_color[*s as usize] = *v,
            Decl::Radius(c, v) => out.border_radius[*c as usize] = *v,
            Decl::RowGap(v) => out.row_gap = *v,
            Decl::ColumnGap(v) => out.column_gap = *v,
            Decl::FlexDirection(v) => out.flex_direction = Some(*v),
            Decl::AlignItems(v) => out.align_items = *v,
            Decl::JustifyContent(v) => out.justify_content = *v,
            Decl::Background(v) => self.background = *v,
            // `color: currentcolor` is `inherit`.
            Decl::Color(ColorValue::Current) => out.color = self.parent.color,
            Decl::Color(ColorValue::Rgba(c)) => out.color = *c,
            Decl::BoxShadow(v) => self.shadows = *v,
            Decl::Opacity(v) => out.opacity = *v,
            Decl::FontFamily(v) => out.font_family = v.clone(),
            Decl::FontSize(v) => out.font_size = *v,
            Decl::FontWeight(v) => out.font_weight = weight(*v, self.parent.font_weight),
            Decl::FontStyle(v) => out.font_style = *v,
            Decl::LineHeight(v) => out.line_height = *v,
            Decl::WhiteSpace(v) => out.white_space = *v,
            Decl::TextOverflow(v) => out.text_overflow = *v,
            Decl::TextAlign(v) => out.text_align = *v,
            Decl::TextTransform(v) => out.text_transform = *v,
            Decl::TextBoxTrim(v) => out.text_box_trim = *v,
            Decl::TextBoxEdge(v) => out.text_box_edge = *v,
            Decl::OutlineWidth(v) => out.outline_width = *v,
            Decl::OutlineStyle(v) => self.outline_style = *v,
            Decl::OutlineColor(v) => self.outline_color = *v,
            Decl::OutlineOffset(v) => out.outline_offset = *v,
            Decl::Transform(v) => out.transform = *v,
            Decl::TransformOrigin(x, y) => out.transform_origin = (*x, *y),
            Decl::BackgroundImage(v) => out.background_image = v.clone(),
            Decl::BackgroundSize(v) => out.background_size = *v,
            Decl::BackgroundPosition(x, y) => out.background_position = (*x, *y),
            Decl::BackgroundRepeat(v) => out.background_repeat = *v,
            Decl::BorderImageSource(v) => out.border_image_source = v.clone(),
            Decl::BorderImageSlice(slices, fill) => out.border_image_slice = (*slices, *fill),
            Decl::ObjectFit(v) => out.object_fit = *v,
            Decl::Rotate(v) => out.rotate = *v,
            Decl::TransitionProperty(v) => out.transitions.property = v.clone(),
            Decl::TransitionDuration(v) => out.transitions.duration = v.clone(),
            Decl::TransitionTimingFunction(v) => out.transitions.easing = v.clone(),
            Decl::TransitionDelay(v) => out.transitions.delay = v.clone(),
            Decl::AnimationName(v) => out.animations.names = v.clone(),
            Decl::AnimationDuration(v) => out.animations.duration = v.clone(),
            Decl::AnimationTimingFunction(v) => out.animations.easing = v.clone(),
            Decl::AnimationDelay(v) => out.animations.delay = v.clone(),
            Decl::AnimationIterationCount(v) => out.animations.count = v.clone(),
            Decl::AnimationDirection(v) => out.animations.direction = v.clone(),
            Decl::AnimationFillMode(v) => out.animations.fill = v.clone(),
            Decl::Content(v) => out.content = v.clone(),
            Decl::Fill(v) => self.fill = *v,
            Decl::Stroke(v) => self.stroke = *v,
            Decl::StrokeWidth(v) => out.stroke_width = *v,
            Decl::Wide(prop, wide) => {
                let inherit = match wide {
                    Wide::Inherit => true,
                    Wide::Initial => false,
                    Wide::Unset => prop.inherited(),
                };
                if inherit {
                    self.inherit(*prop);
                } else {
                    self.reset(*prop);
                }
            }
        }
    }

    fn inherit(&mut self, prop: Prop) {
        copy(prop, self.parent, &mut self.out);
        // The parent's computed border is already resolved: its colour
        // and its width (0 when it had no style) are what inherit.
        match prop {
            Prop::Background => self.background = ColorValue::Rgba(self.parent.background),
            Prop::OutlineStyle => {
                self.outline_style = if self.parent.outline_width > 0.0 {
                    LineStyle::Solid
                } else {
                    LineStyle::None
                }
            }
            Prop::OutlineColor => {
                self.outline_color = ColorValue::Rgba(self.parent.outline_color);
            }
            Prop::Fill => self.fill = ColorValue::Rgba(self.parent.fill),
            Prop::Stroke => self.stroke = ColorValue::Rgba(self.parent.stroke),
            Prop::BorderStyle(s) => {
                self.border_style[s as usize] = if self.parent.border_width.get(s) > 0.0 {
                    LineStyle::Solid
                } else {
                    LineStyle::None
                }
            }
            Prop::BorderColor(s) => {
                self.border_color[s as usize] =
                    ColorValue::Rgba(self.parent.border_color[s as usize]);
            }
            Prop::BoxShadow => {
                self.shadows = Shadows {
                    outer: self.parent.shadow.map(|s| super::value::ShadowValue {
                        x: s.x,
                        y: s.y,
                        blur: s.blur,
                        color: ColorValue::Rgba(s.color),
                    }),
                    inset: self.parent.inset_shadow.map(|s| super::value::ShadowValue {
                        x: s.x,
                        y: s.y,
                        blur: 0.0,
                        color: ColorValue::Rgba(s.color),
                    }),
                }
            }
            _ => {}
        }
    }

    fn reset(&mut self, prop: Prop) {
        copy(prop, &Computed::initial(), &mut self.out);
        match prop {
            Prop::Background => self.background = ColorValue::Rgba(Color::TRANSPARENT),
            Prop::OutlineStyle => self.outline_style = LineStyle::None,
            Prop::OutlineColor => self.outline_color = ColorValue::Current,
            Prop::BorderStyle(s) => self.border_style[s as usize] = LineStyle::None,
            Prop::BorderColor(s) => self.border_color[s as usize] = ColorValue::Current,
            Prop::BoxShadow => self.shadows = Shadows::default(),
            Prop::Fill => self.fill = ColorValue::Rgba(Color::BLACK),
            Prop::Stroke => self.stroke = ColorValue::Rgba(Color::TRANSPARENT),
            _ => {}
        }
    }

    /// The computed style, for a node at `scale` device pixels per
    /// logical one.
    pub fn finish(self, scale: f32) -> Computed {
        let Self {
            mut out,
            background,
            border_style,
            border_color,
            shadows,
            outline_style,
            outline_color,
            fill,
            stroke,
            ..
        } = self;
        let color = out.color;
        let resolve = |value: ColorValue| match value {
            ColorValue::Rgba(c) => c,
            ColorValue::Current => color,
        };
        out.background = resolve(background);
        out.fill = resolve(fill);
        out.stroke = resolve(stroke);
        out.outline_width = match outline_style {
            LineStyle::None => 0.0,
            LineStyle::Solid => snap_border(out.outline_width, scale),
        };
        out.outline_color = resolve(outline_color);
        out.outline_offset = (out.outline_offset * scale).round() / scale;
        for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
            let i = side as usize;
            let width = match border_style[i] {
                LineStyle::None => 0.0,
                LineStyle::Solid => snap_border(out.border_width.get(side), scale),
            };
            out.border_width.set(side, width);
            out.border_color[i] = resolve(border_color[i]);
        }
        out.shadow = shadows.outer.map(|s| Shadow {
            x: s.x,
            y: s.y,
            blur: s.blur,
            color: resolve(s.color),
        });
        out.inset_shadow = shadows.inset.map(|s| InsetShadow {
            x: s.x,
            y: s.y,
            color: resolve(s.color),
        });
        out.border_image = out.border_image_source.as_ref().and_then(|source| {
            let (slices, fill) = out.border_image_slice;
            source.nine(slices.map(|s| s.min(u32::MAX as f32) as u32), fill)
        });
        out.font = fonts::font(&out.font_family, out.font_weight, out.font_style);
        let m = fonts::metrics(out.font);
        let size = out.font_size;
        let line_height = match out.line_height {
            LineHeight::Normal => (m.ascent + m.descent + m.leading) * size,
            LineHeight::Number(n) => n * size,
            LineHeight::Px(px) => px,
        };
        out.line_height_px = ((line_height * scale).round().max(1.0)) / scale;
        out.ascent = m.ascent * size;
        out.descent = m.descent * size;
        out.cap_height = m.cap_height * size;
        out.x_height = m.x_height * size;
        out
    }
}

/// CSS's "snap as a border width": down to whole device pixels, at
/// least one.
fn snap_border(width: f32, scale: f32) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    let device = width * scale;
    if device < 1.0 {
        1.0 / scale
    } else {
        // A hair over, so that 1.5 × 2 isn't 2.9999.
        (device + 1e-3).floor() / scale
    }
}

/// CSS's relative weights, from the inherited one.
fn weight(value: WeightValue, parent: u16) -> u16 {
    match value {
        WeightValue::Absolute(w) => w,
        WeightValue::Bolder => match parent {
            ..350 => 400,
            350..550 => 700,
            550..900 => 900,
            _ => parent,
        },
        WeightValue::Lighter => match parent {
            ..100 => parent,
            100..550 => 100,
            550..750 => 400,
            _ => 700,
        },
    }
}

const INHERITED: [Prop; 13] = [
    Prop::Color,
    Prop::FontFamily,
    Prop::FontSize,
    Prop::FontWeight,
    Prop::FontStyle,
    Prop::LineHeight,
    Prop::WhiteSpace,
    Prop::TextAlign,
    Prop::TextTransform,
    Prop::TextBoxEdge,
    Prop::Fill,
    Prop::Stroke,
    Prop::StrokeWidth,
];

/// One property's computed value, from `from` to `to`.
fn copy(prop: Prop, from: &Computed, to: &mut Computed) {
    match prop {
        Prop::Width => to.width = from.width,
        Prop::Height => to.height = from.height,
        Prop::MinWidth => to.min_width = from.min_width,
        Prop::MinHeight => to.min_height = from.min_height,
        Prop::MaxWidth => to.max_width = from.max_width,
        Prop::MaxHeight => to.max_height = from.max_height,
        Prop::Margin(s) => to.margin.set(s, from.margin.get(s)),
        Prop::Padding(s) => to.padding.set(s, from.padding.get(s)),
        Prop::BorderWidth(s) => to.border_width.set(s, from.border_width.get(s)),
        Prop::BorderStyle(_) => {}
        Prop::BorderColor(s) => to.border_color[s as usize] = from.border_color[s as usize],
        Prop::Radius(c) => to.border_radius[c as usize] = from.border_radius[c as usize],
        Prop::RowGap => to.row_gap = from.row_gap,
        Prop::ColumnGap => to.column_gap = from.column_gap,
        Prop::FlexDirection => to.flex_direction = from.flex_direction,
        Prop::AlignItems => to.align_items = from.align_items,
        Prop::JustifyContent => to.justify_content = from.justify_content,
        Prop::Background => to.background = from.background,
        Prop::Color => to.color = from.color,
        Prop::BoxShadow => {
            to.shadow = from.shadow;
            to.inset_shadow = from.inset_shadow;
        }
        Prop::Opacity => to.opacity = from.opacity,
        Prop::FontFamily => to.font_family = from.font_family.clone(),
        Prop::FontSize => to.font_size = from.font_size,
        Prop::FontWeight => to.font_weight = from.font_weight,
        Prop::FontStyle => to.font_style = from.font_style,
        Prop::LineHeight => to.line_height = from.line_height,
        Prop::WhiteSpace => to.white_space = from.white_space,
        Prop::TextOverflow => to.text_overflow = from.text_overflow,
        Prop::TextAlign => to.text_align = from.text_align,
        Prop::TextTransform => to.text_transform = from.text_transform,
        Prop::TextBoxTrim => to.text_box_trim = from.text_box_trim,
        Prop::TextBoxEdge => to.text_box_edge = from.text_box_edge,
        Prop::OutlineWidth => to.outline_width = from.outline_width,
        Prop::OutlineStyle => {}
        Prop::OutlineColor => to.outline_color = from.outline_color,
        Prop::OutlineOffset => to.outline_offset = from.outline_offset,
        Prop::Transform => to.transform = from.transform,
        Prop::TransformOrigin => to.transform_origin = from.transform_origin,
        Prop::BackgroundImage => to.background_image = from.background_image.clone(),
        Prop::BackgroundSize => to.background_size = from.background_size,
        Prop::BackgroundPosition => to.background_position = from.background_position,
        Prop::BackgroundRepeat => to.background_repeat = from.background_repeat,
        Prop::BorderImageSource => to.border_image_source = from.border_image_source.clone(),
        Prop::BorderImageSlice => to.border_image_slice = from.border_image_slice,
        Prop::ObjectFit => to.object_fit = from.object_fit,
        Prop::Rotate => to.rotate = from.rotate,
        Prop::TransitionProperty => to.transitions.property = from.transitions.property.clone(),
        Prop::TransitionDuration => to.transitions.duration = from.transitions.duration.clone(),
        Prop::TransitionTimingFunction => to.transitions.easing = from.transitions.easing.clone(),
        Prop::TransitionDelay => to.transitions.delay = from.transitions.delay.clone(),
        Prop::AnimationName => to.animations.names = from.animations.names.clone(),
        Prop::AnimationDuration => to.animations.duration = from.animations.duration.clone(),
        Prop::AnimationTimingFunction => to.animations.easing = from.animations.easing.clone(),
        Prop::AnimationDelay => to.animations.delay = from.animations.delay.clone(),
        Prop::AnimationIterationCount => to.animations.count = from.animations.count.clone(),
        Prop::AnimationDirection => to.animations.direction = from.animations.direction.clone(),
        Prop::AnimationFillMode => to.animations.fill = from.animations.fill.clone(),
        Prop::Content => to.content = from.content.clone(),
        Prop::Fill => to.fill = from.fill,
        Prop::Stroke => to.stroke = from.stroke,
        Prop::StrokeWidth => to.stroke_width = from.stroke_width,
    }
}
