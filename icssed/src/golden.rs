//! Golden images: scenes drawn headlessly (tiny-skia, with the fonts of
//! `tests/fonts/` so the system's don't count) and compared pixel by
//! pixel with their references in `tests/golden/`, at scale 1 and 1.5.
//!
//! A reference missing is written. A scene that differs fails, and
//! leaves what it drew in `target/golden/`; after a change meant to
//! show, `ICSSED_GOLDEN=bless cargo test -p icssed golden` writes them all again.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::Instant;

use iced::advanced::graphics::text::font_system;
use iced::keyboard::key::Named;
use iced::{Color, Point, Size};

use crate::testing::{Harness, Pixels};
use crate::{Element, IconFile, Node, Scheme, Sheet, Source};

const FONTS: [&[u8]; 2] = [
    include_bytes!("../tests/fonts/NotoSansDisplay-Regular.ttf"),
    include_bytes!("../tests/fonts/NotoSansDisplay-Bold.ttf"),
];

/// What every scene starts from: the fonts of the tests, a white page.
const PAGE: &str = "scene { background: white; padding: 8px; gap: 8px; \
                    font-family: \"Noto Sans Display\"; font-size: 13px; color: #222; } \
                    button { transition: none; }";

fn fonts() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let mut system = font_system().write().unwrap_or_else(|e| e.into_inner());
        for font in FONTS {
            system.load_font(Cow::Borrowed(font));
        }
    });
}

fn here() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// `name`'s scene, `view` from its root, `act` on it before the frame,
/// at both scales: drawn, then compared with its references.
fn scene(
    name: &str,
    css: &str,
    dir: Option<&Path>,
    size: Size,
    view: impl for<'a> Fn(Node<'a>) -> Element<'a, ()>,
    act: impl Fn(&mut Harness<'_, ()>, &Sheet),
) {
    fonts();
    for scale in [1.0, 1.5] {
        let css = format!("{PAGE} {css}");
        let (sheet, warnings) = Sheet::load(
            &[Source {
                name,
                text: &css,
                dir,
            }],
            Scheme::Light,
        )
        .expect("loads");
        assert!(warnings.is_empty(), "{warnings:?}");
        let root = sheet.root("scene", scale);
        let mut ui = Harness::new(view(root), size, scale);
        ui.frame(Instant::now());
        act(&mut ui, &sheet);
        ui.frame(Instant::now());
        compare(&format!("{name}@{scale}x"), &ui.pixels(Color::WHITE));
    }
}

fn compare(name: &str, drawn: &Pixels) {
    let reference = here().join("tests/golden").join(format!("{name}.png"));
    let bless = std::env::var("ICSSED_GOLDEN").is_ok_and(|v| v == "bless");
    if bless || !reference.exists() {
        drawn.save_png(&reference).expect("the reference written");
        return;
    }
    let expected = ::image::open(&reference)
        .expect("the reference read")
        .into_rgba8();
    let differing = if expected.dimensions() != (drawn.width, drawn.height) {
        usize::MAX
    } else {
        expected
            .pixels()
            .zip(drawn.rgba.as_chunks::<4>().0)
            .filter(|(e, d)| e.0.iter().zip(d.iter()).any(|(a, b)| a.abs_diff(*b) > 2))
            .count()
    };
    if differing > 0 {
        let out: PathBuf = here().join("../target/golden");
        std::fs::create_dir_all(&out).expect("target/golden");
        let actual = out.join(format!("{name}.png"));
        drawn.save_png(&actual).expect("what was drawn written");
        panic!(
            "{name}: {} differ from {}; what was drawn is in {}; \
             ICSSED_GOLDEN=bless makes it the reference",
            if differing == usize::MAX {
                "the sizes".to_string()
            } else {
                format!("{differing} pixels")
            },
            reference.display(),
            actual.display(),
        );
    }
}

/// A `kind` box with a label in it.
fn boxed<'a>(kind: &str, class: &str, label: &'a str) -> Element<'a, ()> {
    crate::row![crate::text(label)]
        .kind(kind)
        .class_if(class, !class.is_empty())
        .into()
}

fn star() -> IconFile {
    IconFile::Svg {
        handle: iced::advanced::svg::Handle::from_memory(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 1l2.1 4.5 4.9.6-3.6 3.4.9 4.9L8 12l-4.3 2.4.9-4.9L1 6.1l4.9-.6z" fill="#000"/></svg>"##
                .as_slice(),
        ),
        symbolic: true,
    }
}

