//! Text: a paragraph kept in the widget tree (shaped again only when the
//! content, its style or its width changes), its box trimmed to its
//! letters as `text-box` says, cut with an ellipsis when it doesn't fit.

use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::Arc;

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph as _, Renderer as _, paragraph::Plain};
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell};
use iced::alignment::Vertical;
use iced::widget::text::{LineHeight, Shaping, Wrapping};
use iced::{Event, Length, Pixels, Point, Rectangle, Size};

use crate::anim::Animated;
use crate::layout::{snap, snap_up};
use crate::paint::fade;
use crate::query::Reported;
use crate::style::{
    Computed, EdgeOver, EdgeUnder, Node, Size as Len, States, TextAlign, TextOverflow,
    TextTransform, WhiteSpace,
};
use crate::{Element, Renderer, Theme};

type Paragraph = <Renderer as text::Renderer>::Paragraph;

pub struct Text<'a> {
    node: Node<'a>,
    style: Arc<Computed>,
    content: Cow<'a, str>,
}

/// `content`, as `node`'s style says (`text-transform` included).
pub fn text<'a>(node: Node<'a>, content: impl Into<Cow<'a, str>>) -> Text<'a> {
    let style = node.base();
    let content = transform(content.into(), style.text_transform);
    Text {
        node,
        style,
        content,
    }
}

/// A mark: the text its node's `content` says (a menu item's check, a
/// calendar's arrows), if any.
pub(crate) fn mark<'a, Message>(node: Node<'a>) -> Element<'a, Message> {
    let content = node.base().content.clone();
    text(node, content.as_deref().unwrap_or("").to_owned()).into()
}

