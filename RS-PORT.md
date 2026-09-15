# Rust port spike: Panel + Clock gadget

## Context

`aria-shell` is currently Python + GTK4 + gtk4-layer-shell (~8.6k lines).
The internal architecture (Config/Service/Module/Gadget) is solid, but GTK4
itself has turned out to be an uncomfortable fit: the development experience
is clunky, and — more importantly — GTK's CSS is too limited for the level
of visual customization we actually want.

### How we got here

- Qt was explicitly ruled out from the start.
- EFL/Edje was considered (the author has 20 years of history with
  `python-efl`) but rejected: the EFL project itself is effectively dead,
  and it would be a more extreme jump than needed anyway.
- A "real" CSS engine without GTK (e.g. Blitz/Stylo) was considered and
  rejected: on Linux, any webview with real CSS still requires WebKitGTK
  underneath, which means GTK comes back in — plus a whole browser engine
  on top. That makes the dependency footprint worse, not better. Blitz
  standalone is also too young/unstable to bet a rewrite on.
- Landed on **Rust + `iced` + `iced_exwlshell`** (formerly `iced_layershell`
  + `iced_sessionlock`, merged/renamed as of v0.20): a lightweight (wgpu),
  non-Qt, non-GTK toolkit with actively maintained Wayland layer-shell
  support (verified via a same-day commit at research time). `iced`'s
  `shader` widget gives direct per-widget wgpu access — this covers the
  "more visual freedom than CSS" requirement (the existing Shadertoy
  wallpaper effect can be ported ~1:1, and arbitrary GPU-drawn effects are
  possible elsewhere too) without having to commit to a specific "skin
  format" right now — that question stays open and isn't blocking for this
  spike.
- Real risks surfaced by actual web research (not from training memory):
  PAM in Rust is the weakest link (even COSMIC's own official greeter has
  open production bugs around it); the idle-notifier protocol, raw
  PipeWire volume control, and the GStreamer→wgpu bridge for video all lack
  mature ready-made crates — expect to hand-roll them. `iced_layershell`
  was just renamed to `iced_exwlshell`: small team, expect API churn.
- **COSMIC** (System76/Pop!_OS, a production DE built on an `iced` fork,
  `libcosmic`) has already solved every one of these subsystems in real,
  shipped Rust code: multi-monitor layer-shell panels, SNI tray, a
  notification daemon, a PAM-based lock screen. Decision: **study and
  adapt their patterns as reference, stay on vanilla `iced`** — don't
  depend on `libcosmic` itself, since it's a fairly opinionated toolkit
  built for a different DE.
- Given the scope of a full rewrite (GTK isn't isolated behind a `gui/`
  layer — it's spread across ~30 files: every component, every module,
  most services), we start with a **small, focused spike**: just the Panel
  with just the Clock gadget (no calendar popover), but with the
  foundational layers (config, service, module/gadget, layer-shell window)
  already in their "definitive" shape — designed to scale to future
  modules without a redesign, but without building generic machinery that
  isn't needed yet (no dyn-dispatch, no config derive-macro, no dynamic
  registry: the module set is closed and compiled-in, so a static `match`
  is enough).

The goal of this spike is to validate the full path end-to-end
(`aria.conf` → typed config → module → gadget → layer-shell window → live
update) before investing further, and to surface any surprises in the
(young, moving) `iced`/`iced_exwlshell` APIs early rather than halfway
through a much bigger effort.

## Python reference files (mirror behavior, don't copy code)

`aria_shell/config.py`, `aria_shell/services/aria_service.py`,
`aria_shell/module.py`, `aria_shell/gadget.py`, `aria_shell/modules/clock.py`,
`aria_shell/components/panel.py`, `aria_shell/gui/window.py`,
`aria_shell/assets/aria.conf`, `aria_shell/utils/_basic.py` (`Singleton`,
`Timer`), `aria_shell/utils/env.py` (`lookup_config_file`).

