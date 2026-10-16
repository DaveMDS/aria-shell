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
wait_for 'surface.popup gadget.themes button.picker' 1 "the picker under it"
[ "$(count_widgets 'surface.launcher item')" -gt 0 ] || { echo "no launcher with results"; false; }
wait_for 'surface.exiter > row > button' 6 "the exit menu"
wait_for 'surface.widgets > line' 5 "the toolkit's widgets"
wait_for 'surface.widgets toggle' 2 "its toggles, the password's peek too"
wait_for 'surface.widgets checkbox' 2 "its check boxes"
wait_for 'surface.widgets radio' 3 "its radios"
wait_for 'surface.widgets switch' 2 "its switches"
wait_for 'surface.widgets#graph > graph' 1 "a graph, in its box"
wait_for 'surface.osd.volume' 1 "the OSD"
wait_for 'notification.normal' 1 "a notification"
wait_for 'notification.critical' 1 "a critical one"
wait_for 'gadget#showcase > indicator' 5 "the bar's buttons"
wait_for 'gadget.workspaces > indicator.danger' 1 "a gadget's state"
wait_for 'gadget.power > indicator.battery.danger' 1 "another"
assert_no_surface launcher
assert_no_surface exiter
shot_surface showcase-light showcase

# A sample's selector, in its tooltip.
set -- $(widget 'gadget.power > indicator.battery.danger')
pointer $(($1 + $3 / 2)) $(($2 + $4 / 2))
wait_for 'surface.panel#showcase gadget.power > tooltip' 1 "the tooltip"
shot_surface showcase-tooltip showcase

# The scheme from a bar: the showcase follows.
click_widget 'surface.panel[output="HEADLESS-1"] gadget.themes > indicator'
assert_eq "$(aria debug theme)" "theme=- scheme=dark" "dark"
settle 0.5
shot_surface showcase-dark showcase
click_widget 'surface.panel[output="HEADLESS-1"] gadget.themes > indicator'
assert_eq "$(aria debug theme)" "theme=- scheme=light" "light again"

# Inert: a click on a part does nothing, the showcase stays.
click_widget 'surface.exiter > row > button.suspend'
click_widget 'surface.launcher item'
assert_surface showcase
wait_for 'surface.exiter > row > button' 6 "still the grid"

# The widgets' box works: a check, a radio, a tab change; the slider
# moves the meter; a field takes what's typed.
checked() { count_widgets "surface.widgets > line:nth-child(2) > $1:checked"; }
assert_eq "$(checked checkbox)" 1 "the check on"
click_widget 'surface.widgets > line:nth-child(2) > checkbox'
assert_eq "$(checked checkbox)" 0 "off"
click_widget 'surface.widgets > line:nth-child(2) > radio:nth-child(4)'
assert_eq "$(checked radio)" 1 "one radio on"
wait_for 'surface.widgets > line:nth-child(2) > radio:checked:nth-child(4)' 1 "the other one"
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

# Tab moves the focus among the widgets, shown: to the slider, past the
# five buttons, the toggle, the check, the two radios and the switch;
# there Page Up moves it, and the meter with it.
aria showcase show; wait_keyboard showcase
before=$(widget 'surface.widgets > line > meter > fill' | awk '{print $3}')
for _ in $(seq 11); do key Tab; done
shot_surface showcase-focus showcase
key Page_Up
after=$(widget 'surface.widgets > line > meter > fill' | awk '{print $3}')
[ "$after" -gt "$before" ] || { echo "the slider didn't take Page Up: $before -> $after"; false; }
aria showcase toggle
assert_no_surface showcase
