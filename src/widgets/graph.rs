//! Graphs for the system monitor, drawn on a `canvas`: a sparkline
//! (one filled series) and a gauge (a bar filled to a fraction), both
//! with a text over them, for the bar; a graph with up to two series,
//! grid lines and its scale for the popup; and a meter (a bar filled
//! to a fraction) made of containers, so the theme styles it like
//! anything else. The canvases sit in a container of their theme node
//! too, for its background and border.

use std::collections::VecDeque;

use iced::alignment::Vertical;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::widget::{Space, canvas as canvas_widget, container, row};
use iced::{Color, Element, Font, Length, Point, Rectangle, Renderer, Size, mouse};

use crate::theme::{self, Node, Theme};

/// One series: its values (oldest first) and its colour.
#[derive(Clone)]
pub struct Series<'a> {
    pub values: &'a VecDeque<f32>,
    pub color: Color,
}

/// A filled polyline of the history over a transparent background,
/// scaled to `max` (or the largest value when `None`): the bar's
/// sparkline. `capacity` is how many values fill the width: the newest
/// sits at the right edge, a young history grows leftwards.
pub struct Sparkline<'a> {
    pub series: Series<'a>,
    pub max: Option<f32>,
    pub capacity: usize,
    /// Text drawn over the middle of it (the instance's `format`).
    pub label: Option<Label>,
}

/// Text drawn over a graph.
#[derive(Debug, Clone)]
pub struct Label {
    pub text: String,
    pub color: Color,
    pub size: f32,
    pub font: Font,
}

impl Label {
    /// The label of `node > label` (its `color`, `font-size`,
    /// `font-family`; the colour falls back to `fallback`).
    fn from_node(theme: &Theme, node: &Node, text: &str, fallback: Option<Color>) -> Self {
        let l = theme.resolve(&node.child("label"));
        Self {
            text: text.to_owned(),
            color: l.color.or(fallback).unwrap_or(Color::WHITE),
            size: l.font_size.unwrap_or(9.0),
            font: l.font().unwrap_or_default(),
        }
    }

    fn draw(&self, frame: &mut Frame) {
        frame.fill_text(Text {
            content: self.text.clone(),
            position: frame.center(),
            color: self.color,
            size: self.size.into(),
            font: self.font,
            align_x: iced::advanced::text::Alignment::Center,
            align_y: Vertical::Center,
            ..Text::default()
        });
    }
}

/// The width a `node`-styled graph takes: the theme's `width`
/// (`default` without one), never less than the label's text with
/// the label node's horizontal padding (4px without one).
fn labelled_width(theme: &Theme, node: &Node, label: Option<&str>, default: f32) -> Option<f32> {
    let s = theme.resolve(node);
    let width = px(s.width).or((default > 0.0).then_some(default))?;
    let Some(text) = label.filter(|t| !t.is_empty()) else {
        return Some(width);
    };
    let l = node.child("label");
    let pad = theme.resolve(&l).padding;
    let pad = if pad.left + pad.right > 0.0 {
        pad.left + pad.right
    } else {
        8.0
    };
    Some(width.max((theme.measure(&l, text).width + pad).ceil()))
}

/// A horizontal bar filled to `fraction`, the label over it: the
/// bar's `mode = gauge`. The chrome (background, border) is the
/// container's, as for the sparkline; only the filled part (drawn as
/// the sparkline's area: dimmed, a full line at its edge) and the
/// label are drawn here.
pub struct Gauge {
    pub fraction: f32,
    pub radius: f32,
    pub fill: Color,
    pub label: Option<Label>,
}

impl<Message> canvas::Program<Message> for Gauge {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        let filled = (self.fraction.clamp(0.0, 1.0) * size.width).round();
        if filled > 0.0 {
            let bar = Path::rounded_rectangle(
                Point::ORIGIN,
                Size::new(filled, size.height),
                self.radius.min(filled / 2.0).into(),
            );
            frame.fill(&bar, dimmed(self.fill));
            let edge = Path::line(
                Point::new(filled - 0.75, 0.0),
                Point::new(filled - 0.75, size.height),
            );
            frame.stroke(
                &edge,
                Stroke::default().with_color(self.fill).with_width(1.5),
            );
        }
        if let Some(label) = &self.label {
            label.draw(&mut frame);
        }
        vec![frame.into_geometry()]
    }
}

/// The popup's graph: up to two series as filled areas, a few grid
/// lines, scaled together.
pub struct Graph<'a> {
    pub series: Vec<Series<'a>>,
    pub max: Option<f32>,
    pub capacity: usize,
    pub grid: Option<Color>,
    /// How the scale's top is written, and the label to write it
    /// with (top left, inside the graph).
    pub unit: Unit,
    pub scale_label: Option<Label>,
}

/// What a graph's values are, for its scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Percent,
    /// Bytes per second.
    Rate,
}

impl Unit {
    fn format(self, v: f32) -> String {
        match self {
            Self::Percent => format!("{v:.0}%"),
            Self::Rate => crate::sysmon::format::rate(v),
        }
    }
}

