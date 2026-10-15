# A bar with `autohide`: hidden at its edge, shown once the pointer has
# rested there a moment (not when it only passes), hidden again a moment
# after it left; never while one of its popups is open. `smart` shows
# it on a desktop without windows too.

conf=$(mktemp -d)
trap 'rm -rf "$conf"' EXIT
cp -r "$XDG_CONFIG_HOME/." "$conf"
sed -i 's/^\[panel\]$/[panel]\nautohide = yes/' "$conf/aria-shell/aria.conf"
restart_shell "$conf"

bar_hidden() {
    surfaces | tr ';' '\n' | grep "^ *panel HEADLESS-1 " | grep -q " hidden"
}

assert_hidden() {
    bar_hidden || { echo "$1"; return 1; }
}

assert_shown() {
    if bar_hidden; then echo "$1"; return 1; fi
}

assert_hidden "the bar starts shown"
shot hidden "0,0 1920x60"

# Passing over the edge doesn't show it.
pointer 960 400
pointer 960 0
pointer 960 400
settle 0.5
assert_hidden "shown by the pointer passing"

# Resting there does.
pointer 960 0
settle 0.1
assert_hidden "shown before the delay"
settle 0.3
assert_shown "not shown after resting at the edge"
shot shown "0,0 1920x60"

# Leaving: it stays a moment, then goes.
pointer 960 500
settle 0.3
assert_shown "hidden at once"
settle 0.6
assert_hidden "still shown after the pointer left"

# With a popup open it stays, and goes once the popup closes.
pointer 960 0
settle 0.5
click_widget 'surface.panel[output="HEADLESS-1"] slot.center gadget.clock > indicator'
assert_surface popup
pointer 300 900
settle 1
assert_shown "hidden with its popup open"
click 300 900
assert_no_surface popup
settle 1
assert_hidden "still shown after its popup closed"

# `smart`: shown on a desktop without windows, hidden while one is open.
sed -i 's/^autohide = yes$/autohide = smart/' "$conf/aria-shell/aria.conf"
restart_shell "$conf"
pointer 300 900
settle 0.5
assert_shown "smart: hidden on an empty desktop"
open_window aria-one
settle 1
assert_hidden "smart: still shown with a window open"
close_window aria-one
settle 0.5
assert_shown "smart: hidden once the window closed"
