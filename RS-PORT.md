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
  Message::Tray(tray::Event)               status notifier items and their menus, applied to `Tray`
  Message::Notifications(notifications::Event)   notifications coming and going, applied to `Notifications`
  Message::Toast(toast::Message)           a click on a notification's surface -> notifications::Command
  + variants injected by #[to_layer_message(multi)] (NewLayerShell, RemoveWindow, ...)

Panel      (panel.rs)       one layer surface on one output; PanelConfig; gadgets: Vec<(Slot, AnyGadget)>
  Message::Gadget(index, gadget::Message)

AnyGadget  (gadget.rs)      closed enum over every gadget type, plus `create(name, &Config, &OutputInfo)`
  Message::Clock(clock::Message) | Message::Workspaces(..) | ...

Clock      (gadgets/clock.rs)  impl Gadget: new / update / view(ctx) / popup_view(ctx) / popup_size(ctx) / subscription
  Message::Calendar(calendar::Message)

TrayGadget (gadgets/tray.rs)   impl Gadget: a row of items from `ctx.tray`, the clicked item's menu in the popup
  Message::OpenMenu(key, n) | Menu(menu::Message) | Activate(key) | Scroll(key, delta) | ...

Themes     (gadgets/themes.rs) impl Gadget: the scheme's icon; left click toggles light/dark, right click a menu
  Message::Toggle | OpenMenu | Menu(menu::Message)   -> Action::Theme(theme::Command)

Custom     (gadgets/custom.rs) impl Gadget: icon and/or label, a program per mouse button / wheel direction
  Message::Left | Right | Middle | Scroll(delta)   -> process::run; with `exec`, Action::Script(Refresh(spec))
  script() -> Option<scripts::Spec>                the `exec` for the daemon; its output read from `ctx.scripts`

Menu       (widgets/menu.rs)  reusable component: Item tree (labels, toggles, separators, submenus unfolding in
                              place) -> update(Message) -> Event, view(theme, node, items), size(..) for the popup

Calendar   (widgets/calendar.rs)  reusable component, not a gadget: state + Message + update + view(today, theme, node)

Compositor (compositor/)    daemon-owned desktop state: workspaces, windows, active/urgent flags
  subscription()            the single IPC stream (compositor/hyprland.rs), yields `Event`s
  apply(Event)              patches the state
  run(Command) -> Task      sends a command (activate workspace/window) to the backend

Tray       (tray/)         daemon-owned status notifier items: `items: Vec<Item>` (props + pixmap icons), loaded menus
  subscription()            one session-bus connection (tray/dbus.rs): the watcher we serve or defer to, the
                            host, one task per item; yields `Event`s (Connected, Added/Updated/Removed, Menu, ..)
  apply(Event) -> Task      patches the state; a `MenuChanged` for a loaded menu re-fetches it
  run(Command, cursor)      Activate/SecondaryActivate/ContextMenu/Scroll on an item, LoadMenu/ExpandMenu/MenuClick
                            over `com.canonical.dbusmenu` (tray/menu.rs)

Notifications (notifications/)  daemon-owned notification daemon: `items: Vec<Notification>` (newest first)
  subscription()            one session-bus connection (notifications/dbus.rs) serving
                            `org.freedesktop.Notifications` (the name re-requested when another daemon drops it);
                            yields `Event`s (Connected, Notify, Close, Expired)
  apply(Event) -> Task      patches the list; a `Notify` starts the expiry timer (serial-checked)
  run(Command) -> Task      Activate (the `default` action, then close) / Invoke(key) / Dismiss: emits
                            `ActionInvoked` / `NotificationClosed`
  toast.rs                  node(n, output) / view(..) / size(..): one layer surface per notification, the
                            daemon stacks them (`AriaShell::sync_toasts`) from the configured corner by margin

Scripts    (scripts.rs)     daemon-owned programs feeding gadgets (`[Custom] exec`): `Spec` (argv, interval,
                            return_type) -> last `Output` (text, icon, classes); one run per distinct spec
  subscription(specs)       from `Panel::scripts()` every time: one runner per spec, restarted by `Refresh`
                            (the generation is part of its identity); yields `Event::Ran(spec, output)`
  apply(Event) / run(Command::Refresh)

