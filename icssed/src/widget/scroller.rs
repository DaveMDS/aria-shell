//! Content taller or wider than its room, scrolled. A box: the content
//! in iced's scrollable (which scrolls: the wheel, a finger, iced's
//! operations), its bars hidden, over the content box and, on the axes
//! it scrolls, the padding, which scrolls with the content as in CSS; its scrollbars in its
//! padding box, as items in one cell of a grid, each placed by its
//! `justify-self` and `align-self`, at the end of its axis unless said,
//! over the content (its margins move it, out of the scroller too), shown
//! while the content overflows; a thumb as long as the part of the
//! content shown, where it's shown. Each is a box as any other. A scrollbar and its thumb are
//! `:active` while the thumb is dragged; a press on the bar elsewhere
//! brings the thumb's middle there and drags it. Scrolled only sideways,
//! it takes the wheel's turns sideways too, as GTK's do.

use std::cell::{Cell, OnceCell, Ref, RefCell};
use std::rc::Rc;

use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::operation::{self, Operation};
use iced::advanced::widget::{Id, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::widget::scrollable::{AbsoluteOffset, Direction, Scrollable, Scrollbar, Viewport};
use iced::{Event, Length, Point, Rectangle, Size, Vector, touch};

use super::boxed::{BoxState, Boxed, border_box};
use super::place::{self, Place, placed};
use crate::layout::{Sizing, content_box, snap};
use crate::query::Reported;
use crate::style::{Align, Ancestors, Computed, MinSize, Node, Size as Len, States, name};
use crate::{Element, Renderer, Theme};

pub struct Scroller<'a, Message> {
    boxed: Boxed<'a>,
    bars: Bars,
    id: Option<Id>,
    /// What the scrollable is made of, until the scroller has its node.
    pending: Cell<Option<Pending<'a, Message>>>,
    scrollable: RefCell<Option<Scrollable<'a, Message, Theme, Renderer>>>,
    /// Its parts, made with its node.
    parts: OnceCell<Parts<'a>>,
    /// Its padding on the axes it scrolls, which scrolls with the
    /// content: its `Padded`'s, set at each layout.
    moving: Rc<Cell<[f32; 4]>>,
}

/// Which ways it scrolls.
#[derive(Clone, Copy, PartialEq)]
enum Bars {
    Vertical,
    Horizontal,
    Both,
}

struct Pending<'a, Message> {
    content: Element<'a, Message>,
    on_scroll: Option<Box<dyn Fn(Viewport) -> Message + 'a>>,
}

/// The boxes it's made of besides its own.
struct Parts<'a> {
    /// The vertical bar, the horizontal one: those it scrolls by.
    bars: [Option<Bar<'a>>; 2],
}

/// A scrollbar and its thumb.
struct Bar<'a> {
    bar: Boxed<'a>,
    thumb: Boxed<'a>,
    vertical: bool,
}

/// A part at `node`.
fn part(node: Node<'_>, kind: crate::style::Name) -> Boxed<'_> {
    let mut place = Place::new(kind);
    place.pin(node);
    Boxed::placed(place)
}

/// `content` scrolled up and down: a `scroller.vertical`, its
/// `scrollbar.vertical > thumb`.
pub fn scroller<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> Scroller<'a, Message> {
    let mut boxed = Boxed::of(name!("scroller"));
    boxed.place.class("vertical");
    Scroller {
        boxed,
        bars: Bars::Vertical,
        id: None,
        pending: Cell::new(Some(Pending {
            content: content.into(),
            on_scroll: None,
        })),
        scrollable: RefCell::new(None),
        parts: OnceCell::new(),
        moving: Rc::default(),
    }
}

/// What the scrollable shows: its bounds, its content's, how far it's
/// scrolled.
#[derive(Debug, Clone, Copy, Default)]
struct Seen {
    bounds: Size,
    content: Size,
    translation: Vector,
}

impl Seen {
    /// Whether the content overflows on the axis, so that its bar shows.
    fn overflows(&self, vertical: bool) -> bool {
        if vertical {
            self.content.height > self.bounds.height
        } else {
            self.content.width > self.bounds.width
        }
    }