fn transform(content: Cow<'_, str>, how: TextTransform) -> Cow<'_, str> {
    match how {
        TextTransform::None => content,
        TextTransform::Uppercase => Cow::Owned(content.to_uppercase()),
        TextTransform::Lowercase => Cow::Owned(content.to_lowercase()),
        TextTransform::Capitalize => {
            let mut out = String::with_capacity(content.len());
            let mut start = true;
            for c in content.chars() {
                if start && c.is_alphanumeric() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                start = c.is_whitespace();
            }
            Cow::Owned(out)
        }
    }
}

struct State {
    paragraph: Plain<Paragraph>,
    /// What `text-box-trim` cuts: from the paragraph's top down to the
    /// box's, from the box's bottom down to the last line box's.
    trim: (f32, f32),
    /// The first line's baseline, from the paragraph's top.
    baseline: f32,
    /// The text cut to fit with an ellipsis, for the width it was cut
    /// at: made at draw, where the width is the final one (layout may
    /// ask several), so behind a `RefCell` (draw has the tree
    /// read-only).
    cut: RefCell<Option<(f32, Plain<Paragraph>)>>,
    /// Its colour's and opacity's transitions and animations.
    animated: Animated,
}

/// What a paragraph is shaped from: `width` to wrap at. Its lines are
/// aligned among themselves by `text-align`; where the paragraph goes in
/// its box is the widget's (`draw`), so the alignment is the same at any
/// width, and a change of width is no new shaping.
fn spec<C>(content: C, s: &Computed, width: f32) -> iced::advanced::Text<C> {
    iced::advanced::Text {
        content,
        bounds: Size::new(width, f32::INFINITY),
        size: Pixels(s.font_size),
        line_height: LineHeight::Absolute(Pixels(s.line_height_px)),
        font: s.font,
        align_x: match s.text_align {
            TextAlign::Start => text::Alignment::Default,
            TextAlign::Left => text::Alignment::Left,
            TextAlign::Center => text::Alignment::Center,
            TextAlign::End | TextAlign::Right => text::Alignment::Right,
        },
        align_y: Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping: match s.white_space {
            WhiteSpace::Normal => Wrapping::Word,
            WhiteSpace::Nowrap => Wrapping::None,
        },
    }
}

/// The trims and the first baseline, from the lines as shaped.
fn trims(paragraph: &Paragraph, s: &Computed) -> ((f32, f32), f32) {
    let mut first = None;
    let mut last = None;
    for run in paragraph.buffer().layout_runs() {
        let line = (run.line_y, run.line_top, run.line_height);
        first.get_or_insert(line);
        last = Some(line);
    }
    let (Some((baseline, top, _)), Some((last_baseline, last_top, last_height))) = (first, last)
    else {
        return ((0.0, 0.0), 0.0);
    };
    let over = match s.text_box_edge.over {
        EdgeOver::Text => s.ascent,
        EdgeOver::Cap => s.cap_height,
        EdgeOver::Ex => s.x_height,
    };
    let under = match s.text_box_edge.under {
        EdgeUnder::Text => s.descent,
        EdgeUnder::Alphabetic => 0.0,
    };
    let trim_top = if s.text_box_trim.start {
        (baseline - over - top).max(0.0)
    } else {
        0.0
    };
    let trim_bottom = if s.text_box_trim.end {
        (last_top + last_height - (last_baseline + under)).max(0.0)
    } else {
        0.0
    };
    ((trim_top, trim_bottom), baseline)
}

impl Text<'_> {
    fn ellipsis(&self) -> bool {
        self.style.text_overflow == TextOverflow::Ellipsis
            && self.style.white_space == WhiteSpace::Nowrap
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Text<'_> {
    fn operate(
        &mut self,
        _tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.custom(None, layout.bounds(), &mut Reported::new(self.node));
    }

    fn size(&self) -> Size<Length> {
        let length = |len: Len| match len {
            Len::Auto => Length::Shrink,
            Len::Px(v) => Length::Fixed(v),
            Len::Fill => Length::Fill,
        };
        Size::new(length(self.style.width), length(self.style.height))
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            paragraph: Plain::new(spec(self.content.to_string(), &self.style, f32::INFINITY)),
            trim: (0.0, 0.0),
            baseline: 0.0,
            cut: RefCell::new(None),
            animated: Animated::new(self.node),
        })
    }

    fn diff(&self, tree: &mut Tree) {
        tree.state.downcast_mut::<State>().animated.diff(self.node);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        tree.state
            .downcast_mut::<State>()
            .animated
            .update(event, shell);
    }

    fn layout(&mut self, tree: &mut Tree, _renderer: &Renderer, limits: &Limits) -> layout::Node {
        let s = &self.style;
        let scale = self.node.scale();
        let state = tree.state.downcast_mut::<State>();
        // Its width when it isn't its content's: its own, its share, the
        // one it's stretched to. Shaped at it, it wraps and aligns there,
        // in one go.
        let known = match s.width {
            Len::Px(v) => Some(snap(v, scale)),
            Len::Fill if !limits.compression().width => Some(limits.max().width),
            _ if limits.min().width > 0.0 => Some(limits.min().width),
            _ => None,
        };
        // One line is the same at any width: shaped once.
        let shaped = match s.white_space {
            WhiteSpace::Normal => known.unwrap_or(limits.max().width),
            WhiteSpace::Nowrap => f32::INFINITY,
        };
        if state.paragraph.update(spec(&self.content, s, shaped)) {
            *state.cut.get_mut() = None;
        }
        (state.trim, state.baseline) = trims(state.paragraph.raw(), s);

        let natural = if self.content.is_empty() {
            Size::ZERO
        } else {
            let bounds = state.paragraph.min_bounds();
            Size::new(
                snap_up(bounds.width, scale),
                snap_up(bounds.height - state.trim.0 - state.trim.1, scale),
            )
        };
        let mut width = known.unwrap_or(natural.width);
        if self.ellipsis() {
            // Cut to fit, down to nothing: CSS's `overflow: hidden`.
            width = width.min(limits.max().width);
        }
        let height = match s.height {
            Len::Px(v) => snap(v, scale),
            Len::Fill if !limits.compression().height => limits.max().height,
            _ => natural.height,
        };
        layout::Node::new(Size::new(
            width.max(limits.min().width),
            height.max(limits.min().height),
        ))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if self.content.is_empty() {
            return;
        }
        let state = tree.state.downcast_ref::<State>();
        // Its colour may follow an ancestor's state: `button:hover label`.
        let resolved = self.node.resolve(States::NONE, &theme.ancestors);
        let moving = state.animated.style(&resolved, theme);
        let s = moving.as_ref().unwrap_or(&resolved);
        let color = fade(s.color, theme.opacity * s.opacity);
        let bounds = layout.bounds();
        let scale = self.node.scale();
        // The first baseline on a whole device pixel.
        let mut y = bounds.y - state.trim.0;
        let baseline = y + state.baseline;
        y += snap(baseline, scale) - baseline;
        // iced aligns a paragraph's lines among themselves; where the
        // paragraph goes in its box is the widget's to say.
        let anchor = |paragraph: &Plain<Paragraph>| {
            let free = bounds.width - paragraph.min_bounds().width;
            let x = match paragraph.align_x() {
                text::Alignment::Center => bounds.x + free / 2.0,
                text::Alignment::Right => bounds.x + free,
                _ => bounds.x,
            };
            Point::new(snap(x, scale), y)
        };

        if !self.ellipsis() || state.paragraph.min_bounds().width <= bounds.width + 0.5 {
            let position = anchor(&state.paragraph);
            renderer.fill_paragraph(state.paragraph.raw(), position, color, *viewport);
            return;
        }
        let mut cut = state.cut.borrow_mut();
        if cut.as_ref().is_none_or(|(width, _)| *width != bounds.width) {
            *cut = Some((bounds.width, self.cut(&state.paragraph, bounds.width)));
        }
        let (_, paragraph) = cut.as_ref().expect("just made");
        renderer.fill_paragraph(paragraph.raw(), anchor(paragraph), color, *viewport);
    }
}

impl Text<'_> {
    /// The text cut to `width` with an ellipsis: the grapheme under the
    /// cut found with the shaped line's hit test.
    fn cut(&self, full: &Plain<Paragraph>, width: f32) -> Plain<Paragraph> {
        let s = &self.style;
        let ellipsis = Plain::<Paragraph>::new(spec("…".to_string(), s, f32::INFINITY));
        let room = (width - ellipsis.min_bounds().width).max(0.0);
        let middle = full.min_bounds().height / 2.0;
        let mut at = full
            .raw()
            .hit_test(Point::new(room, middle))
            .map_or(0, |hit| hit.cursor())
            .min(self.content.len());
        while !self.content.is_char_boundary(at) {
            at -= 1;
        }
        let kept = format!("{}…", self.content[..at].trim_end());
        Plain::new(spec(kept, s, f32::INFINITY))
    }
}

impl<'a, Message> From<Text<'a>> for Element<'a, Message> {
    fn from(text: Text<'a>) -> Self {
        Element::new(text)
    }
}
