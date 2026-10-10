//! The widgets' behaviour, driven headlessly: events in, messages and
//! pixels out.

use std::time::Instant;

use iced::keyboard::key::Named;
use iced::{Color, Point, Size};

use crate::testing::Harness;
use crate::{Element, Node, Scheme, Sheet, Source};

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

#[derive(Debug, Clone, PartialEq)]
enum Message {
    Pressed(&'static str),
    Ended,
}

/// A row of buttons, each `name` at 40x20, no gap.
fn buttons<'a>(root: Node<'a>, names: &[&'static str]) -> Element<'a, Message> {
    crate::row(
        root,
        names.iter().map(|&name| {
            crate::button(root.child("button").class(name), [])
                .on_press(Message::Pressed(name))
                .into()
        }),
    )
    .into()
}

const CSS: &str = "button { width: 40px; height: 20px; background: rgb(255 0 0); \
                            transition: none; } \
                   button:hover { background: rgb(0 0 255); } \
                   button:focus { background: rgb(0 255 0); }";

#[test]
fn a_button_publishes_on_a_release_over_it() {
    let sheet = sheet(CSS);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(buttons(root, &["a", "b"]), Size::new(80.0, 20.0), 1.0);
    ui.click(Point::new(50.0, 10.0));
    assert_eq!(ui.messages, [Message::Pressed("b")]);
    // Pressed on one, released off it: nothing.
    ui.move_to(Point::new(10.0, 10.0));
    ui.press();
    ui.move_to(Point::new(70.0, 30.0));
    ui.release();
    assert_eq!(ui.messages.len(), 1);
}

#[test]
fn a_hover_shows_on_the_frame_it_happens() {
    let sheet = sheet(CSS);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(buttons(root, &["a", "b"]), Size::new(80.0, 20.0), 1.0);
    let now = Instant::now();
    ui.frame(now);
    assert_eq!(ui.pixels(Color::WHITE).at(10, 10), [255, 0, 0, 255]);
    // A frame asked for, the hover in it.
    assert_ne!(
        ui.move_to(Point::new(10.0, 10.0)),
        iced::window::RedrawRequest::Wait
    );
    ui.frame(now);
    let pixels = ui.pixels(Color::WHITE);
    assert_eq!(pixels.at(10, 10), [0, 0, 255, 255]);
    assert_eq!(pixels.at(50, 10), [255, 0, 0, 255]);
}

#[test]
fn an_ancestors_state_reaches_its_descendants() {
    let sheet = sheet(
        "card { width: 60px; height: 30px; padding: 5px; } \
         part { width: fill; height: fill; background: rgb(255 0 0); } \
         card:hover part { background: rgb(0 0 255); }",
    );
    let card = sheet.root("card", 1.0);
    let view = || -> Element<'_, Message> {
        crate::row(card, [crate::row(card.child("part"), []).into()]).into()
    };
    let mut ui = Harness::new(view(), Size::new(60.0, 30.0), 1.0);
    ui.frame(Instant::now());
    assert_eq!(ui.pixels(Color::WHITE).at(30, 15), [255, 0, 0, 255]);
    // On the card's padding, not on the part: the card is hovered.
    ui.move_to(Point::new(2.0, 2.0));
    ui.frame(Instant::now());
    assert_eq!(ui.pixels(Color::WHITE).at(30, 15), [0, 0, 255, 255]);
}

#[test]
fn tab_moves_the_focus_and_enter_presses() {
    let sheet = sheet(CSS);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(
        crate::focus_scope(buttons(root, &["a", "b", "c"])),
        Size::new(120.0, 20.0),
        1.0,
    );
    ui.key(Named::Tab);
    ui.key(Named::Tab);
    ui.frame(Instant::now());
    let pixels = ui.pixels(Color::WHITE);
    assert_eq!(pixels.at(10, 10), [255, 0, 0, 255]);
    assert_eq!(pixels.at(50, 10), [0, 255, 0, 255]);
    ui.key(Named::Enter);
    assert_eq!(ui.messages, [Message::Pressed("b")]);
    // A press of the pointer takes the focus away.
    ui.move_to(Point::new(200.0, 200.0));
    ui.press();
    ui.frame(Instant::now());
    assert_eq!(ui.pixels(Color::WHITE).at(50, 10), [255, 0, 0, 255]);
}

#[test]
fn the_pointer_follows_a_transform() {
    let sheet = sheet(
        "root { padding: 20px; } button { width: 40px; height: 20px; } \
         button:hover { transform: scale(2); }",
    );
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(buttons(root, &["a"]), Size::new(80.0, 60.0), 1.0);
    ui.frame(Instant::now());
    // Hovered, it's drawn twice as big about its centre (40, 30): a
    // point off its box but on its drawing presses it.
    ui.move_to(Point::new(40.0, 30.0));
    ui.frame(Instant::now());
    ui.click(Point::new(5.0, 30.0));
    assert_eq!(ui.messages, [Message::Pressed("a")]);
}

use std::time::Duration;

use iced::window::RedrawRequest;

fn at(start: Instant, ms: u64) -> Instant {
    start + Duration::from_millis(ms)
}

/// Each channel within `slack` of the other's.
fn near(a: [u8; 4], b: [u8; 4], slack: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= slack)
}