    /// How far it's scrolled on the axis, from 0 to 1.
    fn fraction(&self, vertical: bool) -> f32 {
        let (scrolled, room) = if vertical {
            (self.translation.y, self.content.height - self.bounds.height)
        } else {
            (self.translation.x, self.content.width - self.bounds.width)
        };
        if room > 0.0 {
            (scrolled / room).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The part of the content shown on the axis, from 0 to 1.
    fn shown(&self, vertical: bool) -> f32 {
        let (bounds, content) = if vertical {
            (self.bounds.height, self.content.height)
        } else {
            (self.bounds.width, self.content.width)
        };
        if content > 0.0 {
            (bounds / content).min(1.0)
        } else {
            1.0
        }
    }

    /// The offset of `fraction` of the way on the axis.
    fn offset(&self, vertical: bool, fraction: f32) -> AbsoluteOffset<Option<f32>> {
        let room = |content: f32, bounds: f32| (content - bounds).max(0.0) * fraction;
        if vertical {
            AbsoluteOffset {
                x: None,
                y: Some(room(self.content.height, self.bounds.height)),
            }
        } else {
            AbsoluteOffset {
                x: Some(room(self.content.width, self.bounds.width)),
                y: None,
            }
        }
    }
}

/// What a scroller keeps in the widget tree.
struct State {
    scroller: BoxState,
    /// The vertical bar's, the horizontal one's: those it scrolls by.
    bars: [Option<BarState>; 2],
    seen: Seen,
}

struct BarState {
    bar: BoxState,
    thumb: BoxState,
    /// How far the thumb moves from its start to its end.
    travel: f32,
    /// Where the thumb was grabbed, from its start, while it's dragged.
    grabbed: Option<f32>,
}

/// The states of its parts: the scroller, each bar and its thumb.
type Snapshot = [States; 5];

impl State {
    fn snapshot(&self) -> Snapshot {
        let bar = |i: usize| {
            self.bars[i]
                .as_ref()
                .map_or([States::NONE; 2], |b| [b.bar.states, b.thumb.states])
        };
        let ([v, vt], [h, ht]) = (bar(0), bar(1));
        [self.scroller.states, v, vt, h, ht]
    }
}

/// The operation that asks the scrollable what it shows, and scrolls
/// it to `to` first if given. It goes no deeper.
#[derive(Default)]
struct Peek {
    to: Option<AbsoluteOffset<Option<f32>>>,
    seen: Option<Seen>,
}

impl Operation for Peek {
    fn traverse(&mut self, _operate: &mut dyn FnMut(&mut dyn Operation)) {}

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        if let Some(to) = self.to.take() {
            state.scroll_to(to);
        }
        self.seen = Some(Seen {
            bounds: bounds.size(),
            content: content_bounds.size(),
            translation,
        });
    }
}

impl<'a, Message: 'a> Scroller<'a, Message> {
    placed!(boxed.place);

    /// Scrolled sideways instead: a `scroller.horizontal`, its
    /// `scrollbar.horizontal`.
    pub fn horizontal(mut self) -> Self {
        self.bars = Bars::Horizontal;
        self.boxed.place.unclass("vertical");
        self.boxed.place.unclass("horizontal");
        self.boxed.place.class("horizontal");
        self
    }

    /// Scrolled both ways: a `scroller.vertical.horizontal`, both its
    /// scrollbars.
    pub fn both(mut self) -> Self {
        self.bars = Bars::Both;
        self.boxed.place.unclass("vertical");
        self.boxed.place.unclass("horizontal");
        self.boxed.place.class("vertical");
        self.boxed.place.class("horizontal");
        self
    }

    /// Its id, for the app to scroll it (`iced::widget::operation`).
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// What a scroller sends: where it is now.
    pub fn on_scroll(mut self, on_scroll: impl Fn(Viewport) -> Message + 'a) -> Self {
        if let Some(pending) = self.pending.get_mut() {
            pending.on_scroll = Some(Box::new(on_scroll));
        }
        self
    }

