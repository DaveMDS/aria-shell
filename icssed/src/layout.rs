//! The layout: a CSS flexbox subset on iced's `Limits` protocol, shared
//! by the boxes. See the crate's ARCHITECTURE.md, "Layout".
//!
//! A box's node is its margin box. On the main axis a child is `fill`
//! (CSS's `flex: 1 1 0`) or not (`flex: 0 1 auto`); a toolkit widget
//! offered less than its minimum comes back at its minimum, larger than
//! offered (CSS's automatic minimum), and what doesn't fit overflows.
//! Sizes and positions land on whole device pixels.

use iced::advanced::layout::{self, Limits};
use iced::advanced::widget::Tree;
use iced::{Length, Point, Rectangle, Size};

use crate::style::{Align, Computed, Direction, Justify, MinSize, Size as Len};
use crate::{Element, Renderer};

/// `v` on whole device pixels.
pub(crate) fn snap(v: f32, scale: f32) -> f32 {
    (v * scale).round() / scale
}

/// `v` up to whole device pixels.
pub(crate) fn snap_up(v: f32, scale: f32) -> f32 {
    ((v * scale) - 1e-3).ceil().max(0.0) / scale
}

const EPSILON: f32 = 0.01;

/// One axis of a box: what its style and its parent's limits make of
/// its border box.
struct Axis {
    /// The size it asks for, when not its content's.
    target: Option<f32>,
    /// What the parent insists on, and its own `min-*`.
    lo: f32,
    /// The most it may take: the parent's offer within its `max-*`.
    hi: f32,
    explicit_min: Option<f32>,
    own_max: f32,
    /// Border and padding, both sides.
    edges: f32,
}

impl Axis {
    #[allow(clippy::too_many_arguments)]
    fn new(
        len: Len,
        min: MinSize,
        max: f32,
        limit_min: f32,
        limit_max: f32,
        compressed: bool,
        edges: f32,
        scale: f32,
    ) -> Self {
        let explicit_min = match min {
            MinSize::Px(v) => Some(snap(v, scale)),
            MinSize::Auto => None,
        };
        let floor = explicit_min.unwrap_or(0.0).max(edges);
        let own_max = if max.is_finite() {
            snap(max, scale)
        } else {
            max
        }
        .max(floor);
        let hi = limit_max.min(own_max).max(floor);
        let lo = limit_min.min(own_max).max(floor);
        let target = match len {
            Len::Px(v) => Some(snap(v, scale).min(own_max).max(floor)),
            Len::Fill if !compressed => Some(hi),
            _ => None,
        };
        Self {
            target,
            lo,
            hi,
            explicit_min,
            own_max,
            edges,
        }
    }

    /// The most room its content gets, as a border box.
    fn room(&self) -> f32 {
        self.target.unwrap_or(self.hi).min(self.hi).max(self.lo)
    }

    /// Its border box, `content` laid out (the children's extent, plus
    /// the edges).
    fn size(&self, content: f32) -> f32 {
        match self.target {
            Some(t) if t <= self.hi + EPSILON => t.max(self.lo),
            // Offered less: down to the offer, not below its minimum.
            Some(t) => {
                let floor = self.explicit_min.unwrap_or_else(|| content.min(t));
                self.hi.max(floor.min(t))
            }
            None => {
                let size = content.min(self.own_max).max(self.lo);
                match self.explicit_min {
                    // An explicit minimum replaces the automatic one: the
                    // content overflows the box, not the box its parent.
                    Some(min) => size.min(self.hi.max(min)),
                    None => size,
                }
            }
        }
    }
}

/// A box's model on its parent's limits: its two axes, its margins,
/// where its content box starts. What `flex` lays children out in, and
/// what a widget placing its children its own way (a meter's fill, a
/// slider's knob) sizes itself with.
pub(crate) struct Sizing {
    width: Axis,
    height: Axis,
    /// Both sides of each axis.
    margins: Size,
    /// The content box's corner in the margin box.
    origin: Point,
}

