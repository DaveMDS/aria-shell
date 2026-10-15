# The exit menu: `aria exiter show` on the focused output over a grab
# per output; the buttons run their command (here: a line in
# $ARIA_UI_OUT/exits) at once or after a confirmation shown in place,
# by click or keyboard; the countdown confirms by itself.

exits=$ARIA_UI_OUT/exits
assert_no_surface exiter

aria exiter show; wait_keyboard exiter
assert_surface exiter
assert_eq "$(surfaces | tr ';' '\n' | grep -c grab)" 2 "one grab per output"
wait_for 'surface.exiter > row > button' 6 "six buttons"
shot_surface exiter exiter

# A click runs a plain action and closes.
click_widget 'surface.exiter > row > button.suspend'
assert_no_surface exiter
assert_eq "$(cat "$exits")" "suspend"

# The keyboard (the query doesn't see :focus: what Enter runs says
# where it is): `default_button = suspend` has it on open, Right is the third
# button, Down the row below, Down again round to the top.
aria exiter show; wait_keyboard exiter
key Right; key Down; key Down
shot_surface exiter-focus exiter
key Return; settle 0.5
assert_no_surface exiter
assert_eq "$(tail -n 1 "$exits")" "hibernate"

# A dangerous one asks: Escape goes back to the grid, Enter confirms.
aria exiter show; wait_keyboard exiter
click_widget 'surface.exiter > row > button.reboot'
assert_surface exiter
wait_for 'surface.exiter > confirm' 1 "the confirmation"
wait_for 'surface.exiter confirm > countdown' 1 "with its countdown"
wait_for 'surface.exiter > row > button' 0 "no grid meanwhile"
shot_surface exiter-confirm exiter
key Escape
assert_surface exiter
wait_for 'surface.exiter > row > button' 6 "the grid again"
# Back on the grid, the focus on suspend again.
key Return; settle 0.5
assert_no_surface exiter
assert_eq "$(tail -n 1 "$exits")" "suspend"
aria exiter show; wait_keyboard exiter
click_widget 'surface.exiter > row > button.reboot'
key Return; settle 0.5
assert_no_surface exiter
assert_eq "$(tail -n 1 "$exits")" "reboot"

# Cancel by click; then the countdown (3 s) runs it alone.
aria exiter show; wait_keyboard exiter
click_widget 'surface.exiter > row > button.shutdown'
click_widget 'surface.exiter confirm > buttons > button.cancel'
wait_for 'surface.exiter > row > button' 6 "cancelled"
click_widget 'surface.exiter > row > button.shutdown'
sleep 3.6
assert_no_surface exiter
assert_eq "$(tail -n 1 "$exits")" "shutdown"

# A click outside closes; toggle twice.
aria exiter show; wait_keyboard exiter
click 300 700
assert_no_surface exiter
aria exiter toggle; wait_keyboard exiter
assert_surface exiter
aria exiter toggle
assert_no_surface exiter

# Lock from the menu (the command is our own CLI); Enter unlocks.
aria exiter show; wait_keyboard exiter
click_widget 'surface.exiter > row > button.lock'
settle 0.8
assert_no_surface exiter
assert_surface locker
key Return; settle 0.8
assert_no_surface locker

# The same buttons in the launcher, as icons: a plain one runs and
# closes the launcher, a dangerous one opens the exit menu on its
# confirmation.
aria launcher show; wait_keyboard launcher
wait_for 'surface.launcher actions > button' 6 "the six actions in the launcher"
shot_surface launcher-actions launcher
click_widget 'surface.launcher actions > button.suspend'
assert_no_surface launcher
assert_eq "$(tail -n 1 "$exits")" "suspend"
aria launcher show; wait_keyboard launcher
click_widget 'surface.launcher actions > button.reboot'
settle 0.5
wait_for 'surface.exiter > confirm' 1 "the exit menu on reboot's confirmation"
key Escape
assert_no_surface launcher
assert_surface exiter
wait_for 'surface.exiter > row > button' 6 "Escape: its grid"
key Escape
assert_no_surface exiter

assert_eq "$(cat "$exits" | tr '\n' ' ')" "suspend hibernate suspend reboot shutdown suspend " "every action ran once, suspend three times"