    fn parts(&self) -> &Parts<'a> {
        self.parts.get_or_init(|| {
            let node = self.boxed.node();
            let bar = |vertical: bool| {
                let bar =
                    node.child("scrollbar")
                        .class(if vertical { "vertical" } else { "horizontal" });
                Bar {
                    bar: part(bar, name!("scrollbar")),
                    thumb: part(bar.child("thumb"), name!("thumb")),
                    vertical,
                }
            };
            let (vertical, horizontal) = match self.bars {
                Bars::Vertical => (true, false),
                Bars::Horizontal => (false, true),
                Bars::Both => (true, true),
            };
            Parts {
                bars: [vertical.then(|| bar(true)), horizontal.then(|| bar(false))],
            }
        })
    }

    /// iced's scrollable, made the first time: it asks its content's
    /// size as it's made, so the content is reached under the scroller.
    fn scrollable(&self) -> Ref<'_, Scrollable<'a, Message, Theme, Renderer>> {
        if let Some(Pending { content, on_scroll }) = self.pending.take() {
            let node = self.boxed.node();
            let direction = match self.bars {
                Bars::Vertical => Direction::Vertical(Scrollbar::hidden()),
                Bars::Horizontal => Direction::Horizontal(Scrollbar::hidden()),
                Bars::Both => Direction::Both {
                    vertical: Scrollbar::hidden(),
                    horizontal: Scrollbar::hidden(),
                },
            };
            let content = Padded {
                content,
                padding: self.moving.clone(),
            };
            let mut scrollable = place::under(node, || {
                Scrollable::with_direction(Element::new(content), direction)
            });
            // It fills the content box where the scroller has a size of its
            // own; else it's as iced has it: the content's, or the room's
            // when the content fills across.
            let s = self.boxed.style();
            if s.width != Len::Auto {
                scrollable = scrollable.width(Length::Fill);
            }
            if s.height != Len::Auto {
                scrollable = scrollable.height(Length::Fill);
            }
            if let Some(id) = self.id.clone() {
                scrollable = scrollable.id(id);
            }
            if let Some(on_scroll) = on_scroll {
                scrollable = scrollable.on_scroll(on_scroll);
            }
            *self.scrollable.borrow_mut() = Some(scrollable);
        }
        Ref::map(self.scrollable.borrow(), |s| s.as_ref().expect("made"))
    }

    fn scrollable_mut(&mut self) -> &mut Scrollable<'a, Message, Theme, Renderer> {
        drop(self.scrollable());
        self.scrollable.get_mut().as_mut().expect("made")
    }

    /// What the scrollable at `layout` shows, scrolled to `to` first if
    /// given.
    fn peek(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        to: Option<AbsoluteOffset<Option<f32>>>,
    ) -> Seen {
        let mut peek = Peek { to, seen: None };
        Widget::<Message, Theme, Renderer>::operate(
            self.scrollable_mut(),
            tree,
            layout,
            renderer,
            &mut peek,
        );
        peek.seen.unwrap_or_default()
    }

    /// Each part's style for its layout: its own states, under its
    /// parents' in the scroller (the scroller's own ancestors reach it only
    /// at draw). The scroller, each bar and its thumb.
    fn layout_styles(&self, states: Snapshot) -> [Option<std::sync::Arc<Computed>>; 5] {
        let parts = self.parts();
        let scroller = self.boxed.node();
        let seen = |node: Node<'_>, states: States| states & node.sensitivity().any();
        let under = Ancestors::default().push(scroller.depth(), seen(scroller, states[0]));
        let mut out = [
            Some(self.boxed.layout_style(states[0])),
            None,
            None,
            None,
            None,
        ];
        for (i, bar) in parts.bars.iter().enumerate() {
            let Some(bar) = bar else { continue };
            let (b, t) = (states[1 + 2 * i], states[2 + 2 * i]);
            let node = bar.bar.node();
            out[1 + 2 * i] = Some(node.resolve(b, &under));
            let under = under.push(node.depth(), seen(node, b));
            out[2 + 2 * i] = Some(bar.thumb.node().resolve(t, &under));
        }
        out
    }
}

/// The padding box of a box styled `s` whose margin box is `bounds`.
fn padding_box(bounds: Rectangle, s: &Computed, scale: f32) -> Rectangle {
    let border = border_box(bounds, s, scale);
    let b = s.border_width;
    Rectangle {
        x: border.x + b.left,
        y: border.y + b.top,
        width: (border.width - b.left - b.right).max(0.0),
        height: (border.height - b.top - b.bottom).max(0.0),
    }
}

/// Where a margin box `size` long starts in a cell `room` long, as
/// `align` says.
fn aligned(align: Align, room: f32, size: f32, scale: f32) -> f32 {
    match align {
        Align::Start | Align::Stretch => 0.0,
        Align::Center => snap((room - size) / 2.0, scale),
        Align::End => room - size,
    }
}

