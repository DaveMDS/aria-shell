//! The widgets' behaviour, driven headlessly: events in, messages and
//! pixels out.

use std::time::Instant;

use iced::keyboard::key::Named;
use iced::{Color, Event, Point, Rectangle, Size, mouse};

use crate::testing::Harness;
use crate::{Element, Node, Scheme, Sheet, Source};

/// `css` over a root with no background: the pixels are the widgets'.
fn sheet(css: &str) -> Sheet {
    let css = format!("surface {{ background: none; }} {css}");
    let (sheet, warnings) = Sheet::load(
        &[Source {
            name: "t.css",
            text: &css,
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
    crate::row(names.iter().map(|&name| {
        crate::button([])
            .class(name)
            .on_press(Message::Pressed(name))
            .into()
    }))
    .at(root)
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
        ".card { width: 60px; height: 30px; padding: 5px; } \
         part { width: fill; height: fill; background: rgb(255 0 0); } \
         .card:hover part { background: rgb(0 0 255); }",
    );
    let card = sheet.root("card", 1.0);
    let view = || -> Element<'_, Message> {
        crate::row([crate::row([]).kind("part").into()])
            .at(card)
            .into()
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
        ".root { padding: 20px; } button { width: 40px; height: 20px; } \
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
        crate::row([crate::row([]).kind("cell").class_if("on", on).into()])
            .at(root)
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
fn a_box_slides_out_by_its_own_height() {
    let sheet = sheet(
        "cell { width: 20px; height: 20px; background: rgb(255 0 0); \
                transition: transform 1s linear; } \
         cell.hidden { transform: translateY(-100%); }",
    );
    let root = sheet.root("root", 1.0);
    let view = |hidden: bool| -> Element<'_, Message> {
        crate::row([crate::row([])
            .kind("cell")
            .class_if("hidden", hidden)
            .into()])
        .at(root)
        .into()
    };
    let red = [255, 0, 0, 255];
    let white = [255, 255, 255, 255];
    let mut ui = Harness::new(view(false), Size::new(20.0, 20.0), 1.0);
    let start = Instant::now();
    ui.frame(start);
    assert_eq!(ui.pixels(Color::WHITE).at(10, 15), red);
    // From none to -100%: halfway, half of its height up.
    ui.rebuild(view(true));
    ui.frame(at(start, 0));
    ui.frame(at(start, 500));
    let pixels = ui.pixels(Color::WHITE);
    assert_eq!((pixels.at(10, 5), pixels.at(10, 15)), (red, white));
    // Then out of its place.
    ui.frame(at(start, 1100));
    assert_eq!(ui.pixels(Color::WHITE).at(10, 5), white);
}

#[test]
fn an_ancestors_hover_starts_its_descendants_transition() {
    let sheet = sheet(
        ".card { width: 60px; height: 30px; padding: 5px; } \
         part { width: fill; height: fill; background: rgb(255 0 0); \
                transition: background-color 1s linear; } \
         .card:hover part { background: rgb(0 0 255); }",
    );
    let card = sheet.root("card", 1.0);
    let mut ui = Harness::<Message>::new(
        crate::row([crate::row([]).kind("part").into()]).at(card),
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
        crate::row([crate::row([])
            .kind("cell")
            .on_animation_end(Message::Ended)
            .into()])
        .at(root),
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
        crate::row([crate::icon(star).into()]).at(root),
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
fn a_symbolic_icon_fades_with_its_colour_and_opacity() {
    // iced recolours a symbolic svg's RGB alone: the alpha must still
    // reach it.
    let sheet = sheet(
        "row { gap: 0; } icon { width: 16px; height: 16px; color: black; } \
         icon.faded { opacity: 0.5; } icon.thin { color: rgba(0, 0, 0, 0.5); } \
         icon.hidden { opacity: 0; }",
    );
    let root = sheet.root("root", 1.0);
    let square = || {
        crate::IconFile::Svg {
        handle: iced::advanced::svg::Handle::from_memory(
            b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 2 2'><rect width='2' height='2'/></svg>".as_slice(),
        ),
        symbolic: true,
    }
    };
    let mut ui = Harness::<Message>::new(
        crate::row(["", "faded", "thin", "hidden"].map(|class| {
            crate::icon(square())
                .class_if(class, !class.is_empty())
                .into()
        }))
        .at(root),
        Size::new(64.0, 16.0),
        1.0,
    );
    ui.frame(Instant::now());
    let pixels = ui.pixels(Color::WHITE);
    assert_eq!(pixels.at(8, 8), [0, 0, 0, 255]);
    for x in [24, 40] {
        let [r, ..] = pixels.at(x, 8);
        assert!((126..=129).contains(&r), "half way to white: {r}");
    }
    assert_eq!(pixels.at(56, 8), [255, 255, 255, 255]);
}

#[test]
fn a_new_widget_asks_for_its_entry_animations_frames() {
    // Its animation is known at its first draw, after the frame's
    // `update`: that frame still asks for the next, and the next goes on.
    let sheet = sheet(
        "@keyframes enter { from { opacity: 0; } } \
         box { width: 20px; height: 20px; animation: enter 200ms; }",
    );
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::<Message>::new(
        crate::row([crate::row([]).kind("box").into()]).at(root),
        Size::new(20.0, 20.0),
        1.0,
    );
    let start = Instant::now();
    assert_eq!(ui.frame(start), RedrawRequest::NextFrame);
    assert_eq!(ui.frame(at(start, 16)), RedrawRequest::NextFrame);
    // Over (a frame more tells its end), it asks for nothing more.
    ui.frame(at(start, 500));
    ui.frame(at(start, 516));
    assert_eq!(ui.frame(at(start, 532)), RedrawRequest::Wait);
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
        ".list { width: 100px; } entry { height: 20px; } \
         label { width: 10px; height: 10px; }",
    );
    let list = sheet.root("list", 1.0);
    let view = || -> Element<'_, Message> {
        crate::column(["a", "b", "c"].map(|name| {
            crate::button![crate::text(name)]
                .kind("entry")
                .class_if("active", name == "b")
                .on_press(Message::Pressed(name))
                .into()
        }))
        .at(list)
        .into()
    };
    let mut ui = Harness::new(view(), Size::new(100.0, 60.0), 1.0);
    let rect = |y: f32| iced::Rectangle::new(Point::new(0.0, y), Size::new(100.0, 20.0));
    assert_eq!(
        ui.find(&sheet, ".list > entry"),
        [rect(0.0), rect(20.0), rect(40.0)]
    );
    assert_eq!(ui.find(&sheet, "entry:nth-child(2)"), [rect(20.0)]);
    assert_eq!(ui.find(&sheet, "entry:last-child"), [rect(40.0)]);
    assert_eq!(ui.find(&sheet, "entry.active"), [rect(20.0)]);
    assert_eq!(ui.find(&sheet, ".list label").len(), 3);
    assert_eq!(ui.find(&sheet, "entry:first-child label").len(), 1);
    assert!(ui.find(&sheet, "nothing").is_empty());
    ui.click_on(&sheet, "entry:nth-child(3)");
    assert_eq!(ui.messages, [Message::Pressed("c")]);

    // What was found, by name.
    let found = ui.report();
    let active = sheet.matching("entry.active", &found).expect("a selector");
    assert_eq!(active.len(), 1);
    assert_eq!(
        found[active[0]].path(&sheet).as_deref(),
        Some("surface.list > entry.active")
    );
    let other = self::sheet("");
    assert_eq!(found[active[0]].path(&other), None, "another sheet's");
    assert!(other.matching("entry", &found).unwrap().is_empty());
}

#[test]
fn a_box_is_found_and_focused_by_its_id() {
    use iced::advanced::widget::operation::{Operation, Outcome, focusable};
    use iced::advanced::widget::{self, Id};

    struct Bounds(Id, Option<Rectangle>);
    impl Operation<Option<Rectangle>> for Bounds {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<Rectangle>>)) {
            operate(self);
        }
        fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
            if id == Some(&self.0) {
                self.1 = Some(bounds);
            }
        }
        fn finish(&self) -> Outcome<Option<Rectangle>> {
            Outcome::Some(self.1)
        }
    }

    let sheet = sheet(CSS);
    let root = sheet.root("root", 1.0);
    let id = Id::unique();
    let view = crate::row(["a", "b"].map(|name| {
        let b = crate::button([])
            .class(name)
            .on_press(Message::Pressed(name));
        if name == "b" { b.id(id.clone()) } else { b }.into()
    }))
    .at(root);
    let mut ui = Harness::new(view, Size::new(80.0, 20.0), 1.0);
    assert_eq!(
        ui.operate(&mut Bounds(id.clone(), None)),
        Some(Some(Rectangle::new(
            Point::new(40.0, 0.0),
            Size::new(40.0, 20.0)
        )))
    );
    ui.operate::<()>(&mut focusable::focus(id));
    ui.key(Named::Enter);
    assert_eq!(ui.messages, [Message::Pressed("b")]);
}

