# Aria desktop shell for Wayland
![](https://img.shields.io/endpoint?label=LOC&color=blue&logo=python&url=https://ghloc.vercel.app/api/DaveMDS/aria-shell/badge?filter=.py)
![](https://img.shields.io/endpoint?label=CSS&color=pink&url=https://ghloc.vercel.app/api/DaveMDS/aria-shell/badge?filter=.css)

> [!WARNING]
> 
> DEVELOPMENT STAGE
> 
> PLEASE COME BACK IN A FEW WEEKS



lots to write here... help wanted ;)


## Dependencies

### System dependencies
```
Gio 2.0 (GioUnix 2.0)
Gtk >= 4.14
Gtk4LayerShell
vte4 (optional, for the embedded terminal)
libwireplumber (optional, for the audio gadget)
```

### Python dependencies
```
Python >= 3.14
PyGObject >= 3.50.0
pywayland >= 0.4.18
dasbus
psutil (optional, for the perf gadget)
```

### Arch packages
```
gtk4 gtk4-layer-shell vte4
python-gobject python-pywayland python-dasbus python-pam python-psutil
```

### Develop only utilities:
```
pip install pytest pygobject-stubs
```



## Aria panel
 * [x] fully customizable from config file
 * [x] pango markup in labels and tooltips
 * [ ] panel autohide
 * [ ] action/key to show/hide the panels
 * gadgets:
   * [x] clock: show current time and a simple calendar on click
   * [x] perf: cpu, ram, load, temps  (with top-like in popup)
   * [x] workspaces: show workspaces and windows (hyprland and sway supported)
   * [x] custom: custom gadget built in config (label+icon->command/exec on click)
   * [x] audio: main volume control, simple mixer and multimedia controls
     * [x] Use WirePlumber gAPI to provide multichannel mixer controls
     * [x] MPRIS2 for media player controls
     * [ ] show default volume in gadget, mouse-wheel to adjust (optional)
     * [ ] show default mic volume in gadget (optional)
     * [ ] MPD support
   * [x] tray: use (K)StatusNotifierItem protocol
     *  [x] decent DBUS Menu support (com.canonical.dbusmenu)
     *  [ ] support for pixmap icons over dbus
   * [ ] screenshot / screenrecorder
   * [x] theme selector:
     * [x] show a menu with the list of preferred, user and system themes (fully configurable)
     * [x] support "special" light/dark themes
     * [x] list and apply icon-theme (manually or from the desktop theme)
     * [ ] list and apply cursor-theme (manually or from the desktop theme)
     * [ ] copy the theme gtk-4.0 folder in ~/.config/gtk4.0 ?? seems totally wrong...
     * [ ] group themes with same name prefix under a submenu
     * [ ] show the active theme in the menu, check box?
   * [ ] power: idle inhibitor, battery/ac status, power profiles
   * [ ] help: just an icon, on click show a dialog with keybindings and basic info
          (also show on META+F1)
   * [ ] network: super simple NetworkManager, on/off ifaces, ssid list/connect
   * [ ] bluetooth: ???
   * [x] logout: just a button to run Aria Exiter
   * [ ] apps: fixed list of apps to run (like a dock)
   * [ ] home: open a menu (cinnamon style) with app categories and search, AriaLauncher?
   * [ ] fileman: filemanager in a tree of menus?
   * [ ] places: menu with usefully locations, like home, favorites, devices
   * [ ] brightness: set monitor bright....how?


## Aria launcher
- [x] search and run .desktop files
- [x] support multiple search providers (implemented only .desktop app)
- [ ] keep apps list updated when install/uninstall apps
- [ ] remember most used and rank first
- [ ] support secondary commands (fe: Firefox new private window)
- [ ] other search providers? es: files, websearch, ??


## Aria locker
- [x] a lock screen implementing the ext-session-lock-v1 protocol
- [x] show date/time and user name/avatar
- [x] password authentication using PAM
- [ ] background customizations (with same abilities as Aria Wallpaper)
- [ ] buttons to reboot/halt?
- [ ] other info to show?


## Aria exiter
- [x] customizable dialog menu to lock, suspend, hibernate, logout, reboot and shutdown
- [x] all commands can be customized in config file
- [x] confirm dangerous actions with an auto-expiring dialog (configurable)
- [x] custom buttons can be created in config. With label, icon and confirm option.
- [ ] automatic logout command. How to make logout work on every wm?


## Aria idler (EXPERIMENTAL)
NOTE: I'm not sure if this should be an aria responsibility, seems we are fighting with systemd abilities.
- [x] an "idler daemon" implementation, use the ext_idle_notifier_v1 wayland protocol.
- [x] support for arbitrary idled / resumed commands (both aria commands or external commands)
- [x] simple configuration in the aria.conf file with a simple syntax
- [ ] Different timeouts for different scenarios! (AC power, on battery, etc...)
- [ ] manage events like on-lid-closed? How?
- [ ] manage before-sleep and the like?


## Aria wallpaper
- [x] draw the background using the LayerShell protocol
- [x] different background per specific monitor
- [x] ability to change picture fit (fill, contain, cover, etc...)
- [ ] ability to rotate from files in a given folder
- [x] static images
- [x] animated GIF images
- [x] video playback (muted and looped)
- [ ] shadertoys.com shaders from file  (COMING SOON) :D 
- [ ] day-time based wallpapers (formats?)
- [ ] auto-pause when on battery? or when full covered?


## AriaNotifier
- [x] Full-featured notification server, replace mako and friends
- [x] Support icons and images data from DBUS
- [x] Markup support
- [x] Actions support (buttons inside notification)
- [x] Support urgency in CSS
- [ ] Sound support
- [ ] Limit the number of visible notification somehow
- [ ] Persistent notifications. where to show? in clock?


## Aria terminal
- [x] simple persistent terminal that show/hide from the top on command
- [x] configurable opacity, font, size, shell and behaviours
- [x] only available if vte4 is installed
- [x] optionally grab the display when the terminal is visible
- [x] emulate fullscreen using Ctrl+F
- [ ] configurable color palette
- [ ] show/hide animation ala quake console
- [ ] find more cool fonts for the default config (ship one in pkg?)


## Aria commands
- [x] ping
- [x] reload
- [x] lock
- [x] launcher [toggle|show|hide]
- [x] terminal [toggle|show|hide]
- [x] exiter [toggle|show|hide]
- [ ] notify ....
- [ ] osd ...
- [ ] dmenu ...



## REFERENCES
https://github.com/davidmalcolm/pygobject
https://lazka.github.io/pgi-docs
https://api.pygobject.gnome.org
https://docs.gtk.org/gtk4/visual_index.html
https://docs.gtk.org/gtk4/css-properties.html


# CREDITS
- https://github.com/Fabric-Development/fabric
- https://github.com/linkfrg/ignis  usa socket.socket (blocca sui comandi) e un thread per gli eventi
- waybar for the style
