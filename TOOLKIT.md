# Toolkit architecture

How the toolkit UI.md asks for is built: a crate of CSS-styled widgets
on iced, knowing nothing of Aria. UI.md has the rules and the features
needed; this is the design that meets them, and why. Every claim about
iced (0.14, the version `iced_exwlshell` 0.20/0.21 runs on) was checked
in its source in `~/.cargo/registry`; every number was measured on a
throwaway prototype (2026-10, this laptop, release build) that drew a
themed menu headlessly and in a real wgpu window inside a nested
headless Sway at scale 1.5.

"The toolkit" and `tk::` in the examples stand for the crate until it
has a name.

This plan is the toolkit and its gallery; how Aria moves onto it is
decided afterwards, Aria untouched meanwhile (UI.md).

## Decisions at a glance

- **Real iced widgets, written on the advanced API** (`iced::advanced::
  Widget`): one per toolkit piece, composable with iced's own. Not a
  retained tree of our own inside one widget, not iced's stock widgets
  with style closures (see "The road").
- **Our own `Theme` type** as iced's `Theme` parameter. It isn't a
  palette: it's the **draw-time context** a parent hands its children
  (the opacity so far, the states of the ancestors a descendant's style
  depends on, when they last changed). Stock iced widgets we embed
  (scrollable, text_input) get `Catalog` impls that read styles from
  the sheet through it.
- **Styles resolved once**: a widget's place in the element tree is a
  `Node`, a `Copy` handle to an interned path; computed styles are
  cached per (node, own states, ancestors' states) and shared as `Arc`s.
  A view pays a hash lookup per node; matching runs once per distinct
  key.
- **Nodes are explicit in the view** (`menu.child("item")`), as Aria's
  are today: iced's `layout` gets no context from the parent, so the
  path, with the surface's scale at its root, has to be known where the
  widget is built.
- **The scale enters at the root node**, as the device pixel ratio
  enters CSS's computed values: border widths, sizes and positions on
  whole device pixels already in layout, so layout, painting and the
  measured size agree at any scale, fractional too.
- **Our own layout**, a CSS flexbox subset on iced's `Limits` protocol:
  margin, border-box sizing, borders that take room, min/max, `fill`,
  shrinking down to CSS's automatic minimum.
  Not iced's flex (a `fill` child in a shrinking box takes everything),
  not taffy (25× slower nested in iced's per-widget layout, and its
  `fill` semantics differ).
- **States live in the widget tree** (`Tree::state`), which survives
  the rebuilds: hover on any element, `:active`, `:focus`, `:disabled`,
  tracked only by the nodes some rule can make depend on them.
- **Animations ask frames only while they run**, from the tree state,
  for their own surface (no view rebuild per frame).
- **A surface's size is the real layout**, run headlessly (tiny-skia
  renderer, no GPU, no window): the single source of truth for sizes.
- **Tests without a compositor**: layout, events and pixels through
  iced's runtime `UserInterface` and the tiny-skia renderer, at any
  scale; the wgpu path through the gallery in a nested Sway.

## The road: three shapes weighed

1. **Stock iced widgets, styled by closures** (Aria today). Measured in
   the prototype: iced's `column` of `fill` items inside a shrinking
   `container` takes the whole 800px offered, not the widest item
   (UI.md §1); containers have no hover state (the `mouse_area`
   workarounds); a widget's status lives in the widget and dies with
   every rebuild (ARCHITECTURE.md: the late hover); a border takes no
   room; an ancestor's state can only reach the text colour. Each of
   these is a rule of UI.md broken, so: no.
2. **A tree of our own inside one iced widget** (a mini retained-mode
   engine per surface). Full control of cascade and layout, but
   events, focus, overlays, operations, text editing and scrolling all
   rewritten, and iced's own widgets no longer composable. iced's
   machinery isn't the cost: view, diff and layout of a 20-item menu of
   stock widgets, tree kept, is 5.8µs, 0.3µs an item. No reason to
   leave it.
3. **Our widgets on iced's advanced API**, with our `Theme` type.
   Prototyped: everything UI.md marks "yes" in §1-§6 that was tried
   works through public API (below). This is the road.