/// The area colour of a series: its colour, mostly transparent (the
/// line on top is the full one).
fn dimmed(color: Color) -> Color {
    Color {
        a: color.a * 0.35,
        ..color
    }
}

/// The scale of a graph: the given maximum, else the largest value of
/// every series (at least 1, so a flat zero line stays at the bottom).
fn scale(series: &[Series<'_>], max: Option<f32>) -> f32 {
    max.unwrap_or_else(|| {
        series
            .iter()
            .flat_map(|s| s.values.iter().copied())
            .fold(1.0_f32, f32::max)
    })
}

/// The points of one series in `size`: `capacity` values span the
/// width, the newest at the right edge.
fn points(values: &VecDeque<f32>, capacity: usize, max: f32, size: Size) -> Vec<Point> {
    let n = values.len();
    let step = size.width / (capacity.max(2) - 1) as f32;
    values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            Point::new(
                size.width - (n - 1 - i) as f32 * step,
                size.height - (v / max).clamp(0.0, 1.0) * size.height,
            )
        })
        .collect()
}

/// The area under one series, as a closed path; `None` with fewer
/// than two values.
fn area(values: &VecDeque<f32>, capacity: usize, max: f32, size: Size) -> Option<Path> {
    let pts = points(values, capacity, max, size);
    let (first, last) = (pts.first()?, pts.last()?);
    if pts.len() < 2 {
        return None;
    }
    Some(Path::new(|b| {
        b.move_to(Point::new(first.x, size.height));
        for p in &pts {
            b.line_to(*p);
        }
        b.line_to(Point::new(last.x, size.height));
        b.close();
    }))
}

fn draw_series(frame: &mut Frame, series: &Series<'_>, capacity: usize, max: f32, size: Size) {
    let Some(path) = area(series.values, capacity, max, size) else {
        return;
    };
    frame.fill(&path, dimmed(series.color));
    // The top edge, as a line.
    let pts = points(series.values, capacity, max, size);
    let line = Path::new(|b| {
        for (i, p) in pts.iter().enumerate() {
            if i == 0 {
                b.move_to(*p);
            } else {
                b.line_to(*p);
            }
        }
    });
    frame.stroke(
        &line,
        Stroke::default().with_color(series.color).with_width(1.5),
    );
}

impl<Message> canvas::Program<Message> for Sparkline<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let max = scale(std::slice::from_ref(&self.series), self.max);
        draw_series(&mut frame, &self.series, self.capacity, max, bounds.size());
        if let Some(label) = &self.label {
            label.draw(&mut frame);
        }
        vec![frame.into_geometry()]
    }
}

impl<Message> canvas::Program<Message> for Graph<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        if let Some(grid) = self.grid {
            let stroke = Stroke::default().with_color(grid).with_width(1.0);
            for i in 1..4 {
                let y = size.height * i as f32 / 4.0;
                frame.stroke(
                    &Path::line(Point::new(0.0, y), Point::new(size.width, y)),
                    stroke,
                );
            }
        }
        let max = scale(&self.series, self.max);
        for s in &self.series {
            draw_series(&mut frame, s, self.capacity, max, size);
        }
        if let Some(label) = &self.scale_label {
            frame.fill_text(Text {
                content: self.unit.format(max),
                position: Point::new(4.0, 2.0),
                color: label.color,
                size: label.size.into(),
                font: label.font,
                align_x: iced::advanced::text::Alignment::Left,
                align_y: Vertical::Top,
                ..Text::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

/// A sparkline styled as `node`: the theme's `width` (40 without one)
/// x `height` (14), wider if `label` needs it; `color` the series;
/// the label drawn centred as `node > label` (`color`, `font-size`,
/// `font-family`, horizontal `padding`).
pub fn sparkline<'a, M: 'a>(
    theme: &Theme,
    node: &Node,
    values: &'a VecDeque<f32>,
    max: Option<f32>,
    capacity: usize,
    label: Option<&str>,
) -> Element<'a, M> {
    let s = theme.resolve(node);
    let width = labelled_width(theme, node, label, 40.0).unwrap_or(40.0);
    let height = px(s.height).unwrap_or(14.0);
    let label = label
        .filter(|t| !t.is_empty())
        .map(|text| Label::from_node(theme, node, text, s.color));
    let program = Sparkline {
        series: Series {
            values,
            color: s.color.unwrap_or(Color::WHITE),
        },
        max,
        capacity,
        label,
    };
    framed(theme, node, program, width, height)
}

/// `program` in a container of `node`, so the theme's background and
/// border apply and `debug widgets` sees it; the size is the computed
/// one, not the node's `width` (the label may need more).
fn framed<'a, M: 'a, P: canvas::Program<M> + 'a>(
    theme: &Theme,
    node: &Node,
    program: P,
    width: f32,
    height: f32,
) -> Element<'a, M> {
    theme
        .container(
            node,
            canvas_widget(program)
                .width(Length::Fixed(width))
                .height(Length::Fixed(height)),
        )
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .into()
}