/// The base's controls, without their transitions.
const CONTROLS: &str = "toggle, toggle > check, toggle > radio, toggle > radio > dot, \
                        toggle > switch, toggle > switch > knob, \
                        slider > knob { transition: none; }";

#[test]
fn a_toggle_asks_for_the_other_state() {
    let sheet = sheet(CONTROLS);
    let root = sheet.root("root", 1.0);
    let view = |on: bool| -> Element<'_, Message> {
        crate::focus_scope(
            crate::row([crate::toggle(on, Message::Toggled).switch().into()]).at(root),
        )
        .into()
    };
    let mut ui = Harness::new(view(false), Size::new(100.0, 40.0), 1.0);
    assert!(ui.find(&sheet, "toggle.checked").is_empty());
    ui.click_on(&sheet, "toggle");
    assert_eq!(ui.messages, [Message::Toggled(true)]);
    ui.rebuild(view(true));
    assert_eq!(ui.find(&sheet, "toggle.checked > switch > knob").len(), 1);
    // From the keyboard: Tab to it, then Space.
    ui.key(Named::Tab);
    ui.key(Named::Space);
    assert_eq!(
        ui.messages,
        [Message::Toggled(true), Message::Toggled(false)]
    );
}

#[test]
fn a_radio_asks_for_itself() {
    let sheet = sheet(CONTROLS);
    let root = sheet.root("root", 1.0);
    let view = |chosen: &'static str| -> Element<'_, Message> {
        crate::row(["a", "b", "c"].map(|name| {
            crate::toggle(name == chosen, move |_| Message::Pressed(name))
                .radio()
                .label(name)
                .into()
        }))
        .at(root)
        .into()
    };
    let mut ui = Harness::new(view("a"), Size::new(200.0, 40.0), 1.0);
    assert_eq!(
        ui.find(&sheet, "toggle.radio.checked:nth-child(1)").len(),
        1
    );
    ui.click_on(&sheet, "toggle.radio:nth-child(2)");
    assert_eq!(ui.messages, [Message::Pressed("b")]);
    ui.rebuild(view("b"));
    assert_eq!(ui.find(&sheet, "toggle.radio.checked").len(), 1);
    assert_eq!(
        ui.find(&sheet, "toggle.radio.checked:nth-child(2) > radio > dot")
            .len(),
        1
    );
    // The chosen one asks for itself again: nothing changes.
    ui.click_on(&sheet, "toggle.radio:nth-child(2)");
    assert_eq!(ui.messages, [Message::Pressed("b"), Message::Pressed("b")]);
}

