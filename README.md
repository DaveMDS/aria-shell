# Aria desktop shell for Wayland
![Endpoint Badge](https://img.shields.io/endpoint?url=https%3A%2F%2Floc-counter.onrender.com%2F%3Frepo%3DDaveMDS%2Faria-shell%26branch%3Dmain%26ignored%3Dtests%26languages%3DPython&logo=python&label=Lines%20of%20code&color=75a8d3)
![Endpoint Badge](https://img.shields.io/endpoint?url=https%3A%2F%2Floc-counter.onrender.com%2F%3Frepo%3DDaveMDS%2Faria-shell%26branch%3Dmain%26ignored%3Dtests%26languages%3DCSS&label=CSS&color=ff3c78)


> [!WARNING]
> 
> EARLY DEVELOPMENT STAGE
> 
> NOTHING TO SEE HERE ATM
> 
> PLEASE COME BACK IN A FEW MONTHS



lots to write here... help wanted ;)


## Dependencies

### System dependency
```
Gtk4
Gtk4LayerShell
vte4 (optional, for the embedded terminal)
libwireplumber (optional, for the audio gadget)
```

### python dependency
```
PyGObject >= 3.50.0
dasbus
psutil (optional, for the perf gadget)
```


## Aria panel
 * [x] fully customizable from config file
 * [x] pango markup in labels and tooltips
 * [ ] panel autohide
 * gadgets:
   * [x] clock: show current time and a simple calendar on click
   * [x] perf: cpu, ram, load, temps  (with top-like in popup)
   * [x] workspaces: show workspaces and windows (hyprland only, sway coming soon)
   * [x] button: custom gadget built in config (label+icon->command/exec on click)
   * [x] audio: main volume control, simple mixer and multimedia controls
     * [x] Use WirePlumber gAPI to provide multichannel mixer controls
     * [x] MPRIS2 for media player controls
     * [ ] show default volume in gadget, mouse-wheel to adjust (optional)
     * [ ] show default mic volume in gadget (optional)
   * [ ] tray + app menu (like mac menu) see ApplicationWindow
   * [ ] screenshot / screenrecorder
   * [ ] dark/light switch (update aria and gtk/qt config)
   * [ ] power: idle inhibitor, battery/ac status, power profiles
   * [ ] help: just an icon, on click show a dialog with keybindings and basic info
          (also show on META+F1)
   * [ ] network: super simple NetworkManager, on/off ifaces, ssid list/connect
   * [ ] bluetooth: ???
   * [ ] logout: just a button to run AriaLogout
   * [ ] apps: fixed list of apps to run (like a dock)
   * [ ] home: open a menu (cinnamom style) with app categories and search, AriaLauncher?
   * [ ] fileman: filemanager in a tree of menus?
   * [ ] places: menu with usefull location, like home, favorites, devices
   * [ ] brightness: set monitor bright....how?


## Aria launcher
- [x] search and run .desktop files
- [x] support multiple search providers (implemented only .desktop app)
- [ ] keep apps list updated when install/uninstall apps
- [ ] remember most used and rank first
- [ ] support secondary commands (fe: Firefox new private window)
- [ ] other search providers? es: files, websearch, ??


## Aria terminal
- [x] simple persistant terminal that show/hide from the top on command
- [x] configurable opacity, font, size, shell and behaviours
- [x] only available if vte4 is installed
- [x] optionally grab the disaply when the terminal is visible
- [x] emulate fullscreen using Ctrl+F
- [ ] configurable color palette
- [ ] show/hide animation ala quake console
- [ ] find more cool fonts for the default config (ship one in pkg?)


## Aria logout
- see wlogout


## Aria lockidle
- see swaylock
- see hypridle


## AriaNotify
- see mako/swaync


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
