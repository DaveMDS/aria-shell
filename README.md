# Aria Shell

![License](https://img.shields.io/github/license/davemds/aria-shell)
![LOC-RS](https://img.shields.io/endpoint?label=LOC&color=orange&logo=rust&url=https://ghloc.vercel.app/api/DaveMDS/aria-shell/badge?filter=.rs)
![LOC-CSS](https://img.shields.io/endpoint?label=CSS&color=pink&url=https://ghloc.vercel.app/api/DaveMDS/aria-shell/badge?filter=.css)

A fast, modern and customizable desktop shell for your Wayland compositor.

AriaShell is a full-featured desktop shell designed to complement Wayland compositors
such as **Hyprland**, **Sway**, and others. It provides a panel, launcher, lock screen,
exit menu, notification daemon, OSD, wallpaper manager, and more — all configurable and
themeable through a CSS-like stylesheet.

> [!WARNING]
> **The project is in active development.** Expect breaking changes and incomplete features.
>
> AriaShell is being rewritten in **Rust** on [iced](https://iced.rs) (wgpu) and
> [iced_exwlshell](https://github.com/waycrate/exwlshelleventloop); the previous
> Python/GTK4 implementation is in the git history (`git log -- aria-shell-python`).
> The checklists below are the port's status.

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
| `Power`         | ✅ | Battery (charge, time left, health), peripherals' charge, power profiles, idle inhibitor; low battery notifications |
| `Brightness`    | ✅ | Every screen's brightness: the laptop's panel (backlight, through logind) and monitors over DDC/CI (`ddcutil`); wheel, a slider per screen |
| `Network`       | ✅ | NetworkManager: Wi‑Fi networks, wired devices, VPN toggles, Wi‑Fi on/off        |
| `Network`       | 🔲 | Secret agent (VPN / 802.1X passwords asked in the popup), hidden networks, hotspot, mobile broadband, per-BSSID choice, editing profiles, iwd |
| `Clock`         | 🔲 | Tooltip (`tooltip_format`)                                                      |
| `SystemMonitor` | 🔲 | Per-process graphs and command lines, tree view, filtering, battery, more sensors, Intel GPU |
| `Tray`          | 🔲 | Tooltips, overlay icons, menu icons and shortcuts, the `org.freedesktop` SNI name |
| `bluetooth`     | 🔲 | bluetooth manager                                                               |
| `screenshot`    | 🔲 | Screenshot and screen recorder                                                  |
| `apps`          | 🔲 | fixed list of apps to run (like a dock)                                         |
| `home`          | 🔲 | a menu (cinnamon style) with app categories, search, favorites and sys controls |
| `file`          | 🔲 | file browser in a tree of menus?                                                |
| `places`        | 🔲 | menu with usefully locations, like home, favorites, devices                     |
| `Brightness`    | 🔲 | Night light (colour temperature), a monitor's own buttons seen without reopening the popup |

- ✅ Multi-monitor, hot-plug aware
- ✅ Config hot-reload (`aria.conf`) and theme hot-reload
- ✅ Autostart: programs run once when the shell starts (`[autostart]`)
- ✅ One shell per display: a second `aria-shell` refuses to start
- ✅ `make install`, a systemd user service, the programs it starts in systemd scopes of their own
- 🔲 `[panel]` `size`, `align`, `margin`, `opacity`

Full configuration via the `aria.conf` file.


---

### 🚀 Aria Launcher
An application launcher with support for `.desktop` files.

- ✅ Search and run `.desktop` applications (names and descriptions in your language)
- ✅ App list auto-update on install/uninstall
- ✅ The exit menu's actions as a row of buttons above the search field
- ✅ Usage-based ranking (what you launch most comes first; counts in `~/.local/state/aria-shell/launcher-usage`)
- ✅ Secondary commands: an entry's desktop actions (e.g. Firefox's "New Private Window") open as child rows with → or the chevron, ← closes them
- 🔲 `DBusActivatable` entries


---

### 🔒 Aria Locker
A lock screen implementing the `ext-session-lock-v1` Wayland protocol.

- ✅ Date/time and user name/avatar display
- ✅ PAM-based password authentication (with PAM's own messages, e.g. a locked account)
- ✅ Show/hide the password
- 🔲 Background customization (same capabilities as Aria Wallpaper, or the desktop blurred)
- 🔲 A spinner while checking, a shake on failure, a Caps Lock warning
- 🔲 A second PAM prompt (e.g. a one-time code)


---

### 🚪 Aria Exiter
A session management dialog for locking, suspending, hibernating, logging out, rebooting and shutting down.

- ✅ Fully customizable actions and labels via config
- ✅ Auto-expiring confirmation dialogs for dangerous actions
- ✅ Custom buttons with icon, label and confirmation support
- ✅ `logout = auto` asks the compositor itself (Hyprland, Sway)
- ✅ Keyboard navigation
- 🔲 Per-button hotkeys


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
- 🔲 Animated GIFs, videos
- 🔲 `tile` fit mode
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
- 🔲 Sound support, persisting the history and do-not-disturb
- 🔲 `resident` / `transient` and `x` / `y` hints, animations
- 🔲 Limit the number of visible notification somehow


---

### 🔆 Aria OSD
A short-lived bar on every screen when something changes, whatever changed it
(a keybind, a gadget, another app). It only shows,
it never changes anything (`[osd]`, the theme's `osd`).

- ✅ Volume and mute of the default output, the device changing (headphones plugged in)
- ✅ Microphone level and mute (the default input)
- ✅ The microphone in use by an app, and free again
- ✅ Wi‑Fi on / off, network connected / disconnected, a VPN up / down
- ✅ Charger plugged in / out (with the charge), power profile, keep awake on / off
- ✅ A screen's brightness, on that screen (`aria-shell brightness`, the gadget, the laptop's keys)
- ✅ `aria-shell osd show [--icon <name>] [--value <percent>] [text]` from a script
- ✅ Position (top, center, bottom) and duration from the config, size and look from the theme


---

### 💻 Aria Terminal *(not ported yet)*
A lightweight drop-down terminal.

- 🔲 Show/hide on command (Quake-style)
- 🔲 Configurable opacity, font, size and shell
- 🔲 Optional display grab when visible
- 🔲 Fullscreen emulation via `Ctrl+F`
- 🔲 show/hide animation ala quake console


---

### 💤 Aria Idler
What happens when nobody touches the machine (`[Idle]`, `[Idle:battery]`): each
stage has its own timeout from the last input, on AC and on battery.

- ✅ Screens off (back on at the first input), lock, suspend (through logind)
- ✅ Separate timeouts on battery (UPower)
- ✅ Lock before any sleep (lid, `systemctl suspend`) and on `loginctl lock-session`
- ✅ Held by the `Power` gadget's eye or `aria-shell idle inhibit`, by a media player playing,
  and by apps inhibiting idle (a fullscreen video)
- 🔲 A screensaver stage (which would also dim the screens)


---

### 🎨 Theming
The look comes from a CSS-like stylesheet: `assets/base.css` documents the
element tree and the supported properties, a user theme (`[general] style`)
is loaded on top, with light and dark colour schemes. Themes are found in
`~/.config/aria-shell/themes/`, the XDG data dirs and `assets/themes/`.

- ✅ Selectors (type, class, id, attributes, `:hover`/`:active`/`:focus`, `:nth-child`), variables, cascade
- ✅ Light/dark schemes, switched at runtime by the `Themes` gadget
- ✅ Hot reload while editing (a broken file keeps the running theme)
- 🔲 `margin`, `opacity`, gradients, `@import`, `!important`, `@font-face`, transitions
- 🔲 `:hover` on more than buttons, a themed scrollbar
- 🔲 Remember the theme picked at runtime, follow and set the desktop's colour scheme
- 🔲 Shader backgrounds for widgets (`background: shader("x.wgsl")`)
- 🔲 Smooth transparency on the surfaces (shadows, rounded corners, `rgba`
  backgrounds): only 4 levels for now, waiting for an iced release that fixes it


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
aria-shell idle     inhibit [toggle|on|off]
aria-shell osd      show [--icon <name>] [--value <percent>] [text]
aria-shell brightness up [percent]|down [percent]|set <percent> [--output <connector>]
aria-shell volume   up [percent]|down [percent]|set <percent>|mute [toggle|on|off] [--input]
aria-shell debug    surfaces|widgets [selector]|cursor|theme|locale|sysmon|audio|network|idle|power|brightness
TODO: reload
TODO: terminal [toggle|show|hide]
TODO: notify ....
TODO: dmenu ...
```


---

## Dependencies

### System libraries
```
libpam            # lock screen authentication
libpulse          # audio gadget (PipeWire's pipewire-pulse or PulseAudio at runtime)
NetworkManager    # network gadget (over the system bus, at runtime)
systemd-logind    # idle: suspend, lock before sleep; the laptop's brightness (at runtime)
ddcutil           # the Brightness gadget's external monitors (at runtime, optional; needs the
                  # i2c-dev module loaded and access to /dev/i2c-*, as ddcutil's own docs say)
UPower            # the Power gadget, idle's timeouts on battery (at runtime, optional)
power-profiles-daemon or tuned-ppd   # the Power gadget's profiles (at runtime, optional)
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

Build it and install it (dependencies above):

```bash
git clone https://github.com/davemds/aria-shell.git
cd aria-shell
make                              # cargo build --release
sudo make install                 # in /usr/local
# or, for your user only and without root:
make install PREFIX=$HOME/.local
```

That installs the binary, the themes, a systemd user unit, the PAM
service (`/etc/pam.d/aria-shell`, only when installing as root: the
`login` service is used otherwise) and, in `share/doc/aria-shell/`, the
sample `aria.conf` and the compositor examples. `make uninstall` with
the same `PREFIX` removes them. Packagers: `make DESTDIR=... PREFIX=/usr install`.

Run from a checkout (`cargo run`) the shell finds its sample config and
themes in `assets/`.

### Starting it

Once per session, one of:

- **As a systemd user service**, with a session that starts
  `graphical-session.target` (UWSM, sway-systemd, ...):
  `systemctl --user enable aria-shell.service`. The journal has its log
  (`journalctl --user -u aria-shell`).
- **From the compositor**: `exec-once = aria-shell` on Hyprland,
  `exec aria-shell` on Sway.

One shell runs per display: a second `aria-shell` started without
arguments says so and leaves. Every program the shell starts (apps from
the launcher, `[autostart]`, gadget commands) gets a systemd scope of
its own when there's a systemd user manager, so restarting the shell
doesn't take them down with it.

### Key bindings

Keys are the compositor's: bind them to the shell's commands
(`aria-shell launcher toggle`, `aria-shell exiter toggle`,
`aria-shell lock`, ...). Ready-made examples, with the start of the
shell, volume and brightness keys, are in
[`assets/compositors/`](assets/compositors/): `hyprland.lua`
(Hyprland 0.56 and later), `hyprland.conf` (earlier Hyprland),
`sway.conf`.


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