#[test]
fn golden_boxes() {
    scene(
        "boxes",
        "b { width: 40px; height: 28px; } \
         b.fill { background: #4a7bd6; } \
         b.border { border: 1px solid #555; } \
         b.radius { background: #4a7bd6; border-radius: 8px; } \
         b.shadow { background: white; border-radius: 6px; box-shadow: 0 2px 5px rgba(0, 0, 0, 0.4); } \
         b.inset { background: #ddd; box-shadow: inset 0 -3px #4a7bd6; } \
         b.sides { border-left: 4px solid #4a7bd6; border-bottom: 1px solid #555; } \
         b.four { border-style: solid; border-width: 4px 2px 6px 3px; \
                  border-color: #e05050 #50a050 #4070e0 #e0a030; border-radius: 10px; } \
         b.ring { border: 2px solid #4a7bd6; border-left-width: 6px; border-radius: 9px; } \
         b.outline { border: 1px solid #999; border-radius: 5px; \
                     outline: 2px solid #4a7bd6; outline-offset: 2px; } \
         b.faded { background: #4a7bd6; opacity: 0.5; } \
         b.scaled { background: #4a7bd6; transform: scale(0.75); }",
        None,
        Size::new(560.0, 48.0),
        |root| {
            crate::row(
                [
                    "fill", "border", "radius", "shadow", "inset", "sides", "four", "ring",
                    "outline", "faded", "scaled",
                ]
                .map(|class| crate::row([]).kind("b").class(class).into()),
            )
            .at(root)
            .into()
        },
        |_, _| {},
    );
}

#[test]
fn golden_text() {
    scene(
        "text",
        "label.big { font-size: 20px; } label.bold { font-weight: bold; } \
         label.caps { text-transform: uppercase; } \
         box { border: 1px solid #999; padding: 4px; } box.wrap { width: 180px; } \
         box.cut { width: 120px; } box.cut label { white-space: nowrap; text-overflow: ellipsis; } \
         aligned { width: 180px; border: 1px solid #999; padding: 4px; gap: 2px; } \
         label.center { text-align: center; } label.right { text-align: right; } \
         pair { height: 24px; gap: 6px; padding: 0 6px; align-items: center; background: #eee; } \
         pair > icon { height: 14px; } pair.untrimmed label { text-box: normal; }",
        None,
        Size::new(240.0, 260.0),
        |root| {
            let long = "A text long enough to wrap at its box's width, words kept whole.";
            let pair = |untrimmed: bool, text: &'static str| {
                crate::row![crate::icon(star()), crate::text(text)]
                    .kind("pair")
                    .class_if("untrimmed", untrimmed)
            };
            crate::column![
                crate::text("Large, 20px").class("big"),
                crate::text("Bold").class("bold"),
                crate::text("upper case").class("caps"),
                boxed("box", "wrap", long),
                boxed("box", "cut", long),
                crate::column![
                    crate::text("Left"),
                    crate::text("Centred").class("center"),
                    crate::text("Right").class("right"),
                ]
                .kind("aligned"),
                pair(false, "Trimmed: Centred"),
                pair(true, "Untrimmed: Line Box"),
            ]
            .at(root)
            .into()
        },
        |_, _| {},
    );
}

