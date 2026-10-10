//! A graph: series of values, oldest first, as lines with the area under
//! them; the newest at the right edge, the graph's capacity of values
//! across its content box, so a young series grows from the right; the
//! top the largest value unless the app gives one. Lines across at the
//! quarters, a grid; a text over it (the scale's top, a value), laid out
//! as in a row. Its paths are painted as SVG's: a series' `stroke` and
//! `stroke-width` its line, its `fill` the area under it, the grid's
//! `stroke` its lines. A sparkline is a graph too: one series, styled as
//! the app likes.
//!
//! The paths are kept in the widget tree (iced's geometry cache), made
//! again only when the values, the size or the paint change: a frame of
//! another widget's animation doesn't cost the graph's.

use std::borrow::Cow;
use std::cell::RefCell;

use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::layout::{self, Layout, Limits};
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell, mouse, overlay, renderer};
use iced::widget::canvas::{Cache, Fill, Frame, Path, Stroke};
use iced::{Color, Event, Length, Point, Rectangle, Size, Vector};

use super::boxed::{BoxState, Boxed};
use super::text::text;
use crate::layout::{content_box, flex, snap};
use crate::paint::fade;
use crate::query::Reported;
use crate::style::{Direction, Node, States};
use crate::{Element, Renderer, Theme};

pub struct Graph<'a, Message> {
    boxed: Boxed<'a>,
    capacity: usize,
    max: Option<f32>,
    /// Each series' class (the app's), its values.
    series: Vec<(Option<&'a str>, Vec<f32>)>,
    /// The paths' nodes, once the series are all there: made at layout,
    /// once a view.
    parts: Option<Parts<'a>>,
    /// The label, if any.
    children: Vec<Element<'a, Message>>,
}

/// A graph `capacity` values wide: `node`.
pub fn graph<'a, Message>(node: Node<'a>, capacity: usize) -> Graph<'a, Message> {
    Graph {
        boxed: Boxed::new(node),
        capacity: capacity.max(2),
        max: None,
        series: Vec::new(),
        parts: None,
        children: Vec::new(),
    }
}

impl<'a, Message> Graph<'a, Message> {
    /// A series, its values oldest first: a `series`.
    pub fn series(mut self, values: impl IntoIterator<Item = f32>) -> Self {
        self.series.push((None, values.into_iter().collect()));
        self
    }

    /// A series with a class of the app's: `series.<class>`.
    pub fn series_class(mut self, class: &'a str, values: impl IntoIterator<Item = f32>) -> Self {
        self.series
            .push((Some(class), values.into_iter().collect()));
        self
    }

    /// The value at its top: the largest of its series if not given.
    pub fn max(mut self, max: f32) -> Self {
        self.max = Some(max);
        self
    }

    /// A text over it, its `label`.
    pub fn label(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.children.clear();
        self.children
            .push(text(self.boxed.node.child("label"), label).into());
        self
    }
}

/// The paths' nodes, made with the view.
struct Parts<'a> {
    series: Vec<Node<'a>>,
    grid: Node<'a>,
}

impl<'a> Parts<'a> {
    fn new(node: Node<'a>, series: &[(Option<&'a str>, Vec<f32>)]) -> Self {
        let count = series.len();
        Self {
            series: series
                .iter()
                .enumerate()
                .map(|(i, (class, _))| {
                    let series = node.child_at("series", i, count);
                    match class {
                        Some(class) => series.class(class),
                        None => series,
                    }
                })
                .collect(),
            grid: node.child("grid"),
        }
    }
}

/// What the paths were last made of: made again when it changes.
#[derive(PartialEq)]
struct Made {
    size: Size,
    top: f32,
    /// Each series' values and paint; then the grid's paint.
    series: Vec<(Vec<f32>, Paint)>,
    grid: Paint,
}

#[derive(Clone, Copy, PartialEq)]
struct Paint {
    stroke: Color,
    /// In device pixels, whole.
    width: f32,
    fill: Color,
}

struct State {
    boxed: BoxState,
    cache: Cache<Renderer>,
    made: RefCell<Option<Made>>,
}