#[test]
fn a_transition_runs_from_its_cause_to_its_end() {
    let sheet = sheet(
        "cell { width: 20px; height: 20px; background: rgb(255 0 0); \
                transition: background-color 1s linear; } \
         cell.on { background: rgb(0 0 255); }",
    );
    let root = sheet.root("root", 1.0);
    let view = |on: bool| -> Element<'_, Message> {
        crate::row(
            root,
            [crate::row(root.child("cell").class_if("on", on), []).into()],
        )
        .into()
    };
    let mut ui = Harness::new(view(false), Size::new(20.0, 20.0), 1.0);
    let start = Instant::now();
    ui.frame(start);
    assert_eq!(ui.pixels(Color::WHITE).at(10, 10), [255, 0, 0, 255]);
    // The class on: from red, toward blue, from now.
    ui.rebuild(view(true));
    assert_eq!(ui.frame(at(start, 0)), RedrawRequest::NextFrame);
    assert_eq!(ui.frame(at(start, 500)), RedrawRequest::NextFrame);
    let half = ui.pixels(Color::WHITE).at(10, 10);
    assert!(near(half, [128, 0, 128, 255], 6), "{half:?}");
    ui.frame(at(start, 1100));
    assert_eq!(ui.pixels(Color::WHITE).at(10, 10), [0, 0, 255, 255]);
    // Over: no more frames.
    assert_eq!(ui.frame(at(start, 1200)), RedrawRequest::Wait);
}

#[test]
fn an_ancestors_hover_starts_its_descendants_transition() {
    let sheet = sheet(
        "card { width: 60px; height: 30px; padding: 5px; } \
         part { width: fill; height: fill; background: rgb(255 0 0); \
                transition: background-color 1s linear; } \
         card:hover part { background: rgb(0 0 255); }",
    );
    let card = sheet.root("card", 1.0);
    let mut ui = Harness::<Message>::new(
        crate::row(card, [crate::row(card.child("part"), []).into()]),
        Size::new(60.0, 30.0),
        1.0,
    );
    let start = Instant::now();
    ui.frame(start);
    ui.move_to(Point::new(2.0, 2.0));
    // The hover's frame, and the one after: the part's transition has
    // started at the first's draw, it asks its own from the second on.
    assert_eq!(ui.frame(at(start, 0)), RedrawRequest::NextFrame);
    assert_eq!(ui.frame(at(start, 16)), RedrawRequest::NextFrame);
    assert_eq!(ui.frame(at(start, 500)), RedrawRequest::NextFrame);
    let half = ui.pixels(Color::WHITE).at(30, 15);
    assert!(near(half, [128, 0, 128, 255], 6), "{half:?}");
    ui.frame(at(start, 1100));
    assert_eq!(ui.pixels(Color::WHITE).at(30, 15), [0, 0, 255, 255]);
    assert_eq!(ui.frame(at(start, 1200)), RedrawRequest::Wait);
}

#[test]
fn keyframes_animate_and_end() {
    let sheet = sheet(
        "@keyframes fade { from { opacity: 0; } to { opacity: 1; } } \
         cell { width: 20px; height: 20px; background: rgb(0 0 0); \
                animation: fade 1s linear; }",
    );
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(
        crate::row(
            root,
            [crate::row(root.child("cell"), [])
                .on_animation_end(Message::Ended)
                .into()],
        ),
        Size::new(20.0, 20.0),
        1.0,
    );
    let start = Instant::now();
    ui.frame(start);
    assert_eq!(ui.pixels(Color::WHITE).at(10, 10), [255, 255, 255, 255]);
    assert_eq!(ui.frame(at(start, 500)), RedrawRequest::NextFrame);
    let half = ui.pixels(Color::WHITE).at(10, 10);
    assert!(near(half, [128, 128, 128, 255], 6), "{half:?}");
    ui.frame(at(start, 1000));
    assert_eq!(ui.pixels(Color::WHITE).at(10, 10), [0, 0, 0, 255]);
    assert!(ui.messages.is_empty());
    // The frame after the end says so, once; then nothing moves.
    assert_eq!(ui.frame(at(start, 1016)), RedrawRequest::NextFrame);
    assert_eq!(ui.messages, [Message::Ended]);
    assert_eq!(ui.frame(at(start, 1032)), RedrawRequest::Wait);
    ui.frame(at(start, 2000));
    assert_eq!(ui.messages, [Message::Ended]);
}

#[test]
fn an_infinite_animation_never_stops_asking() {
    let sheet = sheet(
        "@keyframes spin { to { rotate: 360deg; } } \
         icon { width: 16px; height: 16px; animation: spin 1s linear infinite; }",
    );
    let root = sheet.root("root", 1.0);
    let star = crate::IconFile::Svg {
        handle: iced::advanced::svg::Handle::from_memory(
            b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 2 2'><rect width='1' height='1'/></svg>".as_slice(),
        ),
        symbolic: true,
    };
    let mut ui = Harness::<Message>::new(
        crate::row(root, [crate::icon(root.child("icon"), star).into()]),
        Size::new(16.0, 16.0),
        1.0,
    );
    let start = Instant::now();
    ui.frame(start);
    for ms in [100, 1000, 5000, 60_000] {
        assert_eq!(ui.frame(at(start, ms)), RedrawRequest::NextFrame);
    }
}

#[test]
fn an_idle_ui_asks_for_nothing() {
    let sheet = sheet(CSS);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(buttons(root, &["a", "b"]), Size::new(80.0, 20.0), 1.0);
    let start = Instant::now();
    ui.frame(start);
    assert_eq!(ui.frame(at(start, 16)), RedrawRequest::Wait);
    // A pointer over nothing any rule sees: still nothing.
    assert_eq!(ui.move_to(Point::new(200.0, 200.0)), RedrawRequest::Wait);
}
