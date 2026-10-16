# AGENTS.md: the toolkit

`icssed/` is a crate of CSS-styled widgets on iced 0.14, knowing nothing
of Aria. How it's built, and why, is ARCHITECTURE.md beside this file;
every element, class and property, with its neutral default, is
`assets/base.css`. Read this first, then those where you need them.

Everything here was learned building it: the rules it keeps, the
patterns that work, what things cost, and the traps already fallen into
once.

## The rules

What the toolkit holds to; a change that breaks one is the wrong change.

- **A toolkit of its own**: reusable widgets styled by CSS, built on
  iced, knowing nothing of any app (no config, services, locale or texts
  of Aria's). Its only UI dependency is iced: no layer-shell, no
  Wayland. A popup, a toast, a dialog are widgets it draws (box, shadow,
  content); the surfaces they live in are the app's to open.
- **Performance matters**: styles resolved once, not at every view; no
  allocations while drawing; what a layout costs (shaped text) kept from
  one layout to the next, since iced lays out a surface's whole tree at
  every rebuild; an idle UI costs nothing (no frame without a change);
  an animation asks for frames only while it runs. Measured, before and
  after a change.
- **No workarounds of any kind**: what iced can't do cleanly is a proper
  widget of the toolkit, or a limit written in ARCHITECTURE.md; never a
  trick in a view.
- **One source of truth for sizes**: a widget's size comes from its own
  layout, never measured again by hand somewhere else. A surface's size
  is `Measure`'s: the real layout, run headlessly.
- **CSS means CSS**: a property has its CSS name, units and behaviour;
  where ours differs, it's written in the reference, or it gets another
  name.
- **Everything is styleable and documented**: no colour, size or font in
  the code of a widget or a view; every element, class and property has
  its line in the reference, and a neutral default in the base.
- **No `Space` fillers**: in a row, the child that takes the free room
  has `width: fill` (as `flex-grow: 1` does), set in the theme.
- **Every widget is in the demo and tested**: a demo of its own, and
  on the overview with every other, in its states; its layout, style
  and behaviour tested without a compositor, its look by golden images
  at two scales.
- **Vanilla iced, public API only**: no fork; anything vendored is
  temporary and marked.

## Commands

```bash
cargo run -p icssed --example demo [-- theme.css]      # every widget: the overview, then a demo each; the theme reloads on save
cargo test -p icssed                                   # style, layout, widgets, golden images
ICSSED_GOLDEN=bless cargo test -p icssed golden            # accept a visual change meant to show (look at it first)
cargo bench -p icssed --features testing               # what it costs: before and after a change
cargo clippy -p icssed --all-targets --features testing
cargo fmt                                          # before every commit
```

## The model in one page

Every widget is an **element**: a type (`label`, `button`, an app's
`entry`), classes, an id, attributes, as in a document. The view says
them on the widget; where the element is in the tree (under which
parent, at which position) the toolkit finds itself, the first time the
runtime reaches the widget, from the container it's in. That place is a
**node**, an interned path in the sheet: its style is computed once per
distinct (node, own states, ancestors' states) and shared. No colour,
size or font is ever written in Rust: the sheet says everything.

```rust
// Once, and again when the theme file changes or the scheme flips.
// The toolkit's base.css always comes first, under every other rule.
let (sheet, warnings) = icssed::Sheet::load(
    &[
        icssed::Source { name: "app.css", text: APP_CSS, dir: Some(app_dir) },
        icssed::Source { name: &theme_name, text: &theme_text, dir: theme_dir },
    ],
    icssed::Scheme::Dark,
)?; // Err only on a syntax error: keep the last good sheet. Log the warnings.

// In view: the top of a view is given its place, a surface's root,
// which carries its scale (device pixels per logical one: the output's
// scale times the UI's). Everything under it finds its own.
fn view<'a>(&'a self, sheet: &'a icssed::Sheet) -> icssed::Element<'a, Message> {
    let rows = self.items.iter().enumerate().map(|(i, item)| {
        icssed::button![icssed::icon(item.icon.clone()), icssed::text(&item.label)]
            .kind("entry")
            .class_if("active", item.active)
            .on_press(Message::Pick(i))
            .into()
    });
    icssed::focus_scope(
        icssed::column![icssed::column(rows).kind("list")].at(sheet.root("popup", self.scale)),
    )
    .into()
}

// The window's iced theme: what it's cleared with, from the root's style.
fn theme(&self) -> icssed::Theme {
    let root = self.sheet.root("popup", self.scale).base();
    icssed::Theme::window(root.background, root.color)
}
```

- `icssed::Element<'a, M>` is `iced::Element<'a, M, icssed::Theme, icssed::Renderer>`.
  The toolkit's `Theme` isn't a palette. It's the draw-time context a
  parent hands its children: the opacity so far, the states of the
  ancestors, when they last changed. iced's own widgets work inside only
  if `icssed::Theme` has their `Catalog`: `text_input` and `scrollable` do
  (as `icssed::input` and `icssed::scroll`). That's on purpose: an unthemed widget
  can't slip in.
- Every widget has a type of its own (`text` a `label`, `icon` an
  `icon`, `row` a `row`, `button` a `button`): `.kind(k)` gives another.
  `.class(c)`, `.class_if(c, on)`, `.named(id)` (CSS's `#id`; `.id(..)`
  is iced's), `.attr(n, v)`, `.disabled_if(off)` say the rest, before or
  after the children: they see them all the same. Positions
  (`:nth-child()` and friends) are known with nothing said.
- `row![a, b]`, `column![..]`, `button![..]` take widgets of any kind;
  `row(iter)` and friends an iterator of `Element`s.
- A widget's place comes from a container of the toolkit: put it in
  one. The top of a view is given its place, `.at(root)`; so is a
  widget inside one of iced's containers that measure their children as
  they're built (`stack`, `row!` of iced's). `icssed::slot(view)` says
  what a view is from where it's put (`.kind("gadget").class("clock")`),
  so the view needn't know; `surface(content).at(root)` makes `content`
  a surface's root box.
- A root, `sheet.root(name, scale)`, is a `surface` with `name` as a
  class: a sheet writes `surface { }` for every one, `surface.popup { }`
  for one. base.css gives a surface the palette's background.
- `Node` is still the place, for reading a style where there's no widget
  (`sheet.root("popup", scale).base().shadow`, `Surface::room(root)`):
  `child(kind)`, `class(c)`, ... step down from a root. Each step is a
  hash lookup (~30 ns), never an allocation after the first time.
- A new scale is a new root, so a new view. A new sheet (theme, scheme)
  means the views must be rebuilt.
- Widgets are **controlled**: the app holds every value (a toggle's
  state, a slider's value, an unfolded submenu, the tab shown), and each
  widget sends the app's messages. The only state a widget keeps itself
  is what the pointer and the keyboard do: hover, press, focus,
  transitions, an open tooltip or select.

### The widgets

The toolkit's widgets are styled by the element kinds of `base.css`,
their own unless `.kind(..)` says otherwise. A box's kind is the app's
to choose.

| Call | Notes |
|---|---|
| `row(children)`, `column(..)`, `button(..)`; `row![..]`, `column![..]`, `button![..]` | the box: CSS box model and a flex subset; `.on_press(m)` makes it a button; `.on_animation_end(m)` for exit animations; `.id(id)` for iced's operations (its bounds, the focus); `.auto_width_if(b)`: its width its content's, whatever the theme says (a length the app decides: a surface as long as its content) |
| `text(content)` | a paragraph: wraps, ellipsis (`white-space: nowrap; text-overflow: ellipsis`), centred on its capitals. No padding or margin: put it in a box |
| `mark()` | a text whose words are its style's `content` (a check, an arrow): the theme's, picked by its classes |
| `icon(source)` | `IconFile::Svg { handle, symbolic }` (a symbolic one takes `color`), `IconFile::Image`, or `(&dyn Icons, name)`: the app's lookup, asked at draw for size and scale; a box (background, border, padding, margin), the picture in its content box |
| `image(handle)` | `object-fit`, `border-radius`; a box as an icon is |
| `surface(content).at(root)` | a surface's root: `content` is its box, and it adds the room the box's shadow needs, so a popup's size includes it; `Surface::room(root)` is that room, for placing the surface |
| `slot(view).kind(k).class(c)` | nothing of its own: `view`'s top widget is the slot's element, with its own classes too; a frame says what a page is, a bar what a gadget is |
| `focus_scope(content)`, `.arrows()` | Tab and Shift+Tab among what's inside, one per surface; `.arrows()`: the four arrows instead (a grid's), to the nearest that way in the row or column, round, once one inside has the focus |
| `toggle(on).on_toggle(Message::Wifi).icon(s).label(t)`, `.checkbox()`, `.radio()`, `.switch()` | a value on or off, `:checked` when on: a `toggle` (a button that stays pressed), a `checkbox`, a `radio`, a `switch`, each the same tree (`indicator > mark`, `icon`, `label`), the look the theme's; a radio, or a group of pressed buttons, is `toggle(p == chosen).on_toggle(move \|_\| Choose(p))` |
| `slider(0.0..=100.0, v, f).step(1.0).on_release(m)` | drag, click, wheel, the arrows |
| `meter(fraction).label(text)` | with a label, a gauge |
| `input(placeholder, value).on_input(f).on_submit(m).secure(b).id(id).leading(e).trailing(e)` | iced's `text_input` in a box; `:focus` while its field has it |
| `scroll(content).horizontal() / .both().id(id).on_scroll(f)` | no box of its own; `max-height` lets it grow, then scroll |
| `menu([MenuItem::new(l).on_press(m).check(b) / .radio(b).icon(s).shortcut(k).submenu(open, items).class(c), MenuItem::separator()])` | Up and Down move the focus once an item has it; a check's box and a radio are a toggle's tree (`item > checkbox > indicator > mark`, `radio`, `:checked` the item too), drawn by the toggles' rules; an icon goes in their column, two columns only for an item with both |
| `tabs([Tab::new(label, m).active(b).class(c)])` | Left and Right move the focus |
| `calendar(Month::new(y, m), title, [names from Monday]).first_weekday(n).today(y, m, d).selected(y, m, d).on_previous(m).on_next(m).on_select(f)` | no locale in the toolkit: the app gives the words |
| `graph(capacity).series(values).series_class(c, values).max(v).label(text)` | a sparkline is a graph styled by the app |
| `tooltip(content, text).delay(d).on_tip(f)`, `tip(text).at(root)` | the tip a `tooltip` beside the children of the element the tooltip is in; an iced overlay in a window; `on_tip` for an app that shows tips in surfaces of its own |
| `select(options, selected, f).placeholder(t).on_open(open, f)` | its options as a menu in an overlay; `on_open` as for tips |

`icssed::Measure::new().size(key, element, max)` lays an element out with no
surface (headless tiny-skia, the real widgets): what a layer surface
must open at. Build the element at the scale it will be shown at. Keep
one `Measure` and one `key` per surface, so text is shaped once (15 µs a
menu, against 135 µs with a fresh tree), and `forget(key)` when the
surface closes.

### Apps with layer surfaces (Aria)

- One root node per surface, made with that surface's scale. Measure a
  popup's content with the same root, so what's measured is what's
  shown.
- A tip or a select's options can't be overlays in a bar: a 30 px layer
  surface would cut them. Give the widget `on_tip` / `on_open`; it sends
  the rectangle to show them by, in the surface's coordinates with the
  scrolls taken into account. Open a popup there with `icssed::tip` or a
  `icssed::menu`, and tell the select whether its options are open.
- A tooltip asks its delayed frame with `RedrawRequest::At`, which
  `iced_exwlshell` 0.20.1 honours.
- The surface's scale, for its root: `iced::window::scale_factor(id)`
  answers the fractional one under `iced_exwlshell`; no event tells a
  change, so ask it when the surface appears and when its output
  changes.
- Aria's `debug widgets` is the query: run `icssed::query::Report` as an
  iced operation, then `sheet.matching("menu item:nth-child(2)", &found)`
  (indices into `found`); `found[i].path(&sheet)` names each one, and its
  root tells the surfaces apart (an operation runs on every window).
- A surface is placed by its box, not by its shadow's room:
  `Surface::room(root)` is what to take off (a popup's anchor, a toast's
  margins).

## Styling: what to know

- **The CSS is a subset, documented in `base.css`'s header**, and it
  means CSS: same names, units and behaviour, or it says where it
  differs. Variables live in `:root`, `:root.light`, `:root.dark` only.
  A syntax error rejects a file; anything else is a warning with its
  `file:line:col`, and is skipped.
- **`-debug: box` shows an element's boxes** (margin, border, padding,
  content, in an inspector's colours), the innermost only:
  `*:hover { -debug: box; }` in a sheet that reloads is an inspector
  under the pointer. A text shows its content box, but never hovers.
- **The toolkit's base is the user-agent origin**: any app or theme rule
  beats it, whatever the specificity.
- **Element types are global, as in CSS.** An app's own `track`, `fill`,
  `knob`, `label`, `icon`, `menu`, `item`, `field`, `series`, `day`, ...
  gets the base's rules for the toolkit's parts. Scope your rules
  (`lanes > track`), or choose names the base doesn't use. The
  old gallery's `track` once took a slider's fill height away, and test
  fixtures named `menu`/`item` got the menu's padding.
- **`:focus` is CSS's**: a click gives it too; `:focus-visible` only
  the keyboard (Tab, the arrows) and the app (a focus operation), and
  an input always. Style the focus ring on `:focus-visible`, as the base
  does; `:focus` alone also shows after a click.
- **States**: `:hover`, `:active` and `:focus` are tracked only on
  nodes some rule can see them on, so a moving pointer over unthemed UI
  asks no frame. `:disabled` and `:checked` are part of the path
  (`disabled_if`, a toggle's value), and only the marked element is
  disabled or checked, not the nodes under it. Paint
  may follow any state, own or an ancestor's. Layout (sizes, padding,
  font) follows only the element's own states: under an ancestor's
  state it isn't applied. The sheet doesn't warn about it yet, though
  it should (ARCHITECTURE.md, "Not done yet"). What a state changes, the descendants
  inherit (`button:hover { color }` reaches its label).
- **`display: none`** leaves an element out of the box it's in (no
  room, no gap, not drawn, no events): the base hides a toggle's
  indicator so. It follows the structure (classes, `:checked`,
  `:disabled`), never a state: `:hover` can't make a part appear.
- **Classes are the view's, states the widget's.** State a widget owns
  at runtime shows as a pseudo-class: an open select is `:active`.
  `content` (the marks: a menu item's check, an arrow) is read from the
  style at rest, so it follows classes, not states.
- **Paths are painted as SVG's**: `fill`, `stroke`, `stroke-width`,
  inherited, with fill initially black, as SVG has it.
- **The scale**: borders snap to whole device pixels as CSS does (at
  least one), and layout puts boxes, line boxes and baselines on the
  device grid, at any scale, fractional too.
- **Text is centred on its letters** (`text-box: trim-both cap
  alphabetic` on every element): a label's box runs from its capitals'
  top to its baseline.
- `transform` takes translate and a uniform `scale` (iced has nothing
  else), a translation's percentage of the box's own size (`translateY(-100%)`
  slides a box out by its height, whatever it is); `rotate` turns only
  icons and images. `opacity` isn't a group:
  overlapping children show through each other.
- **Percentages** where they're of the element itself or of its font:
  `transform`, `transform-origin`, `border-radius` (of the shorter side:
  no ellipses), `background-position`, `background-size` (of the
  background's box), `font-size`, `line-height`. Not
  in the layout's lengths (sizes, padding, margin, gap): those would be
  of the containing block (ARCHITECTURE.md, "Not done yet").
- `min-height` beats `height`, as in CSS: a test's 30 px input stays at
  the base's 32 px `min-height`.
- Animations: `transition` (colours, opacity, shadow, transform,
  rotate) and `@keyframes` with `animation`. Exit animations: the app
  adds a class (`.leaving`) and closes the surface on
  `on_animation_end`.

## What things cost

Measured with `cargo bench -p icssed --features testing` (`benches/costs.rs`:
fixed trees on the headless renderer; this laptop, release, 2026-10).
Update these numbers when a change moves them:

| tree | view | rebuild (view, diff, layout) | update (pointer) | frame (update, draw) |
|---|---|---|---|---|
| a bar, 12 gadgets | 1.0 µs | 4.8 µs | 1.1 µs | 3.8 µs |
| a menu (`icssed::menu`), 20 items | 3.0 µs | 16.5 µs | 2.2 µs | 7.6 µs |
| a list, 500 rows | 43 µs | 224 µs | 35 µs | 97 µs |

A view only describes its widgets: their nodes are found in the diff
that follows, so the view costs less and the rebuild a little more
(~5%) than when views built their nodes themselves.

Loading a theme (the base, the demo's frame and every demo's sheet)
takes 1 ms.

- **An idle UI costs nothing.** A frame comes only when a tracked state
  changes, or while an animation runs. A `:hover` rule on a big list
  makes every row tracked: put it there only if it's wanted.
- **Positional pseudo-classes split rows, a little.** In a sheet
  without them a child's position isn't part of its node: a list's 500
  rows share one node and one style. With them, a row's node carries which of the sheet's
  positions it matches (in every list, whatever kind the rule names), so
  the rows split into a few nodes (first, last, odd, even), each
  resolved once. The base doesn't use them, so a sheet that doesn't
  need them pays nothing.
- iced lays out a surface's whole tree on every rebuild, changed or
  not, and asks every widget for overlays on every event and every
  frame: ~6 ns a widget, a third of the list's update. Smaller trees
  per surface are cheaper; nothing else is wasted.
- Text is shaped once and kept in the tree; a line that doesn't wrap is
  shaped once, at any width. Keep the tree shape stable between views
  (the same widget at the same place), or iced builds a new state and
  shapes again.
- Measure before and after any change on a hot path, with the bench.

## Working on the toolkit

### Map

```
src/style/     the sheet: value.rs (properties: parsing, longhands), computed.rs (Computed,
               the cascade's values), node.rs (Name, Node, States, Ancestors), selector.rs,
               parse.rs, sheet.rs (load, root, stats), cascade.rs (styles, sensitivity,
               the query's matching), fonts.rs, images.rs, keyframes.rs, tests.rs
src/theme.rs   icssed::Theme, and the Catalogs of the stock widgets embedded
src/layout.rs  snap, Sizing (a box's model on its limits), content_box, flex()
src/paint.rs   boxes: quads, paths for sides that differ, shadows, outline, images
src/anim.rs    easing, transitions, keyframes: Animated, kept in a widget's tree state
src/measure.rs Measure       src/query.rs  Report, Found, Reported
src/testing.rs the Harness (feature `testing`)   src/golden.rs the golden scenes
src/widget/    place.rs (where a widget is: Place, what a container hands its children),
               boxed.rs (the shared box), flex.rs, text.rs, icon.rs, image.rs,
               surface.rs, slot.rs, focus.rs, meter.rs, slider.rs, toggle.rs, input.rs,
               scroll.rs, menu.rs, tabs.rs, calendar.rs, graph.rs, tooltip.rs,
               select.rs, tests.rs
assets/base.css               the reference and the neutral defaults
examples/demo/                the demo: overview.{rs,css} every widget at once, a demo each
tests/golden/, tests/fonts/   reference images, the fonts they're drawn with
```

### Invariants

- **No allocation while drawing.** Styles are `Arc`s resolved by key;
  nodes and anything per view are made in `view`, when the runtime
  first reaches a widget (its tree made, diffed) or in `layout` (which
  is `&mut self`), never in `draw`. A cache is compared without a copy,
  and copied only when it changes (`graph.rs`). The ellipsis cut is the
  one exception: once per new width or text.
- **Everything on the device grid**: use `snap`/`snap_up` with the
  node's `scale()` for every size and position a widget computes.
- **Time is `Instant::now()` in `update`**, or the `now` of
  `RedrawRequested`: never the last frame's time.
- **A widget's node isn't known where it's built.** It has a `Place`
  (`widget/place.rs`: its type and classes, then its node and its style
  at rest), resolved the first time the runtime reaches the widget
  (`tag`, `state`, `children`, `diff`, `size`), against the parent a
  container hands it: a container makes and diffs its children's trees
  with `place::trees` / `place::diff`, never `Tree::new` or
  `diff_children` (a lone child: `place::under`). So nothing in a
  constructor may read the style: what depends on it is made at the
  first touch (`Field`'s `TextInput`, `Scroll`'s `Scrollable`, a text's
  `text-transform`). A part made of widgets says its kind
  (`row([]).kind_of(name!("knob"))`: `name!` interns it once).
- **A box widget is built on `widget/boxed.rs`**: `Boxed` (its place,
  through it its node and style at rest) and `BoxState` (states,
  `Animated`, the transform drawn).
  `boxed.draw(...)` paints the box, hands the children their theme
  (states, opacity, when it changed) and draws the outline.
  `boxed.settle(...)` follows hover and asks frames and relayouts. A
  widget placing its children its own way uses `layout::Sizing` and
  `content_box` for its own box (`meter.rs`, `slider.rs`).
- **Every widget reports itself through `operate`**:
  `operation.custom(None, bounds, &mut Reported::new(node))`, so the
  query finds it, plus `operation.focusable(...)` when it takes keys.
  Paint parts that aren't widgets report their own nodes too (a graph's
  series).
- **Every container forwards `overlay`** (`overlay::from_children`).
  Without it, an iced overlay inside it never shows.
- **A wrapped stock widget** (`Field` in `input.rs`, `Scroll`) calls the
  trait's methods explicitly: `Widget::<M, Theme, Renderer>::layout(&mut
  self.inner, ..)`. `TextInput` has inherent `layout`/`draw` methods with
  other signatures.

### Facts about iced 0.14 that shaped the code

- `focus_next`/`focus_previous` stop at the ends: the focus leaves.
  That's right for Tab; an arrows scope (a menu's, tabs', a grid's)
  moves it round with an operation of ours (`Round` in `focus.rs`); a
  grid's picks its target from the focusables' bounds (`Places`).
- iced's `Focusable` has `focus`/`unfocus` and nothing of how: its
  `focus` is the keyboard's and the app's (shown, `:focus-visible`); a
  press is the widget's own (`BoxState::pressed`, `:focus` alone), and
  takes the focus from every other widget, which all see it.
- `Scrollable::operate` doesn't move its content's layout; it calls
  `operation.scrollable(.., translation, ..)` and lets each operation
  apply it. The query does (`Report`). A widget learns where it's shown
  from `overlay(.., translation)`, which iced calls on every event.
- A disabled `text_input` still takes the focus on a click or a focus
  operation. The toolkit keeps both away from it.
- An overlay can tell whether its content sent a message by giving it
  a `Shell` of its own and `merge`-ing it afterwards (the select closes
  on a pick that way).
- A layer (`with_layer`) is drawn over everything in the layer around
  it, also what's drawn there after it: draw what must go over it in
  a layer made after it. A layer's clip is its own bounds, not cut by
  the clip around it: give it the viewport, not something larger.
  tiny-skia draws in order, so the golden images don't show this.
- Before drawing, wgpu merges a layer into the one below it when their
  clips match and the upper starts where the lower ends (images over
  images); in a layer, iced_wgpu draws every nearest-filtered image (an
  svg) before every linear one (a raster). So svg icons over a
  background image vanished, and showed again under a hovered button's
  quad. `draw_box` starts the layer over a background image with a
  transparent quad, marked TEMPORARY, until iced draws them in order
  (iced-rs/iced#3512).
- tiny-skia (the headless renderer, the golden images) places a
  stretched image on a grid as coarse as its stretch and doesn't round
  an image's corners: right on wgpu only.
- Stock widgets' colours come from a style asked at draw, so they don't
  transition; a scrollbar's sizes are set at the view, so they follow no
  state.

### Adding a widget

1. Its file in `src/widget/`, built on `Boxed` if it's a box. Export it
   from `widget/mod.rs`.
2. Its section in `base.css`: an element tree in the comment (every
   element, class and attribute, what each part is) and neutral defaults
   from the palette's variables. No positional pseudo-classes (every
   app's lists would split by them) and no `url()` (the base has no
   directory at runtime).
3. A demo of its own (`examples/demo/<name>.rs` and `.css`, listed in
   `main.rs`'s `demos!`), and its place on the overview
   (`overview.rs`), in its states, with nothing of the app's: the
   base's look beside every other widget's.
4. Tests: behaviour with the `Harness` in `widget/tests.rs`, a golden
   scene in `golden.rs` at both scales, a bench row if it's on a hot
   path.
5. Look at it in the demo on wgpu at scale 1.5, light and dark (see
   "Verifying"); then the docs: its calls in the table above,
   ARCHITECTURE.md where its design adds something (a kind of widget, a
   limit, what's not done), the costs here if a bench moved.

### Adding a property

`value.rs`: a `Prop`, a `Decl`, its longhands, its parsing in
`value_of`, and `inherited()` if it inherits. `computed.rs`: the field,
its initial value, `apply`, `copy` and `INHERITED`; a value that
resolves late (a colour with `currentcolor`) goes through the cascade's
own fields, with `inherit`, `reset` and `finish`. Then its line in
`base.css`'s reference and a test in `style/tests.rs`. If a state may
change it, decide whether it's layout (`Computed::same_layout`) or paint
(`anim.rs`'s `Paint`, if it should transition).

## Verifying

### The harness (tests, and an app's tests with `--features testing`)

```rust
let root = sheet.root("root", 1.5);                   // the harness's scale
let mut ui = Harness::new(view(root), Size::new(300.0, 200.0), 1.5);
ui.move_to(point); ui.press(); ui.release(); ui.click(point);
ui.click_on(&sheet, "menu > item:nth-child(2)");     // by selector
ui.key(Named::Tab); ui.key_with(Named::Tab, Modifiers::SHIFT);
ui.type_text("hi"); ui.send(&[event]);
let found = ui.find(&sheet, "slider > knob");         // border boxes, as shown
let all = ui.report();                                // every widget: sheet.matching(..), found.path(&sheet)
ui.operate(&mut operation);                           // any iced operation, its outcome
ui.frame(now);                                        // RedrawRequested(now), then draw
let pixel = ui.pixels(Color::WHITE).at_point(x, y, 1.5);
ui.rebuild(view(root));                               // a new view, the tree kept
assert_eq!(ui.messages, [..]);
```

Frames come at the times you give, so a transition is sampled where it
should be, and a tooltip's delay is a `frame(start + 700ms)`. They come
whether asked or not, which a window doesn't do: assert what `frame`
returns where an animation must go on (a tooltip's entry animation once
stopped at its first frame in a window, its tests all passing). The
messages don't come back into the view: rebuild with the new values
yourself.

### Golden images

- Each scene is drawn at scale 1 and 1.5 with the fonts of
  `tests/fonts/`, and compared within 2 levels per channel. On a
  failure, what was drawn is in `target/golden/`: look at it.
- Keep scenes deterministic. Turn transitions and animations off in the
  scene's CSS. Use marks the test fonts have (ASCII `content`, not
  ✓ ● ▸), or a system font's fallback sneaks in. Give the content room:
  at 1.5 the rounding can make it a pixel taller than the scene, and
  the layout then shrinks it as CSS does, empty boxes first (a menu's
  separators once vanished that way).
- Root a scene in its root node (`column![..].at(root)`), or the page's
  padding and background are lost.
- tiny-skia draws a scaled raster image at a whole pixel of the image's
  own (`iced_tiny_skia` `raster.rs`: `(x / scale) as i32`): a 4 px
  image drawn 10 px wide at x 22 lands at 20. Give a test's picture the
  size it's drawn at; wgpu places it where it's asked.
- Bless only after looking at the images.

### The demo on wgpu, in a headless nested Sway

`tests/ui/lib.sh` gives the input vocabulary (`pointer`, `click`, `key`,
`type_text`, `inject "scroll 3"`, `settle`); `aria-inject` comes from
`cargo build -p aria-inject`. A run, from the repository's root (build
the demo first), on the overview:

```sh
out=/tmp/demo-run; mkdir -p "$out"
cat > "$out/inner.sh" <<'EOF'
#!/bin/sh
. "$ARIA_UI_ROOT/tests/ui/lib.sh"
inject_start
inject "layout 1500 1350"        # the output's resolution: pointer positions are then a screenshot's pixels
"$ARIA_UI_ROOT/target/debug/examples/demo" > "$DEMO_OUT/demo.log" 2>&1 &
pid=$!
sleep 3
pointer 1490 1340; settle        # out of the way
grim "$DEMO_OUT/light.png"       # the whole output, in device pixels
pointer 1443 40; inject click; settle   # the Dark switch
grim "$DEMO_OUT/dark.png"
pointer 893 186; inject click; settle   # a radio
grim -g "240,40 600x100" "$DEMO_OUT/clicked.png"   # -g takes logical pixels: device ones / 1.5
kill $pid; inject_stop; swaymsg exit
EOF
chmod +x "$out/inner.sh"
cat > "$out/sway.conf" <<CONF
output HEADLESS-1 resolution 1500x1350 position 0,0 scale 1.5
default_border none
exec "$out/inner.sh"
CONF
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=gles2 \
  DEMO_OUT="$out" ARIA_UI_ROOT="$PWD" \
  timeout 90 dbus-run-session -- sway -c "$out/sway.conf" > "$out/sway.log" 2>&1
```

Use `WLR_RENDERER=pixman` without a `/dev/dri/renderD*`. Find positions
in a first whole screenshot; another demo is a click on its entry in
the sidebar, and a pane scrolls with `inject "scroll 30"` over it.
Never `hyprctl`.

## Conventions

Code, comments, docs and commits in English. Comments are terse and say
why. Every item has a doc comment saying what it is in a phrase. Commit
subjects look like `Toolkit: the menu`: informal, no Conventional
Commits, no attribution lines. Run `cargo fmt` before committing; never
push. Keep this file (rules, calls, costs, traps), ARCHITECTURE.md
(design and reasons, limits, what's not done, what's to decide),
`base.css` (the reference) and the demos (their descriptions, their
sheets' tree comments, the overview) current with every change.