impl<Message> Graph<'_, Message> {
    /// The value at the top.
    fn top(&self) -> f32 {
        self.max
            .unwrap_or_else(|| {
                self.series
                    .iter()
                    .flat_map(|(_, values)| values.iter().copied())
                    .fold(0.0, f32::max)
            })
            .max(f32::MIN_POSITIVE)
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for Graph<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.boxed.size()
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            boxed: self.boxed.state(),
            cache: Cache::new(),
            made: RefCell::new(None),
        })
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.state
            .downcast_mut::<State>()
            .boxed
            .animated
            .diff(self.boxed.node);
        tree.diff_children(&self.children);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> layout::Node {
        let s = self
            .boxed
            .layout_style(tree.state.downcast_ref::<State>().boxed.states);
        if self.parts.is_none() {
            self.parts = Some(Parts::new(self.boxed.node, &self.series));
        }
        flex(
            &s,
            Direction::Row,
            self.boxed.node.scale(),
            &mut self.children,
            &mut tree.children,
            renderer,
            limits,
        )
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
        let Some(parts) = &self.parts else {
            return;
        };
        let scale = self.boxed.node.scale();
        let s = self.boxed.layout_style(state.boxed.states);
        let content = content_box(layout.bounds().size(), &s, scale);
        self.boxed.draw(
            &state.boxed,
            renderer,
            theme,
            layout.bounds(),
            cursor,
            |renderer, theme, style, cursor| {
                let paint = |node: Node<'_>| {
                    let s = node.resolve(States::NONE, &theme.ancestors);
                    let opacity = theme.opacity * s.opacity;
                    Paint {
                        stroke: fade(s.stroke, opacity),
                        width: (s.stroke_width * scale).round(),
                        fill: fade(s.fill, opacity),
                    }
                };
                let (size, top, grid) = (content.size(), self.top(), paint(parts.grid));
                let mut last = state.made.borrow_mut();
                let same = last.as_ref().is_some_and(|made| {
                    made.size == size
                        && made.top == top
                        && made.grid == grid
                        && made.series.len() == self.series.len()
                        && made.series.iter().zip(&self.series).zip(&parts.series).all(
                            |(((was, painted), (_, values)), &node)| {
                                was == values && *painted == paint(node)
                            },
                        )
                });
                if !same {
                    state.cache.clear();
                    *last = Some(Made {
                        size,
                        top,
                        series: self
                            .series
                            .iter()
                            .zip(&parts.series)
                            .map(|((_, values), &node)| (values.clone(), paint(node)))
                            .collect(),
                        grid,
                    });
                }
                let made = last.as_ref().expect("just set");
                let geometry = state.cache.draw(renderer, content.size(), |frame| {
                    paths(frame, made, self.capacity, scale);
                });
                let origin = layout.bounds().position() + Vector::new(content.x, content.y);
                renderer.with_translation(Vector::new(origin.x, origin.y), |renderer| {
                    renderer.draw_geometry(geometry);
                });
                for ((child, tree), layout) in self
                    .children
                    .iter()
                    .zip(&tree.children)
                    .zip(layout.children())
                {
                    child
                        .as_widget()
                        .draw(tree, renderer, theme, style, layout, cursor, viewport);
                }
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
        let cursor = tree.state.downcast_ref::<State>().boxed.local(cursor);
        for ((child, tree), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
        let state = &mut tree.state.downcast_mut::<State>().boxed;
        state.animated.update(event, shell);
        let before = state.states;
        let over = cursor.is_over(self.boxed.border_box(layout.bounds()));
        let tracked = self.boxed.tracked(States::NONE);
        self.boxed.settle(state, before, tracked, over, shell);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let bounds = self.boxed.border_box(layout.bounds());
        operation.container(None, bounds);
        operation.custom(None, bounds, &mut Reported::new(self.boxed.node));
        // Its paths, across its content box.
        if let Some(parts) = &self.parts {
            let s = self
                .boxed
                .layout_style(tree.state.downcast_ref::<State>().boxed.states);
            let content = content_box(layout.bounds().size(), &s, self.boxed.node.scale())
                + Vector::new(layout.bounds().x, layout.bounds().y);
            for &node in parts.series.iter().chain([&parts.grid]) {
                operation.custom(None, content, &mut Reported::new(node));
            }
        }
        operation.traverse(&mut |operation| {
            for ((child, tree), layout) in self
                .children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                child
                    .as_widget_mut()
                    .operate(tree, layout, renderer, operation);
            }
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

/// The grid, then each series: the area under it, its line.
fn paths(frame: &mut Frame<Renderer>, made: &Made, capacity: usize, scale: f32) {
    let Size { width, height } = made.size;
    // A line of an odd width in device pixels sits on a pixel's middle.
    let centred = |y: f32, width: f32| {
        let y = snap(y, scale);
        if width as u32 % 2 == 1 {
            y + 0.5 / scale
        } else {
            y
        }
    };
    let grid = made.grid;
    if grid.stroke.a > 0.0 && grid.width > 0.0 {
        let stroke = Stroke::default()
            .with_color(grid.stroke)
            .with_width(grid.width / scale);
        for quarter in 1..4 {
            let y = centred(height * quarter as f32 / 4.0, grid.width);
            frame.stroke(
                &Path::line(Point::new(0.0, y), Point::new(width, y)),
                stroke,
            );
        }
    }
    let step = width / (capacity - 1) as f32;
    for (values, paint) in &made.series {
        let n = values.len().min(capacity);
        if n < 2 {
            continue;
        }
        let points: Vec<Point> = values[values.len() - n..]
            .iter()
            .enumerate()
            .map(|(i, v)| {
                Point::new(
                    width - (n - 1 - i) as f32 * step,
                    height - (v / made.top).clamp(0.0, 1.0) * height,
                )
            })
            .collect();
        if paint.fill.a > 0.0 {
            let area = Path::new(|b| {
                b.move_to(Point::new(points[0].x, height));
                for &p in &points {
                    b.line_to(p);
                }
                b.line_to(Point::new(points[n - 1].x, height));
                b.close();
            });
            frame.fill(&area, Fill::from(paint.fill));
        }
        if paint.stroke.a > 0.0 && paint.width > 0.0 {
            let line = Path::new(|b| {
                b.move_to(points[0]);
                for &p in &points[1..] {
                    b.line_to(p);
                }
            });
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(paint.stroke)
                    .with_width(paint.width / scale),
            );
        }
    }
}

impl<'a, Message: Clone + 'a> From<Graph<'a, Message>> for Element<'a, Message> {
    fn from(graph: Graph<'a, Message>) -> Self {
        Element::new(graph)
    }
}
