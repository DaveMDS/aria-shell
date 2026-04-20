# Aria Shell

![License](https://img.shields.io/github/license/davemds/aria-shell)
![LOC-PY](https://img.shields.io/endpoint?label=LOC&color=blue&logo=python&url=https://ghloc.vercel.app/api/DaveMDS/aria-shell/badge?filter=.py)
![LOC-CSS](https://img.shields.io/endpoint?label=CSS&color=pink&url=https://ghloc.vercel.app/api/DaveMDS/aria-shell/badge?filter=.css)

A fast, modern and customizable desktop shell for your Wayland compositor.

AriaShell is a full-featured desktop shell designed to complement Wayland compositors
such as **Hyprland**, **Sway**, and others. It provides a panel, launcher, lock screen, 
notification daemon, wallpaper manager, terminal, and more — all configurable and themeable.

> [!WARNING]
> **The project is in active development.** Expect breaking changes and incomplete features.

---

## Components
 
### 🗂️ Aria Panel
A fully customizable panel with a rich set of built-in gadgets:
 
| Gadget       | Status | Description                                                                |
|--------------|---|---------------------------------------------------------------------------------|
| `clock`      | ✅ | Current time with a calendar popup                                              |
| `perf`       | ✅ | CPU, RAM, load and temperature monitor with top-like popup                      |
| `workspaces` | ✅ | Workspaces and windows overview (Hyprland & Sway)                               |
| `audio`      | ✅ | Volume control, multichannel mixer (WirePlumber) and MPRIS2 media controls      |
| `tray`       | ✅ | System tray via (K)StatusNotifierItem + DBusMenu                                |
| `theme`      | ✅ | GTK theme & icon theme selector with light/dark mode support                    |
| `custom`     | ✅ | User-defined gadgets with label, icon and command                               |
| `logout`     | ✅ | A Custom button to invoke Aria Exiter                                           |
| `power`      | 🔲 | Idle inhibitor, battery status, power profiles                                  |
| `network`    | 🔲 | Full featured network manager                                                   |
| `bluetooth`  | 🔲 | bluetooth manager                                                               |
| `screenshot` | 🔲 | Screenshot and screen recorder                                                  |
| `apps`       | 🔲 | fixed list of apps to run (like a dock)                                         |
| `home`       | 🔲 | a menu (cinnamon style) with app categories, search, favorites and sys controls |
| `file`       | 🔲 | file browser in a tree of menus?                                                |
| `places`     | 🔲 | menu with usefully locations, like home, favorites, devices                     |
| `brightness` | 🔲 | set monitor bright....how?                                                      |
 
Pango markup is supported in labels and tooltips. Full configuration via the `aria.conf` file.


---

### 🚀 Aria Launcher
An application launcher with support for `.desktop` files and multiple search providers.
 
- ✅ Search and run `.desktop` applications
- ✅ Multiple search provider architecture
- 🔲 App list auto-update on install/uninstall
- 🔲 Usage-based ranking
- 🔲 Secondary commands (e.g. "Firefox — New Private Window")


---

### 🔒 Aria Locker
A lock screen implementing the `ext-session-lock-v1` Wayland protocol.
 
- ✅ Date/time and user name/avatar display
- ✅ PAM-based password authentication
- 🔲 Background customization (same capabilities as Aria Wallpaper)


---

### 🚪 Aria Exiter
A session management dialog for locking, suspending, hibernating, logging out, rebooting and shutting down.

- ✅ Fully customizable actions and labels via config
- ✅ Auto-expiring confirmation dialogs for dangerous actions
- ✅ Custom buttons with icon, label and confirmation support


---

### 🖼️ Aria Wallpaper
A background manager built on the `LayerShell` Wayland protocol. Each monitor can 
have a completely independent wallpaper, and sources can be mixed freely across
displays — a static image on the laptop screen, an animated GIF on a second 
monitor, a live shader on the third.

#### Supported sources

**Static images** — standard raster formats (PNG, JPEG, WEBP).

**Animated GIFs** — frame-accurate GIF playback, looped continuously. Useful for subtle motion loops without the overhead of a video file.

**Video** — muted, looped playback of any GStreamer-supported container (MP4, WEBM, MKV…). Requires the `gstreamer` optional dependency.

**Shadertoy shaders** — GLSL fragment shaders sourced directly from [shadertoy.com](https://shadertoy.com). Save any shader code as a `.shadertoy` file and point the config at it. The shader is executed on the GPU every frame via OpenGL, giving you a fully animated, procedurally generated background with zero CPU cost. Requires the `PyOpenGL` optional dependency.

- ✅ Per-monitor backgrounds
- ✅ Fit modes: fill, contain, cover, etc.
- ✅ Static images
- ✅ Animated GIFs
- ✅ Video playback (muted loop)
- ✅ [Shadertoy](https://shadertoy.com) shader support (`.shadertoy` files)
- 🔲 texture based shader support
- 🔲 Cycle through files in folder
- 🔲 day-time-based wallpapers
- 🔲 auto-pause when on battery? or when full covered?_


---

### 🔔 Aria Notifier
A full-featured desktop notification server, replacing tools like `mako`.
 
- ✅ Icon and image data via DBus
- ✅ Markup support
- ✅ Action buttons inside notifications
- ✅ Urgency styling via CSS
- 🔲 Sound support, notification persistence
- 🔲 Limit the number of visible notification somehow


---

### 💻 Aria Terminal
A lightweight drop-down terminal (requires `vte4`).
 
- ✅ Show/hide on command (Quake-style)
- ✅ Configurable opacity, font, size and shell
- ✅ Optional display grab when visible
- ✅ Fullscreen emulation via `Ctrl+F`
- 🔲 show/hide animation ala quake console


---

### 💤 Aria Idler *(experimental)*
An idle daemon using the `ext_idle_notifier_v1` Wayland protocol.

>[!NOTE] I'm not sure if this thing should be an aria responsibility, seems 
>we are fighting with systemd abilities.

- ✅ Configurable idle/resume commands (Aria or external)
- ✅ Simple syntax in `aria.conf`
- 🔲 Per-scenario timeouts (AC vs battery)
- 🔲 manage events like on-lid-closed? How?
- 🔲 manage before-sleep and the like?


---

### ⌨️ Aria Commands
Control AriaShell programmatically via commands:
 
```
aria ping
aria reload
aria lock
aria launcher [toggle|show|hide]
aria terminal [toggle|show|hide]
aria exiter   [toggle|show|hide]
TODO: notify ....
TODO: osd ...
TODO: dmenu ...
```

---

## Dependencies

### System libraries
```
Gio >= 2.0 (GioUnix 2.0)
Gtk >= 4.14
Gtk4LayerShell
libwireplumber    # optional — audio gadget
gstreamer         # optional — video wallpaper and screensaver
vte4              # optional — embedded terminal
```

### Python packages
```
Python >= 3.14
PyGObject >= 3.50.0
pywayland >= 0.4.18
dasbus
psutil            # optional — perf gadget
PyOpenGL          # optional — shader wallpaper/screensaver
```

### Arch Linux
```bash
sudo pacman -S gtk4 gtk4-layer-shell vte4 \
               python-gobject python-pywayland python-dasbus \
               python-pam python-psutil \
               gst-plugin-gtk4 gst-plugins-base gst-plugins-good gst-libav \
               python-opengl
```

### Development extras
```bash
pip install pytest pygobject-stubs
```

---

## Installation

> Packaging for distributions is a work in progress... help needed!

To run AriaShell just clone the repo and run the main script from source:

```bash
# install dependencies (see above), then:
git clone https://github.com/davemds/aria-shell.git
cd aria-shell/aria_shell
./bin/aria-shell
```


---

## Configuration

AriaShell is configured through a single `aria.conf` file. Each component and 
gadget can be enabled, disabled and tuned independently. Refer to the example 
config included in the repository for a full reference.


---

## Credits & Inspiration
 
- [Fabric](https://github.com/Fabric-Development/fabric)
- [Ignis](https://github.com/linkfrg/ignis)
- [Waybar](https://github.com/Alexays/Waybar) — style inspiration
- Wallpaper shader art: [@1041uuu](https://x.com/1041uuu), [zuranthus/LivePaper](https://github.com/zuranthus/LivePaper)

