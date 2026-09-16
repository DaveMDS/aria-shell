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
support. There's no headless/mock mode — verifying a change means actually
running it on a live Wayland session.

Verifying a layer-shell surface actually appeared (not just "it compiled
and didn't crash"):
```bash
hyprctl layers      # Hyprland
swaymsg -t get_tree # Sway
```

## Architecture

Plain iced Elm architecture, nested: `AriaShell` (daemon, one entry per
open surface) → `Panel` (one layer surface on one output) → `AnyGadget`
(closed enum over gadget types) → e.g. `Clock` (`impl Gadget`). Each
level owns its state and `Message`, routes to children by key, and
`.map()`s their messages up. No globals: `Config` is loaded once and
passed by reference. External event sources are `Subscription`s.

Shared state (the compositor's workspaces/windows; later audio, tray,
...) is owned by the daemon (`Compositor` in `compositor/`), reaches
gadgets read-only through `gadget::Context` in `view`, and is changed by
returning `gadget::Action::Compositor(cmd)` from `update`. Gadgets never
open their own IPC connection. See RS-PORT.md's "Architecture" section
before changing the shape of any of these.

Styling lives in `theme/`: a CSS-like file (`assets/base.css` always,
plus `[general] style`), resolved per widget in `view`. Gadgets never
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

`cargo test` covers the config and theme layers (the parts testable
without a compositor). Everything else needs a real run: `cargo run`, then
`hyprctl layers` to confirm the surfaces, and a screenshot (`grim -g
"0,0 1920x40" out.png`) to confirm what's drawn. Logs go through `log`;
`RUST_LOG=aria_shell=debug cargo run` for more.

## Commit style

Match the existing history's informal style -- do not switch to
Conventional Commits. Short subject, no enforced capitalization, no
trailing period, optional `Component: description` prefix, optional
free-form body when it helps. Do not add a `Co-Authored-By` attribution
line.

## Known risks (see RS-PORT.md for detail)

- `iced_exwlshell` is a small, fast-moving crate (recently renamed from
  `iced_layershell`/`iced_sessionlock`) -- expect API churn, verify against
  its actual source rather than assuming an API shape from memory.
- PAM (needed for the lock screen) has a rough Rust story -- even COSMIC's
  own official greeter has open production auth bugs. Treat this as the
  highest-risk unimplemented piece.
- `libcosmic`/COSMIC's source (`cosmic-panel`, `cosmic-applets`,
  `cosmic-notifications`, `cosmic-greeter`) is a useful pattern reference
  for layer-shell/tray/notifications/lock-screen -- read it for ideas, do
  not add it as a dependency.
