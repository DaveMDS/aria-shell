# AGENTS.md

## What this is

Aria Shell: a desktop shell for Wayland compositors (Hyprland, Sway, ...),
providing a panel, launcher, lock screen, notification daemon, wallpaper
manager, terminal, etc.

**Currently being rewritten from Python/GTK4 to Rust** (`iced` +
`iced_exwlshell`). The Rust code at the repo root is the active project.
Read [RS-PORT.md](RS-PORT.md) before making architectural decisions — it
has the full rationale for the rewrite, alternatives that were considered
and rejected, known risks, and the current status of what's implemented.

`aria-shell-python/` is the previous, considerably more feature-complete
Python/GTK4 implementation. It is **legacy reference only** — a source of
truth for real working behavior (config format, gadget semantics, protocol
usage) to mirror while porting, not a codebase to add features to. Don't
edit it casually; if you need to change it, that's worth flagging, not
assuming.

## Build & run

```bash
cargo build
cargo run
```

Requires a Wayland compositor implementing `wlr-layer-shell-v1` (Hyprland,
Sway, ...). Won't work under X11 or on compositors without layer-shell
support. There's no mock mode, but there is a headless one: `tests/ui/`
runs the shell inside a nested `sway` with no GPU output (see
"Verifying").

With arguments the binary is a client of the running shell:
```bash
aria-shell launcher toggle          # what a compositor keybind runs
aria-shell lock                     # the lock screen (PAM checks the password)
aria-shell debug surfaces           # where our surfaces are (global rects)
aria-shell debug widgets 'launcher item:nth-child(2)'   # widget rects, by theme selector
aria-shell debug cursor             # where the pointer was last seen on us
aria-shell debug audio              # the mixer channels and the media players as the daemon sees them
```

## Architecture

Plain iced Elm architecture, nested: `AriaShell` (daemon, one entry per
open surface) → `Panel` (one layer surface on one output) → `AnyGadget`
(closed enum over gadget types) → e.g. `Clock` (`impl Gadget`). Each
level owns its state and `Message`, routes to children by key, and
`.map()`s their messages up. No globals: `Config` is loaded once and
passed by reference. External event sources are `Subscription`s.

Shared state (the compositor's workspaces/windows, the app icons, the
tray items, the outputs of gadget scripts, the mixer and the media
players, ...) is owned by the daemon (`Compositor` in `compositor/`,
`Icons` in `icons/`, `Tray` in `tray/`, `Scripts` in `scripts.rs`,
`Audio` in `audio/`), reaches gadgets read-only through
`gadget::Context` in `view`, and is changed by returning
`gadget::Action::Compositor(cmd)` / `Action::Tray(cmd)` /
`Action::Script(cmd)` / `Action::Audio(cmd)` from `update`. Gadgets never open their own IPC
or DBus connection, and never run a periodic program themselves (a
`Gadget::script` spec, run once by the daemon for every panel). Command
lines from the config go through `process.rs`: split with shell-like
quoting and run directly, no implicit shell, `aria-shell` meaning this
binary. A popup's
size is `Gadget::popup_size(ctx)`, a function of the state, re-asked
after every update. See RS-PORT.md's "Architecture" section
before changing the shape of any of these.

Styling lives in `theme/`: a CSS-like file (`assets/base.css` always,
plus `[general] style`), loaded for a light or dark scheme
(`:root.light` / `:root.dark` variables, `panel.dark { }` rules; the
`Themes` gadget switches scheme and theme at runtime), resolved per
widget in `view`. Gadgets never
hard-code colours, paddings or spacing: they derive a `theme::Node` from
`ctx.node` (`ctx.node.child("workspace").class_if("active", ..)`) and
build widgets with `ctx.theme.button/container/text/row`. A new element
or class is documented in the tree at the top of `assets/base.css`, and
given a neutral default there.

The Python implementation is a reference for **behaviour** (config
format, gadget semantics, protocol usage), not for structure. Don't port
its `Singleton`/`Service`/`Module` machinery, and only write a "mirrors
`foo.py`" comment where a specific behaviour is being reproduced, not for
structure.

## Verifying

`cargo test` covers the config, theme, desktop-entry, command and search
layers (the parts testable without a compositor).

The UI is verified by **scenarios in `tests/ui/`**: `tests/ui/run.sh`
starts a headless nested `sway` (two outputs, `WLR_BACKENDS=headless`),
the shell inside it with the config in `tests/ui/config` and the desktop
entries in `tests/ui/data`, and runs each `tests/ui/scenarios/*.sh` with
the vocabulary of `tests/ui/lib.sh`: `click_widget '<selector>'`,
`type_text`, `key Down`, `scroll 2`, `assert_surface popup`,
`count_widgets`, `shot_surface`, `sni_start`/`sni_event`/`sni_send`.
Input goes through `tests/ui/inject` (a persistent virtual keyboard +
pointer), positions through the shell's `debug` commands, tray items
through `tests/ui/sni` (a fake status notifier item with a menu, on a
private session bus from `dbus-run-session`), media players through
`tests/ui/mpris` (a fake MPRIS player on the same bus), so nothing
depends on the desktop's compositor or bus. Results land in `target/ui/<scenario>/`
(status, logs, screenshots). Needs `sway`, `grim` and `dbus-run-session`
installed; no root. Add a scenario for every new interactive
piece; run them before reporting a UI change as done.

On the live desktop, the same without the nested compositor: `ydotool`
(uinput) for input, `aria-shell debug ...` for positions, `grim -g` for
screenshots. Don't reach for `hyprctl`: the shell must work on other
compositors, and the shell's own answers are what's being verified.
Logs go through `log`; `RUST_LOG=aria_shell=debug cargo run` for more.

## Commit style

Match the existing history's informal style -- do not switch to
Conventional Commits. Short subject, no enforced capitalization, no
trailing period, optional `Component: description` prefix, optional
free-form body when it helps. Do not add a `Co-Authored-By` attribution
line.

## Translations

UI texts never appear as literals in a view: `ctx.locale.tr("audio.output")`
(`src/locale.rs`), dates with `ctx.locale.date(&dt, fmt)`. Keys are
stable dotted ids; the texts live in `src/locale/en.rs` and `it.rs`,
compiled in. Adding a text: a key in both files, then `cargo test`
(`locale::tests::catalogues_are_complete` names anything missing or
unused). Adding a language: a file with English's keys, one line in
`CATALOGUES`.

## Known risks (see RS-PORT.md for detail)

- `iced_exwlshell` is a small, fast-moving crate (recently renamed from
  `iced_layershell`/`iced_sessionlock`) -- expect API churn, verify against
  its actual source rather than assuming an API shape from memory.
- PAM (the lock screen's password check) has a rough Rust story -- even
  COSMIC's own official greeter has open production auth bugs. Ours is a
  direct `libpam` binding (`locker/pam.rs`), the `login` service unless
  `/etc/pam.d/aria-shell` is installed; treat changes there with care and
  try them (`cargo test -- --ignored pam`, `tests/ui/run.sh locker`).
- `libcosmic`/COSMIC's source (`cosmic-panel`, `cosmic-applets`,
  `cosmic-notifications`, `cosmic-greeter`) is a useful pattern reference
  for layer-shell/tray/notifications/lock-screen -- read it for ideas, do
  not add it as a dependency.
