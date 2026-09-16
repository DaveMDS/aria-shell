# Rust port

## Why

`aria-shell` was Python + GTK4 + gtk4-layer-shell (~8.6k lines, now under
`aria-shell-python/`). GTK4 turned out to be an uncomfortable fit: clunky
development experience and, more importantly, GTK's CSS is too limited for
the level of visual customization we actually want.

### Alternatives considered

- Qt: ruled out from the start.
- EFL/Edje (the author has 20 years of history with `python-efl`): the EFL
  project is effectively dead, and it would be a more extreme jump than
  needed.
- A "real" CSS engine without GTK (Blitz/Stylo): on Linux any webview with
  real CSS still needs WebKitGTK underneath, so GTK comes back plus a whole
  browser engine. Blitz standalone is too young to bet a rewrite on.
- **Chosen: Rust + `iced` + `iced_exwlshell`** (formerly `iced_layershell`
  + `iced_sessionlock`, merged as of v0.20): lightweight (wgpu), non-Qt,
  non-GTK, actively maintained Wayland layer-shell support. `iced`'s
  `shader` widget gives direct per-widget wgpu access, which covers the
  "more visual freedom than CSS" requirement without committing to a
  specific "skin format" yet.
- **COSMIC** (System76, a production DE on an `iced` fork, `libcosmic`)
  has shipped every subsystem we need: multi-monitor layer-shell panels,
  SNI tray, notification daemon, PAM lock screen. Study and adapt their
  patterns as reference; stay on vanilla `iced`, don't depend on
  `libcosmic`.

### Known risks (from real research, not memory)

- PAM in Rust is the weakest link: even COSMIC's official greeter has open
  production auth bugs. Highest-risk unimplemented piece.
- The idle-notifier protocol, raw PipeWire volume control and a
  GStreamer→wgpu bridge for video all lack mature crates; expect to
  hand-roll them.
- `iced_exwlshell` is a small, fast-moving crate: expect API churn, verify
  against its source in `~/.cargo/registry` rather than memory.

## Relationship to the Python implementation

The Python code is a **behaviour** reference, not a structure reference.