/// A bar's margin box in the scroller's padding box `cell`, styled `s`:
/// along its axis as long as the cell unless it has a length of its own,
/// across its `width` (a vertical one's) or `height`, the cell's if
/// `stretch`ed; placed by its `justify-self` and `align-self`.
fn bar_box(s: &Computed, cell: Rectangle, vertical: bool, scale: f32) -> Rectangle {
    let stretched = |len: Len, align: Align, along: bool| {
        len == Len::Auto && (along || align == Align::Stretch)
    };
    let wide = stretched(s.width, s.justify_self, !vertical);
    let tall = stretched(s.height, s.align_self, vertical);
    // Not stretched, nothing asked of it: its margins alone (a negative
    // one asks no more of its size).
    let m = s.margin;
    let (across_w, across_h) = (
        snap(m.left, scale) + snap(m.right, scale),
        snap(m.top, scale) + snap(m.bottom, scale),
    );
    let limits = Limits::new(
        Size::new(
            if wide { cell.width } else { across_w },
            if tall { cell.height } else { across_h },
        ),
        Size::new(
            if wide { cell.width } else { f32::INFINITY },
            if tall { cell.height } else { f32::INFINITY },
        ),
    );
    let (size, _) = Sizing::new(s, scale, &limits).around(Size::ZERO);
    Rectangle {
        x: cell.x + aligned(s.justify_self, cell.width, size.width, scale),
        y: cell.y + aligned(s.align_self, cell.height, size.height, scale),
        width: size.width,
        height: size.height,
    }
}

/// A thumb's margin box at the start of its bar's content box `room`,
/// styled `s`, for a content `shown` of which shows: along, that part of
/// the room, at least its `min-height` (a vertical one's) or
/// `min-width`; across, its `width` (or `height`) if it has one, placed
/// by its `justify-self` (or `align-self`), else the room's. And how
/// far it moves.
fn thumb_box(
    s: &Computed,
    room: Rectangle,
    vertical: bool,
    shown: f32,
    scale: f32,
) -> (Rectangle, f32) {
    let sn = |v: f32| snap(v, scale);
    let m = s.margin;
    let (along, across) = if vertical {
        (room.height, room.width)
    } else {
        (room.width, room.height)
    };
    let (start, end, side, other) = if vertical {
        (sn(m.top), sn(m.bottom), sn(m.left), sn(m.right))
    } else {
        (sn(m.left), sn(m.right), sn(m.top), sn(m.bottom))
    };
    let (min, len, align) = if vertical {
        (s.min_height, s.width, s.justify_self)
    } else {
        (s.min_width, s.height, s.align_self)
    };
    let min = match min {
        MinSize::Px(v) => sn(v),
        MinSize::Auto => 0.0,
    };
    let track = (along - start - end).max(0.0);
    let length = sn(track * shown).max(min).min(track);
    let thick = match len {
        Len::Px(v) => sn(v) + side + other,
        Len::Auto | Len::Fill => across,
    };
    let offset = aligned(align, across, thick, scale);
    let rect = if vertical {
        Rectangle::new(
            Point::new(room.x + offset, room.y),
            Size::new(thick, length + start + end),
        )
    } else {
        Rectangle::new(
            Point::new(room.x, room.y + offset),
            Size::new(length + start + end, thick),
        )
    };
    (rect, (track - length).max(0.0))
}

/// A bar where it's laid out, and its thumb's margin box where it is
/// now: moved from its start as far as the content is scrolled.
#[derive(Clone, Copy)]
struct Placed {
    bar: Rectangle,
    thumb: Rectangle,
}

impl Placed {
    fn new(at: Layout<'_>, vertical: bool, state: &BarState, seen: Seen, scale: f32) -> Self {
        let thumb = at.children().next().expect("a thumb").bounds();
        let by = snap(seen.fraction(vertical) * state.travel, scale);
        Self {
            bar: at.bounds(),
            thumb: thumb
                + if vertical {
                    Vector::new(0.0, by)
                } else {
                    Vector::new(by, 0.0)
                },
        }
    }
}

