# New frames (Message::redraw_scope): what changes on screen is drawn
# again on the surfaces that show it. The other scenarios read the
# widget tree, rebuilt for every surface whatever was drawn, so only
# screenshots see a stale frame: two shots of the same region before
# and after a change must differ (a clock tick on each bar; a hover
# coming and going, which iced only draws on a surface drawn since the
# last message; the system monitor's readings and a player's title in
# their popups; the scheme from one bar on the other), and between two
# ticks a region that shows nothing changing must not.

differs() {
    cmp -s "$ARIA_UI_OUT/$1.png" "$ARIA_UI_OUT/$2.png" && { echo "$3: the same frame"; return 1; }
    return 0
}
same() {
    cmp -s "$ARIA_UI_OUT/$1.png" "$ARIA_UI_OUT/$2.png" || { echo "$3: a different frame"; return 1; }
}
# `x,y wxh` of the first widget the selector matches, for `shot`.
region() {
    geo=$(widget "$1") || { echo "$geo"; return 1; }
    set -- $geo
    echo "$1,$2 ${3}x$4"
}

# A clock tick, a bar's own message: each bar's clock.
for out in HEADLESS-1 HEADLESS-2; do
    r=$(region "panel[output=\"$out\"] gadget.clock")
    shot "clock-$out-1" "$r"
    sleep 1.2
    shot "clock-$out-2" "$r"
    differs "clock-$out-1" "clock-$out-2" "clock tick on $out"
done

# A hover: the widget's own redraw request.
themes='panel[output="HEADLESS-1"] gadget.themes > button'
r=$(region "$themes")
pointer 900 600
settle 0.5
shot hover-1 "$r"
set -- $(widget "$themes")
pointer $(($1 + $3 / 2)) $(($2 + $4 / 2))
settle 0.5
shot hover-2 "$r"
differs hover-1 hover-2 "hover on the themes button"
pointer 900 600
settle 0.5
shot hover-3 "$r"
same hover-1 hover-3 "the hover gone with the pointer"

# Shared state reaching a popup: the system monitor's readings (every
# second)...
click_widget 'panel[output="HEADLESS-1"] gadget#cpu.system-monitor > button.cpu'
assert_surface popup
settle 1
shot_surface sysmon-1 popup
sleep 2
shot_surface sysmon-2 popup
differs sysmon-1 sysmon-2 "system monitor popup after two readings"
click 600 600
assert_no_surface popup

# ... and a player's change, from the bus.
mpris_start
click_widget 'panel[output="HEADLESS-1"] gadget.audio > button.output'
assert_surface popup
i=0
while [ "$(count_widgets 'popup player')" != 1 ] && [ $i -lt 30 ]; do
    sleep 0.1
    i=$((i + 1))
done
settle 1
shot_surface player-1 popup
mpris_send "title A completely different title"
settle 1
shot_surface player-2 popup
differs player-1 player-2 "player title in the popup"
click 600 600
assert_no_surface popup
mpris_stop

# The scheme, an action redrawing everything, from the first bar: the
# start of the second one (static texts, no clock) stays put between
# ticks, then turns dark.
r="1920,0 300x32"
pointer 900 600
settle 1
shot other-1 "$r"
sleep 1.2
shot other-2 "$r"
same other-1 other-2 "the second bar's start between two ticks"
click_widget "$themes"
pointer 900 600
settle 1
shot other-3 "$r"
differs other-2 other-3 "dark scheme on the second bar"