Kept on purpose (it's the user-facing contract):
- the `aria.conf` format: INI, case-sensitive, `[Name]` / `[Name:id]`
  instances, empty value = default, same keys and defaults per section;
- the feature list and the semantics of each gadget/component.

Deliberately **not** mirrored: `Singleton` metaclass, the `AriaService`
base, the `AriaModule`/`Gadget` split, runtime reflection over config
models, dynamic `importlib` module loading. Those solved GTK/Python
problems that iced doesn't have. Don't reintroduce them, and don't add
"mirrors `foo.py`" comments for structure, only where a *behaviour* is
being reproduced.

## Architecture

Plain Elm architecture as iced defines it, nested once per layer. Every
layer is a struct with its own `Message`, `update`, `view` and
`subscription`; the parent routes by key and `.map()`s messages up.

```
AriaShell  (main.rs)        daemon; owns Config, ShellReceiver, Compositor, panels: BTreeMap<window::Id, Panel>
  Message::Shell(ShellEvent)          monitors and surfaces appearing/disappearing
  Message::Panel(window::Id, panel::Message)
  Message::Compositor(compositor::Event)   workspaces/windows changes, applied to `Compositor`
  + variants injected by #[to_layer_message(multi)] (NewLayerShell, RemoveWindow, ...)

Panel      (panel.rs)       one layer surface on one output; PanelConfig; gadgets: Vec<(Slot, AnyGadget)>
  Message::Gadget(index, gadget::Message)

AnyGadget  (gadget.rs)      closed enum over every gadget type, plus `create(name, &Config, &OutputInfo)`
  Message::Clock(clock::Message) | Message::Workspaces(..) | ...

Clock      (gadgets/clock.rs)  impl Gadget: new / update / view(ctx) / popup_view(ctx) / subscription
  Message::Calendar(calendar::Message)

Calendar   (widgets/calendar.rs)  reusable component, not a gadget: state + Message + update + view(today, theme, node)

Compositor (compositor/)    daemon-owned desktop state: workspaces, windows, active/urgent flags
  subscription()            the single IPC stream (compositor/hyprland.rs), yields `Event`s
  apply(Event)              patches the state
  run(Command) -> Task      sends a command (activate workspace/window) to the backend

Theme      (theme/)         daemon-owned styling: base.css + the user's theme, parsed once
  load / try_load(&Config)  css.rs (scanner) -> selector.rs + value.rs (typed rules)
  resolve(&Node) -> Style   cascade for one element path; container()/button()/text()/row() helpers

Icons      (icons/)         daemon-owned app icons: window class -> `Icon` (iced svg/image handle)
  load() -> Task            builds `Index` (icons/theme.rs theme chain + icons/desktop.rs .desktop db) off-thread
  apply(Event::Loaded)      installs it; resolve(class) fills the per-class cache; get(class) in `view`

watch::watch(paths)         (watch.rs) one `notify` subscription for aria.conf, theme files and the
                            icon/applications dirs; yields `Changed(paths)`: config -> rebuild panels,
                            theme -> reload it, anything else -> rebuild the icon index
```

Two things flow between the daemon and the gadgets besides messages:

- **`gadget::Context`** goes *down*, into `view`. It holds
  `gadget::Shared` (`&Compositor`, `&Theme`; later `&Audio`, `&Tray`,
  ...): daemon-owned, read-only, plus the gadget's own `theme::Node`. A
  gadget that shows shared state keeps no copy of it, it filters the
  context in `view`.
- **`gadget::Action`** comes *up*, out of `update`, in place of a bare
  `Task`: `Action::Run(Task)` for the gadget's own async work,
  `Action::Compositor(Command)` (and later `Action::Audio(..)`, ...) for
  things only the daemon can do, `Action::OpenPopup`/`ClosePopup` for a
  popup surface. `Panel::update` turns it into the concrete
  `panel::Action` (same variants, popup bookkeeping done) and
  `AriaShell::perform` into a `Task`. Gadgets never hold an IPC handle.

- **Popups** are xdg popups parented to the panel's layer surface. A
  gadget with one keeps a `gadget::Popup` field, exposes it through
  `Gadget::popup()`, wraps the widget the popup hangs from in
  `popup.anchor(..)` (a `container` tagged with a unique `widget::Id`) and
  returns `popup.toggle(size)` from `update`; the content is
  `Gadget::popup_view`. Under the hood `toggle` yields
  `Action::OpenPopup { anchor, size }` / `ClosePopup(id)`; the panel mints
  the `window::Id`, remembers `popup -> gadget index` and records it in
  the gadget's `Popup`. The daemon keeps `popup -> panel`, asks the widget
  tree for the anchor's bounds with a custom `Operation` (`widget_bounds`
  in `main.rs`) and sends `NewPopUp` placed by `panel::popup_settings`
  (centred on the anchor, below a top bar / above a bottom one).
  `view(popup_id)` routes to `Panel::popup_view` -> `Gadget::popup_view`.
  Whoever closes it (the gadget, or the compositor on a click outside),
  it ends in `ShellEvent::Closed(id)` -> `Panel::popup_closed` -> the
  gadget's `Popup` is marked closed.