process    (process.rs)     split_words (shell-like quoting, no shell), command(line), spawn_detached, run(line):
                            the config's command lines and the launcher's desktop entries; `aria-shell` as
                            the program is this very binary

Theme      (theme/)         daemon-owned styling: base.css + the user's theme, parsed once for one Scheme
  load / try_load(&Config, style, scheme)   css.rs (scanner) -> selector.rs + value.rs (typed rules)
  scheme() / name()         what's loaded; `available()` lists the themes/*.css of every theme dir
  Command                   ToggleScheme | SetScheme | SetStyle: from a gadget (Action::Theme), the daemon
                            keeps `style`/`scheme` at runtime and reloads (no persistence)
  resolve(&Node) -> Style   cascade for one element path; container()/button()/text()/row() helpers

Icons      (icons/)         daemon-owned app icons: window class -> `Icon` (iced svg/image handle)
  load() -> Task            builds `Index` (icons/theme.rs theme chain + icons/desktop.rs .desktop db) off-thread
  apply(Event::Loaded)      installs it; resolve(class) fills the per-class cache; get(class) in `view`
  index() -> Arc<Index>     the desktop db is also what the launcher searches; desktop::launch runs an entry

Launcher   (launcher.rs)    a component the daemon owns while open: `launcher: Option<(window::Id, Launcher)>`
  Message / update -> Action { Run(Task) | Close }, view(Shared), subscription() for Up/Down/Esc
  + `grabs: Vec<window::Id>` in the daemon, one transparent surface per output behind it

commands::listen()          (commands.rs) the command socket as a Subscription; `Command::Launcher(Toggle|Show|Hide)`
commands::send(args)        the client: `aria-shell launcher toggle` is the same binary with arguments

watch::watch(paths)         (watch.rs) one `notify` subscription for aria.conf, theme files and the
                            icon/applications dirs; yields `Changed(paths)`: config -> rebuild panels,
                            theme -> reload it, anything else -> rebuild the icon index