impl Sizing {
    pub fn new(s: &Computed, scale: f32, limits: &Limits) -> Self {
        let sn = |v: f32| snap(v, scale);
        let (m, b, p) = (s.margin, s.border_width, s.padding);
        let (margin_left, margin_top) = (sn(m.left), sn(m.top));
        let margins = Size::new(margin_left + sn(m.right), margin_top + sn(m.bottom));
        let (start_x, start_y) = (b.left + sn(p.left), b.top + sn(p.top));
        let edges = Size::new(
            start_x + b.right + sn(p.right),
            start_y + b.bottom + sn(p.bottom),
        );
        let (lmin, lmax, compressed) = (limits.min(), limits.max(), limits.compression());
        let width = Axis::new(
            s.width,
            s.min_width,
            s.max_width,
            (lmin.width - margins.width).max(0.0),
            (lmax.width - margins.width).max(0.0),
            compressed.width,
            edges.width,
            scale,
        );
        let height = Axis::new(
            s.height,
            s.min_height,
            s.max_height,
            (lmin.height - margins.height).max(0.0),
            (lmax.height - margins.height).max(0.0),
            compressed.height,
            edges.height,
            scale,
        );
        Self {
            width,
            height,
            margins,
            origin: Point::new(margin_left + start_x, margin_top + start_y),
        }
    }

    /// The most room its content gets: its own size when it has one,
    /// else what its parent offers, less its border and padding.
    pub fn room(&self) -> Size {
        Size::new(
            (self.width.room() - self.width.edges).max(0.0),
            (self.height.room() - self.height.edges).max(0.0),
        )
    }

    /// Whether its width and height are its own (a length, `fill`), not
    /// its content's.
    pub fn definite(&self) -> (bool, bool) {
        (self.width.target.is_some(), self.height.target.is_some())
    }

    /// Its margin box around content `content` big, and its content
    /// box in it.
    pub fn around(&self, content: Size) -> (Size, Rectangle) {
        let width = self.width.size(content.width + self.width.edges);
        let height = self.height.size(content.height + self.height.edges);
        let inner = Size::new(
            (width - self.width.edges).max(0.0),
            (height - self.height.edges).max(0.0),
        );
        (
            Size::new(width + self.margins.width, height + self.margins.height),
            Rectangle::new(self.origin, inner),
        )
    }
}

/// The content box of a box styled `s` whose margin box is `size` big.
pub(crate) fn content_box(size: Size, s: &Computed, scale: f32) -> Rectangle {
    let sn = |v: f32| snap(v, scale);
    let (m, b, p) = (s.margin, s.border_width, s.padding);
    let (x, y) = (
        sn(m.left) + b.left + sn(p.left),
        sn(m.top) + b.top + sn(p.top),
    );
    let (right, bottom) = (
        sn(m.right) + b.right + sn(p.right),
        sn(m.bottom) + b.bottom + sn(p.bottom),
    );
    Rectangle {
        x,
        y,
        width: (size.width - x - right).max(0.0),
        height: (size.height - y - bottom).max(0.0),
    }
}

