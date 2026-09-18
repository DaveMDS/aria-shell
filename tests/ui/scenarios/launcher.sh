# The launcher: open, search, keyboard selection, close by every route,
# launch by click and by Enter (two test desktop entries whose Exec is
# `true`, see tests/ui/data), the launched one ranking first, and the
# desktop actions of Zzz opened as child rows.

assert_no_surface launcher

aria launcher show; settle 0.8
assert_surface launcher
assert_surface grab
assert_eq "$(surfaces | tr ';' '\n' | grep -c grab)" 2 "one grab per output"
shot launcher-open

# Typing filters, the first result is selected.
type_text "aria test"
shot_surface launcher-search launcher
assert_eq "$(count_widgets 'launcher item')" 2 "two results for 'aria test'"
assert_eq "$(count_widgets 'item.selected:nth-child(1)')" 1 "the first is selected"

# Arrows move the selection.
key BackSpace; key BackSpace; key BackSpace; key BackSpace; key BackSpace
key BackSpace; key BackSpace; key BackSpace; key BackSpace
n=$(count_widgets 'launcher item')
[ "$n" -gt 2 ] || { echo "expected several results with an empty query, got $n"; exit 1; }
key Down; key Down
assert_eq "$(count_widgets 'item.selected:nth-child(3)')" 1 "third item selected after Down Down"
key Up
assert_eq "$(count_widgets 'item.selected:nth-child(2)')" 1 "second item selected after Up"

# Escape closes.
key Escape
assert_no_surface launcher
assert_no_surface grab

# A click outside closes (on the other output too).
aria launcher show; settle 0.8
click 300 700
assert_no_surface launcher
aria launcher show; settle 0.8
click 2500 700
assert_no_surface launcher

# A click on a result launches it and closes.
aria launcher show; settle 0.8
type_text "aria test"
click_widget 'launcher item'
assert_logged 'launched "aria-test":'
assert_no_surface launcher

# Enter launches the selected one.
aria launcher show; settle 0.8
type_text "aria test"
key Return
assert_eq "$(grep -c 'launched "aria-test":' "$ARIA_UI_OUT/shell.log")" 2 "launched twice"
assert_no_surface launcher

# Usage ranks: Zzz is second by name; launched more often it comes
# first, and the counts are kept in XDG_STATE_HOME.
for _ in 1 2 3; do
    aria launcher show; settle 0.8
    type_text "aria test"
    key Down
    key Return
    settle
done
assert_eq "$(grep -c 'launched "aria-test-zzz":' "$ARIA_UI_OUT/shell.log")" 3 "Zzz launched three times"
aria launcher show; settle 0.8
type_text "aria test"
key Return
assert_eq "$(grep -c 'launched "aria-test-zzz":' "$ARIA_UI_OUT/shell.log")" 4 "Zzz is first once it's used more"
assert_eq "$(cat "$XDG_STATE_HOME/aria-shell/launcher-usage")" "aria-test-zzz 4
aria-test 2" "usage file"

# Desktop actions: Zzz (first now) has two. Right opens them as rows
# below it and selects the first; Down, Enter runs the second.
aria launcher show; settle 0.8
type_text "aria test"
assert_eq "$(count_widgets 'item.action')" 0 "closed at first"
assert_eq "$(count_widgets 'launcher chevron')" 1 "only Zzz has a chevron"
key Right
assert_eq "$(count_widgets 'item.action')" 2 "two action rows"
assert_eq "$(count_widgets 'item.action.selected:nth-child(2)')" 1 "the first action selected"
key Down
assert_eq "$(count_widgets 'item.action.selected:nth-child(3)')" 1 "the second after Down"
shot_surface launcher-actions launcher
key Return
assert_logged 'launched "aria-test-zzz" action "two"'
assert_no_surface launcher

# Left goes back to the entry and closes them.
aria launcher show; settle 0.8
type_text "aria test"
key Right
key Left
assert_eq "$(count_widgets 'item.action')" 0 "closed by Left"
assert_eq "$(count_widgets 'item.selected:nth-child(1)')" 1 "Zzz selected again"
assert_eq "$(count_widgets 'launcher item')" 2 "the two results"

# The chevron opens them with a click, a click on an action runs it.
click_widget 'launcher chevron'
assert_surface launcher
assert_eq "$(count_widgets 'item.action')" 2 "opened by the chevron"
click_widget 'item.action'
assert_logged 'launched "aria-test-zzz" action "one"'
assert_no_surface launcher
assert_eq "$(grep -c 'launched "aria-test-zzz":' "$ARIA_UI_OUT/shell.log")" 4 "actions don't count as plain launches in the log"
assert_eq "$(head -n 1 "$XDG_STATE_HOME/aria-shell/launcher-usage")" "aria-test-zzz 6" "but they do for usage"

# toggle / hide through the socket.
aria launcher toggle; settle 0.8
assert_surface launcher
aria launcher toggle; settle
assert_no_surface launcher
aria launcher hide; settle
assert_no_surface launcher
