use std::sync::Arc;

use iced::Color;

use super::*;

fn load_scheme(sources: &[(&str, &str)], scheme: Scheme) -> (Sheet, Vec<Warning>) {
    let sources: Vec<Source> = sources
        .iter()
        .map(|(name, text)| Source {
            name,
            text,
            dir: None,
        })
        .collect();
    Sheet::load(&sources, scheme).expect("loads")
}

fn load(css: &str) -> (Sheet, Vec<Warning>) {
    load_scheme(&[("t.css", css)], Scheme::Light)
}

/// The node of `path` (`menu > item.active > label`) under a root at
/// `scale`.
fn node<'a>(sheet: &'a Sheet, path: &str, scale: f32) -> Node<'a> {
    let mut steps = path.split(" > ");
    let mut parts = steps.next().expect("a root").split('.');
    let mut node = sheet.root(parts.next().expect("a kind"), scale);
    for class in parts {
        node = node.class(class);
    }
    for step in steps {
        let mut parts = step.split('.');
        node = node.child(parts.next().expect("a kind"));
        for class in parts {
            node = node.class(class);
        }
    }
    node
}

fn style(sheet: &Sheet, path: &str) -> Arc<Computed> {
    node(sheet, path, 1.0).base()
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

const RED: Color = Color::from_rgb(1.0, 0.0, 0.0);

/// What an element no rule colours inherits: the toolkit's own default.
fn plain(sheet: &Sheet) -> Color {
    sheet.root("unstyled", 1.0).base().color
}

#[test]
fn variables_follow_the_scheme_and_the_later_file() {
    let base = ":root { --a: red; --b: blue; } :root.dark { --a: lime; } \
                x { color: var(--a); background: var(--b); }";
    let theme = ":root { --b: yellow; } :root.dark { --b: purple; }";
    let (light, _) = load_scheme(&[("base", base), ("theme", theme)], Scheme::Light);
    let (dark, _) = load_scheme(&[("base", base), ("theme", theme)], Scheme::Dark);
    assert_eq!(style(&light, "x").color, RED);
    assert_eq!(style(&light, "x").background, rgb(255, 255, 0));
    assert_eq!(style(&dark, "x").color, rgb(0, 255, 0));
    assert_eq!(style(&dark, "x").background, rgb(128, 0, 128));
}

#[test]
fn a_later_plain_variable_beats_an_earlier_scheme_one() {
    let base = ":root.dark { --a: lime; } x { color: var(--a); }";
    let theme = ":root { --a: red; }";
    let (dark, _) = load_scheme(&[("base", base), ("theme", theme)], Scheme::Dark);
    assert_eq!(style(&dark, "x").color, RED);
}

#[test]
fn variables_fall_back_or_skip_the_declaration() {
    let (sheet, warnings) = load(
        ":root { --a: var(--b); --b: var(--a); } \
         x { color: var(--nope, teal); } y { color: var(--nope); } z { color: var(--a); }",
    );
    assert_eq!(style(&sheet, "x").color, rgb(0, 128, 128));
    assert_eq!(style(&sheet, "y").color, plain(&sheet));
    let messages: Vec<&str> = warnings.iter().map(|w| w.message.as_str()).collect();
    assert!(messages[0].contains("`--nope` isn't set"), "{messages:?}");
    assert!(
        messages[1].contains("refers back to itself"),
        "{messages:?}"
    );
}

#[test]
fn variables_are_set_in_the_root_only() {
    let (_, warnings) = load("x { --a: red; }");
    assert!(warnings[0].message.contains("`:root`"), "{warnings:?}");
}

#[test]
fn specificity_then_source_order() {
    let (sheet, _) = load(
        "#i { color: red; } .c { color: lime; } x { color: blue; } x { color: purple; } \
         x.c { background: red; } .c { background: blue; }",
    );
    assert_eq!(style(&sheet, "x").color, rgb(128, 0, 128));
    assert_eq!(style(&sheet, "x.c").color, rgb(0, 255, 0));
    assert_eq!(sheet.root("x", 1.0).class("c").id("i").base().color, RED);
    assert_eq!(style(&sheet, "x.c").background, RED);
}

#[test]
fn selectors_match_as_css_does() {
    let (sheet, _) = load(
        "a > b { color: red; } a c { color: blue; } [k=\"v\"] { background: red; } \
         * { padding: 1px; } :root { margin: 2px; }",
    );
    assert_eq!(style(&sheet, "a > b").color, RED);
    assert_eq!(style(&sheet, "a > x > b").color, plain(&sheet));
    assert_eq!(style(&sheet, "a > x > c").color, rgb(0, 0, 255));
    assert_eq!(sheet.root("x", 1.0).attr("k", "v").base().background, RED);
    assert_eq!(
        sheet.root("x", 1.0).attr("k", "w").base().background,
        Color::TRANSPARENT
    );
    assert_eq!(style(&sheet, "x > y").padding.left, 1.0);
    assert_eq!(style(&sheet, "x").margin.left, 2.0);
    assert_eq!(style(&sheet, "x > y").margin.left, 0.0);
}

#[test]
fn the_root_carries_the_scheme_as_a_class() {
    let css = "x.dark { color: red; } x > y.dark { color: blue; }";
    let (light, _) = load_scheme(&[("t", css)], Scheme::Light);
    let (dark, _) = load_scheme(&[("t", css)], Scheme::Dark);
    assert_eq!(style(&dark, "x").color, RED);
    assert_eq!(style(&light, "x").color, plain(&light));
    // Inherited, not matched: only the root carries it.
    assert_eq!(style(&dark, "x > y").color, RED);
}

#[test]
fn own_states_and_ancestors_states() {
    let (sheet, _) = load(
        "entry:hover { color: red; } list:hover entry { background: blue; } \
         list > entry:active:hover { padding: 3px; }",
    );
    let entry = node(&sheet, "list > entry", 1.0);
    let none = Ancestors::default();
    assert_eq!(entry.base().color, plain(&sheet));
    assert_eq!(entry.resolve(States::HOVER, &none).color, RED);
    assert_eq!(entry.resolve(States::ACTIVE, &none).padding.left, 0.0);
    assert_eq!(
        entry
            .resolve(States::ACTIVE | States::HOVER, &none)
            .padding
            .left,
        3.0
    );
    let hovered_list = none.push(0, States::HOVER);
    assert_eq!(
        entry.resolve(States::NONE, &hovered_list).background,
        rgb(0, 0, 255)
    );
    assert_eq!(
        entry.sensitivity(),
        Sensitivity {
            own: States::HOVER | States::ACTIVE,
            descendants: States::NONE,
        }
    );
    assert_eq!(
        node(&sheet, "list", 1.0).sensitivity(),
        Sensitivity {
            own: States::NONE,
            descendants: States::HOVER,
        }
    );
    assert_eq!(node(&sheet, "x", 1.0).sensitivity(), Sensitivity::default());
}

#[test]
fn ancestors_keep_their_depths() {
    let a = Ancestors::default()
        .push(0, States::HOVER)
        .push(2, States::ACTIVE);
    assert_eq!(a.get(0), States::HOVER);
    assert_eq!(a.get(1), States::NONE);
    assert_eq!(a.get(2), States::ACTIVE);
    assert_eq!(a.above(2), Ancestors::default().push(0, States::HOVER));
    // At rest, not kept; pushed again at a depth, replaced.
    assert_eq!(a.push(3, States::NONE), a);
    assert_eq!(a.push(2, States::HOVER).get(2), States::HOVER);
}

#[test]
fn positions_split_only_the_rows_the_sheet_tells_apart() {
    let (plain, _) = load("entry { color: red; }");
    let list = node(&plain, "list", 1.0);
    for i in 0..50 {
        assert_eq!(list.child_at("entry", i, 50).base().color, RED);
    }
    assert_eq!(plain.stats().nodes, 2);

    let (sheet, _) = load(
        "entry:first-child { color: red; } entry:last-child { color: blue; } \
         entry:nth-child(2n) { padding: 1px; }",
    );
    let list = node(&sheet, "list", 1.0);
    let at = |i| list.child_at("entry", i, 5).base();
    assert_eq!(at(0).color, RED);
    assert_eq!(at(4).color, rgb(0, 0, 255));
    assert_eq!(at(1).padding.left, 1.0);
    assert_eq!(at(2).padding.left, 0.0);
    assert_eq!(at(3).padding.left, 1.0);
}

#[test]
fn disabled_is_part_of_the_path() {
    let (sheet, _) = load("x:disabled y { color: red; } x:disabled { color: blue; }");
    let x = sheet.root("x", 1.0).disabled_if(true);
    assert!(x.is_disabled());
    assert!(!x.child("y").class("c").is_disabled());
    assert_eq!(x.base().color, rgb(0, 0, 255));
    assert_eq!(x.child("y").base().color, RED);
    assert_eq!(sheet.root("x", 1.0).child("y").base().color, plain(&sheet));
}

#[test]
fn inheritance_and_the_wide_keywords() {
    let (sheet, _) = load(
        "x { color: red; font-size: 20px; padding: 4px; } \
         x > a { padding: inherit; } x > b { color: initial; } x > c { color: unset; padding: unset; }",
    );
    let y = style(&sheet, "x > y");
    assert_eq!((y.color, y.font_size, y.padding.left), (RED, 20.0, 0.0));
    assert_eq!(style(&sheet, "x > a").padding.left, 4.0);
    assert_eq!(style(&sheet, "x > b").color, Color::BLACK);
    let c = style(&sheet, "x > c");
    assert_eq!((c.color, c.padding.left), (RED, 0.0));
}

#[test]
fn currentcolor_is_the_elements_own_colour() {
    let (sheet, _) = load(
        "x { border: 1px solid; background: currentcolor; color: red; \
             box-shadow: 0 1px 2px; } x > y { color: currentcolor; }",
    );
    let x = style(&sheet, "x");
    assert_eq!(x.border_color, [RED; 4]);
    assert_eq!(x.background, RED);
    assert_eq!(x.shadow.expect("a shadow").color, RED);
    assert_eq!(style(&sheet, "x > y").color, RED);
}

#[test]
fn border_widths_snap_down_to_device_pixels() {
    let (sheet, _) = load(
        "a { border: 1px solid; } b { border: 0.5px solid; } c { border: 1.5px solid; } \
         d { border-width: 4px; }",
    );
    let width = |kind: &str, scale: f32| sheet.root(kind, scale).base().border_width.top;
    assert_eq!(width("a", 1.0), 1.0);
    assert_eq!(width("a", 2.0), 1.0);
    assert!((width("a", 1.5) - 2.0 / 3.0).abs() < 1e-6);
    assert_eq!(width("b", 1.0), 1.0);
    assert_eq!(width("c", 1.0), 1.0);
    assert_eq!(width("c", 2.0), 1.5);
    // No style, no border.
    assert_eq!(width("d", 1.0), 0.0);
}

#[test]
fn shorthands_expand_as_in_css() {
    let (sheet, warnings) = load(
        "a { padding: 1px 2px 3px; margin: -4px 5px; gap: 6px 7px; border-radius: 1px 2px 3px 4px; } \
         b { border: red 2px solid; border-top: none; } \
         c { box-shadow: 0 2px 4px rgba(0, 0, 0, 0.5), inset 0 -3px blue; } \
         d { text-box: cap alphabetic; } e { text-box: trim-start; }",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let a = style(&sheet, "a");
    assert_eq!(
        (
            a.padding.top,
            a.padding.right,
            a.padding.bottom,
            a.padding.left
        ),
        (1.0, 2.0, 3.0, 2.0)
    );
    assert_eq!((a.margin.top, a.margin.right), (-4.0, 5.0));
    assert_eq!((a.row_gap, a.column_gap), (6.0, 7.0));
    assert_eq!(a.border_radius, [1.0, 2.0, 3.0, 4.0]);
    let b = style(&sheet, "b");
    assert_eq!(b.border_width.top, 0.0);
    assert_eq!(b.border_width.left, 2.0);
    assert_eq!(b.border_color[3], RED);
    let c = style(&sheet, "c");
    assert_eq!(c.shadow.expect("outer").blur, 4.0);
    assert_eq!(c.inset_shadow.expect("inset").y, -3.0);
    let d = style(&sheet, "d");
    assert_eq!(
        (d.text_box_trim, d.text_box_edge),
        (
            TextBoxTrim {
                start: true,
                end: true
            },
            TextBoxEdge {
                over: EdgeOver::Cap,
                under: EdgeUnder::Alphabetic
            }
        )
    );
    let e = style(&sheet, "e");
    assert_eq!(
        (e.text_box_trim, e.text_box_edge),
        (
            TextBoxTrim {
                start: true,
                end: false
            },
            TextBoxEdge::AUTO
        )
    );
}

#[test]
fn colours_in_every_syntax() {
    let (sheet, warnings) = load(
        "a { color: #f00; } b { color: #ff000080; } c { color: rgb(255 0 0 / 50%); } \
         d { color: rgba(0, 0, 255, .5); } e { color: hsl(120 100% 50%); } \
         f { color: rebeccapurple; } g { color: transparent; } h { color: hsla(240, 100%, 50%, 1); }",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(style(&sheet, "a").color, RED);
    assert!((style(&sheet, "b").color.a - 128.0 / 255.0).abs() < 1e-3);
    assert_eq!(
        style(&sheet, "c").color,
        Color::from_rgba(1.0, 0.0, 0.0, 0.5)
    );
    assert_eq!(
        style(&sheet, "d").color,
        Color::from_rgba(0.0, 0.0, 1.0, 0.5)
    );
    assert_eq!(style(&sheet, "e").color, rgb(0, 255, 0));
    assert_eq!(style(&sheet, "f").color, rgb(102, 51, 153));
    assert_eq!(style(&sheet, "g").color, Color::TRANSPARENT);
    assert_eq!(style(&sheet, "h").color, rgb(0, 0, 255));
}

#[test]
fn warnings_say_where_and_why() {
    let (_, warnings) = load(
        "a { colr: red; width: 10em; padding: -1px; border: dotted 1px; }\n\
         b + c { color: red; }\n\
         ok, bad::x { color: red; }\n\
         @media screen { x { color: red; } }\n\
         d { color: red !important; e { color: red; } }\n",
    );
    let shown: Vec<String> = warnings.iter().map(|w| w.to_string()).collect();
    let expected = [
        "t.css:1:5: `colr`: unknown property `colr`, skipped",
        "t.css:1:16: `width`: `em` isn't a unit here: lengths are px, skipped",
        "t.css:1:29: `padding`: a negative length isn't allowed here, skipped",
        "t.css:1:44: `border`: `dotted` borders aren't supported: `solid` or `none`, skipped",
        "t.css:2:1: the `+` combinator isn't supported: `b + c` skipped",
        "t.css:3:1: pseudo-elements (`::`) aren't supported: `ok, bad::x` skipped",
        "t.css:4:1: `@media` isn't supported: `@media screen` skipped",
        "t.css:5:5: `color: red !important;`: `!important` isn't supported, skipped",
        "t.css:5:28: `e { color: red; }`: nested rules aren't supported, skipped",
    ];
    assert_eq!(shown, expected);
}

#[test]
fn syntax_errors_reject_the_file() {
    let error = |css: &str| {
        Sheet::load(
            &[Source {
                name: "t",
                text: css,
                dir: None,
            }],
            Scheme::Light,
        )
        .err()
        .map(|e| e.to_string())
    };
    assert_eq!(
        error("a { color: red; ").as_deref(),
        Some("t:1:3: `{` is never closed")
    );
    assert_eq!(
        error("a { color: rgb(1, 2, 3; }").as_deref(),
        Some("t:1:25: `}` where the `(` opened at 1:15 is still open")
    );
    assert_eq!(
        error("a { } }").as_deref(),
        Some("t:1:7: `}` closes nothing")
    );
    assert_eq!(
        error("/* never").as_deref(),
        Some("t:1:1: a comment is never ended")
    );
    assert_eq!(
        error("a { }\nb c").as_deref(),
        Some("t:2:1: `b c` has no `{ }` block")
    );
    // A bracket in a string or a comment doesn't count.
    assert_eq!(error("a { font-family: \"{(\"; } /* } */"), None);
}

#[test]
fn line_heights_on_device_pixels() {
    let (sheet, _) = load("a { font-size: 14px; line-height: 1.5; } b { line-height: 20px; }");
    assert_eq!(sheet.root("a", 1.0).base().line_height_px, 21.0);
    // 31.5 device pixels, rounded to 32.
    assert!((sheet.root("a", 1.5).base().line_height_px - 32.0 / 1.5).abs() < 1e-5);
    assert_eq!(sheet.root("b", 2.0).base().line_height_px, 20.0);
}

#[test]
fn styles_are_resolved_once() {
    let (sheet, _) = load("item { color: red; } item:hover { color: blue; }");
    let menu = node(&sheet, "menu", 1.0);
    for _ in 0..10 {
        let item = menu.child("item");
        item.base();
        item.resolve(States::HOVER, &Ancestors::default());
    }
    let stats = sheet.stats();
    assert_eq!((stats.nodes, stats.styles), (2, 3));
}

/// A directory with `name`, a `width` x `height` png of colour
/// `rgba`, for `url()`s to find.
fn images(test: &str, name: &str, width: u32, height: u32, rgba: [u8; 4]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("icssed-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    ::image::RgbaImage::from_pixel(width, height, ::image::Rgba(rgba))
        .save(dir.join(name))
        .expect("the png written");
    dir
}

fn load_from(css: &str, dir: &std::path::Path) -> (Sheet, Vec<Warning>) {
    Sheet::load(
        &[Source {
            name: "t.css",
            text: css,
            dir: Some(dir),
        }],
        Scheme::Light,
    )
    .expect("loads")
}

#[test]
fn outlines_snap_and_default_to_currentcolor() {
    let (sheet, warnings) = load(
        "a { color: red; outline: 1px solid; outline-offset: 2px; } b { outline-width: 3px; }",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let a = sheet.root("t", 1.5).child("a").base();
    assert!((a.outline_width - 2.0 / 3.0).abs() < 1e-6);
    assert_eq!(a.outline_color, RED);
    assert!((a.outline_offset * 1.5 - (a.outline_offset * 1.5).round()).abs() < 1e-5);
    // No style, no outline.
    assert_eq!(style(&sheet, "t > b").outline_width, 0.0);
}

#[test]
fn transforms_compose_as_css_does() {
    let (sheet, warnings) = load(
        "a { transform: translate(4px, 6px) scale(2); } \
         b { transform: scale(2) translateX(4px); } \
         c { transform: scale(1, 2); } d { transform: rotate(10deg); } \
         e { transform-origin: top left; } f { transform-origin: bottom; } \
         g { transform-origin: 10px 20px; }",
    );
    let t = |kind: &str| style(&sheet, &format!("t > {kind}")).transform;
    assert_eq!(
        t("a"),
        Some(Transform {
            scale: 2.0,
            x: 4.0,
            y: 6.0
        })
    );
    // The last function first: the translation scaled too.
    assert_eq!(
        t("b"),
        Some(Transform {
            scale: 2.0,
            x: 8.0,
            y: 0.0
        })
    );
    assert_eq!(t("c"), None);
    assert_eq!(t("d"), None);
    let messages: Vec<&str> = warnings.iter().map(|w| w.message.as_str()).collect();
    assert!(messages[0].contains("uniform"), "{messages:?}");
    assert!(
        messages[1].contains("`rotate()` isn't supported"),
        "{messages:?}"
    );
    let origin = |kind: &str| style(&sheet, &format!("t > {kind}")).transform_origin;
    assert_eq!(origin("e"), (Origin::Fraction(0.0), Origin::Fraction(0.0)));
    assert_eq!(origin("f"), (Origin::Fraction(0.5), Origin::Fraction(1.0)));
    assert_eq!(origin("g"), (Origin::Px(10.0), Origin::Px(20.0)));
    // Drawn about the origin: the centre stays where it was.
    let bounds = iced::Rectangle::new(iced::Point::new(10.0, 10.0), iced::Size::new(20.0, 20.0));
    let scaled = load("x { transform: scale(2); }").0;
    let transformation = style(&scaled, "t > x").transformation(bounds).expect("one");
    let centre = iced::Point::new(20.0, 20.0) * transformation;
    assert_eq!(centre, iced::Point::new(20.0, 20.0));
    let corner = iced::Point::new(10.0, 10.0) * transformation;
    assert_eq!(corner, iced::Point::new(0.0, 0.0));
}

#[test]
fn backgrounds_load_their_images_from_the_sheets_directory() {
    let dir = images("background", "dot.png", 4, 2, [0, 0, 255, 255]);
    let (sheet, warnings) = load_from(
        "a { background: url(dot.png) red no-repeat; } \
         b { background-image: url(\"dot.png\"); background-size: 10px auto; \
             background-position: center bottom; } \
         c { background-size: cover; } d { background: url(missing.png); }",
        &dir,
    );
    let a = style(&sheet, "t > a");
    assert_eq!(a.background, RED);
    assert_eq!(a.background_repeat, Repeat { x: false, y: false });
    let picture = a.background_image.clone().expect("an image");
    assert!(matches!(
        picture.drawn(),
        Drawn::Raster {
            width: 4,
            height: 2,
            ..
        }
    ));
    let b = style(&sheet, "t > b");
    // The same file, loaded once.
    assert_eq!(b.background_image, Some(picture));
    assert_eq!(b.background_size, BackgroundSize::Px(Some(10.0), None));
    assert_eq!(
        b.background_position,
        (Origin::Fraction(0.5), Origin::Fraction(1.0))
    );
    assert_eq!(b.background_repeat, Repeat { x: true, y: true });
    assert_eq!(
        style(&sheet, "t > c").background_size,
        BackgroundSize::Cover
    );
    assert!(warnings[0].message.contains("missing.png"), "{warnings:?}");
    // Without a directory, a relative url() has nowhere to be found.
    let (_, warnings) = load("x { background-image: url(dot.png); }");
    assert!(warnings[0].message.contains("directory"), "{warnings:?}");
}

#[test]
fn a_border_image_is_cut_in_nine() {
    let dir = images("nine", "frame.png", 6, 6, [255, 0, 0, 255]);
    let (sheet, warnings) = load_from(
        "a { border-image: url(frame.png) 1 2; } b { border-image: url(frame.png) fill 2; } \
         c { border-image-source: url(frame.png); }",
        &dir,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let nine = style(&sheet, "t > a").border_image.clone().expect("cut");
    assert_eq!(nine.slices, [1, 2, 1, 2]);
    assert!(nine.parts[4].is_none(), "no middle without `fill`");
    assert_eq!(nine.parts.iter().filter(|p| p.is_some()).count(), 8);
    let filled = style(&sheet, "t > b").border_image.clone().expect("cut");
    assert!(filled.parts[4].is_some());
    // The initial slice is CSS's 100%: the whole image in a corner.
    let whole = style(&sheet, "t > c").border_image.clone().expect("cut");
    assert_eq!(whole.slices, [6, 0, 0, 6]);
}

#[test]
fn object_fit() {
    let (sheet, _) = load("a { object-fit: cover; } b { object-fit: scale-down; }");
    assert_eq!(style(&sheet, "t > a").object_fit, ObjectFit::Cover);
    assert_eq!(style(&sheet, "t > b").object_fit, ObjectFit::ScaleDown);
    assert_eq!(style(&sheet, "t > c").object_fit, ObjectFit::Fill);
}

#[test]
fn transitions_and_animations_parse_as_lists() {
    let (sheet, warnings) = load(
        "a { transition: background-color 200ms ease-in, opacity 1s linear 0.5s; } \
         b { transition: all 0.3s; transition-duration: 1s, 2s; } \
         c { animation: spin 1s linear infinite, pulse 2s alternate both 100ms; } \
         d { transition: width 1s; } e { animation: 1s spin; rotate: 0.5turn; }",
    );
    let a = style(&sheet, "t > a");
    assert_eq!(
        &a.transitions.property[..],
        [Moving::Background.bit(), Moving::Opacity.bit()]
    );
    assert_eq!(&a.transitions.duration[..], [0.2, 1.0]);
    assert_eq!(&a.transitions.delay[..], [0.0, 0.5]);
    assert_eq!(a.transitions.easing[1], Easing::Linear);
    // A longhand after its shorthand.
    let b = style(&sheet, "t > b");
    assert_eq!(&b.transitions.duration[..], [1.0, 2.0]);
    let c = style(&sheet, "t > c");
    let names: Vec<Option<&str>> = c.animations.names.iter().map(|n| n.as_deref()).collect();
    assert_eq!(names, [Some("spin"), Some("pulse")]);
    assert_eq!(&c.animations.count[..], [f32::INFINITY, 1.0]);
    assert_eq!(c.animations.direction[1], AnimationDirection::Alternate);
    assert_eq!(c.animations.fill[1], FillMode::Both);
    assert_eq!(&c.animations.delay[..], [0.0, 0.1]);
    // No `@keyframes spin` in this sheet: nothing to run.
    assert!(!c.animations.any());
    assert_eq!(style(&sheet, "t > e").rotate, 180.0);
    assert!(
        warnings[0].message.contains("`width` doesn't move here"),
        "{warnings:?}"
    );
}

#[test]
fn keyframes_keep_what_moves() {
    let (sheet, warnings) = load(
        ":root { --to: blue; } \
         @keyframes glow { from { opacity: 0.5; padding: 3px; } 50%, to { color: var(--to); } } \
         @keyframes glow { to { opacity: 0; } } \
         a { animation: glow 1s; } @keyframes nope { 150% { opacity: 1; } }",
    );
    let a = style(&sheet, "t > a");
    assert!(a.animations.any());
    // The later `@keyframes glow` wins, whole.
    let glow = a.animations.keyframes[0].clone().expect("found");
    assert_eq!(glow.frames.len(), 1);
    let messages: Vec<&str> = warnings.iter().map(|w| w.message.as_str()).collect();
    assert!(
        messages[0].contains("`padding` doesn't move in `@keyframes`"),
        "{messages:?}"
    );
    assert!(
        messages[1].contains("a step is `from`, `to` or a percentage"),
        "{messages:?}"
    );
}

#[test]
fn content_is_strings_or_nothing() {
    let (sheet, warnings) = load(
        "a { content: \"✓\"; } b { content: \"one\" ' two'; } c { content: none; } \
         d { content: normal; } e { content: 12px; }",
    );
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let content = |path: &str| style(&sheet, path).content.as_deref().map(str::to_owned);
    assert_eq!(content("a").as_deref(), Some("✓"));
    assert_eq!(content("b").as_deref(), Some("one two"));
    assert_eq!(content("c"), None);
    assert_eq!(content("d"), None);
    // Not inherited, as in CSS.
    assert_eq!(content("a > x"), None);
}
