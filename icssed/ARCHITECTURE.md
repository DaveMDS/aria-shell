# Toolkit architecture

How the toolkit is built, and why: a crate of CSS-styled widgets on
iced, knowing nothing of Aria. How to use it and work on it (its calls,
costs, invariants and traps, how to verify) is AGENTS.md, beside this;
every element, class and property is `assets/base.css`.

Every claim about iced (0.14, the version `iced_exwlshell` 0.20/0.21
runs on) was checked in its source in `~/.cargo/registry`. The numbers
were measured on a throwaway prototype first (2026-10, this laptop,
release build), which drew a themed menu headlessly and in a real wgpu
window inside a nested headless Sway at scale 1.5, and then on the
crate itself (AGENTS.md, "What things cost").

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
- **Nodes are found by the toolkit, the first time the runtime reaches
  a widget**: a view says each element's type and classes
  (`button![..].kind("item").class_if("active", on)`); iced's `layout`
  gets no context from the parent, but a container reaches its children
  (making and diffing their trees) before any of them is measured, and
  hands them its node and their positions then.
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
  scale; the wgpu path through the demo in a nested Sway.

## The road: three shapes weighed

1. **Stock iced widgets, styled by closures** (Aria before the
   toolkit). Measured in the prototype: iced's `column` of `fill` items
   inside a shrinking `container` takes the whole 800px offered, not the
   widest item; containers have no hover state (Aria's `mouse_area`
   workarounds); a widget's status lives in the widget and dies with
   every rebuild (Aria's ARCHITECTURE.md: the late hover); a border
   takes no room; an ancestor's state can only reach the text colour.
   Each of these breaks one of the rules in AGENTS.md, so: no.
2. **A tree of our own inside one iced widget** (a mini retained-mode
   engine per surface). Full control of cascade and layout, but
   events, focus, overlays, operations, text editing and scrolling all
   rewritten, and iced's own widgets no longer composable. iced's
   machinery isn't the cost: view, diff and layout of a 20-item menu of
   stock widgets, tree kept, is 5.8µs, 0.3µs an item. No reason to
   leave it.
3. **Our widgets on iced's advanced API**, with our `Theme` type.
   Prototyped first, then built: everything needed goes through public
   API. This is the road taken.

Measured on a bar-sized tree (12 gadgets, ~70 nodes, a sheet of ~280
rules), per rebuild: prototype view 5.6µs, diff+layout 4.6µs, update
2.2µs, draw 4.1µs; the same bar in stock iced widgets 4.3 / 3.9 / 2.1 /
2.2µs. Both are noise next to a frame.

## Crate

A workspace member (`icssed/`), its own `Cargo.toml`, depending on:

- `iced` 0.14 with its defaults (`wgpu`, `tiny-skia`, `crisp`: quads
  snapped to the pixel grid) and `advanced`, `svg`,
  `image-without-codecs`, `canvas` (paths: borders with sides that
  differ, graphs). With `tiny-skia` on, `iced::Renderer` is iced's
  fallback enum (wgpu first, tiny-skia second) for the whole app: a
  `match` per call, nothing else. tiny-skia is also the headless
  renderer: measuring and tests. `iced_runtime` 0.14 directly, for
  `UserInterface` in the headless harness (iced doesn't re-export it;
  it's in iced's tree anyway).
- `cssparser` (Servo's CSS Syntax Level 3 tokenizer, MPL-2.0: compatible
  with GPL-3): correct tokens, escapes, comments, numbers with units and
  error recovery, our grammar on top. "CSS means CSS" starts at the
  tokenizer.
- `image` 0.25 (png, jpeg, webp) for a theme's images, decoded at load:
  iced draws whole images, so a 9-patch is cut here.
- `rustc-hash` for the interner and caches (a node lookup: 65ns with
  std's SipHash, 32ns with FxHash, measured); `log`.
- dev: `iced` with `tokio`, `tokio`, `notify` and `env_logger`, for the
  demo (its sheets and theme watched).

`iced` is pinned to what `iced_exwlshell` runs on; the toolkit moves
when Aria can. The modules are mapped in AGENTS.md.

## The style engine

### Sheets

`Sheet::load(sources, scheme) -> (Sheet, Vec<Warning>)`: the toolkit's
`base.css`, then the app's base (Aria's elements), then the user theme,
one cascade. The toolkit's base is under the others as CSS's user-agent
stylesheet is: any rule of the app's or the theme's wins over its,
whatever their specificity (else its `:root { color }` would beat a
theme's `surface.panel { color }`). Variables (`:root { --x }`, `var(--x,
fallback)`) and schemes (`:root.light` / `:root.dark` folded over the
plain ones, the root elements carrying the scheme as a class) work as
Aria's did: a sheet is compiled for one scheme; switching scheme or
reloading the file is a new `Sheet`, and the views are rebuilt. Bad
selectors and declarations are warnings, skipped; a syntax error
rejects the file (the caller keeps the last good sheet).

Rules are indexed by the type of their rightmost compound (rules naming
no type kept apart, merged back in cascade order): trying all ~280 on
every node was 46% of Aria's CPU with the pointer moving (Aria's
ARCHITECTURE.md).

Selectors: type, `.class`, `#id`, `[attr="value"]`, `*`, descendant,
child (`>`), `:hover :active :focus :disabled :first-child :last-child
:nth-child() :nth-last-child() :root`. Specificity then source order.
No `!important`, no `@media`; `@keyframes` yes (see Animation).

### Nodes

A widget's place is a `Node<'a>`: `Copy`, a reference to the sheet and
an interned id. The view doesn't build it: it says what each element is,
and the toolkit finds where.

```rust
fn view<'a>(&'a self) -> icssed::Element<'a, Message> {
    let items = self.items.iter().enumerate().map(|(i, it)| {
        icssed::button![icssed::icon(it.icon.clone()), icssed::text(&it.label)]
            .kind("item")
            .class_if("active", it.active)
            .on_press(Message::Pick(i))
            .into()
    });
    icssed::column(items).kind("menu").into()
}
```

A surface's tree starts from its root element, `sheet.root(name,
scale)`, given to the top of the view (`.at(root)`): the scale is
carried by every node under it (see Scale). Every root is a
`surface`, its name a class (`surface.popup`): a sheet styles every
surface at once, as CSS styles every `button`, and one by its class. Each step interns one
`(previous id, Kind | Class | Id | Attr | Position | Disabled)` → id,
so a node is a chain of steps with a canonical id. Names (types,
classes, ids, attribute values) are interned once for every sheet,
`Name`s, so a widget names its type and classes before it knows its
sheet; a thread's lookups skip the lock. A position step is only kept
when the sheet has positional pseudo-classes, and then only as the set
of them the child matches: otherwise the 500 rows of a list share one
node, and one style (measured: 1500 node lookups of a 500-row list
resolve 8 styles, 60µs cold, 48µs warm).

**How a widget finds its place** (`widget/place.rs`). A widget holds a
`Place`: its type and the steps after it until it's reached, its node
and its style at rest after (cached in cells: the first touch may be a
`&self` method). iced builds a tree's state top-down before any layout
(`Tree::new`, `Tree::diff`), and a container makes and diffs its
children's (`place::trees`, `place::diff`): there it sets, in a
thread-local, its sheet, its node and the child's position, and the
child resolves itself the first time it's touched (`tag`, `state`,
`children`, `diff`, `size`). Its own children follow under it. A
surface hands its content its own root node (`At::Is`), a slot the
node it found for itself (a frame saying what a page is), a tooltip
its tip a place beside its own siblings (`At::Aside`). A wrapper of
iced's that keeps out of the thread-local (`mouse_area`, `Map`) is
transparent: its child takes its place.

The node keeps a reference to the sheet that the thread-local holds as
a pointer: the one `unsafe`, sound because a child is owned by the
container that reached it, which borrows the sheet for as long as it
lives. A widget reached outside a container of the toolkit (the top of
a view without `.at(root)`, a child of iced's `stack`, which measures
its children as they're built) has no place: it panics, naming its
type.

Why found and not passed: a view written with nodes threaded through
every call (`row.child("button")`, then `button.child("label")` for each
part) was long, and a widget adding a class to itself after its
children were built (a toggle's `.checked`) couldn't reach them. A tree
of descriptions mounted in a second pass cost 13–50% of a rebuild; this
costs about 5% (`benches/costs.rs`), the node steps the same, moved from
the view to the first diff.

### Computed styles

A `Computed` holds every property, inherited ones from the parent's.
The cache key is (node, own states, ancestors' states): `node.base()`
is the style at rest, taken when the widget finds its place and held by
it for layout; `node.resolve(states, ancestors)` at draw, for a node not at
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
applied, and the sheet is to say so at load (not done yet): the
descendant has no way to ask its own relayout at draw time. What a
state changes, its descendants inherit (a button's `color` under
`:hover` is its label's): a box hands its states down to its children
whenever some rule sees them; an inherited layout property
(`font-size`) reaches only their paint.

## The draw-time context: `Theme`

iced threads a `&Theme` through `draw` (and through every stock
container unchanged), and each window gets its own from the app
(`theme(state, window)`). The toolkit's `Theme` is that channel, per
subtree, from the parent:

- the **opacity** so far;
- the **states of the ancestors** a descendant may depend on (depth,
  states), pushed by a stateful box only while it isn't at rest;
- the last instant an ancestor's states **changed**, which a
  descendant's transition starts from (see Animation);
- and one thing the other way, from the children back up: whether one
  of them painted `-debug: box`, a `Cell` a box clears before drawing
  them and reads after, so that only the innermost shows its boxes
  (`* :hover` matches every hovered ancestor too).

A parent hands its children a modified copy only when something
changes (a hovered item, a faded box, a box changed later than its
ancestors); otherwise the same reference. The copy is inline, the
ancestors a small fixed array: iced's `Theme` parameter is one owned
type for the whole app, so a child's can't borrow its parent's, and a
copy mustn't allocate. The scale isn't here: layout needs it too, and
gets no `Theme` (see Scale).

It implements `iced::theme::Base` (all `iced_exwlshell` asks) and the
`Catalog` traits of the stock widgets the toolkit embeds: `text_input`
(`icssed::input`'s field: its colours from `field`, `field > placeholder`,
`field > selection`) and `scrollable` (`icssed::scroll`: `scrollbar`,
`scrollbar > thumb`, `:hover`/`:active` as iced reports them). Every
other stock widget needs a `Catalog` impl before it can be used: that's
deliberate, an unthemed widget can't slip in.

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
  (`sheet.root(name, scale)`); every node under it carries it, and the
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

A new scale is a new root, so a rebuilt view. The demo learns it
from iced (`window::scale_factor`, `window::Event::Rescaled`); Aria
asks `window::scale_factor` of `iced_exwlshell` when a surface appears
and when its output changes (no event tells it).

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
a column (`flex-direction`: the constructor gives the default, the
theme may change it), `gap` between them. On the main axis a child is
`fill` (CSS's `flex: 1 1 0`) or not (`flex: 0 1 auto`, CSS's initial
value): no other `flex` values, no `flex-grow` factors. Four passes, on
iced's `Limits` (min, max, and "compressed": the parent's size is the
content's):

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
   its share, across at its content (a child that stretches takes the
   box's cross size in 4., not the room the box was offered: it once
   made a bar of `fill` gadgets as tall as its output when they
   shrank); one that comes back at another size (larger: its minimum;
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
`justify-content` says: `start`, `center`, `end` (no `space-*`, no
`align-self` but `fill`'s). Margins are lengths, never `auto` (CSS's way
to push an item over: `fill` does that here).

A child's **minimum** on the main axis is CSS's automatic minimum
(`min-width: auto`, the initial value) unless a length sets it: a
toolkit widget offered less comes back at its minimum, larger than
offered, and what doesn't fit overflows the box, painted as it is
(`overflow: visible`, the only value). A text that wraps has its
longest word as its minimum; one that doesn't (`white-space: nowrap`),
its whole line; one cut with an ellipsis, zero (as CSS's `overflow:
hidden` makes it); a box, its children's with its gaps, padding and
border; an empty box, zero, so it's the first to give way when a box
is squeezed. A stock widget clamps to what it's offered (iced's
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

A widget placing its children its own way (a meter's fill at its
fraction, a slider's knob on its value) sizes its own box with the same
model (`layout::Sizing`, `content_box`) and shares the box's states,
painting and animation (`widget/boxed.rs`).

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
`icssed::Measure` holds a tiny-skia renderer (`renderer::Headless::new(..,
Some("tiny-skia"))`: CPU, no window) and a widget tree kept per surface,
so text is shaped once. Text is measured by the same cosmic-text
paragraphs either renderer uses. The root `surface` widget adds the room
its shadow needs, so the measured size is the surface's (rounded up to
whole logical pixels, as a layer surface's size is). One source of
truth: the element is measured at the scale it will be shown at, its
root node's, and what was measured is what the window lays out.

## Painting

- **Background, border, radius, box-shadow**: one iced `Quad` (outer
  shadow with blur and offset). Linear gradients are in iced's
  `Background` if a theme ever needs them.
- **Borders whose sides differ, with a radius**: paths through the
  `canvas` geometry renderer, from inside the widget: a ring (the outer
  rounded rectangle and the inner one inset per side, even-odd) when
  the sides that show share a colour, a region per side otherwise,
  split at the corners along their diagonal as CSS does.
- **Pixel grid**: quads are snapped by iced's wgpu shader (`crisp`, in
  physical pixels: edges sharp at 1.5); border *widths* aren't, so a
  1px border would be 1.5 device pixels at scale 1.5 and blur on one
  side (seen in the nested Sway). The toolkit puts widths, sizes and
  positions on whole device pixels before drawing (see Scale), as
  browsers do; tiny-skia doesn't snap, and with everything on the grid
  it needn't.
- **Inset shadow**: unblurred bands inside the border.
- **Outline**: a border-only quad around the border box, taking no room
  (the focus ring), following the border's corners.
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
  no source rectangle), the slices drawn stretched. A background has
  layers, as in CSS: its longhands are lists, matched up once when the
  style is computed (`Computed::backgrounds`, each layer cut in nine
  there when `-background-slice` says, the cut kept with the picture);
  not CSS, that slice: e17-style themes stack a gradient and a bevel on
  one box, which CSS can only do with a border. The layers are drawn
  in one layer that clips them to the border box. What the box draws
  after it (its border, its children, its outline) goes in a layer of
  its own made after that one, clipped to the viewport: wgpu draws a
  layer over everything in the layer around it, so in that one it
  would go under the picture.
- **Paths** (a graph's series): painted as SVG's (`fill`, `stroke`,
  `stroke-width`), kept in iced's geometry cache until the values, the
  size or the paint change.

### Text

- A paragraph kept in the tree state (`paragraph::Plain`), shaped again
  only when the content, its style or its width changes (a line that
  doesn't wrap is shaped once, at any width); `Shaping::Advanced` for
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
- `text-align` (iced aligns a paragraph's lines among themselves; where
  the paragraph goes in its box is the widget's to say), `font-style:
  italic` (iced's `Font::style`), `text-transform` (the string, before
  shaping), `font-weight`. No `letter-spacing`: cosmic-text has it,
  iced doesn't pass it.
- **Centred on its letters**: CSS's `text-box: trim-both cap
  alphabetic`. The font's ascent, descent and cap height are reachable
  (`iced::advanced::graphics::text::font_system()` → cosmic-text's
  fontdb and font metrics), cached per font; the line box is trimmed to
  the capitals' top and the baseline, the baseline put on a whole
  device pixel. An input's field, iced's, is laid out the same way:
  its box trimmed, its line box moved so the baseline lands where a
  label's would.

### Icons and images

- `icon(source)`: its size from the theme (`width`, `height`, one
  of them for both, else the font size), reserved before the file is
  found: no `Space` placeholders anywhere.
- Symbolic svgs take `color` at draw, so a button's `:hover` and
  `:disabled` reach its icon through the ancestors' states.
- HiDPI: a named icon is looked up at draw time through the app's
  `Icons` (`lookup(name, size, scale)`), with its node's scale, so a png
  theme's `@2` directories are used on a scale-2 output. The
  freedesktop index stays the app's (Aria has one); the toolkit defines
  the trait.
- Images: `object-fit`, `border-radius` (iced's own: round avatars),
  `opacity`.

## States and events

- State lives in `Tree::state`: hover, press, focus, the transition in
  progress. It survives the rebuild after every message, so a hover
  shows on the first frame, not a rebuild later.
- `update` sets the states from the events, and asks a frame
  (`shell.request_redraw()`) only when a state changed on a sensitive
  node (and the one after, for the transitions the change starts: see
  Animation). Draw reads them.
- A box with `on_press` is a button: `:active` while pressed, the
  message on release over it, `mouse::Interaction::Pointer`.
  `:disabled` when it's marked so: nothing to press then.
- **Keyboard focus**: iced 0.14 has the `Focusable` trait and the
  `focus_next`/`focus_previous` operations. A focusable widget reports
  itself through `operate`; a `focus_scope` widget takes Tab and
  Shift+Tab and runs the operation on its own subtree from `update`, so
  focus stays per surface (an app `Task` would run on every window
  under `iced_exwlshell`). Enter/Space press the focused box; `:focus`
  and `outline` show it. A press of the pointer drops the focus before
  the content sees it, so a pressed text field takes it back. Tab
  leaves the scope at its ends, as iced's operations have it; a menu's
  and tabs' scopes move it with the arrows instead, round from one end
  to the other (an operation of ours), once one of theirs has it; a
  grid's (`.arrows()`) with all four, to the nearest that way in the
  focused one's row or column, found from where the focusables are.
- **Accessibility**: none in iced 0.14 (no AccessKit). A limit.
- **Tooltips, menus, selects**: the toolkit draws them; where they
  appear may be the app's. A tooltip shows its tip after the pointer
  rests (600ms unless said), under what it's about or over it where
  there's no room, in an iced overlay: what a window wants. A bar's
  tips can't be overlays (a 30px layer surface would cut them): with
  `on_tip` the tooltip publishes the rectangle to show it by, then that
  it's gone, and the app opens a popup surface with `icssed::tip` in it.
  The rectangle is where it's shown: the overlay pass tells the widget
  how far the scrolls around it moved it. The same for a select's
  options (`on_open`). Every container hands its children's overlays
  up (`overlay::from_children`), or an iced overlay inside it would
  never show.

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

Each has its element tree in `assets/base.css`, every element and class
with its line, and neutral defaults there; AGENTS.md has their calls.
They come in a few kinds:

- **Boxes** (`row`, `column`, `button`, and what's made of them: a
  toggle is a button holding its icon and its label, and a check box's
  mark, a radio's dot or a switch's knob before them; tabs are buttons
  in a row; a menu is a column of buttons; a calendar is boxes and
  texts). Their states, painting and animation are `widget/boxed.rs`'s.
- **Boxes placing their children their own way**: a meter's fill at
  its fraction, a slider's knob on its value (its track a meter), a
  graph's paths in its content box. Their box model is `layout::Sizing`.
- **Stock widgets under our `Catalog`**: an input's field (iced's
  `text_input`, in a box of ours), a scroll (iced's `scrollable`, no box
  of its own).
- **Widgets with an overlay**: a tooltip's tip, a select's options;
  the app's surface instead where an overlay can't go.
- **A surface's root** (`surface`): the room its shadow needs around
  its box. Popups, toasts and dialogs are `surface` roots with their
  content: the look is the theme's, the surface the app's.

Controlled throughout: the app holds every value and each widget sends
its messages. A widget keeps only what the pointer and the keyboard do
(hover, press, focus, transitions, an open tooltip or select); state it
owns at runtime shows as a pseudo-class (an open select is `:active`),
since classes are the view's.

## Querying the tree

Every widget reports its node and border box through `operate`
(`Operation::custom` with the node as state), where it's shown: the
report moves a scroll's content as it's scrolled (iced's scrollable
gives operations its translation, and lays its content out unmoved).
`query.rs` runs a CSS selector over that, `:nth-child()` included even
where the sheet has none (positions come from the report's tree): the
harness's `find` and `click_on`, and Aria's `debug widgets 'launcher
item:nth-child(2)'` on top of it. `Sheet::matching` says which widgets
match, `Found::path` names one: an operation runs on every window, and
a widget's root is how an app tells their surfaces apart. Every widget
is found, text and paths included; iced's own scrollbars aren't.

