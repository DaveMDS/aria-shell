//! A graph draws series of values, oldest first, as lines with the area
//! under them: `icssed::graph(node, capacity)` and one `.series(values)`
//! or more. A CPU's load, a network's traffic.
//!
//!     icssed::graph(node.child("graph"), 60)
//!         .series_class("down", self.down.iter().copied())
//!         .series_class("up", self.up.iter().copied())
//!         .label("1 MB/s")
//!
//! - The capacity is how many values span its width: the newest at the
//!   right edge, a young series growing from the right.
//! - The top is the largest value, unless `.max(v)` says it.
//! - `.series_class(c, values)` gives a series a class, so the sheet
//!   tells the series apart: `graph.traffic > series.up`.
//! - `.label(text)` is a text over it: the scale's top, a value.
//!
//! The paths are painted as SVG's: `stroke` and `stroke-width` for the
//! line, `fill` for the area under it, the `grid`'s stroke for the lines
//! across at the quarters. A sparkline is a graph too, small and styled
//! by the app. The values below move: a graph keeps its paths until its
//! values change, so a still one costs nothing.

use std::collections::VecDeque;

use icssed::{Element, Node};

use crate::Assets;

pub const TITLE: &str = "Graph";

/// How many values a graph shows.
const CAPACITY: usize = 60;

pub struct State {
    /// Made-up series: a load, traffic in and out.
    load: VecDeque<f32>,
    down: VecDeque<f32>,
    up: VecDeque<f32>,
    /// The next sample's time.
    t: f32,
}

impl Default for State {
    fn default() -> Self {
        let mut state = Self {
            load: VecDeque::new(),
            down: VecDeque::new(),
            up: VecDeque::new(),
            t: 0.0,
        };
        for _ in 0..CAPACITY / 2 {
            state.sample();
        }
        state
    }
}

impl State {
    /// One more value in each series.
    fn sample(&mut self) {
        let t = self.t;
        self.t += 0.25;
        let push = |series: &mut VecDeque<f32>, value: f32| {
            if series.len() == CAPACITY {
                series.pop_front();
            }
            series.push_back(value.max(0.0));
        };
        push(
            &mut self.load,
            50.0 + 30.0 * (t * 0.6).sin() + 10.0 * (t * 1.9).cos(),
        );
        push(&mut self.down, 400.0 + 300.0 * (t * 0.7).sin());
        push(&mut self.up, 120.0 + 90.0 * (t * 1.3).cos());
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Sample,
}

pub fn update(state: &mut State, message: Message) {
    let Message::Sample = message;
    for _ in 0..4 {
        state.sample();
    }
}

pub fn view<'a>(state: &'a State, node: Node<'a>, _assets: &'a Assets) -> Element<'a, Message> {
    let line = node.child("line");
    let more = node.child("button");
    icssed::column(
        node,
        [
            icssed::row(
                line,
                [
                    icssed::graph(line.child("graph"), CAPACITY)
                        .series(state.load.iter().copied())
                        .max(100.0)
                        .label("100%")
                        .into(),
                    icssed::graph(line.child("graph").class("traffic"), CAPACITY)
                        .series_class("down", state.down.iter().copied())
                        .series_class("up", state.up.iter().copied())
                        .label("1 MB/s")
                        .into(),
                    icssed::graph(line.child("sparkline"), CAPACITY / 2)
                        .series(state.load.iter().copied())
                        .max(100.0)
                        .label(format!(
                            "CPU {:.0}%",
                            state.load.back().copied().unwrap_or(0.0)
                        ))
                        .into(),
                ],
            )
            .into(),
            icssed::button(more, [icssed::text(more.child("label"), "Four more values").into()])
                .on_press(Message::Sample)
                .into(),
        ],
    )
    .into()
}
