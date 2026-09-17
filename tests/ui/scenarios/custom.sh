# The Custom gadget: a static button running a program per mouse button
# (the left one opens the launcher through the shell's own CLI, the
# others write to a file), a label from a program's output, JSON output
# with a class and an icon, and an empty output hiding the gadget.

# One button per bar; the static one has an icon and a text.
assert_eq "$(count_widgets 'gadget.custom#static > button')" 2 "one static button per bar"
assert_eq "$(count_widgets 'panel[output="HEADLESS-1"] gadget.custom#static icon')" 1 "its icon"
assert_eq "$(count_widgets 'panel[output="HEADLESS-1"] gadget.custom#static text')" 1 "its label"

# exec's output, once it's in.
i=0
while [ "$(count_widgets 'panel[output="HEADLESS-1"] gadget.custom#echo text')" != 1 ] && [ $i -lt 30 ]; do
    sleep 0.1; i=$((i + 1))
done
assert_eq "$(count_widgets 'panel[output="HEADLESS-1"] gadget.custom#echo text')" 1 "echo's output shown"
assert_eq "$(count_widgets 'panel[output="HEADLESS-1"] gadget.custom#json > button.warning')" 1 "the JSON class on the button"
assert_eq "$(count_widgets 'panel[output="HEADLESS-1"] gadget.custom#json icon')" 1 "the JSON icon"
assert_eq "$(count_widgets 'gadget.custom#silent > button')" 0 "an empty output hides the gadget"
set -- $(widget 'panel[output="HEADLESS-1"] gadget.custom#echo > button')
shot outputs "$(($1 - 80)),0 240x32"

# One run for both panels; a click on either runs it again.
assert_eq "$(count_widgets 'gadget.custom#once > button')" 2 "the same output on both bars"
assert_eq "$(wc -l < "$ARIA_UI_OUT/runs")" 1 "the daemon ran it once"
click_widget 'panel[output="HEADLESS-2"] gadget.custom#once > button'
sleep 0.3
assert_eq "$(wc -l < "$ARIA_UI_OUT/runs")" 2 "a click runs it again"

# The buttons: left opens the launcher, the others run their program.
click_widget 'panel[output="HEADLESS-1"] gadget.custom#static > button'
settle 0.8
assert_surface launcher
key Escape
assert_no_surface launcher

click_widget_with right 'panel[output="HEADLESS-1"] gadget.custom#static > button'
click_widget_with middle 'panel[output="HEADLESS-2"] gadget.custom#static > button'
scroll 1
scroll -1
sleep 0.3
assert_eq "$(tr '\n' ' ' < "$ARIA_UI_OUT/clicks")" "right middle down up " "each button ran its program"
assert_logged 'ran "sh -c'