fn buttons(root: Node<'_>) -> Element<'_, ()> {
    crate::focus_scope(
        crate::row(
            [
                ("", "Default", false),
                ("flat", "Flat", false),
                ("primary", "Primary", false),
                ("danger", "Danger", false),
                ("", "Disabled", true),
            ]
            .map(|(class, label, disabled)| {
                crate::button![crate::icon(star()), crate::text(label)]
                    .class_if(class, !class.is_empty())
                    .disabled_if(disabled)
                    .on_press(())
                    .into()
            }),
        )
        .at(root),
    )
    .into()
}

const BUTTONS: &str = "button > icon { height: 14px; }";

#[test]
fn golden_buttons() {
    scene(
        "buttons",
        BUTTONS,
        None,
        Size::new(470.0, 44.0),
        buttons,
        |_, _| {},
    );
}

#[test]
fn golden_buttons_hovered_and_focused() {
    scene(
        "buttons-states",
        BUTTONS,
        None,
        Size::new(470.0, 44.0),
        buttons,
        |ui, sheet| {
            // The focus on the first, the pointer over the third.
            ui.key(Named::Tab);
            let primary = ui.find(sheet, "button.primary")[0];
            ui.move_to(Point::new(primary.center_x(), primary.center_y()));
        },
    );
}

#[test]
fn golden_flex() {
    scene(
        "flex",
        "track { width: fill; gap: 6px; padding: 6px; border: 1px solid #999; } \
         track.center { justify-content: center; } track.align { align-items: center; } \
         cell { padding: 4px 8px; background: #ddd; } cell.fill { width: fill; } \
         cell.fixed { width: 60px; } cell.tall { height: 40px; } \
         list { max-width: 200px; padding: 4px; border: 1px solid #999; border-radius: 6px; \
                box-shadow: 0 3px 8px rgba(0, 0, 0, 0.25); background: white; } \
         entry { gap: 8px; padding: 5px 8px; align-items: center; } entry > icon { height: 14px; } \
         entry > label { width: fill; white-space: nowrap; text-overflow: ellipsis; } \
         shortcut { color: #888; }",
        None,
        Size::new(320.0, 230.0),
        |root| {
            let track = |class: &str, cells: &[&'static str]| {
                crate::row(cells.iter().map(|&cell| boxed("cell", cell, cell)))
                    .kind("track")
                    .class(class)
            };
            let items = [
                ("Open", "Ctrl+O"),
                ("A label far too long for its menu", "Ctrl+L"),
            ];
            crate::column![
                track("plain", &["fill", "fixed", "fill"]),
                track("center", &["fixed", "fixed"]),
                track("align", &["tall", "fixed"]),
                crate::column(items.map(|(label, key)| {
                    crate::row![
                        crate::icon(star()),
                        crate::text(label),
                        crate::text(key).kind("shortcut"),
                    ]
                    .kind("entry")
                    .into()
                }))
                .kind("list"),
            ]
            .at(root)
            .into()
        },
        |_, _| {},
    );
}

/// Images drawn at their own size only: tiny-skia (iced 0.14) places a
/// stretched image on a grid as coarse as its stretch, and doesn't round
/// an image's corners. Stretched ones (9-patches, `object-fit`) are seen
/// in the demo, on wgpu; their slicing is the style tests'.
#[test]
fn golden_images() {
    // The theme's images, made the same each time.
    let dir = std::env::temp_dir().join(format!("icssed-golden-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    let dots = ::image::RgbaImage::from_fn(8, 8, |x, y| {
        if (2..6).contains(&x) && (2..6).contains(&y) {
            ::image::Rgba([70, 110, 220, 200])
        } else {
            ::image::Rgba([0, 0, 0, 0])
        }
    });
    dots.save(dir.join("dots.png")).expect("dots");
    let photo = ::image::RgbaImage::from_fn(64, 32, |x, y| {
        ::image::Rgba([(x * 4) as u8, (y * 8) as u8, 180, 255])
    });
    let (width, height) = photo.dimensions();
    let photo = iced::advanced::image::Handle::from_rgba(width, height, photo.into_raw());
    scene(
        "images",
        "icon { height: 24px; color: #4a7bd6; } \
         tiled { width: 60px; height: 40px; background: url(dots.png) #ddd; } \
         picture { width: 64px; height: 32px; }",
        Some(&dir),
        Size::new(200.0, 60.0),
        move |root| {
            crate::row![
                crate::icon(star()),
                crate::row([]).kind("tiled"),
                crate::image(photo.clone()).kind("picture"),
            ]
            .at(root)
            .into()
        },
        |_, _| {},
    );
}

fn controls(root: Node<'_>) -> Element<'_, ()> {
    crate::focus_scope(
        crate::column![
            crate::row![
                crate::toggle(false, |_| ()).switch(),
                crate::toggle(true, |_| ()).switch(),
                crate::toggle(true, |_| ()).disabled_if(true).switch(),
            ]
            .kind("line"),
            crate::row![
                crate::slider(0.0..=100.0, 30.0, |_| ()),
                crate::slider(0.0..=1.0, 0.7, |_| ()).disabled_if(true),
            ]
            .kind("line"),
            crate::row![
                crate::meter(0.6),
                crate::meter(0.42).class("gauge").label("CPU 42%"),
            ]
            .kind("line"),
        ]
        .at(root),
    )
    .into()
}

const CONTROLS: &str = "toggle, toggle > switch, toggle > switch > knob, slider > knob { \
                        transition: none; } \
                        line { gap: 10px; align-items: center; } \
                        meter.gauge { width: 100px; height: 16px; }";

#[test]
fn golden_controls() {
    scene(
        "controls",
        CONTROLS,
        None,
        Size::new(360.0, 90.0),
        controls,
        |_, _| {},
    );
}

#[test]
fn golden_controls_focused_and_dragged() {
    scene(
        "controls-states",
        CONTROLS,
        None,
        Size::new(360.0, 90.0),
        controls,
        |ui, sheet| {
            // The slider's knob pressed, then the focus on the second
            // toggle (a press takes the focus away, a key brings it).
            let knob = ui.find(sheet, "slider > knob")[0];
            ui.move_to(knob.center());
            ui.press();
            ui.key(Named::Tab);
            ui.key(Named::Tab);
        },
    );
}

/// The four looks of a toggle, off, on and disabled; the focus on the
/// check box.
#[test]
fn golden_toggles() {
    scene(
        "toggles",
        "toggle, toggle > check, toggle > check > mark, toggle > radio, toggle > radio > dot, \
         toggle > switch, toggle > switch > knob { transition: none; } \
         column { gap: 8px; } line { gap: 10px; align-items: center; }",
        None,
        Size::new(320.0, 140.0),
        |root| {
            let looks = |look: fn(crate::Toggle<'_, ()>) -> crate::Toggle<'_, ()>| {
                crate::row(
                    [
                        (false, false, "Off"),
                        (true, false, "On"),
                        (true, true, "Off limits"),
                    ]
                    .map(|(on, disabled, text)| {
                        look(crate::toggle(on, |_| ()).disabled_if(disabled).label(text)).into()
                    }),
                )
                .kind("line")
            };
            crate::focus_scope(
                crate::column![
                    looks(|t| t),
                    looks(|t| t.check()),
                    looks(|t| t.radio()),
                    looks(|t| t.switch())
                ]
                .at(root),
            )
            .into()
        },
        |ui, _| {
            // The focus on the first check box: past the three buttons
            // (the disabled one takes none).
            for _ in 0..3 {
                ui.key(Named::Tab);
            }
        },
    );
}

#[test]
fn golden_inputs() {
    scene(
        "inputs",
        "column { gap: 6px; } input { transition: none; }",
        None,
        Size::new(230.0, 160.0),
        |root| {
            crate::column![
                crate::input("Placeholder", "").on_input(|_| ()),
                crate::input("Search", "A value")
                    .on_input(|_| ())
                    .leading(crate::icon(star())),
                crate::input("Password", "secret")
                    .on_input(|_| ())
                    .secure(true)
                    .trailing(crate::toggle(false, |_| ()).class("peek").icon(star())),
                crate::input("Disabled", "")
                    .disabled_if(true)
                    .on_input(|_| ()),
            ]
            .at(root)
            .into()
        },
        |_, _| {},
    );
}

#[test]
fn golden_scroll() {
    scene(
        "scroll",
        "scrolls { gap: 12px; align-items: start; } \
         scroll.list { width: 120px; max-height: 80px; } \
         scroll.strip { width: 150px; } \
         rows { padding: 4px 12px 4px 4px; gap: 6px; border: 1px solid #999; } \
         cells { gap: 4px; padding: 0 0 10px 0; } cell { padding: 4px 8px; background: #ddd; }",
        None,
        Size::new(320.0, 100.0),
        |root| {
            let rows = crate::column(
                ["One", "Two", "Three", "Four", "Five", "Six", "Seven"]
                    .map(|label| crate::text(label).into()),
            )
            .kind("rows");
            let cells = crate::row(
                ["A", "B", "C", "D", "E", "F", "G", "H"].map(|label| boxed("cell", "", label)),
            )
            .kind("cells");
            crate::column![
                crate::row![
                    crate::scroll(rows).class("list"),
                    crate::scroll(cells).class("strip").horizontal(),
                ]
                .kind("scrolls"),
            ]
            .at(root)
            .into()
        },
        |ui, _| {
            // The list a little down.
            ui.move_to(Point::new(48.0, 48.0));
            ui.send(&[iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -20.0 },
            })]);
            ui.move_to(Point::new(300.0, 90.0));
        },
    );
}