/// A slider from 0 to 100 at `value`: 160px wide, its track from 8 to
/// 152 (the base's padding, half its knob).
fn slider(root: Node<'_>, value: f32, disabled: bool) -> Element<'_, Message> {
    crate::focus_scope(
        crate::row([crate::slider(0.0..=100.0, value, Message::Value)
            .disabled_if(disabled)
            .step(1.0)
            .on_release(Message::Released)
            .into()])
        .at(root),
    )
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
        crate::row([crate::meter(fraction).label("25%").into()])
            .at(root)
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
    crate::focus_scope(
        crate::row([crate::input("Search", value)
            .disabled_if(disabled)
            .on_input(Message::Typed)
            .on_submit(Message::Submitted)
            .into()])
        .at(root),
    )
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

const SCROLL: &str = "scroll { width: 100px; max-height: 100px; } \
                      line { gap: 0; } line > cell { width: 20px; height: 20px; }";

/// `n` cells of 20px, one under the other (or side by side), in a
/// scroll at most 100px tall (or 100px wide).
fn cells(root: Node<'_>, n: usize, sideways: bool) -> Element<'_, Message> {
    let content = (0..n).map(|_| crate::row([]).kind("cell").into());
    let scroll = if sideways {
        crate::scroll(crate::row(content).kind("line")).horizontal()
    } else {
        crate::scroll(crate::column(content).kind("line"))
    };
    crate::column![scroll].at(root).into()
}

#[test]
fn a_scroll_grows_with_its_content_up_to_its_max() {
    let sheet = sheet(SCROLL);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(cells(root, 3, false), Size::new(200.0, 200.0), 1.0);
    assert_eq!(ui.find(&sheet, "scroll")[0].height, 60.0);
    ui.rebuild(cells(root, 10, false));
    assert_eq!(ui.find(&sheet, "scroll")[0].height, 100.0);
}

/// A turn of the wheel, `lines` up (or down, below zero).
fn wheel(lines: f32) -> Event {
    Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: lines },
    })
}

