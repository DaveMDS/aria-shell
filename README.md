# Aria Shell

![License](https://img.shields.io/github/license/davemds/aria-shell)
![LOC-RS](https://img.shields.io/endpoint?label=LOC&color=orange&logo=rust&url=https://ghloc.vercel.app/api/DaveMDS/aria-shell/badge?filter=.rs)
![LOC-CSS](https://img.shields.io/endpoint?label=CSS&color=pink&url=https://ghloc.vercel.app/api/DaveMDS/aria-shell/badge?filter=.css)

A fast, modern and customizable desktop shell for your Wayland compositor.

AriaShell is a full-featured desktop shell designed to complement Wayland compositors
such as **Hyprland**, **Sway**, and others. It provides a panel, launcher, lock screen,
exit menu, notification daemon, wallpaper manager, and more — all configurable and
themeable through a CSS-like stylesheet.

> [!WARNING]
> **The project is in active development.** Expect breaking changes and incomplete features.
>
> AriaShell is being rewritten in **Rust** on [iced](https://iced.rs) (wgpu) and
> [iced_exwlshell](https://github.com/waycrate/exwlshelleventloop); the previous
> Python/GTK4 implementation is kept in `aria-shell-python/` as the behaviour
> reference until the port has caught up. The tables below are the port's status.

---

## Components

### 🗂️ Aria Panel
A fully customizable panel with a rich set of built-in gadgets, one layer-shell
bar per monitor (or per `[panel:*]` section):

| Gadget          | Status | Description                                                                |
|-----------------|---|---------------------------------------------------------------------------------|
| `Clock`         | ✅ | Current time with a calendar popup                                              |
| `SystemMonitor` | ✅ | CPU, RAM, swap, disks, network, GPU, load and temperature with a btop-like popup (process table, Terminate/Kill) |
| `WorkSpaces`    | ✅ | Workspaces and windows overview (Hyprland & Sway)                               |
| `Audio`         | ✅ | Volume control, multichannel mixer (PipeWire/PulseAudio) and MPRIS2 media controls |
| `Tray`          | ✅ | System tray via (K)StatusNotifierItem + DBusMenu                                |
| `Themes`        | ✅ | Shell theme selector with light/dark mode support                               |
| `Themes`        | 🔲 | Icon theme selector (icon theme is set in the config for now)                   |
| `Notifications` | ✅ | Notification bell with unseen count, history popup and do-not-disturb           |
| `Custom`        | ✅ | User-defined gadgets with label, icon, commands per mouse button and a periodic `exec` (text or JSON) |
| `logout`        | ✅ | A `Custom` button to invoke Aria Exiter (`aria-shell exiter toggle`)            |
| `power`         | 🔲 | Idle inhibitor, battery status, power profiles                                  |
| `network`       | 🔲 | Full featured network manager                                                   |
| `bluetooth`     | 🔲 | bluetooth manager                                                               |
| `screenshot`    | 🔲 | Screenshot and screen recorder                                                  |
| `apps`          | 🔲 | fixed list of apps to run (like a dock)                                         |
| `home`          | 🔲 | a menu (cinnamon style) with app categories, search, favorites and sys controls |
| `file`          | 🔲 | file browser in a tree of menus?                                                |
| `places`        | 🔲 | menu with usefully locations, like home, favorites, devices                     |
| `brightness`    | 🔲 | set monitor bright....how?                                                      |

- ✅ Multi-monitor, hot-plug aware
- ✅ Config hot-reload (`aria.conf`) and theme hot-reload

Full configuration via the `aria.conf` file.


---

### 🚀 Aria Launcher
An application launcher with support for `.desktop` files.

- ✅ Search and run `.desktop` applications (names and descriptions in your language)
- ✅ App list auto-update on install/uninstall
- ✅ The exit menu's actions as a row of buttons above the search field
- ✅ Usage-based ranking (what you launch most comes first; counts in `~/.local/state/aria-shell/launcher-usage`)
- 🔲 Secondary commands (e.g. "Firefox — New Private Window")


---

### 🔒 Aria Locker
A lock screen implementing the `ext-session-lock-v1` Wayland protocol.

- ✅ Date/time and user name/avatar display
- ✅ PAM-based password authentication (with PAM's own messages, e.g. a locked account)
- ✅ Show/hide the password
- 🔲 Background customization (same capabilities as Aria Wallpaper)


---

### 🚪 Aria Exiter
A session management dialog for locking, suspending, hibernating, logging out, rebooting and shutting down.

- ✅ Fully customizable actions and labels via config
- ✅ Auto-expiring confirmation dialogs for dangerous actions
- ✅ Custom buttons with icon, label and confirmation support
- ✅ `logout = auto` asks the compositor itself (Hyprland, Sway)
- ✅ Keyboard navigation


---

### 🖼️ Aria Wallpaper
A background manager built on the `LayerShell` Wayland protocol. Each monitor can
have a completely independent wallpaper, and sources can be mixed freely across
displays — a static image on the laptop screen, an animated GIF on a second
monitor, a live shader on the third.

#### Supported sources

**Static images** — standard raster formats (PNG, JPEG, WEBP).

**Animated GIFs** *(planned)* — frame-accurate GIF playback, looped continuously. Useful for subtle motion loops without the overhead of a video file.

**Shadertoy shaders** *(planned)* — GLSL fragment shaders sourced directly from [shadertoy.com](https://shadertoy.com). Save any shader code as a `.shadertoy` file and point the config at it. The shader is executed on the GPU every frame, giving you a fully animated, procedurally generated background with zero CPU cost.

- ✅ Per-monitor backgrounds
- ✅ Fit modes: cover, contain, fill, none, scale-down (CSS `object-fit`)
- ✅ Static images (reloaded when the file changes)
- 🔲 Animated GIFs
- 🔲 [Shadertoy](https://shadertoy.com) shader support (`.shadertoy` files)
- 🔲 texture based shader support
- 🔲 Cycle through files in folder
- 🔲 day-time-based wallpapers
- 🔲 auto-pause when on battery? or when full covered?_


---

### 🔔 Aria Notifier
A full-featured desktop notification server, replacing tools like `mako`.

- ✅ Icon and image data via DBus
- ✅ Action buttons inside notifications
- ✅ Urgency styling via the theme
- ✅ Configurable corner, timeout, per-notification replacement
- ✅ History and do-not-disturb (the `Notifications` gadget)
- 🔲 Markup support (markup is stripped for now)
- 🔲 Sound support, notification persistence
- 🔲 Limit the number of visible notification somehow


---

### 💻 Aria Terminal *(not ported yet)*
A lightweight drop-down terminal.

- 🔲 Show/hide on command (Quake-style)
- 🔲 Configurable opacity, font, size and shell
- 🔲 Optional display grab when visible
- 🔲 Fullscreen emulation via `Ctrl+F`
- 🔲 show/hide animation ala quake console


---

### 💤 Aria Idler *(experimental, not ported yet)*
An idle daemon using the `ext_idle_notifier_v1` Wayland protocol.

>[!NOTE] I'm not sure if this thing should be an aria responsibility, seems
>we are fighting with systemd abilities.

- 🔲 Configurable idle/resume commands (Aria or external)
- 🔲 Simple syntax in `aria.conf`
- 🔲 Per-scenario timeouts (AC vs battery)
- 🔲 manage events like on-lid-closed? How?
- 🔲 manage before-sleep and the like?


---

### 🎨 Theming
The look comes from a CSS-like stylesheet: `assets/base.css` documents the
element tree and the supported properties, a user theme (`[general] style`)
is loaded on top, with light and dark colour schemes. Themes are found in
`~/.config/aria-shell/themes/`, the XDG data dirs and `assets/themes/`.

- ✅ Selectors (type, class, id, attributes, `:hover`/`:active`/`:focus`, `:nth-child`), variables, cascade
- ✅ Light/dark schemes, switched at runtime by the `Themes` gadget
- ✅ Hot reload while editing (a broken file keeps the running theme)
- 🔲 `margin`, `opacity`, gradients, `@import`, `@font-face`, transitions
- 🔲 Shader backgrounds for widgets (`background: shader("x.wgsl")`)


---

### 🌍 Translations
Every text and date follows `[general] language`, or the environment's locale.
English and Italian so far; adding a language is one file (`src/locale/`), and
`cargo test` checks it's complete.


---

### ⌨️ Aria Commands
Control AriaShell programmatically via commands (the same binary is the client):

```
aria-shell ping
aria-shell lock
aria-shell launcher [toggle|show|hide]
aria-shell exiter   [toggle|show|hide]
aria-shell debug    surfaces|widgets [selector]|cursor|theme|locale|sysmon|audio
TODO: reload
TODO: terminal [toggle|show|hide]
TODO: notify ....
TODO: osd ...
TODO: dmenu ...
```


---

## Dependencies

### System libraries
```
libpam            # lock screen authentication
libpulse          # audio gadget (PipeWire's pipewire-pulse or PulseAudio at runtime)
a Vulkan or OpenGL driver for wgpu
an icon theme (Adwaita, breeze, ...) and the fonts your theme names
```

### Build
```
Rust (stable, edition 2024) and cargo
```

### Arch Linux
```bash
sudo pacman -S rust pam libpulse pipewire-pulse adwaita-icon-theme
```

### Development extras
UI scenarios run in a headless nested Sway (`tests/ui/run.sh`):
```bash
sudo pacman -S sway grim dbus     # tests/ui
sudo pacman -S ydotool            # driving the shell on the live desktop
```

---

## Installation

> Packaging for distributions is a work in progress... help needed!

To run AriaShell just clone the repo and build it:

```bash
# install dependencies (see above), then:
git clone https://github.com/davemds/aria-shell.git
cd aria-shell
cargo build --release
./target/release/aria-shell
```

Run from a checkout the shell finds its sample config and themes in
`assets/`. For the lock screen's PAM service you may install
`assets/pam.d/aria-shell` as `/etc/pam.d/aria-shell` (the default `login`
service is used otherwise).


---

## Configuration

AriaShell is configured through a single `aria.conf` file
(`~/.config/aria-shell/aria.conf`). Each component and gadget can be enabled,
disabled and tuned independently. Refer to the example config included in the
repository (`assets/aria.conf`) for a full reference.


---

## Credits & Inspiration

- [Fabric](https://github.com/Fabric-Development/fabric)
- [Ignis](https://github.com/linkfrg/ignis)
- [Waybar](https://github.com/Alexays/Waybar) — style inspiration
- [COSMIC](https://github.com/pop-os/cosmic-epoch) — a production desktop on iced, studied for the layer-shell, tray, notifications and lock screen patterns
- Wallpaper shader art: [@1041uuu](https://x.com/1041uuu), [zuranthus/LivePaper](https://github.com/zuranthus/LivePaper)