## Testing

All without a compositor, in `cargo test`: the style engine (parsing,
selectors, cascade, variables, warnings); layout (an element measured
headlessly, its nodes' rectangles asserted); events and pixels
(`testing::Harness` drives iced's `UserInterface` with the tiny-skia
renderer: pointer moves, presses, keys, text, frames at chosen times,
RGBA screenshots at any scale); golden images per widget and state, at
scale 1 and 1.5, with embedded fonts so the system's don't change them,
tiny-skia only (wgpu draws differently by a few levels). The wgpu path
is checked with the demo in a nested headless Sway. How, and the
traps: AGENTS.md, "Verifying".

## The demo

`examples/demo/`, a plain `iced::application` window (no layer-shell),
after gtk4-demo: a demo per widget and per subject, each shown live
with its description, its Rust and its CSS. Its first page, the
overview, is every widget at once in its states, as the base and the
theme draw them, with nothing of the demo's own: where a theme is
written and looked at, for a design that holds together. The theme file
given on the command line and the demos' sheets are reloaded on
change; light and dark.

## Performance

| Rule | How |
|---|---|
| styles resolved once | interned nodes, cached computed styles; a view pays ~32ns a node |
| no allocations while drawing | styles held as `Arc`s, paragraphs and path geometry in the tree, the context copied inline and only where it changes; the ellipsis cut the one exception, once per new width or text |
| what a layout costs kept from one layout to the next | shaped text and ellipsis cuts kept in the tree; the layout itself runs on every rebuild of a surface's tree, changed or not (a limit) |
| an idle UI costs nothing | frames only on a state change of a sensitive node, or an animation running |
| an animation asks frames only while it runs | above |
| measured, before and after | `benches/costs.rs`: view, layout, update, draw of fixed trees with the headless renderer; the numbers in AGENTS.md, "What things cost" |

Not the toolkit's to fix: `iced_exwlshell` 0.20 rebuilds every
surface after every message (0.21: only the drawn ones), a frame is
always a full redraw (no damage in iced), and iced asks the whole tree
for overlays at every event and every frame.

## Limits (iced 0.14)

- `rotate` only on svgs and images; `scale` uniform only.
- A transform is a box's: a text or an icon moves with its box.
- On tiny-skia (the CPU renderer), a stretched image is placed on a grid
  as coarse as its stretch, and an image's corners aren't rounded: a
  9-patch, a stretched background or a rounded picture are right on
  wgpu only. The golden images keep to images at their own size.
- Opacity isn't a group (above).
- iced_wgpu merges a layer that starts with images into the one below
  it, and draws a layer's svgs before its rasters, whatever came first:
  icons over a background image vanished. The layer over a background
  image starts with a transparent quad, which keeps the two apart
  (`paint.rs`, TEMPORARY: one quad per such box, until iced draws them
  in order: iced-rs/iced#3512). Among a box's background layers, drawn
  in one layer, an svg one goes under every raster one, wherever it is
  in the list.
- No accessibility.
- No damage tracking: a frame redraws its whole surface.
- Layout runs on every rebuild of a surface's tree, changed or not.
- A stock widget needs our `Catalog` impl before it can be used; its
  colours (an input's text, placeholder and selection, a scrollbar's)
  are asked at draw, so they don't transition. A scrollbar's sizes are
  set at the view, so they follow no state, and the query doesn't find
  it.
- Layout gets no context from the parent: a widget finds its place
  when its tree is made, so it has to be in a container of the toolkit,
  or given its node.
- A descendant's layout can't follow an ancestor's state.
- Operations run on every window (`iced_exwlshell`): ids are unique per
  instance, focus moves inside a scope.

## Not done yet

- The sheet's warning at load for a layout property under an
  ancestor's state (see "What a state may change").
- Keyed children: iced diffs children by position, so a list whose
  items move would keep each place's tree state (its hover, its
  transition), not each item's. A box keyed per child (`.keyed(key)`)
  would carry them along.
- `stack`: children on top of each other (a badge on an icon).
- Percentages in the layout's lengths (`width`, `height`, their min and
  max, `padding`, `margin`, `gap`): of the containing block, as CSS has
  them (padding and margin of its width, both ways; `auto` where it's
  indefinite), resolved in `layout.rs`'s flex; no use for them yet,
  `fill` takes the free room. `<length-percentage>`
  (`style::LengthPercentage`) is there for them.
- States forced on a box in the demo, as a browser's devtools do.

## To decide

- The crate's name, and its licence (GPL-3 like Aria, or a laxer one
  for a toolkit others may use).
- Where the freedesktop icon index lives: Aria for now (its
  `services/icons`, given to `icssed::icon` as `&dyn Icons`), the
  toolkit, or a small crate of its own.