#[test]
fn the_wheel_scrolls_and_the_query_follows() {
    let sheet = sheet(SCROLL);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(cells(root, 10, false), Size::new(200.0, 200.0), 1.0);
    assert_eq!(ui.find(&sheet, "cell")[0].y, 0.0);
    // A line down is iced's 60px.
    ui.move_to(Point::new(10.0, 50.0));
    ui.send(&[wheel(-1.0)]);
    let found = ui.find(&sheet, "cell");
    assert_eq!((found[0].y, found[3].y), (-60.0, 0.0));
    // The scroll itself stays where it is.
    assert_eq!(ui.find(&sheet, "scroll")[0].y, 0.0);
}

#[test]
fn a_sideways_scroll_takes_the_wheel_sideways() {
    let sheet = sheet(SCROLL);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(cells(root, 10, true), Size::new(200.0, 200.0), 1.0);
    ui.move_to(Point::new(50.0, 10.0));
    ui.send(&[wheel(-1.0)]);
    assert_eq!(ui.find(&sheet, "cell")[0].x, -60.0);
}

#[test]
fn a_scrollbar_shows_its_states() {
    let sheet = sheet(SCROLL);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(cells(root, 10, false), Size::new(200.0, 200.0), 1.0);
    let now = Instant::now();
    // The base's bar: 6px at 2px from the scroll's end, its thumb half
    // its height (the content is twice the room).
    let thumb = Point::new(100.0 - 5.0, 20.0);
    let colour = |ui: &mut Harness<'_, Message>| {
        ui.frame(now);
        ui.pixels(Color::WHITE).at_point(thumb.x, thumb.y, 1.0)
    };
    assert_eq!(colour(&mut ui), [201, 201, 208, 255]);
    ui.move_to(thumb);
    assert_eq!(colour(&mut ui), [110, 110, 120, 255]);
    ui.press();
    assert_eq!(colour(&mut ui), [52, 102, 214, 255]);
    // Below the thumb, the rail shows the cells under it.
    assert_eq!(
        ui.pixels(Color::WHITE).at_point(thumb.x, 80.0, 1.0),
        [255, 255, 255, 255]
    );
}

const MENU: &str = "menu item { transition: none; }";

