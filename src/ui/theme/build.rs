//! The themed widgets: plain iced widgets built for a [`Node`], styled
//! with what [`Theme::resolve`] gives it (a button's style closure
//! re-resolves with the node's status, for `:hover` and `:active`), and
//! tagged with its element path for `debug widgets` ([`widget_path`]).
//! Plus the measures a popup's size is worked out from.

use iced::border::Radius;
use iced::font::Family;
use iced::widget::{
    Button, Column, Container, Row, Text, TextInput, button, column, container, row, slider, text,
    text_input, toggler,
};
use iced::{Alignment, Border, Color, Element, Font, Padding, Size};

use super::fonts::family;
use super::{Length, Node, Style, Theme};

use iced::Length as IcedLength;

/// A slider's rail thickness and handle diameter when the theme doesn't
/// say.
const DEFAULT_RAIL: f32 = 4.0;
const DEFAULT_HANDLE: f32 = 12.0;

/// A toggle's height when the theme doesn't say (its width is twice).
const DEFAULT_TOGGLE: f32 = 16.0;

/// iced's text size when no rule sets `font-size`.
const DEFAULT_FONT_SIZE: f32 = 16.0;

impl Theme {
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

    /// A `toggler` styled as `node`, which carries `.on` when it is:
    /// the track is `height` tall (twice as wide), `background` with
    /// the node's border; the knob is the `handle` child: `background`
    /// (the track's `color` when unset) and border. Re-resolved with
    /// `:hover`. Comes in a container tagged with the node path, so the
    /// theme's `toggle.on` rules and `debug widgets` see it.
    pub fn toggler<'a, M: Clone + 'a>(
        &'a self,
        node: &Node,
        on: bool,
        on_toggle: impl Fn(bool) -> M + 'a,
    ) -> Container<'a, M> {
        let node = node.class_if("on", on);
        let s = self.resolve(&node);
        let size = match s.height {
            Some(Length::Px(px)) => px,
            _ => DEFAULT_TOGGLE,
        };
        let id = widget_id(&node);
        let styled = node.clone();
        let toggle = toggler(on).on_toggle(on_toggle).size(size).style(
            move |theme: &iced::Theme, status| {
                let n = styled.toggler_status(status);
                let s = self.resolve(&n);
                let h = self.resolve(&n.child("handle"));
                let track = s.background.unwrap_or(Color {
                    a: 0.3,
                    ..theme.palette().text
                });
                toggler::Style {
                    background: track.into(),
                    background_border_width: s.border_width,
                    background_border_color: s.border_color.unwrap_or(Color::TRANSPARENT),
                    foreground: h
                        .background
                        .or(s.color)
                        .unwrap_or(theme.palette().background)
                        .into(),
                    foreground_border_width: h.border_width,
                    foreground_border_color: h.border_color.unwrap_or(Color::TRANSPARENT),
                    text_color: None,
                    // Round unless the theme set a radius on the track.
                    border_radius: (s.border_radius != Radius::default())
                        .then_some(s.border_radius),
                    padding_ratio: 0.1,
                }
            },
        );
        container(toggle).id(id)
    }

    /// The room a surface needs around its root box `node` for the
    /// box's `box-shadow` to show: a surface is only drawn inside its own
    /// bounds, so one the size of the box cuts the shadow off (iced draws
    /// it `blur` past the box moved by the offset). Zero without one.
    pub fn shadow_room(&self, node: &Node) -> Padding {
        let Some(shadow) = self.resolve(node).shadow.filter(|s| s.color.a > 0.0) else {
            return Padding::ZERO;
        };
        let side = |offset: f32| (shadow.blur_radius + offset).max(0.0).ceil();
        Padding {
            top: side(-shadow.offset.y),
            right: side(shadow.offset.x),
            bottom: side(shadow.offset.y),
            left: side(-shadow.offset.x),
        }
    }

    /// The content of a surface whose root box is `node`: inset by its
    /// [`Theme::shadow_room`], which the surface's size includes.
    pub fn surface<'a, M: 'a>(
        &self,
        node: &Node,
        content: impl Into<Element<'a, M>>,
    ) -> Container<'a, M> {
        container(content)
            .padding(self.shadow_room(node))
            .width(IcedLength::Fill)
            .height(IcedLength::Fill)
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

impl Style {
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
