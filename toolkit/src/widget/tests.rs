//! The widgets' behaviour, driven headlessly: events in, messages and
//! pixels out.

use std::time::Instant;

use iced::keyboard::key::Named;
use iced::{Color, Event, Point, Rectangle, Size, mouse};

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
    Toggled(bool),
    Value(f32),
    Released,
    Typed(String),
    Submitted,
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

#[test]
fn widgets_are_found_by_selector() {
    // No positional pseudo-class in the sheet: the rows share their node,
    // the query still tells them apart.
    let sheet = sheet(
        "menu { width: 100px; } item { height: 20px; } \
         label { width: 10px; height: 10px; }",
    );
    let menu = sheet.root("menu", 1.0);
    let view = || -> Element<'_, Message> {
        crate::column(
            menu,
            ["a", "b", "c"].map(|name| {
                let item = menu.child("item").class_if("active", name == "b");
                crate::button(item, [crate::text(item.child("label"), name).into()])
                    .on_press(Message::Pressed(name))
                    .into()
            }),
        )
        .into()
    };
    let mut ui = Harness::new(view(), Size::new(100.0, 60.0), 1.0);
    let rect = |y: f32| iced::Rectangle::new(Point::new(0.0, y), Size::new(100.0, 20.0));
    assert_eq!(
        ui.find(&sheet, "menu > item"),
        [rect(0.0), rect(20.0), rect(40.0)]
    );
    assert_eq!(ui.find(&sheet, "item:nth-child(2)"), [rect(20.0)]);
    assert_eq!(ui.find(&sheet, "item:last-child"), [rect(40.0)]);
    assert_eq!(ui.find(&sheet, "item.active"), [rect(20.0)]);
    assert_eq!(ui.find(&sheet, "menu label").len(), 3);
    assert_eq!(ui.find(&sheet, "item:first-child label").len(), 1);
    assert!(ui.find(&sheet, "nothing").is_empty());
    ui.click_on(&sheet, "item:nth-child(3)");
    assert_eq!(ui.messages, [Message::Pressed("c")]);
}

/// The base's controls, without their transitions.
const CONTROLS: &str = "toggle, toggle > knob, slider > knob { transition: none; }";

#[test]
fn a_toggle_asks_for_the_other_state() {
    let sheet = sheet(CONTROLS);
    let root = sheet.root("root", 1.0);
    let view = |on: bool| -> Element<'_, Message> {
        crate::focus_scope(crate::row(
            root,
            [crate::toggle(root.child("toggle"), on, Message::Toggled).into()],
        ))
        .into()
    };
    let mut ui = Harness::new(view(false), Size::new(100.0, 40.0), 1.0);
    assert!(ui.find(&sheet, "toggle.on").is_empty());
    ui.click_on(&sheet, "toggle");
    assert_eq!(ui.messages, [Message::Toggled(true)]);
    ui.rebuild(view(true));
    assert_eq!(ui.find(&sheet, "toggle.on > knob").len(), 1);
    // From the keyboard: Tab to it, then Space.
    ui.key(Named::Tab);
    ui.key(Named::Space);
    assert_eq!(
        ui.messages,
        [Message::Toggled(true), Message::Toggled(false)]
    );
}

