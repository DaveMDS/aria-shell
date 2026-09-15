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

Clock      (gadgets/clock.rs)  impl Gadget: new / update / view(ctx) / subscription

Compositor (compositor/)    daemon-owned desktop state: workspaces, windows, active/urgent flags
  subscription()            the single IPC stream (compositor/hyprland.rs), yields `Event`s
  apply(Event)              patches the state
  run(Command) -> Task      sends a command (activate workspace/window) to the backend
```

Two things flow between the daemon and the gadgets besides messages:

- **`gadget::Context`** goes *down*, into `view`. It holds `&Compositor`
  (later `&Audio`, `&Tray`, ...): daemon-owned, read-only. A gadget that
  shows shared state keeps no copy of it, it filters the context in `view`.
- **`gadget::Action`** comes *up*, out of `update`, in place of a bare
  `Task`: `Action::Run(Task)` for the gadget's own async work,
  `Action::Compositor(Command)` (and later `Action::Audio(..)`, ...) for
  things only the daemon can do. `AriaShell::perform` turns it into a
  `Task`. Gadgets never hold an IPC handle.

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
- `cargo build`, `cargo clippy --all-targets`, `cargo test`: clean.

Implemented: config loading, `[panel]` (`outputs`, `position`, `layer`,
`items_*`), multi-output panels, Clock (`format`), Workspaces (all four
keys; windows are dots, not icons) over the Hyprland IPC, with the
daemon-owned `Compositor` / `Context` / `Action` plumbing.

Not yet: Sway backend, window icons in Workspaces (XDG desktop lookup +
icon theme + `svg`/`image` in iced), `[panel]`
`size`/`align`/`margin`/`opacity`, panel height from content, hot-reload,
styling/theme, click/popover on the Clock, every other gadget and
component (tray, notifications, launcher, lock, wallpaper, terminal,
idle).

## Next steps, in order

1. The Clock calendar popover: first non-panel surface, exercises
   `NewPopUp` on the multi-window runtime.
2. Styling: how the bar looks (background, fonts, the workspace buttons)
   before more gadgets pile up on an unstyled row.
3. Config hot-reload (watch the file, rebuild panels).
4. Window icons in Workspaces (needs the XDG/icon-theme service that the
   launcher and tray will need too).