impl<'a, Message: 'a> Scroller<'a, Message> {
    /// Each bar that shows (the content overflows on its axis, neither
    /// it nor its thumb is `display: none`), with its state and where
    /// it is.
    fn shown<'t>(
        &self,
        state: &'t State,
        layout: Layout<'_>,
    ) -> impl Iterator<Item = (usize, &Bar<'a>, &'t BarState, Placed)> {
        let parts = self.parts();
        let seen = state.seen;
        let scale = self.boxed.node().scale();
        layout
            .children()
            .skip(1)
            .zip(parts.bars.iter().zip(&state.bars))
            .enumerate()
            .filter_map(move |(i, (at, (bar, s)))| {
                let (bar, s) = (bar.as_ref()?, s.as_ref()?);
                let shows = !bar.bar.place.hidden()
                    && !bar.thumb.place.hidden()
                    && seen.overflows(bar.vertical);
                shows.then(|| (i, bar, s, Placed::new(at, bar.vertical, s, seen, scale)))
            })
    }
}

impl<'a, Message: 'a> Widget<Message, Theme, Renderer> for Scroller<'a, Message> {
    fn size(&self) -> Size<Length> {
        let own = self.boxed.size();
        if self.boxed.place.hidden() {
            return own;
        }
        // Without a size of its own, the scrollable's: the content's
        // across, when it fills.
        let inner = Widget::<Message, Theme, Renderer>::size(&*self.scrollable());
        let s = self.boxed.style();
        Size::new(
            if s.width == Len::Auto {
                inner.width
            } else {
                own.width
            },
            if s.height == Len::Auto {
                inner.height
            } else {
                own.height
            },
        )
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        let parts = self.parts();
        let bar = |bar: &Option<Bar<'_>>| {
            bar.as_ref().map(|bar| BarState {
                bar: bar.bar.state(),
                thumb: bar.thumb.state(),
                travel: 0.0,
                grabbed: None,
            })
        };
        tree::State::new(State {
            scroller: self.boxed.state(),
            bars: [bar(&parts.bars[0]), bar(&parts.bars[1])],
            seen: Seen::default(),
        })
    }

    fn children(&self) -> Vec<Tree> {
        let scrollable = self.scrollable();
        place::under(self.boxed.node(), || {
            vec![Tree::new(
                &*scrollable as &dyn Widget<Message, Theme, Renderer>,
            )]
        })
    }

    fn diff(&self, tree: &mut Tree) {
        let parts = self.parts();
        let state = tree.state.downcast_mut::<State>();
        state.scroller.animated.diff(self.boxed.node());
        for (bar, state) in parts.bars.iter().zip(&mut state.bars) {
            if let (Some(bar), Some(state)) = (bar, state) {
                state.bar.animated.diff(bar.bar.node());
                state.thumb.animated.diff(bar.thumb.node());
            }
        }
        let scrollable = self.scrollable();
        place::under(self.boxed.node(), || {
            tree.children[0].diff(&*scrollable as &dyn Widget<Message, Theme, Renderer>);
        });
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let styles = self.layout_styles(tree.state.downcast_ref::<State>().snapshot());
        let Some(s) = &styles[0] else {
            unreachable!("a scroller's style")
        };
        let scale = self.boxed.node().scale();
        let sizing = Sizing::new(s, scale, limits);
        let room = sizing.room();
        let (wide, tall) = sizing.definite();
        // Its padding on the axes it scrolls scrolls with the content, as
        // in CSS: the scrollable takes it, and pads the content with it.
        let (down, across) = match self.bars {
            Bars::Vertical => (true, false),
            Bars::Horizontal => (false, true),
            Bars::Both => (true, true),
        };
        let p = s.padding;
        let on = |yes: bool, v: f32| if yes { snap(v, scale) } else { 0.0 };
        let moving = [
            on(down, p.top),
            on(across, p.right),
            on(down, p.bottom),
            on(across, p.left),
        ];
        self.moving.set(moving);
        let extra = Size::new(moving[1] + moving[3], moving[0] + moving[2]);
        let grown = |size: Size| Size::new(size.width + extra.width, size.height + extra.height);
        // The scrollable over the content box (and that padding), filling
        // it where the scroller has a size of its own, the content at least
        // as large.
        let mut lay = |this: &mut Self, min: Size, max: Size| {
            Widget::<Message, Theme, Renderer>::layout(
                this.scrollable_mut(),
                &mut tree.children[0],
                renderer,
                &Limits::new(min, max),
            )
        };
        let exact = Size::new(
            if wide { room.width + extra.width } else { 0.0 },
            if tall {
                room.height + extra.height
            } else {
                0.0
            },
        );
        let mut inner = lay(self, exact, grown(room));
        let shown = Size::new(
            (inner.size().width - extra.width).max(0.0),
            (inner.size().height - extra.height).max(0.0),
        );
        let (size, content) = sizing.around(shown);
        // A scroller held larger than its content (its minimum): the
        // scrollable fills it.
        if (content.width - shown.width).abs() > 0.01
            || (content.height - shown.height).abs() > 0.01
        {
            inner = lay(self, grown(content.size()), grown(content.size()));
        }
        let inner = inner.move_to(Point::new(content.x - moving[3], content.y - moving[0]));
        let seen = self.peek(&mut tree.children[0], Layout::new(&inner), renderer, None);

        // The bars in the padding box, their thumbs at their start.
        let cell = padding_box(Rectangle::new(Point::ORIGIN, size), s, scale);
        let mut children = vec![inner];
        let parts = self.parts();
        let state = tree.state.downcast_mut::<State>();
        state.seen = seen;
        for (i, (bar, state)) in parts.bars.iter().zip(&mut state.bars).enumerate() {
            let (Some(bar), Some(state), Some(b), Some(t)) =
                (bar, state, &styles[1 + 2 * i], &styles[2 + 2 * i])
            else {
                children.push(layout::Node::new(Size::ZERO));
                continue;
            };
            let at = bar_box(b, cell, bar.vertical, scale);
            let room = content_box(at.size(), b, scale);
            let (thumb, travel) = thumb_box(t, room, bar.vertical, seen.shown(bar.vertical), scale);
            state.travel = travel;
            children.push(
                layout::Node::with_children(
                    at.size(),
                    vec![layout::Node::new(thumb.size()).move_to(thumb.position())],
                )
                .move_to(at.position()),
            );
        }
        layout::Node::with_children(size, children)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let scrollable = self.scrollable();
        let inner = layout.children().next().expect("a scrollable");
        self.boxed.draw(
            &state.scroller,
            renderer,
            theme,
            layout.bounds(),
            *viewport,
            cursor,
            |renderer, theme, style, cursor, _| {
                Widget::<Message, Theme, Renderer>::draw(
                    &*scrollable,
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    inner,
                    cursor,
                    viewport,
                );
                // Over the content, which the scrollable draws in a layer
                // of its own.
                renderer.with_layer(*viewport, |renderer| {
                    for (_, bar, s, at) in self.shown(state, layout) {
                        bar.bar.draw(
                            &s.bar,
                            renderer,
                            theme,
                            at.bar,
                            *viewport,
                            cursor,
                            |renderer, theme, _, cursor, _| {
                                bar.thumb.draw(
                                    &s.thumb,
                                    renderer,
                                    theme,
                                    at.thumb,
                                    *viewport,
                                    cursor,
                                    |_, _, _, _, _| {},
                                );
                            },
                        );
                    }
                });
            },
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let inner = layout.children().next().expect("a scrollable");
        let before = tree.state.downcast_ref::<State>().snapshot();

        // Where the pointer is (through the transforms each part was
        // drawn with): over the scroller, each bar and thumb; how far
        // along a bar from its thumb's start.
        let state = tree.state.downcast_ref::<State>();
        let cursor = state.scroller.local(cursor);
        let over = cursor.is_over(self.boxed.border_box(layout.bounds()));
        let mut pointer = [None::<Pointer>; 2];
        for (i, bar, s, at) in self.shown(state, layout) {
            let on_bar = s.bar.local(cursor);
            let on_thumb = s.thumb.local(on_bar);
            let thumb = bar.thumb.border_box(at.thumb);
            let (start, length) = if bar.vertical {
                (thumb.y, thumb.height)
            } else {
                (thumb.x, thumb.width)
            };
            pointer[i] = Some(Pointer {
                bar: on_bar.is_over(bar.bar.border_box(at.bar)),
                thumb: on_thumb.is_over(thumb),
                from_thumb: on_thumb
                    .land()
                    .position()
                    .map(|p| if bar.vertical { p.y } else { p.x } - start),
                length,
            });
        }

        // The bars first: they're over the content. A press on one
        // grabs its thumb (brought there if it wasn't on it), a move
        // drags it, a release lets it go.
        let mut to = None;
        let state = tree.state.downcast_mut::<State>();
        let seen = state.seen;
        for (i, bar) in self.parts().bars.iter().enumerate() {
            let (Some(bar), Some(s)) = (bar, &mut state.bars[i]) else {
                continue;
            };
            let p = pointer[i];
            let vertical = bar.vertical;
            // The offset the thumb shows at, `from_thumb` the pointer from
            // its start, grabbed at `grabbed`.
            let drag = |s: &BarState, from_thumb: f32, grabbed: f32| {
                let moved = seen.fraction(vertical) * s.travel + from_thumb - grabbed;
                let fraction = if s.travel > 0.0 {
                    (moved / s.travel).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                seen.offset(vertical, fraction)
            };
            match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                | Event::Touch(touch::Event::FingerPressed { .. })
                    if !shell.is_event_captured() =>
                {
                    let Some(p) = p.filter(|p| p.bar) else {
                        continue;
                    };
                    let Some(from_thumb) = p.from_thumb else {
                        continue;
                    };
                    let grabbed = if p.thumb {
                        from_thumb
                    } else {
                        let middle = p.length / 2.0;
                        to = Some(drag(s, from_thumb, middle));
                        middle
                    };
                    s.grabbed = Some(grabbed);
                    s.bar.states = s.bar.states.with(States::ACTIVE, true);
                    s.thumb.states = s.thumb.states.with(States::ACTIVE, true);
                    shell.capture_event();
                }
                Event::Mouse(mouse::Event::CursorMoved { .. })
                | Event::Touch(touch::Event::FingerMoved { .. }) => {
                    if let Some(grabbed) = s.grabbed {
                        if let Some(from_thumb) = p.and_then(|p| p.from_thumb) {
                            to = Some(drag(s, from_thumb, grabbed));
                        }
                        shell.capture_event();
                    }
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                | Event::Touch(
                    touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. },
                ) if s.grabbed.is_some() => {
                    s.grabbed = None;
                    s.bar.states = s.bar.states.with(States::ACTIVE, false);
                    s.thumb.states = s.thumb.states.with(States::ACTIVE, false);
                    shell.capture_event();
                }
                _ => {}
            }
        }
        let dragging = state.bars.iter().flatten().any(|s| s.grabbed.is_some());

        // Then the scrollable, but not where a bar is, nor while a thumb
        // is dragged; the wheel's turns over a bar scroll it, wherever
        // the bar is. Scrolled only sideways, the turns go sideways.
        let on_a_bar = pointer.iter().flatten().any(|p| p.bar);
        let turned;
        let event = match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta })
                if self.bars == Bars::Horizontal =>
            {
                let delta = match *delta {
                    mouse::ScrollDelta::Lines { x: 0.0, y } => {
                        mouse::ScrollDelta::Lines { x: y, y: 0.0 }
                    }
                    mouse::ScrollDelta::Pixels { x: 0.0, y } => {
                        mouse::ScrollDelta::Pixels { x: y, y: 0.0 }
                    }
                    delta => delta,
                };
                turned = Event::Mouse(mouse::Event::WheelScrolled { delta });
                &turned
            }
            event => event,
        };
        let inner_cursor = match event {
            Event::Mouse(mouse::Event::WheelScrolled { .. }) if on_a_bar => {
                mouse::Cursor::Available(inner.bounds().center())
            }
            _ if on_a_bar || dragging => mouse::Cursor::Unavailable,
            _ => cursor,
        };
        if !shell.is_event_captured() {
            Widget::<Message, Theme, Renderer>::update(
                self.scrollable_mut(),
                &mut tree.children[0],
                event,
                inner,
                inner_cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
        if to.is_some() {
            self.peek(&mut tree.children[0], inner, renderer, to);
            // The next frame tells the app (`on_scroll`).
            shell.request_redraw();
        }
        let seen = self.peek(&mut tree.children[0], inner, renderer, None);

        // Each part's states settled; a frame when the thumbs moved, a
        // layout when a part's sizes follow a state that changed.
        let parts = self.parts();
        let state = tree.state.downcast_mut::<State>();
        if seen.translation != state.seen.translation {
            shell.request_redraw();
        }
        state.seen = seen;
        let mut settle = |boxed: &Boxed<'_>, s: &mut BoxState, before: States, over: bool| {
            s.animated.update(event, shell);
            boxed.settle(s, before, boxed.tracked(States::NONE), over, shell);
        };
        settle(&self.boxed, &mut state.scroller, before[0], over);
        for (i, bar) in parts.bars.iter().enumerate() {
            let (Some(bar), Some(s)) = (bar, &mut state.bars[i]) else {
                continue;
            };
            let p = pointer[i].unwrap_or_default();
            settle(&bar.bar, &mut s.bar, before[1 + 2 * i], p.bar);
            settle(&bar.thumb, &mut s.thumb, before[2 + 2 * i], p.thumb);
        }
        let after = state.snapshot();
        if after != before {
            let (was, is) = (self.layout_styles(before), self.layout_styles(after));
            let same = was.iter().zip(&is).all(|pair| match pair {
                (Some(a), Some(b)) => a.same_layout(b),
                _ => true,
            });
            if !same {
                shell.invalidate_layout();
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        let cursor = state.scroller.local(cursor);
        let on_a_bar = self
            .shown(state, layout)
            .any(|(_, bar, s, at)| s.bar.local(cursor).is_over(bar.bar.border_box(at.bar)));
        let dragging = state.bars.iter().flatten().any(|s| s.grabbed.is_some());
        if on_a_bar || dragging {
            return mouse::Interaction::Idle;
        }
        Widget::<Message, Theme, Renderer>::mouse_interaction(
            &*self.scrollable(),
            &tree.children[0],
            layout.children().next().expect("a scrollable"),
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let bounds = self.boxed.border_box(layout.bounds());
        operation.custom(None, bounds, &mut Reported::new(self.boxed.node()));
        let inner = layout.children().next().expect("a scrollable");
        // The bars that show, and their thumbs: where they are.
        let bars: Vec<_> = self
            .shown(tree.state.downcast_ref::<State>(), layout)
            .map(|(_, bar, _, at)| {
                (
                    (bar.bar.node(), bar.bar.border_box(at.bar)),
                    (bar.thumb.node(), bar.thumb.border_box(at.thumb)),
                )
            })
            .collect();
        operation.traverse(&mut |operation| {
            Widget::<Message, Theme, Renderer>::operate(
                self.scrollable_mut(),
                &mut tree.children[0],
                inner,
                renderer,
                operation,
            );
            for &((bar, at), (thumb, thumb_at)) in &bars {
                operation.custom(None, at, &mut Reported::new(bar));
                operation.traverse(&mut |operation| {
                    operation.custom(None, thumb_at, &mut Reported::new(thumb));
                });
            }
        });
        // The operation may have scrolled it.
        let seen = self.peek(&mut tree.children[0], inner, renderer, None);
        tree.state.downcast_mut::<State>().seen = seen;
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        let inner = layout.children().next().expect("a scrollable");
        Widget::<Message, Theme, Renderer>::overlay(
            self.scrollable_mut(),
            &mut tree.children[0],
            inner,
            renderer,
            viewport,
            translation,
        )
    }
}

/// The content with the scroller's padding on the axes it scrolls, inside
/// the scrollable: as in CSS, that padding scrolls with it. Nothing of
/// its own besides: its tree is the content's.
struct Padded<'a, Message> {
    content: Element<'a, Message>,
    /// Top, right, bottom, left: the scroller's, set at its layout.
    padding: Rc<Cell<[f32; 4]>>,
}

impl<Message> Widget<Message, Theme, Renderer> for Padded<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let [top, right, bottom, left] = self.padding.get();
        let less = |size: Size| {
            Size::new(
                (size.width - left - right).max(0.0),
                (size.height - top - bottom).max(0.0),
            )
        };
        let limits =
            Limits::with_compression(less(limits.min()), less(limits.max()), limits.compression());
        let node = self
            .content
            .as_widget_mut()
            .layout(tree, renderer, &limits)
            .move_to(Point::new(left, top));
        let size = node.size();
        layout::Node::with_children(
            Size::new(size.width + left + right, size.height + top + bottom),
            vec![node],
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let layout = layout.children().next().expect("the content");
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let layout = layout.children().next().expect("the content");
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let layout = layout.children().next().expect("the content");
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let layout = layout.children().next().expect("the content");
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        let layout = layout.children().next().expect("the content");
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

/// Where the pointer is on a bar that shows.
#[derive(Debug, Clone, Copy, Default)]
struct Pointer {
    bar: bool,
    thumb: bool,
    /// How far along from the thumb's start (its border box's).
    from_thumb: Option<f32>,
    /// The thumb's length (its border box's).
    length: f32,
}

impl<'a, Message: 'a> From<Scroller<'a, Message>> for Element<'a, Message> {
    fn from(scroller: Scroller<'a, Message>) -> Self {
        Element::new(scroller)
    }
}