To study later (pattern reference only, never a dependency) once tray,
notifications, and the lock screen are tackled: the open-source code of
`cosmic-panel`, `cosmic-applets` (SNI status-area applet),
`cosmic-notifications`, `cosmic-greeter` (System76, MPL-2.0).

## Initial setup

- Started on a dedicated git branch (`rust-spike`) for this experimental
  work.
- The repo has since been reorganized: Rust is now the project at the repo
  root (`Cargo.toml`, `src/`, `assets/`), and the original Python
  implementation was moved to `aria-shell-python/` (its own `pyproject.toml`,
  `aria_shell/`, `tests/`, `README.md`, `LICENSE`, `.gitignore`, `Makefile`)
  as a subordinate, legacy reference implementation — not deleted, since it
  documents real behavior worth mirroring while the Rust port is incomplete.
- Single Cargo package (no workspace yet — introduce one only when/if a
  second crate shows up), named `aria-shell`.

## File layout

```
aria-shell/                     (repo root)
├── Cargo.toml
├── assets/
│   └── aria.conf               trimmed dev/sample config (Clock section only)
├── src/
│   ├── main.rs                 iced app + top-level Message enum
│   ├── config/
│   │   ├── mod.rs              AriaConfig (loader/singleton), mirrors config.py::AriaConfig
│   │   ├── model.rs            ConfigSection trait + parsing helpers (bool/list)
│   │   └── general.rs          GeneralConfig, mirrors AriaConfigGeneralModel
│   ├── service.rs               Service trait + ServiceCell<T>, mirrors aria_service.py
│   ├── module.rs                 Module trait + GadgetRunContext, mirrors module.py
│   ├── panel.rs                  layer-shell window + PanelState, mirrors panel.py + window.py
│   └── modules/
│       ├── mod.rs               GadgetSlot enum + request_gadget() (the "registry")
│       └── clock.rs             ClockConfig, ClockModule, ClockState, clock::Message
└── aria-shell-python/            legacy Python/GTK4 implementation (reference only)
```

## Cargo.toml (verify against real docs before building)

```toml
[package]
name = "aria-shell"
version = "0.0.1"
edition = "2021"

[dependencies]
iced = { version = "0.14", features = ["wgpu", "tokio"] }
iced_exwlshell = "0.20.1"
configparser = "3"
chrono = { version = "0.4", default-features = false, features = ["clock"] }
```

(`tokio` feature added during implementation -- `iced::time::every` needs
it, see the resolved-uncertainties section below.)

- `configparser` over `rust-ini`: closer behavior to Python's stdlib
  (`Ini::new_cs()` for case-sensitivity, matching `optionxform = str`;
  configurable inline comments, matching `inline_comment_prefixes=('#',)`).
  **Highest-risk point in the whole config layer**: confirm `new_cs()` is
  actually case-sensitive on section names, or `Clock:2` would silently
  break.
- `chrono` for `strftime`-style formatting (`ClockConfig.format` passed
  straight into `.format()`, same `%`-specifier language as Python).

## Key architecture decisions

**Config (`config/model.rs`)**: no generic runtime introspection (Rust has
no `get_annotations` equivalent). Every typed section hand-writes its own
`from_section(&HashMap<String,String>) -> Self` behind a shared
`ConfigSection { const SECTION; fn from_section(...) }` trait. No generic
`validate_<key>` hook mechanism: if a future config needs one, it's just a
function call inside its own `from_section`, not a runtime-dispatched hook.

**Service (`service.rs`)**: the layer is fully defined (`trait Service` +
`ServiceCell<T>` backed by `OnceLock<Mutex<T>>`, one static instance per
type) but **has zero consumers in this spike** — Clock doesn't use a
Service in Python either (it drives its own `Timer`), and the periodic tick
has a better native fit in iced's `Subscription` system. Building a Service
just for the tick would fight the framework.

