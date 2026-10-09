# The showcase: `aria showcase show` draws every part of the UI (the
# popups, the launcher, the exit menu, the OSD, two notifications, every
# kind of button) on one surface between the bars of the focused
# output, inert; it follows the scheme; Escape, a click beside the
# parts or the command again close it.

assert_no_surface showcase
aria showcase show; settle 1
assert_surface showcase
assert_eq "$(surfaces | tr ';' '\n' | grep -c showcase)" 1 "on one output"
assert_eq "$(count_widgets 'popup gadget.clock calendar')" 1 "the calendar"
assert_eq "$(count_widgets 'popup gadget.themes menu > item')" 3 "a menu: light, dark, base"
[ "$(count_widgets 'launcher item')" -gt 0 ] || { echo "no launcher with results"; false; }
assert_eq "$(count_widgets 'exiter > button')" 6 "the exit menu"
assert_eq "$(count_widgets 'osd.volume')" 1 "the OSD"
assert_eq "$(count_widgets 'notification.normal')" 1 "a notification"
assert_eq "$(count_widgets 'notification.critical')" 1 "a critical one"
assert_eq "$(count_widgets 'gadget#showcase > button')" 3 "the bar's buttons"
assert_eq "$(count_widgets 'popup#showcase > buttons > button')" 6 "the other buttons"
assert_eq "$(count_widgets 'popup#showcase > buttons > button.danger')" 1 "a danger one"
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
click_widget 'exiter > button.suspend'
click_widget 'launcher item'
assert_surface showcase
assert_eq "$(count_widgets 'exiter > button')" 6 "still the grid"

# Escape (it has the keyboard since the click).
key Escape; settle 0.5
assert_no_surface showcase

# A click beside the parts closes: the surface's top-left corner.
aria showcase show; settle 1
set -- $(surfaces | tr ';' '\n' | grep showcase | awk '{print $3}' | tr ',' ' ')
click $(($1 + 5)) $(($2 + 5))
assert_no_surface showcase

# The command toggles.
aria showcase toggle; settle 1
assert_surface showcase
aria showcase toggle; settle 0.5
assert_no_surface showcase