Measured on a bar-sized tree (12 gadgets, ~70 nodes, a sheet of ~280
rules), per rebuild: prototype view 5.6µs, diff+layout 4.6µs, update
2.2µs, draw 4.1µs; the same bar in stock iced widgets 4.3 / 3.9 / 2.1 /
2.2µs. Both are noise next to a frame; the prototype's draw resolves its
style again per widget, which a real one keeps from the view.

## Crate

A workspace member (`toolkit/`), its own `Cargo.toml`, depending on:

- `iced` 0.14 with `wgpu`, `tiny-skia` (the headless renderer: measuring
  and tests; also the CPU fallback a GPU-less CI lacked), `advanced`,
  `svg`, `image-without-codecs`, `canvas` (paths: borders with sides
  that differ, graphs), `crisp` (quads snapped to the pixel grid). With
  `tiny-skia` on, `iced::Renderer` is iced's fallback enum (wgpu first,
  tiny-skia second) for the whole app: a `match` per call, nothing else.
  `iced_runtime` 0.14 directly, for `UserInterface` in the headless
  harness (iced doesn't re-export it; it's a published iced crate).
- `cssparser` (Servo's CSS Syntax Level 3 tokenizer, MPL-2.0: compatible
  with GPL-3): correct tokens, escapes, comments, numbers with units and
  error recovery, our grammar on top. "CSS means CSS" starts at the
  tokenizer.