- **Styling is a CSS-like theme file**, resolved per widget in `view`.
  `assets/base.css` (compiled in, always first) documents the element
  tree and the supported properties for theme authors; `[general] style`
  names a user theme loaded on top (`themes/<name>.css` in the config
  dirs, the XDG data dirs, then `assets/`; or a path). Both are parsed
  and type-checked at load (`theme/css.rs` scanner, `selector.rs`,
  `value.rs`); bad selectors/declarations are logged with `file:line:col`
  and skipped, a syntax error rejects the file (and on hot reload keeps
  the last good theme). `:root { --x: v }` variables are substituted
  textually after all files are merged, so a user theme can override a
  variable the base uses. Cascade is specificity then source order.
  In `view`, every widget has a `theme::Node`: its element path
  (`panel.top#2 > slot.start > gadget.workspaces > workspace.active`),
  an immutable `Arc` list so style closures can own one. `Theme::resolve`
  walks root→leaf applying matching rules and inheriting `color` and
  `font-*`; `Theme::button/container/text/row` do that and return plain
  iced widgets. A button's style closure re-resolves with
  `node.status(status)`, which is how `:hover`/`:active` work. Text gets
  an explicit colour only when a rule set it on the text node itself;
  otherwise iced's own cascade (container/button `text_color`) carries
  it, so `workspace:hover { color }` reaches the label. Class names are
  the Rust roles (`panel`, `slot`, `gadget.clock`, `workspace`, ...),
  not the Python/GTK ones. The bar thickness is the theme's `min-height`
  on `panel` (layer-shell needs it before layout); a reload that changes
  it sends `LayoutChange` + `ExclusiveZoneChange`. Surfaces are created
  transparent (`daemon(..).style`), the theme's `panel`/`popup`
  background is what shows, so `rgba` bars and rounded popups work.
  Gadgets must not hard-code colours/paddings/spacing: derive nodes from
  `ctx.node` and go through the theme helpers.
- **App icons** (`icons/`) are a daemon-owned source with the usual
  verbs. Resolution is `class` -> desktop entry (by id, `StartupWMClass`,
  `Exec` basename, reverse-DNS suffix) -> `Icon=` -> theme lookup, then
  the `[apps_class_map]` override, the class as an icon name, and
  `application-x-executable`. The theme lookup is an **in-memory index**:
  every directory the `index.theme` chain lists (theme, `Inherits`,
  `hicolor`, then `pixmaps/`) is read once into a name -> (dir, ext) map,
  so a lookup is a hash probe plus the spec's size choice (exact match,
  svg first, else closest; scale-1 dirs only; no xpm), with no `stat`
  per candidate. Built with `spawn_blocking` from the boot `Task`, so the
  bars show dots first and icons a few ms later. Warm timings in release
  on this machine: Adwaita+AdwaitaLegacy+hicolor (2k icons, 700 dirs)
  ≈ 10 ms, breeze+hicolor (8k names, 18.7k files) ≈ 23 ms, 80 desktop
  entries ≈ 2 ms. The `applications/` dirs and every indexed theme dir
  are watched (≈ 725 inotify watches here, the limit is 524k): an
  install or removal rebuilds the index after the burst settles and the
  daemon re-resolves the classes it shows (the Python version missed
  this). `Icons::resolve` is called by the daemon after every compositor
  event, never from `view`; handles are created once and cloned, since
  iced caches decoded images by handle id. Icon size and tint come from
  the theme (`window { height; color }`; only `-symbolic` svgs are
  tinted). Not done: `icon-theme.cache` (GTK's mmap cache) as a
  zero-scan fast path, worth it only if cold starts turn out slow.
- **No global state.** `Config` is loaded in `AriaShell::new` and passed
  by `&` down to gadget construction. This is what makes hot-reload
  possible later (replace the value, rebuild panels) and what makes the
  config layer unit-testable.
- **Multi-monitor from day one.** The daemon starts in
  `StartMode::Background` (no surface). It subscribes to the shell
  broadcast (`iced_wayland_subscriber`); on `OutputAdded` it opens one
  layer surface per `[panel*]` section that wants that output
  (`NewLayerShellSettings { output_option: OutputOption::GlobalName(id) }`),
  on `OutputRemoved` it removes them. The broadcast replays current
  outputs to late subscribers, so startup and hot-plug are the same path.
- **External event sources are `Subscription`s**, not services. A timer
  is a stream; the compositor IPC socket is a stream (`compositor::hyprland::events`);
  a DBus connection will be one too. Shared connections live in the
  daemon's subscription, their events fan out through `update`, never a
  mutex-guarded static.
- **Subscription identity.** iced dedups subscriptions by hash. Every
  level keys its children's subscriptions with `.with(key)` (gadget index
  in `Panel`, `window::Id` in `AriaShell`) so two identical gadgets on two
  outputs keep separate streams.
