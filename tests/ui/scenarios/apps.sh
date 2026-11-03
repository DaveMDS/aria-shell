# The Apps gadget, a dock: on a bottom bar per output an instance
# pinning aria-dock; on a top bar on the second output another, pinning
# aria-other. A pinned app launches with a click; windows group under
# their app (by StartupWMClass, by [apps_class_map], else their own
# class) after a separator; a click focuses an app's window, or shows
# its windows when more; the pointer resting on it shows its name; the
# menu lists them, runs an action, pins an app on every bar and unpins
# it, closes its windows; the middle button launches again.

conf=$ARIA_UI_OUT/conf
data=$ARIA_UI_OUT/data
mkdir -p "$conf/aria-shell" "$data/applications"
launch=$ARIA_UI_OUT/launch.sh
printf '#!/bin/sh\necho "$*" >> "%s/launches"\n' "$ARIA_UI_OUT" > "$launch"
chmod +x "$launch"
cat > "$data/applications/aria-dock.desktop" << EOF
[Desktop Entry]
Type=Application
Name=Aria Dock
Exec=$launch dock
Icon=application-x-executable
StartupWMClass=aria-one
Actions=private;

[Desktop Action private]
Name=Private Window
Exec=$launch dock-private
EOF
cat > "$data/applications/aria-other.desktop" << EOF
[Desktop Entry]
Type=Application
Name=Aria Other
Exec=$launch other
Icon=application-x-executable
StartupWMClass=aria-two
EOF
cat > "$conf/aria-shell/aria.conf" << EOF
[general]
language = en
theme =
reload_theme = no
reload_config = no

[apps_class_map]
aria-one-b = aria-dock

[panel]
outputs = all
position = bottom
items_center = Apps

[panel:top]
outputs = HEADLESS-2
position = top
items_center = Apps:top

[Apps]
apps = aria-dock

[Apps:top]
apps = aria-other.desktop
EOF
export XDG_DATA_HOME=$data
restart_shell "$conf"

b1='surface.panel.bottom[output="HEADLESS-1"] gadget.apps'
top='surface.panel.top gadget.apps'
app() { echo "indicator.app[app=\"$1\"]"; }
# The last line launch.sh wrote, once there's one more than `$1`.
launched() {
    i=0
    while [ "$(wc -l < "$ARIA_UI_OUT/launches" 2> /dev/null || echo 0)" -le "$1" ] && [ $i -lt 40 ]; do
        sleep 0.05; i=$((i + 1))
    done
    tail -n 1 "$ARIA_UI_OUT/launches" 2> /dev/null
}
shot_bar() {
    set -- "$1" $(widget "$2")
    shot "$1" "$(($2 - 8)),$(($3 - 4)) $(($4 + 16))x$(($5 + 8))"
}

# Pinned, not running: each instance its own, no separator.
wait_for "$b1 indicator.app" 1 "one app on the bottom bar"
wait_for "$b1 $(app aria-dock).pinned" 1 "aria-dock pinned"
wait_for "$b1 indicator.app.running" 0 "not running"
wait_for "$b1 separator" 0 "no separator without running apps"
wait_for "$top $(app aria-other).pinned" 1 "aria-other pinned on the top bar"
wait_for "$top indicator.app" 1 "only it"
shot_bar bottom-pinned "$b1"

# A click launches it.
click_widget "$b1 $(app aria-dock)"
assert_eq "$(launched 0)" dock "a click launched aria-dock"

# Its window (StartupWMClass): running where pinned, after a separator
# where it isn't.
open_window aria-one "Dock one"
wait_for "$b1 $(app aria-dock).running.active" 1 "aria-dock running, focused"
wait_for "$b1 $(app aria-dock) windows > dot" 1 "a dot for its window"
wait_for "$b1 indicator.app" 1 "still one app"
wait_for "$top indicator.app" 2 "aria-dock on the top bar too"
wait_for "$top separator" 1 "after a separator"
wait_for "$top $(app aria-dock).pinned" 0 "not pinned there"

# A window with no desktop entry is an app of its own; one mapped by
# [apps_class_map] joins aria-dock.
open_window aria-three "Three"
wait_for "$b1 $(app aria-three).running" 1 "a window with no entry, by its class"
wait_for "$b1 separator" 1 "after a separator"
open_window aria-one-b "Dock two"
wait_for "$b1 $(app aria-dock) windows > dot" 2 "two windows for aria-dock"
wait_for "$b1 $(app aria-dock) dot.active:nth-child(2)" 1 "the second focused"
shot_bar bottom-running "$b1"

# A click on an app with one window focuses it; on one with more shows
# them, its name over them, to pick one.
click_widget "$b1 $(app aria-three)"
wait_for "$b1 $(app aria-three).active" 1 "aria-three focused"
wait_for "$b1 $(app aria-dock).active" 0 "aria-dock not"
assert_no_surface popup
click_widget "$b1 $(app aria-dock)"
assert_surface popup
wait_for 'surface.popup menu > header' 1 "its name"
wait_for 'surface.popup menu > item' 2 "its windows, nothing else"
wait_for 'surface.popup menu > separator' 0
shot_surface windows popup
click_widget 'surface.popup menu > item:nth-child(2)'
assert_no_surface popup
wait_for "$b1 $(app aria-dock) dot.active:nth-child(1)" 1 "the window picked focused"
# Clicked again it closes; the right button on it: the whole menu in
# its place, and back.
click_widget "$b1 $(app aria-dock)"
assert_surface popup
click_widget "$b1 $(app aria-dock)"
assert_no_surface popup
click_widget "$b1 $(app aria-dock)"
assert_surface popup
click_widget_with right "$b1 $(app aria-dock)"
wait_for 'surface.popup menu > item' 5 "the whole menu"
click_widget "$b1 $(app aria-dock)"
wait_for 'surface.popup menu > item' 2 "its windows again"
click_widget "$b1 $(app aria-dock)"
assert_no_surface popup

