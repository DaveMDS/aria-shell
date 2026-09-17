# The Notifications gadget: a bell on both bars with the count of the
# notifications not looked at; a left click opens the history popup,
# whose rows are the very notification views shown on the desktop; a
# row's ✕ drops it (and its toast); do-not-disturb (right click, or the
# popup's button) keeps new ones off the screen but in the history,
# critical ones excepted; a middle click closes the toasts; Clear
# empties the history; the history is capped (history = 5); a row
# click invokes the default action.

nsend() {
    notify-send "$@"
}
toasts() {
    surfaces | tr ';' '\n' | grep -c ' notification '
}
bell='panel[output="HEADLESS-1"] gadget.notifications > button'

assert_eq "$(count_widgets 'gadget.notifications > button')" 2 "a bell per bar"
assert_eq "$(count_widgets 'gadget.notifications > button text')" 0 "no count at rest"

# Two notifications: the count on both bars, the button marked new.
id1=$(nsend -p -t 0 -i dialog-information "First" "with a body")
id2=$(nsend -p -t 0 "Second" "another one")
settle
assert_eq "$(count_widgets 'gadget.notifications > button.new text')" 2 "count on both bars"
assert_eq "$(toasts)" 2
shot bell "$(($(widget "$bell" | cut -d' ' -f1) - 60)),0 140x32"

# The popup: the rows, both unseen; the count is gone once looked at.
click_widget "$bell"
assert_surface popup
assert_eq "$(count_widgets 'popup gadget.notifications list > notification')" 2 "two rows"
assert_eq "$(count_widgets 'popup gadget.notifications list > notification.unseen')" 2
assert_eq "$(count_widgets 'gadget.notifications > button text')" 0 "count cleared"
assert_eq "$(count_widgets 'gadget.notifications > button.new')" 0
assert_eq "$(count_widgets 'popup notification time')" 2 "ages shown"
shot_surface popup popup

# The row and the desktop toast of one notification are the same view:
# same parts, same body width, and the same height once the row's
# extras (age, ✕) are accounted for on the summary line only.
set -- $(widget 'popup notification#'"$id1"'')
row_w=$3
set -- $(widget 'popup notification#'"$id1"' body')
row_body_w=$3
set -- $(widget 'notification#'"$id1"':root')
toast_w=$3
set -- $(widget 'notification#'"$id1"':root body')
toast_body_w=$3
# Both bodies span their container minus the same padding and icon
# column.
assert_eq "$((row_w - row_body_w))" "$((toast_w - toast_body_w))" "the body follows the width"
set -- $(widget 'popup notification#'"$id1"' icon')
assert_eq "$3" 32 "the row draws the icon at the toast's size"

# ✕ on the first row: the row and its toast go.
click_widget 'popup notification#'"$id1"' button.close'
assert_eq "$(count_widgets 'popup gadget.notifications list > notification')" 1
assert_eq "$(toasts)" 1 "its toast closed too"
assert_eq "$(count_widgets 'popup notification#'"$id2"'')" 1 "the other stays"

# Do not disturb from the popup: a new notification is listed, not
# shown; a critical one shows anyway.
click_widget 'popup gadget.notifications header button.dnd'
assert_eq "$(count_widgets 'popup gadget.notifications header button.dnd.on')" 1
assert_eq "$(count_widgets 'gadget.notifications > button.dnd')" 2 "quiet on both bars"
nsend -t 0 "Quiet" "not on screen" > /dev/null
settle
assert_eq "$(toasts)" 1 "no new toast"
assert_eq "$(count_widgets 'popup gadget.notifications list > notification')" 2 "but in the history"
assert_eq "$(count_widgets 'popup gadget.notifications list > notification.unseen')" 1
nsend -t 0 -u critical "Loud" "on screen anyway" > /dev/null
settle
assert_eq "$(toasts)" 2 "critical shows"
shot_surface popup-dnd popup

# A click outside closes the popup; the count shows the two new ones.
click 600 600
assert_no_surface popup
assert_eq "$(count_widgets 'gadget.notifications > button.new')" 2

# Right click: quiet off. Middle click: the toasts go, the history stays.
click_widget_with right "$bell"
assert_eq "$(count_widgets 'gadget.notifications > button.dnd')" 0
click_widget_with middle "$bell"
assert_eq "$(toasts)" 0 "all toasts dismissed"
click_widget "$bell"
assert_surface popup
assert_eq "$(count_widgets 'popup gadget.notifications list > notification')" 3 "history intact"

# Clear: empty.
click_widget 'popup gadget.notifications header button.clear'
assert_eq "$(count_widgets 'popup gadget.notifications list > notification')" 0
assert_eq "$(count_widgets 'popup gadget.notifications empty')" 1 "the empty text"
shot_surface popup-empty popup
click 600 600
assert_no_surface popup

# The cap: six in, five kept, the newest first.
i=1
while [ $i -le 6 ]; do
    nsend -t 0 "Number $i" "" > /dev/null
    i=$((i + 1))
done
settle
assert_eq "$(toasts)" 6
click_widget_with middle "$bell"
assert_eq "$(toasts)" 0
click_widget "$bell"
assert_eq "$(count_widgets 'popup gadget.notifications list > notification')" 5 "capped"
assert_eq "$(count_widgets 'popup notification#5')" 0 "the oldest (id 5) dropped"
assert_eq "$(count_widgets 'popup notification#10')" 1 "the newest kept"
click_widget 'popup gadget.notifications header button.clear'
click 600 600
assert_no_surface popup

# A row click sends the default action back to the client (its toast,
# still up, goes too).
nsend -t 0 -A default=Open "Act" "click the row" > "$ARIA_UI_OUT/action" &
settle 0.5
assert_eq "$(toasts)" 1
click_widget "$bell"
assert_eq "$(count_widgets 'popup gadget.notifications list > notification')" 1
click_widget 'popup gadget.notifications list > notification body'
settle
wait
assert_eq "$(cat "$ARIA_UI_OUT/action")" default "default action from the row"
assert_eq "$(count_widgets 'popup gadget.notifications list > notification')" 0 "the row is gone"
assert_eq "$(toasts)" 0 "and its toast"