/// A box's children laid out on its main axis, the box sized around
/// them: its margin box, the children placed in it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn flex<M>(
    s: &Computed,
    direction: Direction,
    scale: f32,
    children: &mut [Element<'_, M>],
    trees: &mut [Tree],
    renderer: &Renderer,
    limits: &Limits,
) -> layout::Node {
    let row = direction == Direction::Row;
    let main_of = |size: Size| if row { size.width } else { size.height };
    let cross_of = |size: Size| if row { size.height } else { size.width };
    let pack = |main: f32, cross: f32| {
        if row {
            Size::new(main, cross)
        } else {
            Size::new(cross, main)
        }
    };
    let sn = |v: f32| snap(v, scale);

    let sizing = Sizing::new(s, scale, limits);
    let (main, cross) = if row {
        (&sizing.width, &sizing.height)
    } else {
        (&sizing.height, &sizing.width)
    };

    let n = children.len();
    let gap = sn(s.gap(direction));
    let gaps = gap * n.saturating_sub(1) as f32;
    let main_room = (main.room() - main.edges).max(0.0);
    let cross_room = (cross.room() - cross.edges).max(0.0);

    // Which children are `fill` on each axis, and which stretch across.
    let kinds: Vec<(bool, bool)> = children
        .iter()
        .map(|c| {
            let size = c.as_widget().size();
            let (on_main, across) = if row {
                (size.width, size.height)
            } else {
                (size.height, size.width)
            };
            let stretch = across.is_fill()
                || (s.align_items == Align::Stretch && matches!(across, Length::Shrink));
            (on_main.is_fill(), stretch)
        })
        .collect();
    let fills = kinds.iter().filter(|(fill, _)| *fill).count();

    // 1. The children not `fill` at their content size: their bases. A
    //    box sized by its content measures its `fill` ones too: each
    //    gets at least its content, as CSS's flex fraction has it.
    let main_auto = main.target.is_none();
    let mut nodes = vec![layout::Node::default(); n];
    let mut bases = vec![0.0; n];
    let mut widest_fill: f32 = 0.0;
    for (i, (child, tree)) in children.iter_mut().zip(trees.iter_mut()).enumerate() {
        let fill = kinds[i].0;
        if fill && !main_auto {
            continue;
        }
        let content = Limits::with_compression(
            Size::ZERO,
            pack(main_room, cross_room),
            Size::new(true, true),
        );
        let node = child.as_widget_mut().layout(tree, renderer, &content);
        if fill {
            widest_fill = widest_fill.max(main_of(node.size()));
        } else {
            bases[i] = main_of(node.size());
        }
        nodes[i] = node;
    }
    let total_base: f32 = bases.iter().sum();

    // 2. The room the children share.
    let inner_main = match main.target {
        Some(_) => main_room,
        None => {
            let content = total_base + widest_fill * fills as f32 + gaps + main.edges;
            content.clamp(main.lo, main.hi.max(main.lo)) - main.edges
        }
    }
    .max(0.0);

    // 3. The free room: shared by the `fill` children when there's
    //    some, given up by the others in proportion to their bases when
    //    it's negative. A child coming back at another size (its minimum,
    //    its `max-*`) keeps it, and the rest is shared again.
    let grow = inner_main - gaps - total_base >= 0.0;
    let mut sizes: Vec<f32> = (0..n)
        .map(|i| if kinds[i].0 { 0.0 } else { bases[i] })
        .collect();
    let mut frozen: Vec<bool> = (0..n).map(|i| grow && !kinds[i].0).collect();
    loop {
        let open: Vec<usize> = (0..n).filter(|&i| !frozen[i]).collect();
        if open.is_empty() {
            break;
        }
        let fixed: f32 = (0..n).filter(|&i| frozen[i]).map(|i| sizes[i]).sum();
        let left = inner_main - gaps - fixed;
        let exact: Vec<f32> = if grow {
            let share = left.max(0.0) / open.len() as f32;
            vec![share; open.len()]
        } else {
            let open_base: f32 = open.iter().map(|&i| sizes_base(&kinds, &bases, i)).sum();
            let deficit = (open_base - left).max(0.0);
            open.iter()
                .map(|&i| {
                    let base = sizes_base(&kinds, &bases, i);
                    if open_base > 0.0 {
                        (base - deficit * base / open_base).max(0.0)
                    } else {
                        0.0
                    }
                })
                .collect()
        };
        // On whole device pixels, the rounding spread over the shares.
        let mut acc = 0.0;
        for (&i, share) in open.iter().zip(exact) {
            sizes[i] = sn(acc + share) - sn(acc);
            acc += share;
        }
        let mut settled = true;
        for &i in &open {
            // Across, its content: a child that stretches is laid out
            // again at the box's cross size once it's known (4.), not at
            // the room the box was offered.
            let compress = if row {
                Size::new(false, true)
            } else {
                Size::new(true, false)
            };
            let at =
                Limits::with_compression(pack(sizes[i], 0.0), pack(sizes[i], cross_room), compress);
            let node = children[i]
                .as_widget_mut()
                .layout(&mut trees[i], renderer, &at);
            let got = main_of(node.size());
            nodes[i] = node;
            if (got - sizes[i]).abs() > EPSILON {
                sizes[i] = got;
                frozen[i] = true;
                settled = false;
            }
        }
        if settled {
            break;
        }
    }

    // 4. Across: the box's cross size, the children that stretch laid
    //    out again at it.
    let content_cross = nodes.iter().map(|n| cross_of(n.size())).fold(0.0, f32::max);
    let cross_size = cross.size(content_cross + cross.edges);
    let inner_cross = (cross_size - cross.edges).max(0.0);
    for (i, (child, tree)) in children.iter_mut().zip(trees.iter_mut()).enumerate() {
        if kinds[i].1 && (cross_of(nodes[i].size()) - inner_cross).abs() > EPSILON {
            let main_size = main_of(nodes[i].size());
            let at = Limits::new(pack(main_size, inner_cross), pack(main_size, inner_cross));
            nodes[i] = child.as_widget_mut().layout(tree, renderer, &at);
        }
    }

    // 5. The box's main size, the children placed in it.
    let used = nodes.iter().map(|n| main_of(n.size())).sum::<f32>() + gaps;
    let main_size = main.size(used + main.edges);
    let free = (main_size - main.edges - used).max(0.0);
    let mut at = match (fills, s.justify_content) {
        (0, Justify::Center) => sn(free / 2.0),
        (0, Justify::End) => free,
        _ => 0.0,
    };
    let (origin_main, origin_cross) = if row {
        (sizing.origin.x, sizing.origin.y)
    } else {
        (sizing.origin.y, sizing.origin.x)
    };
    for node in &mut nodes {
        let size = cross_of(node.size());
        let offset = match s.align_items {
            Align::Stretch | Align::Start => 0.0,
            Align::Center => sn((inner_cross - size) / 2.0),
            Align::End => inner_cross - size,
        };
        let position = pack(origin_main + at, origin_cross + offset);
        node.move_to_mut(Point::new(position.width, position.height));
        at += main_of(node.size()) + gap;
    }
    let size = pack(main_size, cross_size);
    layout::Node::with_children(
        Size::new(
            size.width + sizing.margins.width,
            size.height + sizing.margins.height,
        ),
        nodes,
    )
}

