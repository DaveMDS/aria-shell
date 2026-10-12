# The showcase: `aria showcase show` draws every part of the UI (the
# popups, the launcher, the exit menu, the OSD, two notifications, the
# bar's buttons) on one surface between the bars of the focused output,
# inert; it follows the scheme; Escape, a click beside the parts or the
# command again close it.

assert_no_surface showcase
aria showcase show; wait_keyboard showcase
assert_surface showcase
assert_eq "$(surfaces | tr ';' '\n' | grep -c showcase)" 1 "on one output"
wait_for 'popup gadget.clock calendar' 1 "the calendar"
wait_for 'popup gadget.themes menu > item' 3 "a menu: light, dark, base"
[ "$(count_widgets 'launcher item')" -gt 0 ] || { echo "no launcher with results"; false; }
wait_for 'exiter > row > button' 6 "the exit menu"
wait_for 'osd.volume' 1 "the OSD"
wait_for 'notification.normal' 1 "a notification"
wait_for 'notification.critical' 1 "a critical one"
wait_for 'gadget#showcase > button' 3 "the bar's buttons"
assert_no_surface launcher
assert_no_surface exiter
shot_surface showcase-light showcase

# The scheme from a bar: the showcase follows.
click_widget 'panel[output="HEADLESS-1"] gadget.themes > button'
assert_eq "$(aria debug theme)" "style=- scheme=dark" "dark"
settle 0.5
shot_surface showcase-dark showcase
click_widget 'panel[output="HEADLESS-1"] gadget.themes > button'
assert_eq "$(aria debug theme)" "style=- scheme=light" "light again"

# Inert: a click on a part does nothing, the showcase stays.
click_widget 'exiter > row > button.suspend'
click_widget 'launcher item'
assert_surface showcase
wait_for 'exiter > row > button' 6 "still the grid"

# Escape (it has the keyboard since the click).
key Escape; settle 0.5
assert_no_surface showcase

# A click beside the parts closes: the surface's top-left corner.
aria showcase show; wait_keyboard showcase
set -- $(surfaces | tr ';' '\n' | grep showcase | awk '{print $3}' | tr ',' ' ')
click $(($1 + 5)) $(($2 + 5))
assert_no_surface showcase

# The command toggles.
aria showcase toggle; wait_keyboard showcase
assert_surface showcase
aria showcase toggle
assert_no_surface showcase