```

Two things flow between the daemon and the gadgets besides messages:

- **`gadget::Context`** goes *down*, into `view`. It holds
  `gadget::Shared` (`&Compositor`, `&Theme`, `&Icons`, `&Tray`; later
  `&Audio`, ...): daemon-owned, read-only, plus the gadget's own `theme::Node`. A
  gadget that shows shared state keeps no copy of it, it filters the
  context in `view`.
- **`gadget::Action`** comes *up*, out of `update`, in place of a bare
  `Task`: `Action::Run(Task)` for the gadget's own async work,
  `Action::Compositor(Command)` / `Action::Tray(Command)` (and later
  `Action::Audio(..)`, ...) for things only the daemon can do,
  `Action::OpenPopup`/`ClosePopup` for a popup surface, `Action::Many`
  for several at once. `Panel::update` turns it into the concrete
  `panel::Action` (same variants, popup bookkeeping done) and
  `AriaShell::perform` into a `Task`. Gadgets never hold an IPC handle.

- **Popups** are xdg popups parented to the panel's layer surface. A
  gadget with one keeps a `gadget::Popup` field, exposes it through
  `Gadget::popup()`, wraps the widget the popup hangs from in
  `popup.anchor(..)` (a `container` tagged with a `widget::Id` unique to
  the `Popup`; `anchor_nth(n, ..)` / `toggle_nth(n)` when several widgets
  may host it, one per tray item) and returns `popup.toggle()` from
  `update`; the content is `Gadget::popup_view`. **Its size is a
  function of the state**: `Gadget::popup_size(ctx)` (a Wayland surface
  needs it up front, iced can't size it from the content). The daemon
  asks for it when the popup opens and again after every shared-state
  or gadget update (`sync_popups`); a different answer is sent as
  `PopUpReposition` with the same anchor, which is how a tray menu
  grows when a submenu unfolds. `Theme::measure(node, text)` /
  `line_height(node)` exist for that (cosmic-text through
  `iced::advanced::graphics::text::Paragraph`). Under the hood `toggle`
  yields `Action::OpenPopup { anchor }` / `ClosePopup(id)`; the panel
  mints the `window::Id`, remembers `popup -> gadget index` and records
  it in the gadget's `Popup`. The daemon keeps `popup -> (panel, anchor
  rect, size)`, asks the widget tree for the anchor's bounds with a
  custom `Operation` (`widget_bounds` in `main.rs`) and sends `NewPopUp`
  placed by `panel::popup_settings` (centred on the anchor, below a top
  bar / above a bottom one). `view(popup_id)` routes to
  `Panel::popup_view` -> `Gadget::popup_view`. Whoever closes it (the
  gadget, or the compositor on a click outside), it ends in
  `ShellEvent::Closed(id)` -> `Panel::popup_closed` -> the gadget's
  `Popup` is marked closed and `Gadget::popup_closed` runs.

- **Light/dark**: a theme is loaded for a `theme::Scheme` (`[general]
  color_scheme`, default light; the `Themes` gadget switches it at
  runtime, in memory only — following or setting the desktop's scheme is
  for later, per DE). `:root.light { }` / `:root.dark { }` blocks hold
  the scheme's variables, folded per file over the plain `:root` ones
  (`base :root`, `base :root.<scheme>`, `user :root`, `user
  :root.<scheme>`: a user theme's plain variable still beats the base's
  scheme one); a theme that only sets `:root` looks the same in both.
  `Theme::resolve` gives the root node of every path the scheme as a
  class, so `panel.dark { }` rules work without touching the nodes
  gadgets build. `base.css` carries both palettes (Catppuccin-like
  Latte / Mocha). `debug theme` prints `style=<name|-> scheme=<..>`.
- **Programs, not shells**: every command line in the config
  (`[Custom] command*`, `command_wheel_*`, `exec`) is a program and its
  arguments, split with shell-like quoting (`process::split_words`) and
  run directly, detached; the user writes `sh -c '...'` when a shell
  is wanted. The shell's own commands are its CLI (`command =
  aria-shell launcher toggle`), with that name resolved to the running
  binary so a dev build works the same; no `aria ` prefix magic as the
  Python one had. `exec` programs are run by the daemon (`scripts.rs`),
  once per distinct spec however many panels show the gadget: two
  monitors don't run `checkupdates` twice (it fails when they do), and
  the output is shared state read from `ctx.scripts`; a gadget asking
  for a fresh run after a click returns `Action::Script(Refresh)`.
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
- **The launcher is a component, not a gadget**: it has its own layer
  surface and the keyboard, one instance at a time, on the focused
  output (`Compositor::focused_output`, from Hyprland's `j/monitors[].focused`
  and the `focusedmonv2` event; the first output if unknown). Opened by
  `aria-shell launcher toggle|show|hide` over the command socket
  (`$XDG_RUNTIME_DIR/aria-shell/cmd.sock`, the Python line protocol:
  `OK ...` / `ERR ...` per line; parsing is in the listener, the daemon
  only sees valid `Command`s). The surface is `Layer::Overlay`, no
  anchors (the compositor centres it), sized by the theme's `launcher {
  width; height }`, `KeyboardInteractivity::OnDemand` (Hyprland focuses
  an on-demand layer when it maps; `Exclusive` would also force the
  *pointer* onto it, see below); the search
  field is focused with `operation::focus` on the `ShellEvent::NewShell`
  of that surface (earlier, the widget tree doesn't exist yet). A click
  outside closes it as it does for a popup: while it's open the daemon
  keeps one full-screen transparent surface per output on `Layer::Top`
  (above the bars, below the launcher, `exclusive_zone: -1`) whose
  `mouse_area` yields `Close`; the click is swallowed. Any of those
  surfaces closing (`ShellEvent::Closed`) closes the rest. It also
  closes when the keyboard leaves its surface
  (`window::Event::Unfocused`: a keybind moved focus, a click on a
  window), which is how cosmic-launcher closes; on its own that isn't
  enough here because a click on a bar or on an empty desktop doesn't
  move keyboard focus on Hyprland/Sway. It searches
  the desktop db already in `icons::Index` (an `Arc` snapshot, replaced
  when the index is rebuilt), scoring as the Python did (exact 10,
  prefix 8, substring 6 over id/name/comment, empty query lists all);
  `NoDisplay` and entries without `Exec` are skipped. Icons are
  resolved by the daemon for the listed ids (`Icons::resolve` accepts
  a desktop id, `for_class` tries `by_id` first) after every launcher
  update, never in `view`. Launching is hand-rolled in
  `icons::desktop::launch` (the Python used `gtk-launch`): spec
  quoting, field codes, `Path=`, `Terminal=true` via `[launcher]
  terminal` (default `$TERMINAL`, else `xterm`) with `-e`, own process
  group, stdio to null, reaped by a thread. Not done: `DBusActivatable`,
  startup notification, other providers (the Python had only apps too).
  The Python `[launcher]` keys `width/height/icon_size/opacity` are
  theme matters here (`launcher`, `launcher icon { height }`), not
  config.
- **The tray** (`tray/`) is the first DBus source, the shape notifications
  and MPRIS will copy: one `zbus::Connection` (tokio feature) opened in
  the subscription's stream, handed to the daemon as
  `Event::Connected(conn)` so `Tray::run` can call methods with it; the
  stream runs the host loop. The
  `org.kde.StatusNotifierWatcher` object is always served
  (`#[interface]`, item keys are `<bus name><path>`, an item is
  unregistered when its name leaves the bus via `NameOwnerChanged`);
  the name is requested with `DoNotQueue`, and if another bar owns it
  (noctalia on this desktop) we act as a plain host of that watcher,
  which uses the same proxy and signals as talking to our own, and
  retry when the owner goes away. One tokio task per item: `GetAll`
  on `org.freedesktop.DBus.Properties`, then `NewIcon`/`NewStatus`/...
  re-read the properties they cover (`PropertiesChanged` too, for the
  apps that emit it; proxies are built with `CacheProperties::No`,
  most items never emit it). `IconPixmap` (`a(iiay)`, ARGB32 network
  order) is picked (largest <= 64px) and converted to RGBA once per
  change into an `image::Handle` kept in the `Item`; `IconName` goes
  through `Icons::resolve_name` (the item's `IconThemePath` searched
  first, then the theme, then the generic fallback). Menus:
  `com.canonical.dbusmenu` `GetLayout(0, -1)` after `AboutToShow`
  (apps build their menus on it: nm-applet's VPN submenu appears
  then), parsed into a `Menu` tree (invisible items dropped, `_`
  mnemonics stripped), shown by the gadget as a column of buttons with
  submenus unfolding in place; a click sends `Event(id, "clicked")` and
  closes the popup; `LayoutUpdated`/`ItemsPropertiesUpdated` re-fetch
  a loaded menu. Left click: `Activate`, or the menu if `ItemIsMenu`;
  right: the menu, or `ContextMenu` when there's none; middle:
  `SecondaryActivate`; wheel: `Scroll(clicks, orientation)`, positive
  up as KDE sends it. The click methods get the pointer's global
  position as the daemon estimates it. No tooltips (a 32px surface
  would clip them), no overlay icons, no menu icons or shortcuts.
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

