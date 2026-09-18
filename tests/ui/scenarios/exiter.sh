# The exit menu: `aria exiter show` on the focused output over a grab
# per output; the buttons run their command (here: a line in
# $ARIA_UI_OUT/exits) at once or after a confirmation shown in place,
# by click or keyboard; the countdown confirms by itself.

exits=$ARIA_UI_OUT/exits
assert_no_surface exiter

aria exiter show; settle 0.8
assert_surface exiter
assert_eq "$(surfaces | tr ';' '\n' | grep -c grab)" 2 "one grab per output"
assert_eq "$(count_widgets 'exiter > button')" 6 "six buttons"
assert_eq "$(count_widgets 'exiter > button.lock.selected')" 1 "the first is selected"
shot_surface exiter exiter

# A click runs a plain action and closes.
click_widget 'exiter > button.suspend'
assert_no_surface exiter
assert_eq "$(cat "$exits")" "suspend"

# The keyboard: Right Right Enter is the third button.
aria exiter show; settle 0.8
key Right; key Right
assert_eq "$(count_widgets 'exiter > button.hibernate.selected')" 1 "selection moved"
key Return; settle 0.5
assert_no_surface exiter
assert_eq "$(tail -n 1 "$exits")" "hibernate"

# A dangerous one asks: Escape goes back to the grid, Enter confirms.
aria exiter show; settle 0.8
click_widget 'exiter > button.reboot'
assert_surface exiter
assert_eq "$(count_widgets 'exiter > confirm')" 1 "the confirmation"
assert_eq "$(count_widgets 'exiter confirm > countdown')" 1 "with its countdown"
assert_eq "$(count_widgets 'exiter > button')" 0 "no grid meanwhile"
shot_surface exiter-confirm exiter
key Escape
assert_surface exiter
assert_eq "$(count_widgets 'exiter > button')" 6 "the grid again"
click_widget 'exiter > button.reboot'
key Return; settle 0.5
assert_no_surface exiter
assert_eq "$(tail -n 1 "$exits")" "reboot"

# Cancel by click; then the countdown (3 s) runs it alone.
aria exiter show; settle 0.8
click_widget 'exiter > button.shutdown'
click_widget 'exiter confirm > button.cancel'
assert_eq "$(count_widgets 'exiter > button')" 6 "cancelled"
click_widget 'exiter > button.shutdown'
sleep 3.6
assert_no_surface exiter
assert_eq "$(tail -n 1 "$exits")" "shutdown"

# A click outside closes; toggle twice.
aria exiter show; settle 0.8
click 300 700
assert_no_surface exiter
aria exiter toggle; settle 0.8
assert_surface exiter
aria exiter toggle; settle 0.5
assert_no_surface exiter

# Lock from the menu (the command is our own CLI); Enter unlocks.
aria exiter show; settle 0.8
click_widget 'exiter > button.lock'
settle 0.8
assert_no_surface exiter
assert_surface locker
key Return; settle 0.8
assert_no_surface locker

# The same buttons in the launcher, as icons: a plain one runs and
# closes the launcher, a dangerous one opens the exit menu on its
# confirmation.
aria launcher show; settle 0.8
assert_eq "$(count_widgets 'launcher actions > button')" 6 "the six actions in the launcher"
shot_surface launcher-actions launcher
click_widget 'launcher actions > button.suspend'
assert_no_surface launcher
assert_eq "$(tail -n 1 "$exits")" "suspend"
aria launcher show; settle 0.8
click_widget 'launcher actions > button.reboot'
settle 0.5
assert_eq "$(count_widgets 'exiter > confirm')" 1 "the exit menu on reboot's confirmation"
key Escape
assert_no_surface launcher
assert_surface exiter
assert_eq "$(count_widgets 'exiter > button')" 6 "Escape: its grid"
key Escape
assert_no_surface exiter

assert_eq "$(cat "$exits" | tr '\n' ' ')" "suspend hibernate reboot shutdown suspend " "every action ran once, suspend twice"
