# The tray: a status notifier item registers with the shell's watcher
# and shows on both bars; clicks and the wheel reach it; its menu opens
# in a popup (separator, disabled row, checkmark, a submenu unfolding in
# place and resizing the popup); a row click sends the event and closes
# the popup; its layout changes are followed; unregistering removes it.

wait_for 'gadget.tray > button' 0 "no items before"
sni_start
settle 1
wait_for 'gadget.tray > button' 2 "one item per bar"
set -- $(widget 'panel[output="HEADLESS-1"] gadget.tray > button')
shot bar-item "$(($1 - 40)),0 120x32"

# The click methods get the pointer position.
click_widget 'panel[output="HEADLESS-1"] gadget.tray > button'
event=$(sni_event)
assert_contains "$event" "activate " "left click activates"
set -- $event
[ $(($2 - 1920 + 1920)) -ge 0 ] || exit 1
click_widget_with middle 'panel[output="HEADLESS-1"] gadget.tray > button'
assert_contains "$(sni_event)" "secondary " "middle click"
set -- $(widget 'panel[output="HEADLESS-2"] gadget.tray > button')
pointer $(($1 + $3 / 2)) $(($2 + $4 / 2))
scroll 2
# Two clicks down; positive is up (as KDE's host sends it), and the
# wheel's continuous twin event must not count again.
assert_eq "$(sni_event)" "scroll -2 vertical" "wheel on the second bar"
sni_quiet

# Right click: the menu, fetched after AboutToShow.
click_widget_with right 'panel[output="HEADLESS-2"] gadget.tray > button'
assert_eq "$(sni_event)" "about-to-show 0"
settle
assert_surface popup
wait_for 'menu > item' 4 "Open, Options, Disabled, Quit (Hidden dropped)"
wait_for 'menu > separator' 1
wait_for 'menu > item.submenu' 1
wait_for 'menu > submenu > item' 0 "submenu folded"
shot_surface menu popup
set -- $(surfaces | tr ';' '\n' | grep ' popup ' | sed 's/.* \([0-9]*\)x\([0-9]*\)$/\1 \2/')
folded_h=$2

# Unfold the submenu: AboutToShow for its node, the popup grows.
click_widget 'menu > item.submenu'
assert_eq "$(sni_event)" "about-to-show 3"
settle
wait_for 'menu > item.submenu.expanded' 1
wait_for 'menu > submenu > item.checked' 1 "Beep is checked"
set -- $(surfaces | tr ';' '\n' | grep ' popup ' | sed 's/.* \([0-9]*\)x\([0-9]*\)$/\1 \2/')
[ "$2" -gt "$folded_h" ] || { echo "popup didn't grow: $folded_h -> $2"; exit 1; }
shot_surface menu-unfolded popup

# A disabled row does nothing; a row click is an event and closes it.
click_widget 'menu > item:disabled'
sni_quiet
assert_surface popup
click_widget 'menu > submenu > item.checked'
assert_eq "$(sni_event)" "menu-event 4 clicked"
assert_no_surface popup

# The app changes a label while the menu is open: reloaded.
click_widget_with right 'panel[output="HEADLESS-1"] gadget.tray > button'
assert_eq "$(sni_event)" "about-to-show 0"
settle
assert_surface popup
sni_send "menu-label 7 Exit now"
assert_eq "$(sni_event)" "about-to-show 0" "reload after LayoutUpdated"
settle
set -- $(surfaces | tr ';' '\n' | grep ' popup ' | sed 's/.* \([0-9]*\)x\([0-9]*\)$/\1 \2/')
[ "$1" -gt 0 ] || exit 1
click 600 600
assert_no_surface popup

# Property changes are followed.
sni_send "icon 40 200 40"
shot bar-item-green "$(($(widget 'panel[output="HEADLESS-1"] gadget.tray > button' | cut -d' ' -f1) - 40)),0 120x32"
sni_send "status NeedsAttention"
wait_for 'gadget.tray > button.attention' 2 "attention on both bars"

# Gone from the bus: gone from the bars.
sni_stop
settle 1
wait_for 'gadget.tray > button' 0 "removed"