/// A gauge styled as `node`, sized and framed as [`sparkline`] is
/// (the node's `background`, `border`, `border-radius` on the
/// container); `node > fill { background }` (else `color`) the filled
/// part, `node > label` the text.
pub fn gauge<'a, M: 'a>(
    theme: &Theme,
    node: &Node,
    fraction: f32,
    label: Option<&str>,
) -> Element<'a, M> {
    let s = theme.resolve(node);
    let width = labelled_width(theme, node, label, 40.0).unwrap_or(40.0);
    let height = px(s.height).unwrap_or(14.0);
    let fill = theme.resolve(&node.child("fill"));
    let fill = fill
        .background
        .or(fill.color)
        .or(s.color)
        .unwrap_or(Color::WHITE);
    let program = Gauge {
        fraction,
        radius: s.border_radius.top_left,
        fill,
        label: label
            .filter(|t| !t.is_empty())
            .map(|text| Label::from_node(theme, node, text, s.color)),
    };
    framed(theme, node, program, width, height)
}

/// A graph styled as `node`, full width, `height` tall (default 160),
/// its scale's top written top left as `node > scale` (`color`,
/// `font-size`; `unit` says how),
/// `background`/`border` from the node (as a container around the
/// canvas), the first series in `color`, the second in `node > second
/// { color }`, the grid in `border-color`.
pub fn graph<'a, M: 'a>(
    theme: &Theme,
    node: &Node,
    series: &[&'a VecDeque<f32>],
    max: Option<f32>,
    capacity: usize,
    unit: Unit,
) -> Element<'a, M> {
    let s = theme.resolve(node);
    let height = px(s.height).unwrap_or(160.0);
    let scale_node = node.child("scale");
    let scale_style = theme.resolve(&scale_node);
    let scale_label = Some(Label {
        text: String::new(),
        color: scale_style.color.or(s.color).unwrap_or(Color::WHITE),
        size: scale_style.font_size.unwrap_or(10.0),
        font: scale_style.font().unwrap_or_default(),
    });
    let first = s.color.unwrap_or(Color::WHITE);
    let second = theme.resolve(&node.child("second")).color.unwrap_or(first);
    let program = Graph {
        series: series
            .iter()
            .enumerate()
            .map(|(i, values)| Series {
                values,
                color: if i == 0 { first } else { second },
            })
            .collect(),
        max,
        capacity,
        grid: s.border_color,
        unit,
        scale_label,
    };
    theme
        .container(
            node,
            canvas_widget(program)
                .width(Length::Fill)
                .height(Length::Fixed(height)),
        )
        .width(Length::Fill)
        .into()
}

/// A bar filled to `fraction` (0..1), styled as `node` (`height`,
/// `background`, `border-radius`) with the filled part as `node >
/// fill`; the node gets `.warning` / `.critical` from the caller.
pub fn meter<'a, M: 'a>(theme: &Theme, node: &Node, fraction: f32) -> Element<'a, M> {
    let s = theme.resolve(node);
    let height = px(s.height).unwrap_or(8.0);
    let p = (fraction.clamp(0.0, 1.0) * 1000.0).round() as u16;
    let fill_node = node.child("fill");
    let mut bar = row![];
    if p > 0 {
        bar = bar.push(
            theme
                .container(&fill_node, Space::new().width(Length::Fill))
                .width(Length::FillPortion(p))
                .height(Length::Fill),
        );
    }
    if p < 1000 {
        bar = bar.push(Space::new().width(Length::FillPortion(1000 - p)));
    }
    container(
        theme
            .container(node, bar.width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fixed(height)),
    )
    .width(Length::Fill)
    .into()
}

fn px(l: Option<theme::Length>) -> Option<f32> {
    match l {
        Some(theme::Length::Px(px)) => Some(px),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_and_area() {
        let a: VecDeque<f32> = [1.0, 5.0, 3.0].into();
        let b: VecDeque<f32> = [2.0].into();
        let series = [
            Series {
                values: &a,
                color: Color::WHITE,
            },
            Series {
                values: &b,
                color: Color::WHITE,
            },
        ];
        assert_eq!(scale(&series, None), 5.0);
        assert_eq!(scale(&series, Some(100.0)), 100.0);
        let zero: VecDeque<f32> = [0.0, 0.0].into();
        assert_eq!(
            scale(
                &[Series {
                    values: &zero,
                    color: Color::WHITE
                }],
                None
            ),
            1.0
        );
        assert!(area(&b, 10, 5.0, Size::new(100.0, 10.0)).is_none());
        assert!(area(&a, 10, 5.0, Size::new(100.0, 10.0)).is_some());
        // Newest at the right edge, 100/9 apart, scaled to max 5.
        let pts = points(&a, 10, 5.0, Size::new(90.0, 10.0));
        assert_eq!(pts[2], Point::new(90.0, 4.0));
        assert_eq!(pts[1], Point::new(80.0, 0.0));
        assert_eq!(pts[0], Point::new(70.0, 8.0));
    }
}