- zbus 5 with `default-features = false, features = ["tokio"]` runs on
  iced's tokio runtime (`tokio::spawn` from a subscription works). A
  `#[proxy]` caches properties by default and only invalidates them on
  `PropertiesChanged`, which SNI items rarely emit: build item proxies
  with `.cache_properties(CacheProperties::No)`.
  `request_name_with_flags(.., DoNotQueue)` returns
  `Err(Error::NameTaken)`, not `Ok(RequestNameReply::Exists)`, when
  another connection owns the name. A `#[interface]` method gets the
  caller with `#[zbus(header)] h: Header<'_>` (`h.sender()`) and emits
  signals through `#[zbus(signal_emitter)]`; from outside a method,
  `SignalEmitter::new(&conn, path)`. Signal streams from different
  signals are different types: merge them with `stream::select_all`
  over `BoxStream`s. `zvariant` converts an `OwnedValue` to a tuple /
  `Vec` / `String` with `try_from`; a `Structure` inside an `av` comes
  back as a tuple the same way.
- `Message::PopUpReposition { settings: IcedNewPopupSettings, id }`
  resizes and re-places a mapped popup (xdg_popup v3 `reposition`;
  Hyprland and Sway have it, older compositors log and ignore); the
  runtime updates the surface size on the following configure.
- One wheel click reaches iced as **several** `WheelScrolled` events
  in `iced_exwlshell`: `Pixels { 0, 0 }` (from `axis_source`), `Lines
  { y }` (from `axis_value120`/`axis_discrete`) and `Pixels { y: 15 }`
  (from `axis`), signs negated (up is positive). Touchpads send only
  `Pixels`. The tray counts `Lines` as clicks, accumulates `Pixels`
  into clicks of 15 and drops a `Pixels` arriving within 100ms of a
  `Lines` (the same click); `-0.0.signum()` is `-1.0`, so a zero delta
  must be filtered before taking its sign.