/// What a child gives up from when shrinking: its base, nothing for a
/// `fill` one (its basis is 0).
fn sizes_base(kinds: &[(bool, bool)], bases: &[f32], i: usize) -> f32 {
    if kinds[i].0 { 0.0 } else { bases[i] }
}

/// A box's sizes as iced sees them, margins included.
pub(crate) fn length(len: Len, margins: f32) -> Length {
    match len {
        Len::Auto => Length::Shrink,
        Len::Px(v) => Length::Fixed(v + margins),
        Len::Fill => Length::Fill,
    }
}

#[cfg(test)]
mod tests {
    use iced::advanced::layout::{self, Limits};
    use iced::advanced::widget::Tree;
    use iced::{Rectangle, Size};

    use crate::measure::headless;
    use crate::{Element, Scheme, Sheet, Source};

    fn sheet(css: &str) -> Sheet {
        let (sheet, warnings) = Sheet::load(
            &[Source {
                name: "t.css",
                text: css,
                dir: None,
            }],
            Scheme::Light,
        )
        .expect("loads");
        assert!(warnings.is_empty(), "{warnings:?}");
        sheet
    }

    fn lay(element: Element<'_, ()>, max: Size) -> layout::Node {
        let mut element = element;
        let mut tree = Tree::new(element.as_widget());
        element
            .as_widget_mut()
            .layout(&mut tree, &headless(), &Limits::new(Size::ZERO, max))
    }

