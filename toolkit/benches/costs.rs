//! What the toolkit costs, headless: a theme's load, and for fixed trees
//! (a bar, a menu, a long list) the view, the diff and layout of a
//! rebuild, an update, a frame. Numbers to compare before and after a
//! change (UI.md: "Measured, before and after a change"):
//!
//!     cargo bench -p tk --features testing

use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use iced::{Point, Size};
use tk::testing::Harness;
use tk::{Element, IconFile, Measure, Node, Scheme, Sheet, Source};

/// The mean of `iterations` runs of `f`, after a tenth of them to warm.
fn time(name: &str, iterations: u32, mut f: impl FnMut()) {
    for _ in 0..iterations / 10 + 1 {
        f();
    }
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let each = start.elapsed() / iterations;
    println!("{name:<44} {:>10.2} µs", each.as_secs_f64() * 1e6);
}

const CSS: &str = "bar { height: 32px; padding: 0 6px; gap: 2px; align-items: center; \
                   background: #222; color: #eee; } \
                   gadget { padding: 2px 8px; gap: 6px; align-items: center; border-radius: 4px; \
                   transition: background-color 120ms; } \
                   gadget:hover { background: #444; } gadget > icon { height: 16px; } \
                   menu { max-width: 260px; padding: 4px; border: 1px solid #555; \
                   border-radius: 6px; box-shadow: 0 3px 8px rgba(0, 0, 0, 0.3); } \
                   item { gap: 8px; padding: 4px 8px; align-items: center; } \
                   item:hover { background: #333; } item > icon { height: 16px; } \
                   item > label { width: fill; white-space: nowrap; text-overflow: ellipsis; } \
                   list { gap: 1px; } row > label { padding: 2px 4px; }";

fn star() -> IconFile {
    IconFile::Svg {
        handle: iced::advanced::svg::Handle::from_memory(
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 1l2 5h5l-4 3 2 5-5-3-5 3 2-5-4-3h5z"/></svg>"##
                .as_slice(),
        ),
        symbolic: true,
    }
}

/// A bar: twelve gadgets, an icon and a label each.
fn bar<'a>(root: Node<'a>, icon: &IconFile) -> Element<'a, ()> {
    tk::row(
        root,
        (0..12).map(|i| {
            let gadget = root.child("gadget");
            tk::button(
                gadget,
                [
                    tk::icon(gadget.child("icon"), icon.clone()).into(),
                    tk::text(gadget.child("label"), format!("Gadget {i}")).into(),
                ],
            )
            .on_press(())
            .into()
        }),
    )
    .into()
}

/// A menu: twenty items, an icon, a label and a shortcut each.
fn menu<'a>(root: Node<'a>, icon: &IconFile) -> Element<'a, ()> {
    tk::menu(
        root,
        (0..20).map(|i| {
            tk::MenuItem::new(format!("Menu item number {i}"))
                .icon(icon.clone())
                .shortcut("Ctrl+K")
                .on_press(())
        }),
    )
}

/// A list: five hundred rows of a label.
fn list(root: Node<'_>) -> Element<'_, ()> {
    tk::column(
        root,
        (0..500).map(|i| {
            let row = root.child("row");
            tk::row(
                row,
                [tk::text(row.child("label"), format!("Row {i}")).into()],
            )
            .into()
        }),
    )
    .into()
}

fn tree<'a>(
    name: &str,
    sheet: &'a Sheet,
    kind: &str,
    size: Size,
    view: impl Fn(Node<'a>) -> Element<'a, ()>,
    iterations: u32,
) {
    let root = sheet.root(kind, 1.0);
    time(&format!("{name}: view"), iterations, || {
        black_box(view(root));
    });
    let mut ui = Harness::new(view(root), size, 1.0);
    time(
        &format!("{name}: rebuild (view, diff, layout)"),
        iterations,
        || {
            ui.rebuild(view(root));
        },
    );
    let mut x = 0.0;
    time(
        &format!("{name}: update (the pointer moving)"),
        iterations,
        || {
            x = (x + 37.0) % size.width;
            ui.move_to(Point::new(x, size.height / 2.0));
        },
    );
    let now = Instant::now();
    time(&format!("{name}: frame (update, draw)"), iterations, || {
        ui.frame(now);
    });
}

fn main() {
    let gallery = include_str!("../examples/gallery.css");
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/examples"));
    time("sheet: base and the gallery's, loaded", 200, || {
        black_box(
            Sheet::load(
                &[Source {
                    name: "gallery.css",
                    text: gallery,
                    dir: Some(dir),
                }],
                Scheme::Light,
            )
            .expect("loads"),
        );
    });
    let (sheet, _) = Sheet::load(
        &[Source {
            name: "bench.css",
            text: CSS,
            dir: None,
        }],
        Scheme::Light,
    )
    .expect("loads");
    let icon = star();
    tree(
        "bar",
        &sheet,
        "bar",
        Size::new(1400.0, 32.0),
        |r| bar(r, &icon),
        2000,
    );
    tree(
        "menu",
        &sheet,
        "menu",
        Size::new(300.0, 700.0),
        |r| menu(r, &icon),
        2000,
    );
    tree("list", &sheet, "list", Size::new(300.0, 12000.0), list, 200);

    let mut measure = Measure::new();
    let root = sheet.root("menu", 1.0);
    let max = Size::new(800.0, 1000.0);
    time("menu: measured, its tree kept", 2000, || {
        black_box(measure.size("menu", menu(root, &icon), max));
    });
    let mut fresh = 0u64;
    time("menu: measured, a fresh tree", 200, || {
        fresh += 1;
        black_box(measure.size(("fresh", fresh), menu(root, &icon), max));
        measure.forget(("fresh", fresh));
    });
    println!("{:?}", sheet.stats());
}
