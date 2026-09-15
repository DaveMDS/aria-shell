# Aria Shell

![License](https://img.shields.io/github/license/davemds/aria-shell)
![LOC-RS](https://img.shields.io/endpoint?label=LOC&color=orange&logo=rust&url=https://ghloc.vercel.app/api/DaveMDS/aria-shell/badge?filter=.rs)

A fast, modern and customizable desktop shell for your Wayland compositor.

AriaShell is a full-featured desktop shell designed to complement Wayland
compositors such as **Hyprland**, **Sway**, and others. It provides a panel,
launcher, lock screen, notification daemon, wallpaper manager, terminal, and
more — all configurable and themeable.

> [!WARNING]
> **This is a ground-up rewrite in Rust, in early development.** The
> currently working feature set is minimal (a layer-shell panel with a
> Clock gadget) — see [RS-PORT.md](RS-PORT.md) for the full rationale,
> status, and what's planned next.
>
> The previous, considerably more feature-complete Python/GTK4
> implementation lives on in [`aria-shell-python/`](aria-shell-python/) as a
> reference while the Rust port catches up — see
> [its README](aria-shell-python/README.md).

---

## Why a rewrite

The Python/GTK4 implementation had a solid internal architecture
(config/service/module/gadget) but GTK4 itself turned out to be an
uncomfortable fit: a clunky development experience, and — more importantly
— a CSS dialect too limited for the level of visual customization this
project wants. The move to Rust + [`iced`](https://iced.rs) +
[`iced_exwlshell`](https://github.com/waycrate/exwlshelleventloop) (a
lightweight, actively maintained Wayland layer-shell binding) trades that
for a toolkit with direct wgpu access per widget — enough headroom to build
genuinely custom visual themes later, without pulling in Qt or a browser
engine. The full reasoning, alternatives considered, and rejected, and
open risks are written up in [RS-PORT.md](RS-PORT.md).

## Building and running

Requires a Wayland compositor implementing `wlr-layer-shell-v1` (Hyprland,
Sway, and similar — not X11, and not compositors without layer-shell
support).

```bash
git clone https://github.com/davemds/aria-shell.git
cd aria-shell
cargo build
cargo run
```

## Configuration

A trimmed sample `assets/aria.conf` is included for local development,
read automatically when no installed config is found. Only the sections
the current feature set actually uses are present; see
[RS-PORT.md](RS-PORT.md) for what's implemented so far.

---

## Credits & Inspiration

- [Fabric](https://github.com/Fabric-Development/fabric)
- [Ignis](https://github.com/linkfrg/ignis)
- [Waybar](https://github.com/Alexays/Waybar) — style inspiration
- [COSMIC](https://github.com/pop-os/cosmic-epoch) (`libcosmic`) — pattern
  reference for layer-shell/tray/notifications/lock-screen in Rust
