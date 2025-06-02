Aria desktop shell for Wayland
==============================

> [!WARNING]
> 
> EARLY DEVELOPMENT STAGE
> 
> NOTHING TO SEE HERE ATM
> 
> PLEASE COME BACK IN A FEW MONTHS



lots to write here... help wanted ;)

TODO
----
* AriaPopup need some love
* Launcher: spawned app terminate with the shell !!

Keybindings????
How to receive commands? like:
 - show-launcher
 - show-locker
 - show-logout
 - restart
 - reload-config
 
Aria panel
----------
 * panel autohide
 * pango markup in labels and tooltips? optional?
 * modules
   * perf: cpu, ram, load, temps  (with top-like in popup)
   * tray + app menu (like mac menu) see ApplicationWindow
   * dark/light switch (update aria and gtk/qt config)
   * audio: volume, simple mixer (pulse) and multimedia controls (mpris?)
   * power: idle inhibitor, battery/ac status, power profiles
   * help: just an icon, on click show a dialog with keybindings and basic info
           (also show on META+F1)
   * network: super simple NetworkManager, on/off ifaces, ssid list/connect
   * bluetooth: ???
   * logout: just a button to run AriaLogout
   * apps: fixed list of apps to run (like a dock)
   * home: open a menu (cinnamom style) with app categories and search, AriaLauncher?
   * fileman: filemanager in a tree of menus?
   * places: menu with usefull location, like home, favorites, devices
   * brightness: set monitor bright....how?

Aria launcher
-------------
- [x] search and run .desktop files
- [ ] more fuzzy match, with score level. And remember most used to rank first
- [ ] support secondary commands (fe: Firefox new private window)
- [ ] backandable providers support, es: files, websearch, ??

Aria logout
-----------
- see wlogout

Aria lockidle
-------------
- see swaylock
- see hypridle

AriaNotify
----------
- see mako/swaync


CREDITS
=======
- https://github.com/Fabric-Development/fabric
- https://github.com/linkfrg/ignis  usa socket.socket (blocca sui comandi) e un thread per gli eventi
- waybar for the style