- A widget's on-screen bounds for `debug widgets` are the container's
  outer bounds; `Theme::button` puts the node's padding on the button,
  so the row inside must not get it again (`theme.row(&node, ..)`
  would: the Workspaces gadget does that, doubling `workspace`
  padding; left as is since the shipped themes are tuned to it).
- Theme selectors are global: `gadget.tray item` matched the menu
  rows under `popup > gadget.tray > menu > item` too and won on
  specificity, so base.css uses `gadget.tray > item` for the bar.
- A layer surface's `margin: Some((top, right, bottom, left))` and
  `Message::MarginChange { id, margin }` place it from its anchored
  edges; `exclusive_zone: None` keeps it out of the bars' zones (Sway
  and wlroots arrange exclusive surfaces of every layer first, so an
  overlay toast still sits under a `top`-layer bar). That's how the
  notifications stack: one surface each, the daemon recomputes the
  margins on every change.
- iced's `container` and `button` lay their content out inside the
  padding only: the border is drawn over it and takes no room, so a
  surface measured for its content adds padding, not `border-width`.
  Wrapped text is measured with `Paragraph::with_text` bounded on the
  width (`Theme::measure_in`); it matches `text(..).wrapping(Word)` in
  a container of that width to the pixel.
- `gdbus call` infers `[255, 0]` as `ai`: an `image-data` hint from
  the shell needs `@ay [..]` in the tuple.
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
- **Driving the app from the CLI**, without compositor-specific tools
  (Sway and others are next): input with `ydotool` (uinput, so it works
  under any compositor; keys, clicks, `mousemove -a` absolute moves
  which land exactly with a flat pointer profile), positions from the
  shell's own `debug` socket commands. Setup done on this machine:
  `uinput` module loaded at boot (`/etc/modules-load.d/uinput.conf`),
  udev rule `KERNEL=="uinput", GROUP="input", MODE="0660"`, user in
  `input`, `ydotoold` as a systemd user service (`newgrp` isn't enough
  for the user manager, a re-login was). Alternatives weighed:
  Hyprland's `hl.dsp.cursor.move` / `hl.dsp.focus` (no click,
  Hyprland-only), `wtype` (keyboard only, `zwp_virtual_keyboard_v1`,
  which Sway, Hyprland and cosmic-comp all have), `wlr-virtual-pointer`
  (not universal: cosmic-comp lacks it).
  - `aria-shell debug surfaces` → `panel HDMI-A-1 0,0 1920x30; grab
    ...; launcher HDMI-A-1 700,330 520x420`: every surface with the
    global rectangle it *asked for* (`OutputInfo::logical_position/
    logical_size` from xdg-output plus anchor and size; popups are the
    requested placement, before any slide). Wayland doesn't tell a
    client where it really is: another client's exclusive zone shifts
    a bar (noctalia's bar reserves 35px here, so our top bar sits at
    y=35 while `debug surfaces` says 0). Exact in a harness where only
    the shell runs.
  - `aria-shell debug cursor` → `panel HDMI-A-1 local 960,15 global
    960,15`: where the pointer was last seen over one of our surfaces
    (the daemon tracks `CursorMoved`). The driver knows where it put the
    pointer, so `asked - local` is the surface's real origin: the
    calibration for the case above (asked 960,50, local 960,15 → y=35).
  - A session: `aria-shell launcher show; ydotool type 'term'; ydotool
    key 108:1 108:0` (evdev codes: 103 Up, 108 Down, 28 Enter, 1 Esc);
    `grim -g "700,330 520x420" shot.png` with the rectangle from `debug
    surfaces`; `ydotool mousemove -a -x 900 -y 461; ydotool click 0xC0`
    on a row; the log says `launched "kitty"` and `debug surfaces` no
    longer lists the launcher. Kill what you launched.
  - `aria-shell debug widgets [selector]` → `launcher > list >
    item.selected:nth-child(1) 720,388 480x41; ...`: every themed widget
    (the theme helpers tag containers, and buttons' content, with the
    node path as `widget::Id`; the calendar cells go through the theme
    for this) with its global rectangle, filtered by a theme selector
    (`:nth-child(n)` was added for it). A driver says "the third row"
    instead of measuring a screenshot.