- **Closed gadget set.** No plugins, no `dyn`. Adding a gadget: one file
  under `gadgets/`, one variant in `AnyGadget` and `gadget::Message`, one
  arm in each `match` in `gadget.rs`.
- **Reusable widgets live in `widgets/`** (`Calendar` so far), as plain
  Elm components: a host embeds one as a field, calls `update` with the
  widget's `Message` and `.map()`s its `view`. Nothing in iced or a
  third-party crate fit (`iced_aw::date_picker` is a modal picker, and
  would pin our iced version); COSMIC's calendar is inside `libcosmic`.
- **Config sections** implement `config::Section` by hand
  (`const NAME` + `from_raw(&RawSection)`). `RawSection` has the typed
  accessors (`str_or`, `bool_or`, `list_or`; add `int_or` when a section
  needs it).
- **Shared state is owned by the daemon**, one struct per source
  (`Compositor` now; audio, tray, notifications later), each with the same
  three verbs: `subscription()` (one stream for the whole process),
  `apply(Event)`, `run(Command) -> Task`. Gadgets read it through
  `Context` and change it through `Action`. This is the shape to copy for
  the next shared source; don't give gadgets their own connection.

### Facts about the crates, verified in source (v0.20.1 / iced 0.14)

- `iced::time::every` needs the `tokio` feature on `iced`. We use
  `tokio::time::sleep` directly for the wall-clock-aligned clock tick.
- `LayerSize::FILL` with only `Anchor::Top` fills the whole output height;
  always set `LayerSize::fill_width(h)` for a bar.
- `exclusive_zone` is a plain pixel count; there is no "auto from content".
- `Anchor` is a bitflag re-export of the protocol type.
- `OutputInfo` (sctk) carries `id` (`wl_registry` global name, what
  `OutputOption::GlobalName` wants) and `name: Option<String>` (connector,
  e.g. `HDMI-A-1`, what the `outputs =` config key matches).
- `Subscription::with(v)` includes `v` in the recipe hash; `.map(f)` only
  includes `TypeId::of::<F>()`.
- `configparser` strips inline comments from the first `#` anywhere in a
  value (Python needs leading whitespace). We disable inline comments
  entirely so `#ff0000` survives.
