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
bell='surface.panel[output="HEADLESS-1"] gadget.notifications > button'

wait_for 'gadget.notifications > button' 2 "a bell per bar"
wait_for 'gadget.notifications > button label' 0 "no count at rest"

# Two notifications: the count on both bars, the button marked new.
id1=$(nsend -p -t 0 -i dialog-information "First" "with a body")
id2=$(nsend -p -t 0 "Second" "another one")
settle
wait_for 'gadget.notifications > button.active label' 2 "count on both bars"
assert_eq "$(toasts)" 2
shot bell "$(($(widget "$bell" | cut -d' ' -f1) - 60)),0 140x32"

# The popup: the rows, both unseen; the count is gone once looked at.
click_widget "$bell"
assert_surface popup
wait_for 'surface.popup gadget.notifications list > notification' 2 "two rows"
wait_for 'surface.popup gadget.notifications list > notification.unseen' 2
wait_for 'gadget.notifications > button label' 0 "count cleared"
wait_for 'gadget.notifications > button.active' 0
wait_for 'surface.popup notification time' 2 "ages shown"
shot_surface popup popup

# The row and the desktop toast of one notification are the same view:
# same parts, same body width, and the same height once the row's
# extras (age, ✕) are accounted for on the summary line only.
set -- $(widget 'surface.popup notification[id="'"$id1"'"]')
row_w=$3
set -- $(widget 'surface.popup notification[id="'"$id1"'"] body')
row_body_w=$3
set -- $(widget 'surface.notification[id="'"$id1"'"]')
toast_w=$3
set -- $(widget 'surface.notification[id="'"$id1"'"] > notification body')
toast_body_w=$3
# Both bodies span their frame (the row's border, the toast's surface)
# minus the same border, padding and icon column.
assert_eq "$((row_w - row_body_w))" "$((toast_w - toast_body_w))" "the body follows the width"
set -- $(widget 'surface.popup notification[id="'"$id1"'"] icon')
assert_eq "$3" 32 "the row draws the icon at the toast's size"

# ✕ on the first row: the row and its toast go.
click_widget 'surface.popup notification[id="'"$id1"'"] button.close'
wait_for 'surface.popup gadget.notifications list > notification' 1
assert_eq "$(toasts)" 1 "its toast closed too"
wait_for 'surface.popup notification[id="'"$id2"'"]' 1 "the other stays"

# Do not disturb from the popup: a new notification is listed, not
# shown; a critical one shows anyway.
click_widget 'surface.popup gadget.notifications header toggle.dnd'
wait_for 'surface.popup gadget.notifications header toggle.dnd.checked' 1
wait_for 'gadget.notifications > button.muted' 2 "quiet on both bars"
nsend -t 0 "Quiet" "not on screen" > /dev/null
settle
assert_eq "$(toasts)" 1 "no new toast"
wait_for 'surface.popup gadget.notifications list > notification' 2 "but in the history"
wait_for 'surface.popup gadget.notifications list > notification.unseen' 1
nsend -t 0 -u critical "Loud" "on screen anyway" > /dev/null
settle
assert_eq "$(toasts)" 2 "critical shows"
shot_surface popup-dnd popup

# A click outside closes the popup; the count shows the two new ones,
# still quiet.
click 600 600
assert_no_surface popup
wait_for 'gadget.notifications > button.muted > label' 2

# Right click: quiet off. Middle click: the toasts go, the history stays.
click_widget_with right "$bell"
wait_for 'gadget.notifications > button.muted' 0
wait_for 'gadget.notifications > button.active' 2 "new again, not quiet"
click_widget_with middle "$bell"
assert_eq "$(toasts)" 0 "all toasts dismissed"
click_widget "$bell"
assert_surface popup
wait_for 'surface.popup gadget.notifications list > notification' 3 "history intact"

# Clear: empty.
click_widget 'surface.popup gadget.notifications header button.clear'
wait_for 'surface.popup gadget.notifications list > notification' 0
wait_for 'surface.popup gadget.notifications empty' 1 "the empty text"
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
wait_for 'surface.popup gadget.notifications list > notification' 5 "capped"
wait_for 'surface.popup notification[id="5"]' 0 "the oldest (id 5) dropped"
wait_for 'surface.popup notification[id="10"]' 1 "the newest kept"
click_widget 'surface.popup gadget.notifications header button.clear'
click 600 600
assert_no_surface popup

# A row click sends the default action back to the client (its toast,
# still up, goes too).
nsend -t 0 -A default=Open "Act" "click the row" > "$ARIA_UI_OUT/action" &
settle 0.5
assert_eq "$(toasts)" 1
click_widget "$bell"
wait_for 'surface.popup gadget.notifications list > notification' 1
click_widget 'surface.popup gadget.notifications list > notification body'
settle
wait
assert_eq "$(cat "$ARIA_UI_OUT/action")" default "default action from the row"
wait_for 'surface.popup gadget.notifications list > notification' 0 "the row is gone"
assert_eq "$(toasts)" 0 "and its toast"