/// A slider from 0 to 100 at `value`: 160px wide, its track from 8 to
/// 152 (the base's padding, half its knob).
fn slider(root: Node<'_>, value: f32, disabled: bool) -> Element<'_, Message> {
    crate::focus_scope(crate::row(
        root,
        [crate::slider(
            root.child("slider").disabled_if(disabled),
            0.0..=100.0,
            value,
            Message::Value,
        )
        .step(1.0)
        .on_release(Message::Released)
        .into()],
    ))
    .into()
}

#[test]
fn a_slider_puts_its_knob_on_the_value() {
    let sheet = sheet(CONTROLS);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(slider(root, 40.0, false), Size::new(200.0, 40.0), 1.0);
    let track = ui.find(&sheet, "slider > track")[0];
    assert_eq!((track.x, track.width), (8.0, 144.0));
    // 40% of the track, on a whole pixel; the fill up to it.
    assert_eq!(ui.find(&sheet, "slider > knob")[0].center_x(), 66.0);
    assert_eq!(ui.find(&sheet, "track > fill")[0].width, 58.0);
    // At the ends, the knob overhangs the track by half its width.
    ui.rebuild(slider(root, 0.0, false));
    assert_eq!(ui.find(&sheet, "slider > knob")[0].x, 0.0);
    ui.rebuild(slider(root, 100.0, false));
    assert_eq!(ui.find(&sheet, "slider > knob")[0].x, 144.0);
}

#[test]
fn a_slider_follows_the_pointer() {
    let sheet = sheet(CONTROLS);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(slider(root, 40.0, false), Size::new(200.0, 40.0), 1.0);
    // Pressed at a quarter of the track, dragged to half, then past
    // its end, let go.
    ui.move_to(Point::new(8.0 + 36.0, 8.0));
    ui.press();
    ui.move_to(Point::new(8.0 + 72.0, 30.0));
    ui.move_to(Point::new(190.0, 8.0));
    ui.release();
    assert_eq!(
        ui.messages,
        [
            Message::Value(25.0),
            Message::Value(50.0),
            Message::Value(100.0),
            Message::Released
        ]
    );
}

#[test]
fn a_slider_steps_with_the_wheel_and_the_keys() {
    let sheet = sheet(CONTROLS);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(slider(root, 40.0, false), Size::new(200.0, 40.0), 1.0);
    let wheel = |delta| Event::Mouse(mouse::Event::WheelScrolled { delta });
    ui.move_to(Point::new(80.0, 8.0));
    // Two lines, then a touchpad's pixels: a step every 10.
    ui.send(&[wheel(mouse::ScrollDelta::Lines { x: 0.0, y: 2.0 })]);
    ui.send(&[wheel(mouse::ScrollDelta::Pixels { x: 0.0, y: 15.0 })]);
    ui.send(&[wheel(mouse::ScrollDelta::Pixels { x: 0.0, y: 5.0 })]);
    assert_eq!(
        ui.messages,
        [
            Message::Value(42.0),
            Message::Value(43.0),
            Message::Value(44.0)
        ]
    );
    ui.messages.clear();
    // The keys, once it has the focus.
    ui.key(Named::ArrowRight);
    assert!(ui.messages.is_empty());
    ui.key(Named::Tab);
    for key in [
        Named::ArrowRight,
        Named::PageUp,
        Named::Home,
        Named::End,
        Named::ArrowLeft,
    ] {
        ui.key(key);
    }
    assert_eq!(
        ui.messages,
        [
            Message::Value(45.0),
            Message::Value(55.0),
            Message::Value(0.0),
            Message::Value(100.0),
            Message::Value(99.0)
        ]
    );
}

#[test]
fn a_disabled_slider_takes_nothing() {
    let sheet = sheet(CONTROLS);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(slider(root, 40.0, true), Size::new(200.0, 40.0), 1.0);
    ui.click(Point::new(100.0, 8.0));
    ui.send(&[Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
    })]);
    ui.key(Named::Tab);
    ui.key(Named::ArrowRight);
    assert!(ui.messages.is_empty());
}