/// A menu: two items, a disabled one, a check, a submenu unfolded or not.
fn file_menu(root: Node<'_>, expanded: bool) -> Element<'_, Message> {
    use crate::MenuItem;
    crate::focus_scope(
        crate::column([crate::menu([
            MenuItem::new("Open").on_press(Message::Pressed("open")),
            MenuItem::new("Save").on_press(Message::Pressed("save")),
            MenuItem::new("Paste"),
            MenuItem::separator(),
            MenuItem::new("Hidden")
                .check(true)
                .on_press(Message::Pressed("hidden")),
            MenuItem::new("More")
                .on_press(Message::Pressed("more"))
                .submenu(
                    expanded,
                    [MenuItem::new("Rename").on_press(Message::Pressed("rename"))],
                ),
        ])
        .into()])
        .at(root),
    )
    .into()
}

#[test]
fn a_menu_presses_its_items_and_unfolds_in_place() {
    let sheet = sheet(MENU);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(file_menu(root, false), Size::new(300.0, 300.0), 1.0);
    let labels = ui.find(&sheet, "menu > item > label");
    assert_eq!(labels.len(), 5);
    // Every label in one column, the marks' and the arrow's before and
    // after it.
    assert!(labels.iter().all(|l| l.x == labels[0].x), "{labels:?}");
    assert_eq!(ui.find(&sheet, "menu > item > check").len(), 5);
    assert_eq!(ui.find(&sheet, "menu > item.submenu > arrow").len(), 1);
    for label in ["menu > item:nth-child(2)", "menu > item:nth-child(3)"] {
        ui.click_on(&sheet, label);
    }
    assert_eq!(ui.messages, [Message::Pressed("save")]);
    assert!(ui.find(&sheet, "submenu").is_empty());
    ui.rebuild(file_menu(root, true));
    assert_eq!(ui.find(&sheet, "menu > item.expanded").len(), 1);
    ui.click_on(&sheet, "submenu > item");
    assert_eq!(
        ui.messages,
        [Message::Pressed("save"), Message::Pressed("rename")]
    );
}

#[test]
fn up_and_down_move_the_focus_among_a_menus_items() {
    let sheet = sheet(MENU);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(file_menu(root, false), Size::new(300.0, 300.0), 1.0);
    // Nothing focused: the arrows wait for Tab.
    ui.key(Named::ArrowDown);
    ui.key(Named::Enter);
    assert!(ui.messages.is_empty());
    ui.key(Named::Tab);
    // Down past the disabled one, then back up.
    ui.key(Named::ArrowDown);
    ui.key(Named::ArrowDown);
    ui.key(Named::Enter);
    ui.key(Named::ArrowUp);
    ui.key(Named::Space);
    assert_eq!(
        ui.messages,
        [Message::Pressed("hidden"), Message::Pressed("save")]
    );
}

/// Three tabs, the second shown.
fn pages(root: Node<'_>) -> Element<'_, Message> {
    crate::focus_scope(
        crate::column([crate::tabs(
            ["one", "two", "three"]
                .map(|name| crate::Tab::new(name, Message::Pressed(name)).active(name == "two")),
        )
        .into()])
        .at(root),
    )
    .into()
}

#[test]
fn tabs_press_and_take_the_arrows() {
    let sheet = sheet("tabs > tab { transition: none; }");
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(pages(root), Size::new(300.0, 60.0), 1.0);
    assert_eq!(ui.find(&sheet, "tabs > tab").len(), 3);
    assert_eq!(
        ui.find(&sheet, "tab.active")[0],
        ui.find(&sheet, "tab:nth-child(2)")[0]
    );
    ui.click_on(&sheet, "tab:nth-child(3)");
    // Tab to the first, Right to the second, Left and Left again: back
    // round to the third.
    ui.key(Named::Tab);
    ui.key(Named::ArrowRight);
    ui.key(Named::Enter);
    ui.key(Named::ArrowLeft);
    ui.key(Named::ArrowLeft);
    ui.key(Named::Space);
    assert_eq!(
        ui.messages,
        [
            Message::Pressed("three"),
            Message::Pressed("two"),
            Message::Pressed("three")
        ]
    );
}

#[test]
fn a_childs_inherited_colour_follows_its_parents_state() {
    let sheet = sheet(
        "button { width: 40px; height: 20px; transition: none; } \
         button:hover { color: rgb(255 0 0); } label { width: 40px; }",
    );
    let root = sheet.root("root", 1.0);
    let view = crate::row![crate::button![crate::text("████")].on_press(Message::Ended)].at(root);
    let mut ui = Harness::new(view, Size::new(40.0, 20.0), 1.0);
    let now = Instant::now();
    let label = ui.find(&sheet, "label")[0];
    ui.frame(now);
    let at = |ui: &mut Harness<'_, Message>| {
        ui.pixels(Color::WHITE)
            .at_point(label.x + 4.0, label.center_y(), 1.0)
    };
    assert_ne!(at(&mut ui), [255, 0, 0, 255]);
    ui.move_to(Point::new(10.0, 10.0));
    ui.frame(now);
    assert_eq!(at(&mut ui), [255, 0, 0, 255]);
}

/// October 2026, its weeks from `first`, today the 10th.
fn october(root: Node<'_>, first: u32) -> Element<'_, Message> {
    let calendar = crate::calendar(
        crate::Month::new(2026, 10),
        "October 2026",
        ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"],
    )
    .first_weekday(first)
    .today(2026, 10, 10)
    .selected(2026, 9, 30)
    .on_previous(Message::Pressed("previous"))
    .on_next(Message::Pressed("next"))
    .on_select(|day| Message::Value(day as f32));
    crate::column![calendar].at(root).into()
}

#[test]
fn a_calendar_shows_its_month_and_picks_a_day() {
    let sheet = sheet("calendar day { transition: none; }");
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(october(root, 0), Size::new(300.0, 300.0), 1.0);
    // Monday first: the 1st, a Thursday, after three days of September;
    // eight of November end the sixth week.
    assert_eq!(ui.find(&sheet, "week > day").len(), 42);
    assert_eq!(ui.find(&sheet, "day.other").len(), 11);
    let first_week = ui.find(&sheet, "week:first-child > day");
    let today = ui.find(&sheet, "day.today");
    assert_eq!(today.len(), 1);
    // The 10th: a Saturday, the sixth of the second week.
    assert_eq!(today[0].x, first_week[5].x);
    // September's 30th is shown, but not as the month's.
    assert!(ui.find(&sheet, "day.selected").is_empty());
    ui.click_on(&sheet, "day.today");
    ui.click_on(&sheet, "week:first-child > day:first-child");
    ui.click_on(&sheet, "button.previous");
    ui.click_on(&sheet, "button.next");
    assert_eq!(
        ui.messages,
        [
            Message::Value(10.0),
            Message::Pressed("previous"),
            Message::Pressed("next")
        ]
    );
    // Sunday first: four days of September before the 1st.
    ui.rebuild(october(root, 6));
    assert_eq!(ui.find(&sheet, "day.other").len(), 11);
    let first_week = ui.find(&sheet, "week:first-child > day");
    assert_eq!(ui.find(&sheet, "day.today")[0].x, first_week[6].x);
}

/// A graph 100x50 with no border or padding, its series filled red.
const GRAPH: &str = "graph { width: 100px; height: 50px; padding: 0; border: none; } \
                     graph > series { fill: rgb(255 0 0); stroke: none; } \
                     graph > grid { stroke: none; }";

#[test]
fn a_graph_draws_its_series_from_the_right() {
    let sheet = sheet(GRAPH);
    let root = sheet.root("root", 1.0);
    // Half its capacity, rising from nothing to its top.
    let view = |values: Vec<f32>| -> Element<'_, Message> {
        crate::column([crate::graph(11).series(values).max(10.0).label("10").into()])
            .at(root)
            .into()
    };
    let mut ui = Harness::new(
        view((5..=10).map(|v| v as f32).collect()),
        Size::new(100.0, 50.0),
        1.0,
    );
    let now = Instant::now();
    ui.frame(now);
    let pixels = ui.pixels(Color::WHITE);
    let red = [255, 0, 0, 255];
    // The newest at the right edge, full; the series starts half way.
    assert_eq!(pixels.at(97, 5), red);
    assert_eq!(pixels.at(55, 45), red);
    assert_eq!(pixels.at(45, 45), [255, 255, 255, 255]);
    assert_eq!(pixels.at(55, 5), [255, 255, 255, 255]);
    assert_eq!(ui.find(&sheet, "graph > series").len(), 1);
    assert_eq!(ui.find(&sheet, "graph > label")[0].x, 0.0);
    // New values, drawn anew.
    ui.rebuild(view(vec![0.0; 11]));
    ui.frame(now);
    assert_eq!(ui.pixels(Color::WHITE).at(97, 5), [255, 255, 255, 255]);
}

/// A 40x20 button with a tip, its own way or the app's.
fn tipped(root: Node<'_>, the_apps: bool) -> Element<'_, Message> {
    let button = crate::button([]).on_press(Message::Pressed("tipped"));
    let tip = crate::tooltip(button, "A tip");
    let tip = if the_apps {
        tip.on_tip(|shown| match shown {
            Some(_) => Message::Pressed("shown"),
            None => Message::Pressed("hidden"),
        })
    } else {
        tip
    };
    crate::column([tip.into()]).at(root).into()
}

/// The button 80px in, a tip wider than it.
const TIPPED: &str = ".root { padding: 0 80px; align-items: start; } \
                      button { width: 40px; height: 20px; } \
                      tooltip { animation: none; margin: 6px 0; }";

#[test]
fn a_tip_shows_after_a_rest_and_goes_with_the_pointer() {
    let sheet = sheet(TIPPED);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(tipped(root, false), Size::new(200.0, 100.0), 1.0);
    let start = Instant::now();
    ui.move_to(Point::new(100.0, 10.0));
    ui.frame(start + Duration::from_millis(100));
    assert!(ui.find(&sheet, "tooltip").is_empty());
    ui.frame(start + Duration::from_millis(700));
    let tip = ui.find(&sheet, "tooltip");
    assert_eq!(tip.len(), 1);
    // Under the button, its margin away, centred on it.
    assert_eq!(tip[0].y, 20.0 + 6.0);
    assert!((tip[0].center_x() - 100.0).abs() <= 1.0, "{tip:?}");
    ui.move_to(Point::new(190.0, 80.0));
    ui.frame(start + Duration::from_millis(800));
    assert!(ui.find(&sheet, "tooltip").is_empty());
    // Pressed, it doesn't show until the pointer leaves.
    ui.move_to(Point::new(100.0, 10.0));
    ui.press();
    ui.release();
    ui.frame(Instant::now() + Duration::from_secs(1));
    assert!(ui.find(&sheet, "tooltip").is_empty());
}

#[test]
fn an_app_can_show_a_tip_itself() {
    let sheet = sheet(TIPPED);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(tipped(root, true), Size::new(200.0, 100.0), 1.0);
    ui.move_to(Point::new(100.0, 10.0));
    ui.frame(Instant::now() + Duration::from_millis(700));
    assert!(ui.find(&sheet, "tooltip").is_empty());
    ui.move_to(Point::new(190.0, 80.0));
    assert_eq!(
        ui.messages,
        [Message::Pressed("shown"), Message::Pressed("hidden")]
    );
}

/// A select of three in a focus scope, `chosen` if any.
fn chooser(root: Node<'_>, chosen: Option<usize>, disabled: bool) -> Element<'_, Message> {
    crate::focus_scope(
        crate::column([crate::select(["one", "two", "three"], chosen, |i| {
            Message::Value(i as f32)
        })
        .disabled_if(disabled)
        .placeholder("none yet")
        .into()])
        .at(root),
    )
    .into()
}

const CHOOSER: &str = ".root { padding: 10px; align-items: start; } \
                       select { transition: none; } menu item { transition: none; }";

#[test]
fn a_select_opens_its_options_and_picks_one() {
    let sheet = sheet(CHOOSER);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(chooser(root, None, false), Size::new(300.0, 200.0), 1.0);
    assert!(ui.find(&sheet, "select > menu").is_empty());
    ui.click_on(&sheet, "select");
    let select = ui.find(&sheet, "select")[0];
    let items = ui.find(&sheet, "select > menu > item");
    assert_eq!(items.len(), 3);
    // Under it, as wide at least.
    let list = ui.find(&sheet, "select > menu")[0];
    assert!(list.y >= select.y + select.height, "{list:?} {select:?}");
    assert!(list.width >= select.width);
    ui.click(items[1].center());
    assert_eq!(ui.messages, [Message::Value(1.0)]);
    assert!(ui.find(&sheet, "select > menu").is_empty());
    // Escape closes it, and so does a press elsewhere.
    ui.click_on(&sheet, "select");
    ui.key(Named::Escape);
    assert!(ui.find(&sheet, "select > menu").is_empty());
    ui.click_on(&sheet, "select");
    ui.click(Point::new(290.0, 190.0));
    assert!(ui.find(&sheet, "select > menu").is_empty());
    assert_eq!(ui.messages.len(), 1);
    // Chosen, it's the options' one `.selected`: labels alone, no mark.
    ui.rebuild(chooser(root, Some(1), false));
    ui.click_on(&sheet, "select");
    assert_eq!(ui.find(&sheet, "select > menu > item.selected").len(), 1);
    assert_eq!(
        ui.find(&sheet, "select > menu > item.selected:nth-child(2)")
            .len(),
        1
    );
    assert!(ui.find(&sheet, "select > menu > item > check").is_empty());
}

#[test]
fn up_and_down_choose_on_a_closed_select() {
    let sheet = sheet(CHOOSER);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(chooser(root, Some(1), false), Size::new(300.0, 200.0), 1.0);
    ui.key(Named::Tab);
    ui.key(Named::ArrowDown);
    ui.key(Named::ArrowDown);
    ui.key(Named::ArrowUp);
    // Enter opens it.
    ui.key(Named::Enter);
    assert_eq!(ui.find(&sheet, "select > menu > item").len(), 3);
    assert_eq!(ui.messages, [Message::Value(2.0), Message::Value(1.0)]);
}

#[test]
fn a_disabled_select_stays_shut() {
    let sheet = sheet(CHOOSER);
    let root = sheet.root("root", 1.0);
    let mut ui = Harness::new(chooser(root, Some(0), true), Size::new(300.0, 200.0), 1.0);
    ui.click_on(&sheet, "select");
    ui.key(Named::Tab);
    ui.key(Named::ArrowDown);
    assert!(ui.find(&sheet, "select > menu").is_empty());
    assert!(ui.messages.is_empty());
}

#[test]
fn an_app_can_show_a_selects_options_itself() {
    let sheet = sheet(CHOOSER);
    let root = sheet.root("root", 1.0);
    let view = |opened: bool| -> Element<'_, Message> {
        crate::column([
            crate::select(["one", "two"], None, |i| Message::Value(i as f32))
                .on_open(opened, |at| match at {
                    Some(_) => Message::Pressed("open"),
                    None => Message::Pressed("close"),
                })
                .into(),
        ])
        .at(root)
        .into()
    };
    let mut ui = Harness::new(view(false), Size::new(300.0, 200.0), 1.0);
    ui.click_on(&sheet, "select");
    assert!(ui.find(&sheet, "select > menu").is_empty());
    ui.rebuild(view(true));
    ui.click_on(&sheet, "select");
    assert_eq!(
        ui.messages,
        [Message::Pressed("open"), Message::Pressed("close")]
    );
}

#[test]
fn widgets_without_nodes_find_their_place() {
    use crate::text;
    let sheet = sheet(
        "label { width: 10px; height: 10px; } \
         button.primary > label { width: 30px; } \
         .root > label:last-child { width: 20px; }",
    );
    let root = sheet.root("root", 1.0);
    // The class comes after the children are built: they see it all the
    // same, found only when the runtime reaches them.
    let view = || -> Element<'_, Message> {
        crate::row![
            text("a"),
            crate::button![text("b")]
                .class("primary")
                .on_press(Message::Pressed("b")),
            text("c"),
        ]
        .at(root)
        .into()
    };
    let mut ui = Harness::new(view(), Size::new(100.0, 10.0), 1.0);
    let widths = |ui: &mut Harness<'_, Message>, selector| {
        ui.find(&sheet, selector)
            .iter()
            .map(|r| r.width)
            .collect::<Vec<_>>()
    };
    assert_eq!(widths(&mut ui, ".root > label"), [10.0, 20.0]);
    assert_eq!(widths(&mut ui, ".root > button.primary > label"), [30.0]);
    ui.rebuild(view());
    assert_eq!(widths(&mut ui, "button.primary > label"), [30.0]);
    ui.click_on(&sheet, "button.primary");
    assert_eq!(ui.messages, [Message::Pressed("b")]);
}