#[test]
fn golden_menu() {
    scene(
        "menu",
        // Marks the test fonts have.
        "menu { max-width: 220px; border: 1px solid #999; border-radius: 6px; \
                box-shadow: 0 3px 8px rgba(0, 0, 0, 0.25); background: white; } \
         menu item, menu item > check, menu item > check > mark, menu item > radio, \
         menu item > radio > dot { transition: none; } \
         menu item.submenu > arrow { content: \">\"; } \
         menu item.expanded > arrow { content: \"v\"; }",
        None,
        Size::new(240.0, 320.0),
        |root| {
            use crate::MenuItem;
            crate::focus_scope(
                crate::column![crate::menu([
                    MenuItem::new("Open")
                        .icon(star())
                        .shortcut("Ctrl+O")
                        .on_press(()),
                    MenuItem::new("A label far too long for its menu")
                        .shortcut("Ctrl+L")
                        .on_press(()),
                    MenuItem::new("Disabled"),
                    MenuItem::separator(),
                    MenuItem::new("Checked").check(true).on_press(()),
                    MenuItem::new("Unchecked").check(false).on_press(()),
                    MenuItem::new("Radio on").radio(true).on_press(()),
                    MenuItem::new("Radio off").radio(false).on_press(()),
                    MenuItem::separator(),
                    MenuItem::new("More")
                        .on_press(())
                        .submenu(true, [MenuItem::new("Inside").icon(star()).on_press(())]),
                    MenuItem::new("Folded").on_press(()).submenu(false, []),
                ])]
                .at(root),
            )
            .into()
        },
        |ui, _| {
            // The focus on the second item.
            ui.key(Named::Tab);
            ui.key(Named::ArrowDown);
        },
    );
}