#[test]
fn a_meter_fills_its_fraction_under_its_label() {
    let sheet = sheet("meter { width: 100px; height: 20px; padding: 2px; }");
    let root = sheet.root("root", 1.0);
    let view = |fraction: f32| -> Element<'_, Message> {
        crate::row(
            root,
            [crate::meter(root.child("meter"), fraction)
                .label("25%")
                .into()],
        )
        .into()
    };
    let mut ui = Harness::new(view(0.25), Size::new(200.0, 40.0), 1.0);
    assert_eq!(
        ui.find(&sheet, "meter > fill")[0],
        Rectangle {
            x: 2.0,
            y: 2.0,
            width: 24.0,
            height: 16.0
        }
    );
    // Centred, as the base places it.
    let label = ui.find(&sheet, "meter > label")[0];
    assert!((label.center_x() - 50.0).abs() <= 0.5, "{label:?}");
    assert!((label.center_y() - 10.0).abs() <= 0.5, "{label:?}");
    // Out of range, it's full or empty.
    ui.rebuild(view(1.5));
    assert_eq!(ui.find(&sheet, "meter > fill")[0].width, 96.0);
    ui.rebuild(view(f32::NAN));
    assert_eq!(ui.find(&sheet, "meter > fill")[0].width, 0.0);
}

/// A search input in a focus scope, 200px wide.
fn search<'a>(root: Node<'a>, value: &str, disabled: bool) -> Element<'a, Message> {
    crate::focus_scope(crate::row(
        root,
        [
            crate::input(root.child("input").disabled_if(disabled), "Search", value)
                .on_input(Message::Typed)
                .on_submit(Message::Submitted)
                .into(),
        ],
    ))
    .into()
}

/// The base's border colours of an input, at rest and focused.
const BORDER: [u8; 4] = [201, 201, 208, 255];
const FOCUSED: [u8; 4] = [52, 102, 214, 255];

#[test]
fn an_input_takes_the_focus_and_the_keys() {
    let sheet = sheet("input { transition: none; }");
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(search(root, "", false), Size::new(240.0, 40.0), 1.0);
    let now = Instant::now();
    ui.frame(now);
    assert_eq!(ui.pixels(Color::WHITE).at(100, 0), BORDER);
    // A press on its padding, off the field, puts the focus in it.
    ui.click(Point::new(3.0, 16.0));
    ui.frame(now);
    assert_eq!(ui.pixels(Color::WHITE).at(100, 0), FOCUSED);
    ui.type_text("hi");
    ui.key(Named::Enter);
    assert_eq!(
        ui.messages,
        [
            Message::Typed("h".into()),
            Message::Typed("hi".into()),
            Message::Submitted
        ]
    );
    // A press elsewhere takes it away; Tab brings it back.
    ui.click(Point::new(230.0, 30.0));
    ui.frame(now);
    assert_eq!(ui.pixels(Color::WHITE).at(100, 0), BORDER);
    ui.key(Named::Tab);
    ui.frame(now);
    assert_eq!(ui.pixels(Color::WHITE).at(100, 0), FOCUSED);
}

#[test]
fn a_disabled_input_is_out_of_reach() {
    let sheet = sheet("input { transition: none; }");
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(search(root, "", true), Size::new(240.0, 40.0), 1.0);
    let now = Instant::now();
    let field = ui.find(&sheet, "input > field")[0];
    ui.click(field.center());
    ui.click(Point::new(3.0, 16.0));
    ui.key(Named::Tab);
    ui.type_text("hi");
    ui.frame(now);
    assert!(ui.messages.is_empty());
    assert_eq!(ui.pixels(Color::WHITE).at(100, 0), BORDER);
}

#[test]
fn an_inputs_field_is_centred_on_its_letters() {
    let sheet =
        sheet("input { width: 200px; height: 30px; min-height: 0; padding: 0 8px; border: none; }");
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(search(root, "", false), Size::new(240.0, 40.0), 1.0);
    // Its box is its capitals' height, centred as a label's would be.
    let field = ui.find(&sheet, "input > field")[0];
    let cap = root.child("input").child("field").base().cap_height;
    assert!((field.height - cap).abs() <= 1.0, "{field:?}, cap {cap}");
    assert!((field.center_y() - 15.0).abs() <= 0.5, "{field:?}");
    assert_eq!((field.x, field.width), (8.0, 184.0));
}
