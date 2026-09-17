# The notification daemon: notify-send reaches our server on the
# scenario's bus; each notification gets a surface stacked from the
# top-right corner of the focused output; a replacement keeps its place;
# a click invokes the default action, an action button its action (the
# client sees the key), a right click dismisses, CloseNotification and
# the timeout remove it; an app icon, an image file and image-data draw.

nsend() {
    notify-send "$@"
}
call() {
    method=$1
    shift
    gdbus call --session --dest org.freedesktop.Notifications \
        --object-path /org/freedesktop/Notifications --method "$method" "$@" 2>&1 || :
}
# The surfaces are listed in creation order.
toast_geo() {
    surfaces | tr ';' '\n' | grep ' notification ' | sed -n "${1:-1}p" | sed 's/^ *//'
}

assert_logged "serving org.freedesktop.Notifications"
assert_contains "$(call org.freedesktop.Notifications.GetServerInformation)" "'aria-shell'" "server info"
assert_contains "$(call org.freedesktop.Notifications.GetCapabilities)" "'actions'" "capabilities"

# One notification, on the focused output, below the bar at the top right.
id1=$(nsend -p -t 0 -i dialog-information "First summary" "A body that is long enough to wrap onto a second line in the toast")
settle
assert_eq "$(count_widgets 'notification')" 1 "one toast"
set -- $(toast_geo)
assert_eq "$2" HEADLESS-1 "on the focused output"
x=${3%,*}; y=${3#*,}; w=${4%x*}; h=${4#*x}
assert_eq "$w" 360 "theme width"
assert_eq "$((x + w))" $((1920 - 8)) "8px from the right edge"
assert_eq "$y" $((32 + 8)) "under the bar, 8px down"
[ "$h" -gt 40 ] || { echo "toast too short: $h"; exit 1; }
first_h=$h
assert_eq "$(count_widgets 'notification#'"$id1"' icon')" 1 "icon drawn"
assert_eq "$(count_widgets 'notification body')" 1
shot toast-1 "$x,$y ${w}x$h"

# A second one stacks above the first (newest nearest the corner);
# critical: stays, urgent border.
id2=$(nsend -p -u critical "Second" "short")
settle
assert_eq "$(count_widgets 'notification')" 2
assert_eq "$(count_widgets 'notification.critical')" 1
set -- $(toast_geo 1)
y1=${3#*,}
set -- $(toast_geo 2)
y2=${3#*,}; h2=${4#*x}
assert_eq "$y2" 40 "the newest at the corner"
assert_eq "$y1" $((y2 + h2 + 8)) "the first pushed down by it, with the gap"
set -- $(widget 'notification#'"$id2"' summary')
assert_eq "$2" $((40 + 10)) "newest on top (summary inside its padding)"
shot toasts-2 "0,0 1920x300"

# Replacing keeps the id and the place, changes the text and size.
nsend -r "$id1" -t 0 "First replaced" "tiny"
settle
assert_eq "$(count_widgets 'notification')" 2 "still two"
assert_eq "$(count_widgets 'notification#'"$id1"'')" 1 "same id"
set -- $(toast_geo 1)
h=${4#*x}
[ "$h" -lt "$first_h" ] || { echo "replacement didn't shrink: $first_h -> $h"; exit 1; }

# CloseNotification removes it; the other moves up.
call org.freedesktop.Notifications.CloseNotification "$id1" > /dev/null
settle
assert_eq "$(count_widgets 'notification')" 1
set -- $(toast_geo)
assert_eq "${3#*,}" 40 "the survivor took the first place"

# A right click dismisses.
click_widget_with right 'notification#'"$id2"' summary'
assert_eq "$(count_widgets 'notification')" 0 "dismissed"

# Actions: buttons, the clicked key goes back to the client; the
# default action on a click on the body.
nsend -t 0 -A default=Open -A later=Later "With actions" "pick one" > "$ARIA_UI_OUT/action" &
settle 0.5
assert_eq "$(count_widgets 'notification actions button')" 1 "default has no button"
click_widget 'notification actions button'
settle
wait
assert_eq "$(cat "$ARIA_UI_OUT/action")" later "the action key reached the client"
assert_eq "$(count_widgets 'notification')" 0 "closed after the action"
nsend -t 0 -A default=Open "Click me" "" > "$ARIA_UI_OUT/action" &
settle 0.5
click_widget 'notification summary'
settle
wait
assert_eq "$(cat "$ARIA_UI_OUT/action")" default "a click invokes default"

# Expiry: the app's timeout, then the configured 2s default.
nsend -t 500 "Quick" "gone in half a second" > /dev/null
settle
assert_eq "$(count_widgets 'notification')" 1
settle 0.8
assert_eq "$(count_widgets 'notification')" 0 "expired on the app's timeout"
nsend "Default timeout" "gone in 2s" > /dev/null
settle 1
assert_eq "$(count_widgets 'notification')" 1
settle 1.5
assert_eq "$(count_widgets 'notification')" 0 "expired on the configured duration"

# Markup is shown as text; an image file and image-data draw an icon.
nsend -t 0 "Markup" "<b>bold</b> &amp; <i>plain</i>" > /dev/null
settle
png=$(find /usr/share/icons -name '*.png' -path '*48*' | head -n 1)
[ -n "$png" ] || png=$(find /usr/share/icons -name '*.png' | head -n 1)
nsend -t 0 -i "$png" "File icon" "from a path" > /dev/null
call org.freedesktop.Notifications.Notify "app" 0 "" "Pixels" "image-data" '[]' \
    "{'image-data': <(2, 2, 8, true, 8, 4, @ay [255,0,0,255, 0,255,0,255, 0,0,255,255, 255,255,255,255])>}" 0 > /dev/null
settle
assert_eq "$(count_widgets 'notification')" 3
assert_eq "$(count_widgets 'notification icon')" 2 "file and pixel icons"
shot toasts-icons "0,0 1920x500"
# Sanity on the text: the markup toast is as tall as a one-line body.
set -- $(toast_geo 3)
h=${4#*x}
[ "$h" -lt 90 ] || { echo "markup toast too tall: $h"; exit 1; }
