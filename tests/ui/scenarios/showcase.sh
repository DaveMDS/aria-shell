# The showcase: `aria showcase show` draws every part of the UI (the
# popups, the launcher, the toolkit's widgets, the exit menu, the OSD, two notifications, a bar
# of the gadgets' states) on one surface between the bars of the
# focused output, inert; it follows the scheme; Escape, a click beside
# the parts or the command again close it.

assert_no_surface showcase
aria showcase show; wait_keyboard showcase
assert_surface showcase
assert_eq "$(surfaces | tr ';' '\n' | grep -c showcase)" 1 "on one output"
wait_for 'surface.popup gadget.clock calendar' 1 "the calendar"
wait_for 'surface.popup gadget.themes menu > item' 4 "a menu: light, dark, base, the theme's settings"
[ "$(count_widgets 'surface.launcher item')" -gt 0 ] || { echo "no launcher with results"; false; }
wait_for 'surface.exiter > row > button' 6 "the exit menu"
wait_for 'surface.widgets > line' 5 "the toolkit's widgets"
wait_for 'surface.widgets toggle' 9 "its toggles, the password's peek too"
wait_for 'surface.widgets#graph > graph' 1 "a graph, in its box"
wait_for 'surface.osd.volume' 1 "the OSD"
wait_for 'notification.normal' 1 "a notification"
wait_for 'notification.critical' 1 "a critical one"
wait_for 'gadget#showcase > button' 5 "the bar's buttons"
wait_for 'gadget.workspaces > button.danger' 1 "a gadget's state"
wait_for 'gadget.power > button.battery.danger' 1 "another"
assert_no_surface launcher
assert_no_surface exiter
shot_surface showcase-light showcase

# A sample's selector, in its tooltip.
set -- $(widget 'gadget.power > button.battery.danger')
pointer $(($1 + $3 / 2)) $(($2 + $4 / 2))
wait_for 'surface.panel#showcase gadget.power > tooltip' 1 "the tooltip"
shot_surface showcase-tooltip showcase

# The scheme from a bar: the showcase follows.
click_widget 'surface.panel[output="HEADLESS-1"] gadget.themes > button'
assert_eq "$(aria debug theme)" "theme=- scheme=dark" "dark"
settle 0.5
shot_surface showcase-dark showcase
click_widget 'surface.panel[output="HEADLESS-1"] gadget.themes > button'
assert_eq "$(aria debug theme)" "theme=- scheme=light" "light again"

# Inert: a click on a part does nothing, the showcase stays.
click_widget 'surface.exiter > row > button.suspend'
click_widget 'surface.launcher item'
assert_surface showcase
wait_for 'surface.exiter > row > button' 6 "still the grid"

# The widgets' box works: a check, a radio, a tab change; the slider
# moves the meter; a field takes what's typed.
checked() { count_widgets "surface.widgets > line:nth-child(2) > toggle.checked.$1"; }
assert_eq "$(checked check)" 1 "the check on"
click_widget 'surface.widgets > line:nth-child(2) > toggle.check'
assert_eq "$(checked check)" 0 "off"
click_widget 'surface.widgets > line:nth-child(2) > toggle.radio:nth-child(4)'
assert_eq "$(checked radio)" 1 "one radio on"
wait_for 'surface.widgets > line:nth-child(2) > toggle.radio.checked:nth-child(4)' 1 "the other one"
click_widget 'surface.widgets tabs > tab:nth-child(3)'
wait_for 'surface.widgets tabs > tab.active:nth-child(3)' 1 "the third tab"
before=$(widget 'surface.widgets > line > meter > fill' | awk '{print $3}')
set -- $(widget 'surface.widgets > line > slider')
drag $(($1 + $3 * 4 / 10)) $(($2 + $4 / 2)) $(($1 + $3 * 9 / 10)) $(($2 + $4 / 2))
after=$(widget 'surface.widgets > line > meter > fill' | awk '{print $3}')
[ "$after" -gt "$before" ] || { echo "the meter didn't fill: $before -> $after"; false; }
shot_surface showcase-widgets showcase


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