- **UI scenarios** (`tests/ui/`, see AGENTS.md "Verifying"): `run.sh`
  starts a headless nested Sway (`WLR_BACKENDS=headless
  WLR_RENDERER=pixman`, two 1920x1080 outputs, `sway.conf` execs
  `inner.sh` so everything inherits the nested `WAYLAND_DISPLAY`), the
  shell with `tests/ui/config` and `tests/ui/data` (a harmless
  `aria-test.desktop` with `Exec=true`), then each scenario with
  `lib.sh`'s vocabulary; `target/ui/<scenario>/` gets status, logs and
  screenshots. Facts learned building it:
  - The shell renders under the pixman compositor with wgpu on the real
    GPU (Vulkan, Intel here); a GPU-less CI would need iced's
    `tiny-skia` fallback, untested.
  - The nested shell must not talk to the desktop: `run.sh` unsets
    `HYPRLAND_INSTANCE_SIGNATURE`/`SWAYSOCK`, and the command socket is
    per display (`$XDG_RUNTIME_DIR/aria-shell/<WAYLAND_DISPLAY>.sock`)
    since the first run took over the desktop shell's socket.
  - Sway's seat has no devices in headless mode: the deprecated `swaymsg
    seat - cursor set/press` do nothing for clients (no pointer
    capability) and `wtype` creates a virtual keyboard per run, and the
    seat getting its *first* keyboard makes Sway reset keyboard focus
    (three `Unfocused` on the launcher, which closes). Hence
    `tests/ui/inject`: a tiny `wayland-client` program holding one
    `zwp_virtual_keyboard_v1` and one `zwlr_virtual_pointer_v1` for the
    whole scenario, driven over fifos (`move X Y` absolute over the
    layout, `click`, `key Down`, `type text`), with its own generated
    keymap (one keycode per keysym, Unicode `Uxxxx` names, as `wtype`
    does). Virtual pointer absolute motion maps to the whole layout
    when the pointer isn't bound to an output.
  - `set -e` is ignored inside a subshell used as an `if` condition
    (bash): the scenario runs as a plain command and its status is read
    after.
- COSMIC as reference (checked in `cosmic-launcher`, `cosmic-panel`,
  `cosmic-applets`, `cosmic-comp`, `libcosmic`, `cosmic-settings-daemon`
  at 2026-09): **no automated UI tests anywhere**, only unit tests in
  the libraries and a manual QA checklist (`cosmic-launcher/TESTING.md`).
  cosmic-comp has `winit`/`x11` backends to run nested for development,
  no headless test harness. cosmic-launcher: a separate process,
  toggled by DBus activation (`org.freedesktop.Application`,
  `run_single_instance`), `KeyboardInteractivity::Exclusive` on a
  top-anchored layer surface, closed on `LayerEvent::Unfocused` (their
  compositor doesn't force the pointer onto exclusive layers), a dummy
  layer surface + their `overlap-notify` protocol to keep clear of the
  panel. Popups in the applets are xdg popups with the grab, as ours.
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
- A layer surface with `Anchor::empty()` and `LayerSize::px(w, h)` is
  centred on its output by Hyprland; `exclusive_zone: Some(-1)` on a
  full-size surface covers the bars too.
- Hyprland routes **all pointer input** (every output) to a layer
  surface with `KeyboardInteractivity::Exclusive` while it's mapped:
  the click-catching surfaces never saw a click (`listen_with` showed
  every press on the launcher's window). `OnDemand` gets keyboard
  focus on map just the same and leaves the pointer alone.
- `iced::keyboard::listen()` only yields *ignored* key events, and a
  focused `text_input` captures Escape (and drops its focus); use
  `iced::event::listen_with` (gets every event with its status and
  `window::Id`; takes a plain `fn`, so filter on the window afterwards)
  for keys the launcher wants regardless. Up/Down aren't captured by
  `text_input`.
- The focus/scroll tasks live at `iced::widget::operation::{focus,
  snap_to, scroll_to}`; they run on every window, so the widget ids
  must be `Id::unique()`. `snap_to(id, RelativeOffset { y: i / (n-1) })`
  always keeps item `i` of `n` in view without knowing the row height.
- `tokio::spawn` inside a `stream::channel` subscription works (iced's
  tokio executor runs it on the runtime) but needs the `rt` feature.
- Hyprland `j/monitors[].focused` / `focusedmonv2>>NAME,WSID` give the
  focused monitor by connector name.
- `hyprctl dispatch 'hl.dsp.focus({ monitor = "HDMI-A-2" })'` moves
  focus to a monitor, handy to test per-output behaviour.

## Status (2026-09-17)

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
- Theming: with no `[general] style` the bar is `base.css` alone; with
  `style = manjaro` (or `waybar`) it follows `assets/themes/<name>.css`;
  editing the file restyles live, changing `min-height`
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
- Launcher: `aria-shell launcher toggle` from a terminal opens it centred
  on the focused output (`hyprctl layers`: `aria-launcher` 520x420 at
  700,330 on HDMI-A-1, at 2620,330 after `hl.dsp.focus({ monitor =
  "HDMI-A-2" })`), with an `aria-launcher-grab` full-screen surface on
  each output; screenshot shows the 80 apps with icons, names and
  comments, the first row selected in the accent colour, the input
  with its `:focus` border. `hide`/`show`/`toggle` and `ping` over the
  socket verified, unknown commands get `ERR`. Driven with `ydotool`
  and the `debug` commands (no hands): typing filters live (`term` →
  Alacritty, kitty, Micro), Down/Down/Up move the selection and the
  input keeps focus, a click on the kitty row launches it and closes
  the launcher, `alacr` + Enter launches Alacritty, Esc closes, a
  click outside (desktop, bar, other monitor) closes it through the
  grab surface, moving keyboard focus elsewhere closes it. The click
  outside is caught by a `listen_with` on button *release*
  (`launcher::grab_clicks`), not a `mouse_area` on the grab: Hyprland
  keeps pointer focus where it was until the pointer moves, so a
  second click on the bar button that opened the launcher, without
  moving, comes tagged with the launcher's window and no cursor
  position; the daemon closes on a click reported on the launcher
  while the pointer was last seen (`Message::Cursor`) on another
  window. On the release because closing on the press destroys the
  surface before its release, and Hyprland then swallows the next
  click. Verified: button → open, same click again → closed, again →
  open; clicks on the search field and on rows behave as before.
- Tray: on the real desktop (noctalia owns the watcher, we host
  through it) nm-applet (icon by name, from Adwaita) and MEGAsync
  (22px `IconPixmap`) show on both bars at 16px; a right click on
  MEGAsync opens its 3-row menu sized to its labels, on nm-applet the
  full menu with disabled rows, separators, ✓ on the two checkmarks,
  "Connessioni VPN ▸" unfolding in place (the app fills it on
  `AboutToShow`) and the popup growing 451 -> 480px; a click outside
  closes it. Not clicked on the live desktop (the rows do things).
- `tests/ui/run.sh`: `launcher` (open, search, arrows, Esc, click
  outside on both outputs, click a result, Enter, toggle/hide),
  `clock` (popup on both outputs, today, next/prev month, centred under
  the clock, click outside) and `tray` (a fake item registers with our
  watcher and shows on both bars; left/middle click and the wheel
  reach it with the expected arguments; the menu opens after
  `AboutToShow(0)` with 4 rows, a separator, the hidden row dropped;
  the submenu unfolds after `AboutToShow(3)`, the checked child shows,
  the popup grows; a disabled row does nothing; a row click sends
  `clicked` and closes the popup; `LayoutUpdated` reloads an open
  menu; `NewIcon` recolours the icon, `NewStatus` adds `.attention`;
  unregistering removes it) pass in the headless Sway.
- Custom: `tests/ui/run.sh custom` (a static button with icon and
  label, left click opens the launcher through `aria-shell launcher
  show`, right/middle/wheel run their programs; `exec` output as text
  and as JSON with a class and an icon, an empty output hides the
  gadget, one run for both panels and another after a click) passes;
  on the desktop `[Custom]` opens the launcher and `[Custom:updates]`
  shows `checkupdates`' count on both bars.
- Themes: `tests/ui/run.sh themes` (light at start, a left click on
  either bar toggles, the menu lists Light/Dark/Base/manjaro/waybar
  with the current ones checked, picking manjaro restyles live with
  its 28px bars, Base goes back) passes; on the desktop a click flips
  both bars' palette (`debug theme`).
- Notifications: `tests/ui/run.sh notifications` (`notify-send` on the
  scenario's bus: one surface per notification on the focused output,
  360px wide, 8px from the right edge and under the bar, the newest at
  the corner and the older ones pushed down with the gap; a theme icon,
  a body wrapping on two lines; critical gets the `.critical` border
  and no timeout; `-r` replaces in place and the surface shrinks;
  `CloseNotification` removes one and the survivor moves up; a right
  click dismisses; the `default` action has no button and a click on
  the body sends it, a button click sends its key (notify-send `-A`
  prints it) and closes; `-t 500` and the configured `duration = 2`
  expire; markup shown as text; an image file and an `image-data`
  hint draw) passes, screenshots in `target/ui/notifications/`. Not
  yet run on the real desktop.
- `cargo build`, `cargo clippy --workspace --all-targets`, `cargo test`
  (81 tests: notifications config/timeouts/replacement/markup/image-data, config, theme incl. scheme variables and root class,
  selectors, desktop entries, commands, launcher search, tray
  key/pixmap/props/menu parsing, wheel clicks, menu widget, command
  line splitting, script outputs, custom gadget): clean.

Implemented: config loading and hot-reload, `[general]` (`style`,
`reload_style`, `reload_config`, `icon_theme`), `[apps_class_map]`,
`[panel]` (`outputs`, `position`, `layer`, `items_*`), multi-output
panels, Clock (`format`, calendar popup), Workspaces (all four keys,
window icons) over the Hyprland IPC, with the daemon-owned
`Compositor` / `Context` / `Action` plumbing and the popup plumbing
(`Panel` <-> `Gadget` popup hooks), the CSS-like theme system
(`theme/`, `assets/base.css`, hot reload, bar thickness from the theme),
the command socket + CLI client, the launcher (`[launcher] terminal`),
the tray (`[Tray]`, SNI watcher/host, pixmap and named icons, dbusmenu
popups with inline submenus, popups sized from state and resized live),
light/dark schemes and the `[Themes]` gadget (toggle, theme picker),
`[Custom]` gadgets (label/icon, a program per button and wheel
direction, `exec` with `interval`/`format`/`return_type`/`hide_empty`,
run once by the daemon for every panel), the notification daemon
(`[notifications]` `enabled`/`duration`/`position`; one overlay layer
surface per notification, sized from the theme's `notification { width }`
and the measured content, stacked by margin from the corner with the
`notifications { padding, gap }` of the theme; summary, body with the
markup stripped, icon from `image-data` / `image-path` / `app_icon`,
action buttons, `default` on click, right click dismisses, expiry with
critical ones staying; `NotificationClosed` / `ActionInvoked`; another
daemon owning the name is waited out).

Not yet: Sway backend, `[panel]`
`size`/`align`/`margin`/`opacity`, panel height from content, Clock
`tooltip_format`, theme
properties beyond the current set (`margin`, `opacity`, gradients,
`@import`, `!important`, `@font-face` for theme-shipped fonts,
transitions), `:hover` on non-button widgets (needs a `mouse_area`
wrapper), every other gadget and component (lock, wallpaper,
terminal, idle), notification niceties (a "do not disturb" / history
gadget, `resident`/`transient` hints, sound, a per-app `image-data`
downscale, `x`/`y` hints, animation), launcher `DBusActivatable` entries,
a themed scrollbar (iced's default for now), persisting the theme
picked at runtime and following/setting the desktop's colour scheme
(portal / gsettings, per DE), tray tooltips / overlay
icons / menu icons and shortcuts / `org.freedesktop.StatusNotifierItem`
(the KDE name is what every app uses).

## Next steps, in order

1. Try the notifications on the real desktop (nothing owns the name
   there): `notify-send` from a terminal, a Firefox download, an
   `image-data` app (a chat client); a Notifications gadget on the bar
   (count, do-not-disturb, the recent ones in a popup) if wanted.
2. More theme surface as gadgets need it (`margin` via a wrapping
   container, `opacity`, `@font-face`, scrollbars); the `shader` widget
   for `background: shader("x.wgsl")` when a theme asks for more than
   CSS.