- `configparser::Ini::new_cs()` is case-sensitive on sections and keys.
- `configparser` stores sections in a `HashMap` unless its `indexmap`
  feature is on; we need it, `Config::instances` (and so the order of
  `[panel:*]` bars and of gadgets' sections) is file order.
- Hyprland 0.56 (Lua config) changed the IPC dispatch syntax: the command
  socket takes `dispatch hl.dsp.focus({ workspace = 3 })` /
  `dispatch hl.dsp.focus({ window = "address:0x..." })`; the old
  `dispatch workspace 3` is an error. `j/workspaces`, `j/clients`,
  `j/monitors`, `j/activewindow` and the `.socket2.sock` event names are
  unchanged. Dispatcher names: `/usr/share/hypr/stubs/hl.meta.lua`.
- Hyprland's "active workspace" is one per monitor (`j/monitors[].activeWorkspace`),
  and `workspacev2` only tells you the focused monitor switched. `Compositor`
  models it that way: `ActiveWorkspace(id)` clears the flag only among
  workspaces on the same output.
- `j/workspaces` comes in creation order and includes special workspaces
  (negative ids); we sort by id and drop those.
- A `tooltip` on a 32px layer surface would be clipped to the surface, so
  the Workspaces gadget has none (Python showed name/title tooltips).
- `Message::NewPopUp { settings: IcedNewPopupSettings, id }` (added by the
  macro). `IcedNewPopupSettings::new(parent, size, anchor_pos, anchor_size)`
  then `.anchor(PopupAnchor::Bottom).gravity(PopupGravity::Bottom)` puts
  the popup centred under the anchor rect; defaults flip/slide it back on
  screen. The runtime takes the grab serial from the last pointer button
  itself, so a popup opened from a click gets the implicit grab: a click
  outside dismisses it (`xdg_popup.popup_done` -> `ShellEvent::Closed`)
  and the compositor swallows that click.
- A widget's on-screen bounds are only known to the widget tree: query
  them with a custom `widget::Operation` via `iced::advanced::widget::operate`
  (needs the `advanced` feature). The runtime runs the operation on every
  window and can't tell which answered, so tag widgets with
  `widget::Id::unique()` per instance, never a fixed name.
  `container::visible_bounds` from iced 0.13 is gone in 0.14.
- `container.center(Length::Fill)` sets width *and* height, overriding a
  fixed size set before it; use `align_x`/`align_y` for a fixed cell.
- No pointer injection on this setup (no `ydotool`/`wtype`, `/dev/uinput`
  is root-only, Hyprland's Lua dispatchers move the cursor but can't
  click): clicks have to be done by the user, screenshots with `grim -g`
  (`-s 3` for a zoomed crop).
- `button.style(impl Fn(&Theme, Status) -> Style + 'a)` /
  `container.style(Fn(&Theme) -> Style + 'a)`: the closures can own data
  with the view's lifetime, which is what lets them hold a `theme::Node`
  and a `&'a Theme`. `button::Style::text_color` is a plain `Color`
  (falls back to `theme.palette().text`); `container::Style::text_color`
  and `text::Style::color` are `Option`s and `None` inherits at draw time.
- `iced::font::Family::Name` wants a `&'static str`: theme font names are
  leaked once each (`theme::intern`). Fonts come from the system via
  cosmic-text's fontdb (`FontSystem::new_with_fonts` loads system fonts);
  `Font::with_name("JetBrainsMono NF")` works for an installed font. A
  `Font` is one family, so a CSS `font-family` list is resolved at theme
  load to the first installed name, asking
  `iced::advanced::graphics::text::font_system()` (`.raw().db().faces()`).
  `text` defaults to `Shaping::Basic`, which does **no per-glyph font
  fallback** (an icon font as the family shows boxes for digits);
  `Theme::text` sets `Shaping::Advanced`.
- `daemon(..).style(|state, theme| iced::theme::Style)` is one background
  for every window (no window id); per-surface looks are done by the
  view's root container over a transparent background.
- `Padding::horizontal(x)`/`vertical(y)` are *setters* in iced 0.14, not
  the sums (`left + right`).
- iced image caches (`iced_wgpu/src/image/{vector,raster}.rs`):
  `svg::Handle::from_path` id is the path hash (stable), the file is read
  and parsed with usvg on the render thread at first draw, rasters are
  cached per (handle, size, color); `image::Handle::from_rgba` gets a
  *new* id per call. When a new entry lands, entries not drawn that
  frame are evicted, so pre-warming icons that aren't on screen is
  pointless. `svg::Style { color }` tints (for symbolic icons).
  `iced` feature `image-without-codecs` + our own `image = { features
  = ["png"] }` keeps only the PNG decoder in the binary.
- The boot closure of `iced_exwlshell::daemon` may return `(State,
  Task)`: that's where the icon index build starts.
- `notify` 8: `recommended_watcher(handler)` runs its own thread; watch
  the parent directory (editors save by rename) and filter on paths.
  `futures::mpsc::UnboundedReceiver::try_next` is deprecated for
  `try_recv`.

## Status (2026-09-16)

Verified on the real Hyprland session with two outputs:

- `hyprctl layers` shows one `aria-panel` surface per output, full width,
  32px, in the configured layer, with an exclusive zone.
- `assets/aria.conf` drives it: `[panel]` with `items_center = Clock` and
  `items_end = Clock:2`, each `[Clock*]` with its own `format`. Screenshots
  confirm both gadgets render on both bars and the seconds tick.
- `[WorkSpaces]` (section spelled as in the Python config) on each bar
  shows only that monitor's workspaces, the per-monitor active one
  highlighted, one marker per window (filled for the active window),
  and follows `hyprctl dispatch` switches live (Hyprland 0.56.2).
- Clicking the Clock opens a month calendar popup centred under it
  (weeks start on Monday, today highlighted, `<`/`>` change month, no
  locale for month names); clicking the clock again or anywhere outside
  closes it. Verified with screenshots on both outputs.
- Theming: with `[general] style = example` the bar follows
  `assets/themes/example.css` (36px, monospace, translucent `rgba`
  background, `gadget.clock#2` in the accent colour, `workspace`
  `height: fill`); editing the file restyles live, changing `min-height`
  resizes the layer surfaces (`hyprctl layers`), a syntax error logs
  `path:line:col` and keeps the running theme. The calendar popup has
  the themed background, rounded corners over a transparent surface,
  today in the accent colour. Screenshots on both outputs.
- Config hot-reload: editing `items_end` and `style =` in `aria.conf`
  while running closes and reopens the bars with the new gadgets and
  theme (log shows the rebuild; `hyprctl layers` shows new surfaces at
  the new height).
- The two GTK-era themes ported to the new vocabulary
  (`assets/themes/manjaro.css`, `waybar.css`): reversed-colour cells via
  `slot.start > gadget:first-child` / `gadget { height: fill }`,
  translucent `rgba` bar, font fallback lists. What didn't port: inset
  box-shadows (iced has none; plain backgrounds instead), gadgets that
  don't exist yet (kept as rules for `gadget.cpu`/`gadget.audio`).
- Window icons: Firefox (hicolor PNG), Code (`com.visualstudio.code.oss`
  svg via the desktop entry) and kitty drawn at 16px in the workspace
  buttons on both outputs; creating/removing a `.desktop` in
  `~/.local/share/applications` rebuilds the index (80 -> 81 -> 80
  apps in the log); `icon_theme = breeze` via config reload switches
  the chain live.
- `cargo build`, `cargo clippy --all-targets`, `cargo test` (39 tests,
  config + theme layers): clean.

Implemented: config loading and hot-reload, `[general]` (`style`,
`reload_style`, `reload_config`, `icon_theme`), `[apps_class_map]`,
`[panel]` (`outputs`, `position`, `layer`, `items_*`), multi-output
panels, Clock (`format`, calendar popup), Workspaces (all four keys;
windows are dots, not icons) over the Hyprland IPC, with the daemon-owned
`Compositor` / `Context` / `Action` plumbing and the popup plumbing
(`Panel` <-> `Gadget` popup hooks), the CSS-like theme system
(`theme/`, `assets/base.css`, hot reload, bar thickness from the theme).

Not yet: Sway backend, `[panel]`
`size`/`align`/`margin`/`opacity`, panel height from content, Clock
`tooltip_format`, theme
properties beyond the current set (`margin`, `opacity`, gradients,
`@import`, `!important`, `@font-face` for theme-shipped fonts,
transitions), `:hover` on non-button widgets (needs a `mouse_area`
wrapper), every other gadget and component (tray, notifications,
launcher, lock, wallpaper, terminal, idle).

## Next steps, in order

1. The next gadget/component that needs a new shared source (tray over
   SNI/DBus, or the launcher on top of the desktop db in `icons/`).
2. More theme surface as gadgets need it (`margin` via a wrapping
   container, `opacity`, `@font-face`); the `shader` widget for
   `background: shader("x.wgsl")` when a theme asks for more than CSS.