#[test]
fn golden_tabs() {
    scene(
        "tabs",
        "tabs { width: 260px; } tabs > tab { transition: none; }",
        None,
        Size::new(280.0, 44.0),
        |root| {
            crate::focus_scope(
                crate::column![crate::tabs(
                    ["Overview", "Details", "Settings"]
                        .map(|title| crate::Tab::new(title, ()).active(title == "Details")),
                )]
                .at(root),
            )
            .into()
        },
        |ui, sheet| {
            // The focus on the first, the pointer over the third.
            ui.key(Named::Tab);
            let third = ui.find(sheet, "tab:nth-child(3)")[0];
            ui.move_to(third.center());
        },
    );
}

#[test]
fn golden_calendar() {
    scene(
        "calendar",
        "calendar day { transition: none; } \
         calendar > header > button.previous > arrow { content: \"<\"; } \
         calendar > header > button.next > arrow { content: \">\"; }",
        None,
        Size::new(240.0, 300.0),
        |root| {
            crate::column![
                crate::calendar(
                    crate::Month::new(2026, 10),
                    "October 2026",
                    ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"],
                )
                .today(2026, 10, 10)
                .selected(2026, 10, 15)
                .on_previous(())
                .on_next(())
                .on_select(|_| ())
            ]
            .at(root)
            .into()
        },
        |ui, sheet| {
            // The pointer over the 20th.
            let day = ui.find(sheet, "week:nth-child(4) > day:nth-child(2)")[0];
            ui.move_to(day.center());
        },
    );
}

#[test]
fn golden_graphs() {
    scene(
        "graphs",
        "graph { width: 140px; height: 60px; } graph > series.second { stroke: #c33; fill: none; } \
         sparkline { width: 70px; height: 20px; justify-content: center; align-items: center; \
                     background: #ddd; } \
         sparkline > series { stroke: #4a7bd6; fill: rgba(74, 123, 214, 0.3); }",
        None,
        Size::new(250.0, 76.0),
        |root| {
            let wave = |phase: f32, n: usize| {
                (0..n).map(move |i| 50.0 + 40.0 * (i as f32 / 5.0 + phase).sin())
            };
            crate::row![
                crate::graph(40)
                    .series(wave(0.0, 40))
                    .series_class("second", wave(1.5, 25))
                    .max(100.0)
                    .label("100"),
                crate::graph(20)
                    .kind("sparkline")
                    .series(wave(0.5, 20))
                    .max(100.0)
                    .label("42%"),
            ]
            .at(root)
            .into()
        },
        |_, _| {},
    );
}

#[test]
fn golden_tooltip() {
    scene(
        "tooltip",
        "scene { padding: 8px 60px; align-items: start; } tooltip { animation: none; } \
         button { transition: none; }",
        None,
        Size::new(220.0, 70.0),
        |root| {
            crate::column![crate::tooltip(
                crate::button![crate::text("Rest here")].on_press(()),
                "A tip, under what it's about",
            )]
            .at(root)
            .into()
        },
        |ui, sheet| {
            let button = ui.find(sheet, "button")[0];
            ui.move_to(button.center());
            ui.frame(std::time::Instant::now() + std::time::Duration::from_secs(1));
        },
    );
}

#[test]
fn golden_select() {
    scene(
        "select",
        "scene { align-items: start; } select { transition: none; width: 150px; } \
         menu item { transition: none; } select > arrow { content: \"v\"; }",
        None,
        Size::new(220.0, 160.0),
        |root| {
            crate::focus_scope(
                crate::column![crate::select(
                    ["Follow the system", "Light", "Dark"],
                    Some(2),
                    |_| (),
                )]
                .at(root),
            )
            .into()
        },
        |ui, sheet| {
            ui.click_on(sheet, "select");
            let light = ui.find(sheet, "select > menu > item")[1];
            ui.move_to(light.center());
        },
    );
}
