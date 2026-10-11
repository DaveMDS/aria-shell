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
assert_eq "$(count_widgets 'exiter > row > button')" 6 "the exit menu"
assert_eq "$(count_widgets 'osd.volume')" 1 "the OSD"
assert_eq "$(count_widgets 'notification.normal')" 1 "a notification"
assert_eq "$(count_widgets 'notification.critical')" 1 "a critical one"
assert_eq "$(count_widgets 'gadget#showcase > button')" 3 "the bar's buttons"
assert_eq "$(count_widgets 'popup#showcase > buttons > row > button')" 14 "the other buttons"
assert_eq "$(count_widgets 'popup#showcase > buttons > row > button.danger')" 2 "a danger one, and disabled"
assert_eq "$(count_widgets 'popup#showcase > buttons > row > button.plain')" 3 "three plain ones"
assert_eq "$(count_widgets 'popup#showcase > buttons > row > toggle.radio')" 2 "two radios"
assert_eq "$(count_widgets 'popup#showcase > buttons > row > toggle')" 6 "four toggles and the radios"
assert_eq "$(count_widgets 'popup#showcase > buttons > tabs > tab')" 3 "tabs, one open"
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
assert_eq "$(count_widgets 'exiter > row > button')" 6 "still the grid"

# The sample plain buttons, radios and tabs follow a click.
click_widget 'popup#showcase button.plain:nth-child(1)'
assert_eq "$(count_widgets 'popup#showcase button.plain.active:nth-child(1)')" 1 "the first plain one on"
click_widget 'popup#showcase button.plain:nth-child(1)'
assert_eq "$(count_widgets 'popup#showcase button.plain.active:nth-child(1)')" 0 "and off again"
click_widget 'popup#showcase toggle.radio:nth-child(2)'
assert_eq "$(count_widgets 'popup#showcase toggle.radio.checked:nth-child(2)')" 1 "the second radio chosen"
assert_eq "$(count_widgets 'popup#showcase toggle.radio.checked')" 1 "only it"
click_widget 'popup#showcase toggle.switch'
assert_eq "$(count_widgets 'popup#showcase toggle.switch.checked')" 1 "the switch on"
click_widget 'popup#showcase tabs > tab:nth-child(3)'
assert_eq "$(count_widgets 'popup#showcase tabs > tab.active:nth-child(3)')" 1 "the third tab open"
assert_eq "$(count_widgets 'popup#showcase tabs > tab.active')" 1 "only it"

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
