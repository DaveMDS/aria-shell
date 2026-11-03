# The Custom gadget: a static button running a program per mouse button
# (the left one opens the launcher through the shell's own CLI, the
# others write to a file), a label from a program's output, JSON output
# with a class, an icon and a tip, and an empty output hiding the
# gadget; the `tooltip` key's tip.

# One button per bar; the static one has an icon and a text.
wait_for 'gadget.custom#static > indicator' 2 "one static button per bar"
wait_for 'surface.panel[output="HEADLESS-1"] gadget.custom#static icon' 1 "its icon"
wait_for 'surface.panel[output="HEADLESS-1"] gadget.custom#static label' 1 "its label"

# exec's output, once it's in.
i=0
while [ "$(count_widgets 'surface.panel[output="HEADLESS-1"] gadget.custom#echo label')" != 1 ] && [ $i -lt 30 ]; do
    sleep 0.1; i=$((i + 1))
done
wait_for 'surface.panel[output="HEADLESS-1"] gadget.custom#echo label' 1 "echo's output shown"
wait_for 'surface.panel[output="HEADLESS-1"] gadget.custom#json > indicator.warning' 1 "the JSON class on the button"
wait_for 'surface.panel[output="HEADLESS-1"] gadget.custom#json icon' 1 "the JSON icon"
wait_for 'gadget.custom#silent > indicator' 0 "an empty output hides the gadget"
set -- $(widget 'surface.panel[output="HEADLESS-1"] gadget.custom#echo > indicator')
shot outputs "$(($1 - 80)),0 240x32"

# One run for both panels; a click on either runs it again.
wait_for 'gadget.custom#once > indicator' 2 "the same output on both bars"
assert_eq "$(wc -l < "$ARIA_UI_OUT/runs")" 1 "the daemon ran it once"
click_widget 'surface.panel[output="HEADLESS-2"] gadget.custom#once > indicator'
sleep 0.3
assert_eq "$(wc -l < "$ARIA_UI_OUT/runs")" 2 "a click runs it again"

# The buttons: left opens the launcher, the others run their program.
click_widget 'surface.panel[output="HEADLESS-1"] gadget.custom#static > indicator'
settle 0.8
assert_surface launcher
key Escape
assert_no_surface launcher

click_widget_with right 'surface.panel[output="HEADLESS-1"] gadget.custom#static > indicator'
click_widget_with middle 'surface.panel[output="HEADLESS-2"] gadget.custom#static > indicator'
scroll 1
scroll -1
sleep 0.3
assert_eq "$(tr '\n' ' ' < "$ARIA_UI_OUT/clicks")" "right middle down up " "each button ran its program"
assert_logged 'ran "sh -c'

# The pointer resting on it: the `tooltip` key's tip (at the output's
# edge: not centred, kept inside it); away, gone. JSON's `tooltip` replaces the key's (one letter: much
# narrower); no `tooltip`, no tip.
tip_on() {
    pointer 960 600
    assert_no_surface tooltip
    set -- $(widget "surface.panel[output=\"HEADLESS-1\"] gadget.custom#$1 > indicator")
    cx=$(($1 + $3 / 2))
    pointer $cx $(($2 + $4 / 2))
}
tip_on static
assert_surface tooltip
set -- $(widget 'surface.tooltip')
shot tip "$(($1 - 30)),$(($2 - 20)) $(($3 + 60))x$(($4 + 60))"
tip_on json
assert_surface tooltip
set -- $(widget 'surface.tooltip > label')
[ "$3" -gt 150 ] || { echo "the key's tip, not the output's: $*"; exit 1; }
shot tip-json "$(($1 - 30)),$(($2 - 20)) $(($3 + 60))x$(($4 + 60))"
tip_on echo
settle 1
assert_no_surface tooltip
pointer 960 600
