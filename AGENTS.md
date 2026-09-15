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

Mirrors the Python implementation's `config`/`service`/`module`/`gadget`
layering, adapted to Rust idioms (traits + associated types instead of
runtime reflection, a flat top-level `Message` enum instead of dynamic
dispatch — the module set is closed and compiled-in, not a real plugin
system). See RS-PORT.md's "Key architecture decisions" section for the
reasoning behind each choice before changing the shape of these traits.

Source code comments referencing a Python file (e.g. "mirrors
`aria_shell.config.AriaConfigModel`") are intentional -- they document
*why* something is shaped the way it is, by pointing at the original
behavior being mirrored. Keep that pattern when porting a new piece.

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
