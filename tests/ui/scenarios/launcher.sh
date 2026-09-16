# The launcher: open, search, keyboard selection, close by every route,
# launch by click and by Enter (a test desktop entry whose Exec is
# `true`, see tests/ui/data).

assert_no_surface launcher

aria launcher show; settle 0.8
assert_surface launcher
assert_surface grab
assert_eq "$(surfaces | tr ';' '\n' | grep -c grab)" 2 "one grab per output"
shot launcher-open

# Typing filters, the first result is selected.
type_text "aria test"
shot_surface launcher-search launcher
assert_eq "$(count_widgets 'launcher item')" 1 "one result for 'aria test'"
assert_eq "$(count_widgets 'item.selected')" 1 "it is selected"

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
assert_logged 'launched "aria-test"'
assert_no_surface launcher

# Enter launches the selected one.
aria launcher show; settle 0.8
type_text "aria test"
key Return
assert_eq "$(grep -c 'launched "aria-test"' "$ARIA_UI_OUT/shell.log")" 2 "launched twice"
assert_no_surface launcher

# toggle / hide through the socket.
aria launcher toggle; settle 0.8
assert_surface launcher
aria launcher toggle; settle
assert_no_surface launcher
aria launcher hide; settle
assert_no_surface launcher