**Module/Gadget (`module.rs`, `modules/mod.rs`)**: **a flat `Message` enum,
no `dyn Any`/type-erasure.** Why: the module set is closed and known at
compile time (no dynamic plugins to support), and `iced_exwlshell`'s macro
(`#[to_layer_message]`/`#[to_exwlshell_message]`, exact name to confirm)
decorates a single top-level `Message` enum — fighting that with
per-module erasure would go against the crate's intended usage. The
"registry" that replaces Python's dynamic `importlib` becomes a
`GadgetSlot` enum + a `request_gadget(name, ...)` function with a `match`
on the string (mirrors `request_module_gadget`): adding a future module
(e.g. Workspaces) = one new file + one enum variant + one match arm + one
`Message` variant in `main.rs` — no trait redesign.

**Clock tick**: a single top-level subscription
(`iced::time::every(Duration::from_secs(1))`) updates every Clock instance
in `update()`, mirroring Python's single `Timer` broadcasting to all
`self.gadgets`. `Module::subscription()` exists in the trait for future
modules that need their own event source (e.g. Workspaces via the
compositor's IPC socket); Clock leaves it as `Subscription::none()`.

**Panel/layer-shell window (`panel.rs`)**: only what a top-anchored bar
needs — no generic reusable `AriaWindow` for launcher/lock/exiter (out of
scope). `PanelState` has 3 slots (start/center/end) like
`AriaPanel.populate`, but for this spike it's **hardcoded** to a single
centered Clock: no `[panel]`/`PanelConfig` parsing yet (no
`items_start`/`items_center`/`items_end`).

## Explicitly out of scope for this spike

DBus/tray, audio, notifications, terminal, wallpaper, lock screen, idle
daemon, multi-monitor gadget duplication, config/style hot-reload,
packaging, click/tooltip/popover on the Clock.

## Status: spike implemented and verified (2026-09-15)

Built on branch `rust-spike`. `cargo build` is clean (zero warnings after
marking the intentionally-unused foundational pieces `#[allow(dead_code)]`).
Ran on the real Hyprland session with `cargo run` / the built binary
directly:

- `hyprctl layers` shows a genuine `aria-panel` layer-shell surface
  (`namespace: aria-panel`) on the `top` layer, full output width, fixed
  32px height, stacked correctly below the pre-existing bar on the same
  output (Hyprland's own exclusive-zone stacking, not a bug).
- Screenshot confirms the Clock gadget renders and reads a real `[Clock]`
  config section: initially showed `15 Sep 2026 21:48` (the configured
  `%e %b %Y  %H:%M` format). At the time of this test the config still
  lived at `aria_shell/assets/aria.conf` (pre-reorg); the crate now reads
  the trimmed `assets/aria.conf` at the repo root instead (see "Initial
  setup" above) -- same lookup logic, same result.
- Live-edited `format =` to `RUST SPIKE TEST %H:%M:%S`, restarted: bar
  showed `RUST SPIKE TEST 21:49:52`, seconds visibly ticking one screenshot
  to the next — proves the `aria.conf → ClockConfig → view()` path and the
  1Hz `iced::time::every` subscription are both real, not hardcoded.
  Config file was reverted after the test.
- Process stayed alive and responsive across restarts and a 70+ second
  run with no panics.

### How each flagged uncertainty actually resolved (found in the real
`iced_exwlshell` 0.20.1 source, not guessed)

1. `iced::time::every` requires the `tokio` (or `smol`) Cargo feature on
   `iced` -- added `features = ["wgpu", "tokio"]`.
2. `exclusive_zone` is a plain `i32` field on `LayerShellSettings`
   (default `-1`); no separate "auto" mode exists in this crate. Used a
   fixed pixel value (`PANEL_HEIGHT = 32`) matching the bar's own height.
3. `Anchor` **is** bitflag-combinable (`Anchor::Top | Anchor::Left |
   Anchor::Right` compiles and works) -- it's a re-export of the raw
   `zwlr_layer_surface_v1::Anchor` protocol type.
4. Output targeting wasn't needed/exercised (single-output dev machine,
   default `StartMode` picked the active output) -- still open for the
   multi-monitor phase.
5. `LayerSize::fill_width(height)` exists and is exactly what a
   full-width, fixed-height bar needs. Its default (`LayerSize::FILL`,
   used when `size` is left out of `LayerShellSettings`) fills the
   *entire remaining output height* when only `Anchor::Top` is set (no
   `Bottom`) -- this was a real bug hit during the first run (the surface
   was 1045px tall instead of 32px), fixed by setting `size` explicitly.
6. Both `#[to_layer_message]` and `#[to_exwlshell_message]` exist (the
   crate's real examples use `to_layer_message`); used that one.
7. `configparser::ini::Ini::new_cs()` is confirmed case-sensitive on both
   section names and keys -- required for `Clock:2` to work correctly.

## Genuinely uncertain API points — verify against real docs/compiler, don't trust memory

1. Which `iced` 0.14 feature flags are needed for `Task`/async and for
   `iced::time::every` (exact module path to confirm).
2. Exact semantics of `LayerShellSettings.exclusive_zone` (fixed pixel
   count vs. protocol-level "auto", i.e. `-1`, as in raw
   `zwlr_layer_surface_v1::set_exclusive_zone`).
3. Whether `Anchor` is a combinable bitflag (`Anchor::Top | Anchor::Left |
   Anchor::Right`) or something else — only a single `Anchor::Bottom` was
   seen in the real examples fetched during research.
4. Output/monitor targeting API name and shape (`StartMode::TargetOutput`
   vs `OutputOption` — conflicting signals, likely version drift). Not
   needed for this spike, but will matter as soon as multi-monitor support
   is added.
5. `LayerSize` constructor for "fill width, fixed height" (`fill_width(h)`
   seen once, not confirmed against authoritative docs).
6. Exact attribute macro name: `#[to_layer_message]` (seen in a real
   example) vs `#[to_exwlshell_message]` (named in the docs.rs summary for
   0.20.1) — could be an alias, or the example could be from a different
   crate version.
7. Exact `configparser` accessor for a section's raw map, and whether it
   supports value-less keys (`allow_no_value`-style, not needed for this
   spike).

The first `cargo build` is the natural checkpoint to resolve these against
real compiler errors/docs — not before.

## End-to-end flow: aria.conf → ClockConfig

1. `main()` calls `AriaConfig::global()` → loads the file (same lookup as
   `lookup_config_file`: XDG dirs, then fall back to the trimmed
   `assets/aria.conf` at the repo root for local-dev convenience).
2. `request_gadget("Clock", output_name)` →
   `AriaConfig::global().section::<ClockConfig>(Some("Clock"))` → raw
   case-sensitive section lookup → `ClockConfig::from_section` (defaults to
   `"%H:%M"` if missing/empty, no validation, same as Python).
3. With the current sample file: `"Clock"` → `format = "%H:%M:%S"`. A
   `"Clock:2"` instance section works the same way (section lookup is
   generic on the name) but isn't reachable from the default panel in this
   spike, since there's no `items_center` config support yet.

## Verification plan

1. `cargo build` (from the repo root) — first real checkpoint: this is
   where every uncertain API point above gets resolved against actual
   compiler errors/docs.
2. On a Wayland session with a `wlr-layer-shell-v1` compositor (Hyprland or
   Sway — not X11, not a compositor without layer-shell): `cargo run`.
3. Expected visual result: a thin bar anchored to the top edge, full width
   of the output, above normal windows, showing the current time as plain
   text, updating once per second, no decorations, not stealing keyboard
   focus (clicking through to windows underneath should still work).
4. Manual checks:
   - Restart `cargo run` — the bar should reappear in the same place.
   - Edit the `format =` line under `[Clock]` in `assets/aria.conf` and
     restart (no hot-reload in this spike) — the displayed format should
     change, proving the
     config→`ClockConfig`→`view()` path is real, not hardcoded.
   - `hyprctl clients` / `swaymsg -t get_tree` — the surface should show up
     as a layer-shell surface, not a normal toplevel, confirming
     `iced_exwlshell` actually went through the layer-shell path.
5. Explicitly not tested this round: multi-monitor, clicking the Clock,
   hot-reload, any other module, tray/notifications/wallpaper/lock.
