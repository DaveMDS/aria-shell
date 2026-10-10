//! Golden images: scenes drawn headlessly (tiny-skia, with the fonts of
//! `tests/fonts/` so the system's don't count) and compared pixel by
//! pixel with their references in `tests/golden/`, at scale 1 and 1.5.
//!
//! A reference missing is written. A scene that differs fails, and
//! leaves what it drew in `target/golden/`; after a change meant to
//! show, `TK_GOLDEN=bless cargo test -p tk golden` writes them all again.

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
    let bless = std::env::var("TK_GOLDEN").is_ok_and(|v| v == "bless");
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
             TK_GOLDEN=bless makes it the reference",
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

/// A box with a label in it.
fn boxed<'a>(parent: Node<'a>, kind: &str, class: &str, label: &'a str) -> Element<'a, ()> {
    let node = parent.child(kind).class_if(class, !class.is_empty());
    crate::row(node, [crate::text(node.child("label"), label).into()]).into()
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
                root,
                [
                    "fill", "border", "radius", "shadow", "inset", "sides", "four", "ring",
                    "outline", "faded", "scaled",
                ]
                .map(|class| crate::row(root.child("b").class(class), []).into()),
            )
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
            let aligned = root.child("aligned");
            let pair = |untrimmed: bool, text: &'static str| -> Element<'_, ()> {
                let node = root.child("pair").class_if("untrimmed", untrimmed);
                crate::row(
                    node,
                    [
                        crate::icon(node.child("icon"), star()).into(),
                        crate::text(node.child("label"), text).into(),
                    ],
                )
                .into()
            };
            crate::column(
                root,
                [
                    crate::text(root.child("label").class("big"), "Large, 20px").into(),
                    crate::text(root.child("label").class("bold"), "Bold").into(),
                    crate::text(root.child("label").class("caps"), "upper case").into(),
                    boxed(root, "box", "wrap", long),
                    boxed(root, "box", "cut", long),
                    crate::column(
                        aligned,
                        [
                            crate::text(aligned.child("label"), "Left").into(),
                            crate::text(aligned.child("label").class("center"), "Centred").into(),
                            crate::text(aligned.child("label").class("right"), "Right").into(),
                        ],
                    )
                    .into(),
                    pair(false, "Trimmed: Centred"),
                    pair(true, "Untrimmed: Line Box"),
                ],
            )
            .into()
        },
        |_, _| {},
    );
}

fn buttons(root: Node<'_>) -> Element<'_, ()> {
    crate::focus_scope(crate::row(
        root,
        [
            ("", "Default", false),
            ("flat", "Flat", false),
            ("primary", "Primary", false),
            ("danger", "Danger", false),
            ("", "Disabled", true),
        ]
        .map(|(class, label, disabled)| {
            let node = root
                .child("button")
                .class_if(class, !class.is_empty())
                .disabled_if(disabled);
            crate::button(
                node,
                [
                    crate::icon(node.child("icon"), star()).into(),
                    crate::text(node.child("label"), label).into(),
                ],
            )
            .on_press(())
            .into()
        }),
    ))
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
         menu { max-width: 200px; padding: 4px; border: 1px solid #999; border-radius: 6px; \
                box-shadow: 0 3px 8px rgba(0, 0, 0, 0.25); background: white; } \
         item { gap: 8px; padding: 5px 8px; align-items: center; } item > icon { height: 14px; } \
         item > label { width: fill; white-space: nowrap; text-overflow: ellipsis; } \
         shortcut { color: #888; }",
        None,
        Size::new(320.0, 230.0),
        |root| {
            let track = |class: &str, cells: &[&'static str]| -> Element<'_, ()> {
                let node = root.child("track").class(class);
                crate::row(
                    node,
                    cells
                        .iter()
                        .map(|&cell| boxed(node, "cell", cell, cell))
                        .collect::<Vec<_>>(),
                )
                .into()
            };
            let menu = root.child("menu");
            let items = [
                ("Open", "Ctrl+O"),
                ("A label far too long for its menu", "Ctrl+L"),
            ];
            crate::column(
                root,
                [
                    track("plain", &["fill", "fixed", "fill"]),
                    track("center", &["fixed", "fixed"]),
                    track("align", &["tall", "fixed"]),
                    crate::column(
                        menu,
                        items.map(|(label, key)| {
                            let item = menu.child("item");
                            crate::row(
                                item,
                                [
                                    crate::icon(item.child("icon"), star()).into(),
                                    crate::text(item.child("label"), label).into(),
                                    crate::text(item.child("shortcut"), key).into(),
                                ],
                            )
                            .into()
                        }),
                    )
                    .into(),
                ],
            )
            .into()
        },
        |_, _| {},
    );
}

/// Images drawn at their own size only: tiny-skia (iced 0.14) places a
/// stretched image on a grid as coarse as its stretch, and doesn't round
/// an image's corners. Stretched ones (9-patches, `object-fit`) are seen
/// in the gallery, on wgpu; their slicing is the style tests'.
#[test]
fn golden_images() {
    // The theme's images, made the same each time.
    let dir = std::env::temp_dir().join(format!("tk-golden-{}", std::process::id()));
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
            crate::row(
                root,
                [
                    crate::icon(root.child("icon"), star()).into(),
                    crate::row(root.child("tiled"), []).into(),
                    crate::image(root.child("picture"), photo.clone()).into(),
                ],
            )
            .into()
        },
        |_, _| {},
    );
}