# The pointer resting on an icon: the app's name in a tip off the bar,
# centred on its icon, its margin over the bar; on to the next icon,
# that one's; away, gone.
pointer 960 400
assert_no_surface tooltip
set -- $(widget "$b1 $(app aria-dock)")
icon_x=$(($1 + $3 / 2))
pointer $icon_x $(($2 + $4 / 2))
assert_surface tooltip
set -- $(widget 'surface.tooltip')
[ $(($1 + $3 / 2 - icon_x)) -le 1 ] && [ $((icon_x - $1 - $3 / 2)) -le 1 ] || { echo "the tip not centred on the icon ($icon_x): $*"; exit 1; }
set -- "$@" $(widget "$b1")
assert_eq "$(($6 - $2 - $4))" 6 "the tip its margin over the bar"
set -- $(widget 'surface.tooltip > label')
shot tip "$(($1 - 30)),$(($2 - 20)) $(($3 + 60))x$(($4 + 60))"
set -- $(widget "$b1 $(app aria-three)")
pointer $(($1 + $3 / 2)) $(($2 + $4 / 2))
settle 1
assert_surface tooltip
assert_eq "$(surfaces | tr ';' '\n' | grep -c ' tooltip ')" 1 "one tip at a time"
pointer 960 400
assert_no_surface tooltip

# The menu: its windows, its action, a new window, close (pinned by the
# config: no unpin, no pin).
click_widget_with right "$b1 $(app aria-dock)"
assert_surface popup
wait_for 'surface.popup menu > item' 5 "two windows, an action, new window, close all"
wait_for 'surface.popup menu > separator' 3
wait_for 'surface.popup menu > header' 1 "the app's name over the menu"
shot_surface menu popup
# A long title: one line, cut where the menu's max-width ends.
click_widget_with right "$b1 $(app aria-dock)"
assert_no_surface popup
retitle_window aria-one "$(printf 'A very long window title %.0s' 1 2 3 4 5 6 7 8)"
click_widget_with right "$b1 $(app aria-dock)"
assert_surface popup
set -- $(widget 'surface.popup menu')
[ "$3" -le 360 ] || { echo "the menu wider than its max-width: $3"; exit 1; }
set -- $(widget 'surface.popup menu > item:nth-child(2)')
h1=$4
set -- $(widget 'surface.popup menu > item:nth-child(3)')
assert_eq "$h1" "$4" "a long title on one line"
shot_surface menu-long popup
# A window: focused.
click_widget 'surface.popup menu > item:nth-child(2)'
assert_no_surface popup
wait_for "$b1 $(app aria-dock) dot.active:nth-child(1)" 1 "the menu's window focused"
# The action.
n=$(wc -l < "$ARIA_UI_OUT/launches")
click_widget_with right "$b1 $(app aria-dock)"
assert_surface popup
click_widget 'surface.popup menu > item:nth-child(5)'
assert_eq "$(launched "$n")" dock-private "the desktop action ran"
assert_no_surface popup

# Pinned from the top bar: on every bar, in the file; the bottom one,
# pinned there by the config too, can unpin it from the file only.
click_widget_with right "$top $(app aria-dock)"
assert_surface popup
wait_for 'surface.popup menu > item' 6 "two windows, an action, new window, pin, close all"
click_widget 'surface.popup menu > item:nth-child(9)'
assert_no_surface popup
wait_for "$top $(app aria-dock).pinned" 1 "pinned on the top bar"
wait_for "$top separator" 1 "aria-three still after the separator"
assert_eq "$(cat "$XDG_STATE_HOME/aria-shell/pinned-apps")" aria-dock "kept in the state file"
shot_bar top-pinned "$top"
click_widget_with right "$top $(app aria-dock)"
assert_surface popup
click_widget 'surface.popup menu > item:nth-child(9)'
assert_no_surface popup
wait_for "$top $(app aria-dock).pinned" 0 "unpinned"
assert_eq "$(cat "$XDG_STATE_HOME/aria-shell/pinned-apps")" "" "gone from the state file"

# Close all: both windows go, it stays pinned on the bottom bar only.
click_widget_with right "$b1 $(app aria-dock)"
assert_surface popup
click_widget 'surface.popup menu > item:nth-child(9)'
assert_no_surface popup
settle 0.3
sync_windows
wait_for "$b1 $(app aria-dock).running" 0 "its windows closed"
wait_for "$b1 $(app aria-dock)" 1 "still pinned"
wait_for "$top $(app aria-dock)" 0 "gone from the top bar"
wait_for "$b1 $(app aria-three)" 1 "the other app left alone"

# The middle button launches again.
n=$(wc -l < "$ARIA_UI_OUT/launches")
click_widget_with middle "$top $(app aria-other)"
assert_eq "$(launched "$n")" other "the middle button launched aria-other"

close_window aria-three
wait_for "$b1 separator" 0 "no running app, no separator"