- `rustc-hash` for the interner and caches (a node lookup: 65ns with
  std's SipHash, 32ns with FxHash, measured).
- dev: `iced_test` 0.14 (its `Simulator` and `Selector` trait), `png`.

`iced` is pinned to what `iced_exwlshell` runs on; the toolkit moves
when Aria can.

Modules:

```
style/      the sheet: parse.rs (cssparser → rules, warnings with file:line:col),
            selector.rs, value.rs (lengths, colours, times, easing), cascade.rs,
            computed.rs (the computed style), node.rs (interner, Node), sheet.rs
theme.rs    the draw-time context (iced's Theme parameter), Catalog impls
layout.rs   the flex subset, shared by the containers
paint.rs    boxes: quads, paths, shadows, outline, transforms, opacity
anim.rs     transitions, keyframes, easing
widget/     box, text, icon, image, toggle, slider, input (iced's text_input),
            scroll (iced's scrollable), menu, calendar, tabs, graph, tooltip, surface
measure.rs  a tree laid out with no surface
query.rs    widgets by selector (debug and tests)
testing.rs  the headless harness (feature `testing`)
assets/base.css     the toolkit's base stylesheet: every element, class and
                    property, documented, with neutral defaults
examples/gallery.rs the gallery
```

## The style engine

### Sheets

`Sheet::load(sources, scheme) -> (Sheet, Vec<Warning>)`: the toolkit's
`base.css`, then the app's base (Aria's elements), then the user theme,
one cascade. The toolkit's base is under the others as CSS's user-agent
stylesheet is: any rule of the app's or the theme's wins over its,
whatever their specificity (else its `:root { color }` would beat a
theme's `panel { color }`). Variables (`:root { --x }`, `var(--x, fallback)`) and
schemes (`:root.light` / `:root.dark` folded over the plain ones, the
root elements carrying the scheme as a class) work as Aria's do today:
a sheet is compiled for one scheme; switching scheme or reloading the
file is a new `Sheet`, and the views are rebuilt. Bad selectors and
declarations are warnings, skipped; a syntax error rejects the file
(the caller keeps the last good sheet).

Rules are indexed by the type of their rightmost compound (rules naming
no type kept apart, merged back in cascade order): trying all ~280 on
every node was 46% of Aria's CPU with the pointer moving
(ARCHITECTURE.md).

Selectors: type, `.class`, `#id`, `[attr="value"]`, `*`, descendant,
child (`>`), `:hover :active :focus :disabled :first-child :last-child
:nth-child() :root`. Specificity then source order. No `!important`, no
`@media`; `@keyframes` yes (see Animation).

### Nodes

A widget's place is a `Node<'a>`: `Copy`, a reference to the sheet and
an interned id. The view builds them top-down, as Aria's `theme::Node`
paths are built today, without allocating:

```rust
fn view<'a>(&'a self, menu: tk::Node<'a>) -> tk::Element<'a, Message> {
    tk::column(menu, self.items.iter().enumerate().map(|(i, it)| {
        let item = menu.child("item").class_if("active", it.active);
        tk::row(item, [
            tk::icon(item.child("icon"), &it.icon).into(),
            tk::text(item.child("label"), &it.label).into(),
        ])
        .on_press(Message::Pick(i))
        .into()
    }))
    .into()
}
```

A surface's tree starts from its root element, `sheet.root(kind,
scale)`: the scale is carried by every node under it (see Scale). Each call interns
one step (`(previous id, Kind | Class | Id | Attr | Position)` → id),
so a node is a chain of steps with a canonical id.
Static names are `&'static str`; ids and attribute values are interned
strings, looked up without allocating. A position step (`child_at(kind,
i, n)`) is only kept when the sheet has positional pseudo-classes:
otherwise the 500 rows of a list share one node, and one style (measured:
1500 node lookups of a 500-row list resolve 8 styles, 60µs cold, 48µs
warm).

Why explicit: iced's `layout(&mut self, tree, renderer, limits)` gets
nothing from the parent but limits, and `draw` only a `&Theme`; a style
needs its ancestors and its scale at layout already (padding, font
size, border widths). Passing
context through the children's `Tree` states would break at the first
stock container in between. The explicit node is the price, and Aria
pays it today.

### Computed styles

A `Computed` holds every property, inherited ones from the parent's.
The cache key is (node, own states, ancestors' states): `node.base()`
is the style at rest, taken in the view and held by the widget for
layout; `node.resolve(states, ancestors)` at draw, for a node not at
rest or under an ancestor that isn't. Results are `Arc<Computed>`; the
caches live in the `Sheet` (`RefCell`: views are single-threaded) and
go with it. They grow with the distinct paths seen, bounded by the UI's
shapes, not by its data.

A node is **sensitive** when some rule with a state could match it,
as subject or as an ancestor of the subject (structure only, states
ignored): only sensitive nodes track their hover and press, so a
pointer moving over an unthemed area asks no frame.

**What a state may change.** Paint properties (colours, background,
border colour, shadow, opacity, transform, outline) under any state,
own or ancestors'. Layout properties (sizes, padding, margin, border
widths, gap, font) under the node's own states: a change relayouts
(`shell.invalidate_layout()`). Under an ancestor's state they're not
applied, and the sheet says so at load: the descendant has no way to
ask its own relayout at draw time.

## The draw-time context: `Theme`

iced threads a `&Theme` through `draw` (and through every stock
container unchanged), and each window gets its own from the app
(`theme(state, window)`). The toolkit's `Theme` is that channel, per
subtree, from the parent:

- the **opacity** so far;
- the **states of the ancestors** a descendant may depend on (depth,
  states), pushed by a stateful box only while it isn't at rest;
- the last instant an ancestor's states **changed**, which a
  descendant's transition starts from (see Animation).

A parent hands its children a modified copy only when something
changes (a hovered item, a faded box, a box changed later than its
ancestors); otherwise the same reference. The copy is inline, the
ancestors a small fixed array: iced's `Theme` parameter is one owned
type for the whole app, so a child's can't borrow its parent's, and a
copy mustn't allocate. The scale isn't here: layout needs it too, and
gets no `Theme` (see Scale).

It implements `iced::theme::Base` (all `iced_exwlshell` asks) and the
`Catalog` traits of the stock widgets the toolkit embeds: `scrollable`
(prototyped: `scrollbar`, `scrollbar > thumb`, `:hover`/`:active` on
the bar reaching the thumb), `text_input`. Every other stock widget
needs a `Catalog` impl before it can be used: that's deliberate, an
unthemed widget can't slip in.

Inherited colours keep iced's own channel too: the `renderer::Style
{ text_color }` a parent passes is the current (state-resolved) colour,
so a stock widget's text follows it.

## Scale

A surface's scale, its device pixels per logical pixel (the output's
scale times the UI scale the app gives), is layout's business as much
as painting's: a 1px border is 1.5 device pixels at 1.5, and a width
snapped at draw only disagrees with the room layout gave it (a child's
background flush with it leaves a hairline, or covers half of it).
iced gives `layout` nothing but limits, and its `Renderer` trait has no
scale (0.14): the node is the only channel, as for the path. So the
scale enters at the root, as the device pixel ratio enters CSS's
computed values:

- the app makes a surface's root node with its scale
  (`sheet.root(kind, scale)`); every node under it carries it, and the
  computed styles are made for it (a second scale, a second set of
  cached styles);
- border widths are snapped in the computed style, as CSS snaps them
  ("snap as a border width": down to whole device pixels, at least
  one), so layout and painting use the same width;
- layout rounds sizes and positions to whole device pixels: box edges,
  line boxes and icons land on the device grid at any scale, fractional
  too;
- draw reads it from the node as well: an icon's size in device
  pixels, which picks a png theme's `@2` directory.

A new scale is a new root, so a rebuilt view. The gallery learns it
from iced (`window::scale_factor`, `window::Event::Rescaled`); Aria's
surfaces from `iced_exwlshell`, later (see "To decide").

## Layout

### The box model

A box's layout node is its **margin box**; it draws and hit-tests its
border box (the node less the margins). `width`/`height` (px, `auto`:
the content, or `fill`) are the border box (`box-sizing: border-box`),
and the border takes room inside it as in CSS, then the padding.
`min-*` (a length, or `auto`: CSS's automatic minimum, below) and
`max-*` (a length, or `none`) clamp the border box.

`fill` is the one extension, documented as such: on the main axis it
is `flex: 1 1 0` (shares the free room with the other `fill`s), on the
cross axis `align-self: stretch`. To iced it's `Length::Fill`, so iced's
own containers understand it.

### The flex subset

A box lays its children out on one line (no `flex-wrap`), in a row or
a column (`flex-direction`; the constructor gives the default, the
theme may change it), `gap` between them. On the main axis a child is
`fill` (CSS's `flex: 1 1 0`) or not (`flex: 0 1 auto`, CSS's initial
value): no other `flex` values, no `flex-grow` factors (UI.md's "no").
Four passes, on iced's `Limits` (min, max, and "compressed": the
parent's size is the content's):

1. **content**: the children not `fill` on the main axis laid out at
   their content size (`max-content`, within the room the box can
   give): their basis. A `fill` child's basis is 0; its content size is
   taken only when the box itself shrinks to its content (compressed,
   so that iced's own widgets answer with theirs too);
2. **the box's size**: definite, or the content clamped by min/max;
3. **the free room**, what the box's inner size leaves after the bases
   and the gaps: shared equally by the `fill` children when there's
   some, taken from the others in proportion to their bases when it's
   negative (CSS's `flex-shrink: 1`). Each child is laid out again at
   its share; one that comes back at another size (larger: its minimum;
   smaller: its `max-*`) keeps it, and what's left is shared again among
   the others, as CSS's loop does. With nobody at a limit, the usual
   case, that's one round;
4. **across**: the box's cross size is definite, or its largest
   child's, clamped by min/max. `align-items` places the children
   across: `stretch` (CSS's initial value: a child whose cross size is
   `auto` laid out again at exactly the box's), `start`, `center`,
   `end`; `fill` across is `align-self: stretch`, whatever
   `align-items` says. The toolkit's `base.css` sets `center` where its
   widgets want it (a button's icon and label); the initial value stays
   CSS's.

The room left on the main axis when no child is `fill` goes where
`justify-content` says: `start`, `center`, `end` (UI.md's "alignment of
a box's content"; no `space-*`, no `align-self` but `fill`'s). Margins
are lengths, never `auto` (CSS's way to push an item over: `fill` does
that here).

A child's **minimum** on the main axis is CSS's automatic minimum
(`min-width: auto`, the initial value) unless a length sets it: a
toolkit widget offered less comes back at its minimum, larger than
offered, and what doesn't fit overflows the box, painted as it is
(`overflow: visible`, the only value). A text that wraps has its
longest word as its minimum; one that doesn't (`white-space: nowrap`),
its whole line; one cut with an ellipsis, zero (as CSS's `overflow:
hidden` makes it); a box, its children's with its gaps, padding and
border. A stock widget clamps to what it's offered (iced's
`Limits::resolve`): its minimum is zero.

So a `fill` item in a menu that shrinks to its content is as wide as
the widest item, capped by the menu's `max-width` (prototype: 260px
menu, five 246px items; iced's flex: 800px); and in a row too narrow
for its label, the icon and the shortcut beside it stop at their
minimum and the label takes the whole cut, ending in its ellipsis. A
child's margins are its own (inside its node), so the parent needs
nothing of the child but iced's `size()` and its layout.

Sizes and positions are rounded to whole device pixels (see Scale),
line boxes too (`line-height` resolved and rounded), which ends Aria's
"parity" trick.

### Why not taffy

taffy is CSS-correct and caches within its tree, but a toolkit box
can't see through `Element`s: each box would build its own taffy tree
and measure its children by calling their iced layout, nested boxes
measuring their own children again at each level. The prototype's
five-item menu, tree kept, took 28µs to lay out with taffy, 1.1µs with
the subset; and a label with `flex-basis: 0` counts nothing in its
row's max-content, so the menu shrank to 115px. The subset is small,
ours, and fast.

### Measuring without a surface

A layer surface needs its size before the first frame. The toolkit lays
the element out headlessly, with the real widgets and the real layout:
`tk::Measure` holds a tiny-skia renderer (`iced::Renderer::Secondary`,
made by `renderer::Headless::new(.., Some("tiny-skia"))`: CPU, no
window) and a widget tree kept per surface, so text is shaped once.
Text is measured by the same cosmic-text paragraphs either renderer
uses. Cost, the prototype's five-item menu: 28µs with a fresh tree
(every text shaped), 1.1µs with a kept one. The root `surface` widget
adds the room its shadow needs, so the measured size is the surface's
(rounded up to whole logical pixels, as a layer surface's size is).
One source of truth: the element is measured at the scale it will be
shown at, its root node's, and what was measured is what the window
lays out.

### Keyed children

iced diffs children by position. A box can be keyed (`.keyed(key)` per
child), so that a list whose items move keeps each item's tree state
(its hover, its transition) with the item.

## Painting

- **Background, border, radius, box-shadow**: one iced `Quad` (outer
  shadow with blur and offset). Linear gradients are in iced's
  `Background` if a theme ever needs them.
- **Borders whose sides differ, with a radius**: a path (outer rounded
  rectangle, inner one inset per side, even-odd fill) through the
  `canvas` geometry renderer from inside the widget (prototyped).
  Per-side colours the same way.
- **Pixel grid**: quads are snapped by iced's wgpu shader (`crisp`, in
  physical pixels: edges sharp at 1.5); border *widths* aren't, so a
  1px border is 1.5 device pixels at scale 1.5 and blurs on one side
  (seen in the nested Sway). The toolkit puts widths, sizes and
  positions on whole device pixels before drawing (see Scale), as
  browsers do; tiny-skia doesn't snap, and with everything on the grid
  it needn't.
- **Inset shadow**: unblurred bands inside the border, as Aria's today.
- **Outline**: a border-only quad around the border box, taking no room
  (the focus ring).
- **Opacity**: iced has no layer opacity (only images and svgs carry
  one). The toolkit multiplies alpha down the tree through `Theme`:
  every colour, image and svg of the subtree fades. Unlike CSS, it
  isn't a group: where children overlap inside a faded box, the lower
  shows through the upper. Written in the reference.
- **Transform**: `translate` and `scale` with iced's
  `Renderer::with_transformation`, painted only (layout untouched, as
  in CSS); the box maps the cursor back through it so hit testing
  follows. `scale` is uniform (`Transformation::scale` takes one
  factor: no `scaleX`/`scaleY`) and `rotate` isn't there (iced's
  `Transformation` has only translate and scale): limits. An svg or
  image rotates (its own `rotation`), which is what a spinner needs.
- **Background images and 9-patches**: CSS's `background-image` and
  `border-image`, the image sliced at load (iced draws whole images:
  no source rectangle), the slices drawn stretched.

### Text

- A paragraph kept in the tree state (`paragraph::Plain`), shaped again
  only when the content or its style changes; `Shaping::Advanced` for
  per-glyph fallback.
- `white-space`: `normal` wraps at words (iced's `Wrapping::Word`: a
  word longer than the line overflows it, as in CSS), `nowrap` keeps
  one line.
- `text-overflow: ellipsis`, on a text that doesn't wrap: when the
  shaped line overflows, the grapheme under the cut is found with the
  paragraph's `hit_test`, and a second paragraph holds the prefix and
  "…". It's made at draw time, for the final width, and kept in a
  `RefCell` of the tree state (`draw` has it read-only) until the
  width or the text changes: the one allocation while drawing, once
  per new width or text, never on a frame at rest. The flex passes lay
  a child out at several widths, and the prototype's first try, cutting
  in `layout`, shaped twice per pass (the menu's layout 17µs; at draw,
  1.1µs). CSS also asks `overflow: hidden`, which ours doesn't have: a
  cut text is its own clip.
- `text-align`, `font-style: italic` (iced's `Font::style`),
  `text-transform` (the string, before shaping), `font-weight`.
  `letter-spacing`: cosmic-text has it, iced doesn't pass it; UI.md
  says no.
- **Centred on its letters**: CSS's `text-box: trim-both cap
  alphabetic`. The font's ascent, descent and cap height are reachable
  (`iced::advanced::graphics::text::font_system()` → cosmic-text's
  fontdb and font metrics: verified), cached per font; the line box is
  trimmed to the capitals' top and the baseline.

### Icons and images

- `icon(node, source)`: its size from the theme (`height`), reserved
  before the file is found: no `Space` placeholders anywhere.
- Symbolic svgs take `color` at draw, so a button's `:hover` and
  `:disabled` reach its icon through the ancestors' states.
- HiDPI: the widget asks an app-provided `IconResolver` for (name,
  size, scale) at draw time, with its node's scale, so a png theme's
  `@2` directories are used on a scale-2 output. The freedesktop index
  stays the app's (Aria has one); the toolkit defines the trait.
- Images: `border-radius` is iced's own (round avatars), `opacity` too.

## States and events

- State lives in `Tree::state`: hover, press, focus, the transition in
  progress, the last frame's time. It survives the rebuild after every
  message, so a hover shows on the first frame, not a rebuild later.
- `update` sets the states from the events, and asks a frame
  (`shell.request_redraw()`) only when a state changed on a sensitive
  node (and the one after, for the transitions the change starts: see
  Animation). Draw reads them.
- A box with `on_press` is a button: `:active` while pressed, the
  message on release over it, `mouse::Interaction::Pointer`.
  `:disabled` when it has none and is marked so.
- **Keyboard focus** (UI.md: "?"): iced 0.14 has the `Focusable` trait
  and the `focus_next`/`focus_previous` operations. A focusable box
  reports itself through `operate`; a `focus_scope` widget takes Tab
  and Shift+Tab and runs the operation on its own subtree from
  `update`, so focus stays per surface (an app `Task` would run on
  every window under `iced_exwlshell`). Enter/Space press the focused
  box; `:focus` and `outline` show it.
- **Accessibility**: none in iced 0.14 (no AccessKit). A limit.
- **Tooltips, menus, selects**: the toolkit draws them; where they
  appear is the app's. A tooltip widget publishes "show this tip at
  this rectangle" after the hover delay and "hide" on leaving: a bar
  opens a popup surface for it (a tooltip inside a 30px layer surface
  would be cut), the gallery shows it in an iced overlay
  (`tooltip_layer`). The same for a select's list.

## Animation

- **Transitions** (`transition: <property> <duration> <easing>
  <delay>`): colours (interpolated premultiplied, as CSS does: the
  prototype's plain interpolation from `transparent` went through grey;
  `color`, `background-color`, `border-color`, `outline-color`),
  opacity, transform, `rotate`, shadow. Layout properties don't
  animate.
- **Keyframes** (`@keyframes`, `animation: name duration easing count
  direction fill-mode`): what transitions move, and an svg's or an
  image's turn (CSS's `rotate`, which only they take). Started when the
  widget first appears (its tree state is created) or when its class
  changes to one naming an animation. A property a step leaves out comes
  from the steps around it that set it, or from the element's own value
  at the ends, as CSS's implicit keyframes.
- **Easing**: CSS's keywords and `cubic-bezier()`, `steps()`; our own
  solver (iced's `Animation` uses lilt, whose custom easing is a plain
  `fn`: no parameters).
- **Where a transition lives**: it starts at draw, where the widget's
  target style is resolved (its own states, its node, its ancestors'
  states through `Theme`), and is kept in its tree state behind a
  `Cell`: `draw` has the tree read-only, and a descendant learns its
  ancestors' states nowhere else (`update` gets no `Theme`). iced's own
  `lazy` keeps a `RefCell` in its tree state the same way. A transition
  is anchored to the instant of its cause, the widget's own change or
  the ancestor's that `Theme` carries, not to the frame that first
  draws it: a descendant scrolled back into view doesn't replay it.
  Interrupted, it goes on from the value it reached (a reversed one
  shortened as CSS does).
- **Frames**: a frame runs `update`, with `RedrawRequested`, then
  `draw` (`iced_exwlshell`'s `multi_window.rs`). A widget with a
  transition running asks the next frame from `update` until it ends;
  a widget whose states or node changed asks one frame more than the
  change's, since the transitions its change starts at draw reach
  their own `update` only at the next. `iced_exwlshell` turns that into
  a frame of that surface only, with no view rebuild (verified in
  `multi_window.rs`): an animation costs an update and a draw of its
  own surface per frame. Times come from `Instant::now()` in `update`,
  never from the last frame (the prototype's first try took the last
  frame's time, which was seconds old, and stopped every transition on
  its first frame: seen only in the real window).
- **Exit animations**: the app adds a class (`.leaving`), the widget
  runs it and publishes `on_animation_end`; then the app closes the
  surface.

## Widgets

Each has its element tree in `assets/base.css`, every element and
class with its line, and neutral defaults there. The first set, from
UI.md §7:

| Widget | What it is |
|---|---|
| `row`, `column` (box) | the box model and the flex subset; `on_press` makes it a button |
| `stack` | children on top of each other (a badge on an icon) |
| `text` | paragraph, ellipsis, trim |
| `icon`, `image` | above |
| `button` | a box with `on_press`, the `button` element, its variants as classes |
| `toggle` | a button holding its `knob`, `.on` when it is; the knob slid by the theme's transform |
| `slider` | its track a meter, its `knob` centred on the value; dragged, clicked, scrolled, keys when focused |
| `input` | iced's `text_input` under our `Catalog`, in a box with the app's children before and after it (an icon, the peek button); `:focus` while its field has it |
| `scroll` | iced's `scrollable` under our `Catalog` (`scrollbar.vertical`, `.horizontal`, `thumb`); no box of its own, its sizes its node's (`max-height`: it grows, then scrolls); sideways, it takes the wheel sideways |
| `menu` | checks, radios, submenus, separators |
| `calendar` | day and month names given by the app (no locale in the toolkit) |
| `tabs` | |
| `meter` | its `fill` at a fraction of its content box; with a `label` over it, a gauge |
| `sparkline`, `graph` | canvas paths |
| `tooltip`, `select` | above (select later) |
| `surface` | a surface's root: the shadow's room around its box, clicks on the room ignored |
| `focus_scope` | above |

Popups, toasts and dialogs are `surface` roots with their content: the
look is the theme's, the surface Aria's.

## Querying the tree

Every widget reports its node and border box through `operate`
(`Operation::custom` with the node as state), where it's shown: the
report moves a scroll's content as it's scrolled (iced's scrollable
gives operations its translation, and lays its content out unmoved). `query.rs` runs a CSS
selector over that: Aria's `debug widgets 'launcher item:nth-child(2)'`
on top of it, and an `iced_test::Selector` impl so a test clicks
"`menu item.active`". Every widget is found, text and canvas included
(today only the theme helpers' containers are).

## Testing

All without a compositor, in `cargo test`:

- **Style engine**: parsing, selectors, cascade, variables, warnings.
- **Layout**: an element measured headlessly, its nodes' rectangles
  asserted (the prototype printed exact rectangles this way).
- **Events and pixels**: `testing::Harness` drives iced's
  `UserInterface` with the tiny-skia renderer: pointer moves, presses,
  keys, frames at chosen times (a transition sampled frame by frame),
  and RGBA screenshots at any scale (1, 1.5, 2: what Aria never tried
  on a scaled output). `iced_test::Simulator` works too, at its fixed
  scale 2.
- **Golden images** per widget and state, with an embedded font so
  system fonts don't change them; tiny-skia only (wgpu draws
  differently by a few levels).
- **The wgpu path**: the gallery in a nested headless Sway (an output at
  scale 1.5, input through `tests/ui/inject`, `grim`), as the
  prototype was checked; and Aria's own scenarios.

## The gallery

`examples/gallery.rs`, a plain `iced::application` window (no
layer-shell): every widget in every state side by side (states can be
forced on a box, as a browser's devtools do), the theme file given on
the command line and reloaded on change, light and dark. It's where a
theme is written and where the golden images come from.

## Performance

| UI.md rule | How |
|---|---|
| styles resolved once | interned nodes, cached computed styles; a view pays ~32ns a node |
| no allocations while drawing | styles held as `Arc`s, paragraphs in the tree, the context copied inline and only where it changes; the ellipsis cut the one exception, once per new width or text |
| what a layout costs kept from one layout to the next | shaped text and ellipsis cuts kept in the tree: ~0.3µs an item; the layout itself runs on every rebuild of a surface's tree, changed or not (a limit) |
| an idle UI costs nothing | frames only on a state change of a sensitive node, or an animation running |
| an animation asks frames only while it runs | above |
| measured, before and after | `benches/`: view, layout, update, draw of fixed trees with the headless renderer; Aria's CPU at rest in the nested Sway, as ARCHITECTURE.md measured it |

Measured (`cargo bench -p tk --features testing`, this laptop, 2026-10,
µs): a theme's load (the base and the gallery's) 374.

| tree | view | rebuild (view, diff, layout) | update | frame |
|---|---|---|---|---|
| a bar, 12 gadgets | 1.6 | 4.2 | 0.8 | 3.2 |
| a menu, 20 items | 3.8 | 17 | 1.4 | 6.1 |
| a list, 500 rows | 56 | 198 | 22 | 80 |

Not the toolkit's to fix: `iced_exwlshell` 0.20 rebuilds every
surface after every message (0.21: only the drawn ones), and a frame
is always a full redraw (no damage in iced).

## Order of work

Each step ends in the gallery and in `cargo test`, measured where
UI.md's rules ask it. The tests come once a step's look is agreed on,
the headless harness and the query with them.

1. ✅ The crate, the gallery window, the style engine (sheets,
   selectors, cascade, variables, schemes, nodes, computed styles,
   warnings): the gallery loads a theme, and reloads it when the file
   changes.
2. ✅ Boxes, text and icons: the box model, the flex subset, the scale,
   painting (background, border, radius, shadow), wrapping, ellipsis
   and trim; `Measure`; the headless harness (`testing`), the golden
   images, `benches/`.
3. ✅ States and events: hover, press, focus, disabled, the ancestors'
   states through `Theme`; buttons, `focus_scope`, the query.
4. ✅ The rest of painting and text: borders whose sides differ,
   outline, opacity, transform, background images and 9-patches;
   `text-align`, italic, `text-transform`; images.
5. ✅ Animation: transitions, keyframes, exit animations.
6. The widgets of UI.md §7: toggle ✅, slider ✅, meter and gauge ✅,
   surface ✅ (with step 2), input ✅, scroll ✅, menu, calendar,
   tabs, graphs, tooltip; select last.
   A widget placing its children its own way (a meter's fill, a
   slider's knob) shares the box's states, painting and animation
   (`widget/boxed.rs`) and its box model (`layout::Sizing`).

## Limits (iced 0.14)

- `rotate` only on svgs and images; `scale` uniform only.
- A transform is a box's: a text or an icon moves with its box.
- On tiny-skia (the CPU renderer), a stretched image is placed on a grid
  as coarse as its stretch, and an image's corners aren't rounded: a
  9-patch, a stretched background or a rounded picture are right on
  wgpu only. The golden images keep to images at their own size.
- Opacity isn't a group (above).
- No accessibility.
- No damage tracking: a frame redraws its whole surface.
- Layout runs on every rebuild of a surface's tree, changed or not.
- A stock widget needs our `Catalog` impl before it can be used; its
  colours (an input's text, placeholder and selection, a scrollbar's)
  are asked at draw, so they don't transition. A scrollbar's sizes are
  set at the view, so they follow no state, and the query doesn't find
  it.
- Layout gets no context from the parent: nodes are explicit.
- A descendant's layout can't follow an ancestor's state.
- Operations run on every window (`iced_exwlshell`): ids are unique per
  instance, focus moves inside a scope.

## To decide

- The crate's name, and its licence (GPL-3 like Aria, or a laxer one
  for a toolkit others may use).
- `flex-direction` in the theme or only in the code (proposed: the
  constructor's default, the theme may change it).
- Where the freedesktop icon index lives: Aria (proposed), the toolkit,
  or a small crate of its own.
- How Aria learns a surface's scale for its root node when it's
  fractional (with Aria's move, later): sctk's `OutputInfo` has the
  integer `scale_factor`; the fractional one (`wp-fractional-scale`)
  only reaches `iced_exwlshell`'s viewport, and no event tells the app
  (0.20 and 0.21: no `Rescaled`).