    /// A box with nothing in it: its size is its style's.
    fn cell(class: &str) -> Element<'_, ()> {
        crate::row([]).kind("cell").class(class).into()
    }

    /// A box with one `fixed` child.
    fn holding<'a>(class: &'a str, fixed: &'a str) -> Element<'a, ()> {
        crate::row([cell(fixed)]).kind("cell").class(class).into()
    }

    fn rects(node: &layout::Node) -> Vec<Rectangle> {
        node.children().iter().map(|c| c.bounds()).collect()
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle {
            x,
            y,
            width,
            height,
        }
    }

    const MAX: Size = Size::new(1000.0, 1000.0);

    #[test]
    fn fill_children_share_the_free_room() {
        let sheet = sheet(
            ".row { width: 300px; gap: 10px; } cell { height: 20px; } \
             cell.fill { width: fill; } cell.w80 { width: 80px; }",
        );
        let row = sheet.root("row", 1.0);
        let node = lay(
            crate::row([cell("fill"), cell("w80"), cell("fill")])
                .at(row)
                .into(),
            MAX,
        );
        assert_eq!(node.size(), Size::new(300.0, 20.0));
        assert_eq!(
            rects(&node),
            [
                rect(0.0, 0.0, 100.0, 20.0),
                rect(110.0, 0.0, 80.0, 20.0),
                rect(200.0, 0.0, 100.0, 20.0)
            ]
        );
    }

    #[test]
    fn auto_width_is_the_content_s_whatever_the_theme_says() {
        // The app's length (a surface as long as its content): the
        // theme's `fill` gives way, on the box and inside its parent.
        let sheet = sheet(
            ".row { width: fill; gap: 10px; } cell { height: 20px; } \
             cell.w80 { width: 80px; } cell.fill { width: fill; }",
        );
        let row = sheet.root("row", 1.0);
        let node = lay(
            crate::row([cell("w80"), cell("w80")])
                .at(row)
                .auto_width_if(true)
                .into(),
            MAX,
        );
        assert_eq!(node.size(), Size::new(170.0, 20.0));
        let node = lay(
            crate::row([
                crate::row([cell("w80")])
                    .kind("cell")
                    .class("fill")
                    .auto_width_if(true)
                    .into(),
                cell("fill"),
            ])
            .at(row)
            .into(),
            MAX,
        );
        assert_eq!(
            rects(&node),
            [rect(0.0, 0.0, 80.0, 20.0), rect(90.0, 0.0, 910.0, 20.0)]
        );
    }

    #[test]
    fn a_child_filling_across_takes_the_box_s_height_not_the_room() {
        // CSS's stretch: an auto-sized row is as tall as its tallest
        // child's content; a child stretched across takes that height,
        // whatever room the row was offered.
        let sheet = sheet(
            ".row { width: 300px; } cell.h20 { width: 50px; height: 20px; } \
             cell.fill { width: 50px; height: fill; }",
        );
        let row = sheet.root("row", 1.0);
        let node = lay(crate::row([cell("h20"), cell("fill")]).at(row).into(), MAX);
        assert_eq!(node.size(), Size::new(300.0, 20.0));
        assert_eq!(
            rects(&node),
            [rect(0.0, 0.0, 50.0, 20.0), rect(50.0, 0.0, 50.0, 20.0)]
        );
    }

    #[test]
    fn children_filling_across_keep_the_content_s_height_when_shrunk() {
        // Too wide for the row: the children shrink (CSS's flex-shrink),
        // laid out again at their shares; across, still their content.
        let sheet = sheet(
            ".row { width: 50px; } cell.fill { height: fill; } \
             cell.w40 { width: 40px; height: 12px; }",
        );
        let row = sheet.root("row", 1.0);
        let node = lay(
            crate::row([holding("fill", "w40"), holding("fill", "w40")])
                .at(row)
                .into(),
            MAX,
        );
        assert_eq!(node.size(), Size::new(50.0, 12.0));
    }

    #[test]
    fn shares_are_rounded_to_device_pixels_and_add_up() {
        let sheet = sheet(".row { width: 100px; } cell { width: fill; height: 10px; }");
        let row = sheet.root("row", 1.0);
        let node = lay(
            crate::row([cell("a"), cell("b"), cell("c")]).at(row).into(),
            MAX,
        );
        let widths: Vec<f32> = rects(&node).iter().map(|r| r.width).collect();
        assert_eq!(widths, [33.0, 34.0, 33.0]);
    }

    #[test]
    fn a_box_sized_by_its_content_gives_each_fill_child_its_content() {
        let sheet = sheet(
            "cell.fill { width: fill; } cell.w50 { width: 50px; height: 10px; } \
             cell.w150 { width: 150px; height: 10px; }",
        );
        let row = sheet.root("row", 1.0);
        let node = lay(
            crate::row([holding("fill", "w50"), holding("fill", "w150")])
                .at(row)
                .into(),
            MAX,
        );
        assert_eq!(node.size().width, 300.0);
        assert_eq!(
            rects(&node),
            [rect(0.0, 0.0, 150.0, 10.0), rect(150.0, 0.0, 150.0, 10.0)]
        );
    }

    #[test]
    fn children_shrink_down_to_their_automatic_minimum() {
        let sheet = sheet(
            ".row { width: 150px; } cell { height: 10px; } cell.w100 { width: 100px; } \
             cell.w90 { width: 90px; min-width: 90px; }",
        );
        let row = sheet.root("row", 1.0);
        // Empty, they can go down to nothing: each gives up the same.
        let node = lay(
            crate::row([cell("w100"), cell("w100"), cell("w100")])
                .at(row)
                .into(),
            MAX,
        );
        let widths: Vec<f32> = rects(&node).iter().map(|r| r.width).collect();
        assert_eq!(widths, [50.0, 50.0, 50.0]);
        // One holds 90px it can't give up: the others share the rest.
        let node = lay(
            crate::row([holding("w100", "w90"), cell("w100"), cell("w100")])
                .at(row)
                .into(),
            MAX,
        );
        let widths: Vec<f32> = rects(&node).iter().map(|r| r.width).collect();
        assert_eq!(widths, [90.0, 30.0, 30.0]);
    }

    #[test]
    fn a_text_shrinks_no_lower_than_its_content_unless_it_cuts() {
        // CSS's automatic minimum of a text: the whole line when it
        // doesn't wrap, so a button's label stays whole; nothing with an
        // ellipsis, which cuts it to fit.
        let sheet = sheet(
            ".row { width: 40px; } label { white-space: nowrap; } \
             label.cut { text-overflow: ellipsis; }",
        );
        let row = sheet.root("row", 1.0);
        const WIDE: &str = "A label far wider than its row";
        let text = |class: &'static str| -> Element<'_, ()> {
            crate::row([crate::text(WIDE).class(class).into()])
                .at(row)
                .into()
        };
        assert!(rects(&lay(text("whole"), MAX))[0].width > 40.0);
        assert_eq!(rects(&lay(text("cut"), MAX))[0].width, 40.0);
        let node = lay(
            crate::row![crate::button![crate::text(WIDE)]]
                .at(row)
                .into(),
            MAX,
        );
        assert!(rects(&node)[0].width > 40.0);
    }

    #[test]
    fn what_cannot_shrink_overflows() {
        let sheet = sheet(
            ".row { width: 100px; gap: 4px; } \
             cell.w80 { width: 80px; min-width: 80px; height: 10px; }",
        );
        let row = sheet.root("row", 1.0);
        let node = lay(
            crate::row([holding("a", "w80"), holding("b", "w80")])
                .at(row)
                .into(),
            MAX,
        );
        assert_eq!(node.size().width, 100.0);
        assert_eq!(
            rects(&node),
            [rect(0.0, 0.0, 80.0, 10.0), rect(84.0, 0.0, 80.0, 10.0)]
        );
    }

    #[test]
    fn an_explicit_min_width_lets_the_content_overflow_instead() {
        let sheet = sheet(
            ".row { width: 50px; } cell.a { min-width: 0; } \
             cell.w90 { width: 90px; min-width: 90px; height: 10px; }",
        );
        let row = sheet.root("row", 1.0);
        let node = lay(crate::row([holding("a", "w90")]).at(row).into(), MAX);
        assert_eq!(rects(&node)[0].width, 50.0);
        assert_eq!(rects(&node.children()[0])[0].width, 90.0);
    }

    #[test]
    fn a_column_of_stretched_items_is_as_wide_as_the_widest() {
        let sheet = sheet(
            ".column { padding: 2px; } cell.w50 { width: 50px; height: 10px; } \
             cell.w90 { width: 90px; height: 10px; }",
        );
        let column = sheet.root("column", 1.0);
        let node = lay(
            crate::column([holding("a", "w50"), holding("b", "w90")])
                .at(column)
                .into(),
            MAX,
        );
        assert_eq!(node.size(), Size::new(94.0, 24.0));
        assert_eq!(
            rects(&node),
            [rect(2.0, 2.0, 90.0, 10.0), rect(2.0, 12.0, 90.0, 10.0)]
        );
    }

    #[test]
    fn max_width_caps_a_box_and_its_stretched_items() {
        let sheet = sheet(
            ".column { max-width: 60px; } cell.w50 { width: 50px; height: 10px; } \
             cell.w90 { width: 90px; min-width: 90px; height: 10px; }",
        );
        let column = sheet.root("column", 1.0);
        let node = lay(
            crate::column([holding("a", "w50"), holding("b", "w90")])
                .at(column)
                .into(),
            MAX,
        );
        // The second can't go under its 90px: it overflows the column.
        assert_eq!(node.size().width, 60.0);
        assert_eq!(rects(&node)[0].width, 60.0);
        assert_eq!(rects(&node)[1].width, 90.0);
    }

    #[test]
    fn align_items_and_justify_content() {
        let sheet = sheet(
            ".row { width: 200px; height: 60px; } .row.center { align-items: center; \
             justify-content: center; } .row.end { align-items: end; justify-content: end; } \
             cell { width: 40px; height: 20px; }",
        );
        let lay_row = |class: &str| {
            let row = sheet.root("row", 1.0).class(class);
            lay(crate::row([cell("a"), cell("b")]).at(row).into(), MAX)
        };
        assert_eq!(
            rects(&lay_row("center")),
            [rect(60.0, 20.0, 40.0, 20.0), rect(100.0, 20.0, 40.0, 20.0)]
        );
        assert_eq!(
            rects(&lay_row("end")),
            [rect(120.0, 40.0, 40.0, 20.0), rect(160.0, 40.0, 40.0, 20.0)]
        );
        // Stretched across by default: an auto height takes the row's.
        let sheet = super::tests::sheet(".row { height: 60px; } cell { width: 40px; }");
        let row = sheet.root("row", 1.0);
        let node = lay(crate::row([cell("a")]).at(row).into(), MAX);
        assert_eq!(rects(&node), [rect(0.0, 0.0, 40.0, 60.0)]);
    }

    #[test]
    fn margins_border_and_padding() {
        let sheet = sheet(
            ".row { margin: 5px; border: 2px solid; padding: 10px; } \
             cell { width: 20px; height: 20px; margin: 3px; }",
        );
        let row = sheet.root("row", 1.0);
        let node = lay(crate::row([cell("a")]).at(row).into(), MAX);
        // 5 + 2 + 10, then the cell's own margin box: 26 wide.
        assert_eq!(
            node.size(),
            Size::new(5.0 + 2.0 + 10.0 + 26.0 + 10.0 + 2.0 + 5.0, 60.0)
        );
        assert_eq!(rects(&node), [rect(17.0, 17.0, 26.0, 26.0)]);
    }

    #[test]
    fn positions_land_on_device_pixels() {
        let sheet = sheet(
            ".row { padding: 3px; gap: 1px; align-items: center; height: 31px; } \
             cell { width: 7px; height: 9px; }",
        );
        let scale = 1.5;
        let row = sheet.root("row", scale);
        let node = lay(crate::row([cell("a"), cell("b")]).at(row).into(), MAX);
        for r in rects(&node).iter().chain([node.bounds()].iter()) {
            for v in [r.x, r.y, r.width, r.height] {
                let device = v * scale;
                assert!((device - device.round()).abs() < 1e-3, "{r:?} at {scale}");
            }
        }
    }

    #[test]
    fn a_surface_leaves_room_for_its_shadow() {
        let sheet =
            sheet(".popup { box-shadow: 0 3px 8px black; } cell { width: 100px; height: 40px; }");
        let popup = sheet.root("popup", 1.0);
        let node = lay(
            crate::surface(crate::row([cell("a")])).at(popup).into(),
            MAX,
        );
        // Blur 8 on every side, the offset moving 3 of it down.
        assert_eq!(node.size(), Size::new(116.0, 56.0));
        assert_eq!(rects(&node), [rect(8.0, 5.0, 100.0, 40.0)]);
        // What an app placing the surface takes off.
        assert_eq!(
            crate::Surface::<()>::room(popup),
            crate::style::Sides {
                top: 5.0,
                right: 8.0,
                bottom: 11.0,
                left: 8.0
            }
        );
    }
}
