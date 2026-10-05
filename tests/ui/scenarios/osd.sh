# The OSD: `aria-shell osd show` puts a bar at the bottom of every
# output, a second one updates it in place and starts the timer again,
# and it goes `duration` after the last (1 s in the test config). The
# network going up and down shows by itself, through the fake
# NetworkManager; the first reading (it coming up) shows nothing. The
# volume is the machine's own mixer here: not driven, not asserted.

count_osd() {
    surfaces | tr ';' '\n' | grep -c '^ *osd ' || true
}

# Until n OSD surfaces are open (3 s).
wait_osd() {
    i=0
    while [ "$(count_osd)" != "$1" ] && [ $i -lt 30 ]; do
        sleep 0.1; i=$((i + 1))
    done
    assert_eq "$(count_osd)" "$1" "$2"
}

# Until the selector matches n widgets (the bus is async).
wait_for() {
    i=0
    while [ "$(count_widgets "$1")" != "$2" ] && [ $i -lt 30 ]; do
        sleep 0.1; i=$((i + 1))
    done
    assert_eq "$(count_widgets "$1")" "$2" "$3"
}

# --- from a script -------------------------------------------------------------
assert_eq "$(count_osd)" 0 "no OSD at rest"
aria osd show --icon audio-volume-high-symbolic --value 40 Test
wait_osd 2 "one OSD per output"
# 320x56 (base.css), centred, 100 px above the bottom edge.
assert_contains "$(surfaces)" "osd HEADLESS-1 800,924 320x56"
assert_contains "$(surfaces)" "osd HEADLESS-2 2720,924 320x56"
assert_eq "$(widget 'osd[output="HEADLESS-1"]')" "800 924 320 56" "the box fills it"
assert_eq "$(count_widgets 'osd.custom icon')" 2 "the icon"
assert_eq "$(count_widgets 'osd.custom meter')" 2 "the level"
assert_eq "$(count_widgets 'osd.custom label')" 2 "the text"
assert_eq "$(count_widgets 'osd.custom value')" 2 "the percent"
settle 0.3
shot_surface osd-custom osd

# Another one while it shows: the same surfaces, the new content.
aria osd show --value 75
settle 0.2
assert_eq "$(count_osd)" 2 "still one per output"
assert_eq "$(count_widgets 'osd.custom label')" 0 "no text this time"
assert_eq "$(count_widgets 'osd.custom meter')" 2
sleep 0.6
assert_eq "$(count_osd)" 2 "the timer started again with the second"
wait_osd 0 "gone after its duration"

# Nothing to show is refused.
if aria osd show 2> /dev/null; then
    echo "osd show with nothing to show was accepted"; exit 1
fi
assert_eq "$(count_osd)" 0

# --- the network -----------------------------------------------------------------
nm_start
wait_for 'gadget.network > button.disconnected' 2 "NetworkManager known, nothing connected"
settle 0.5
assert_eq "$(count_osd)" 0 "its first reading shows nothing"
nm_send "wired Office"
nm_send "wired-up"
wait_osd 2 "connected: the OSD"
assert_eq "$(count_widgets 'osd.network label')" 2 "with the connection's name"
assert_eq "$(count_widgets 'osd.network meter')" 0 "and no level"
shot_surface osd-network osd
wait_osd 0

# Down, from the gadget's popup: the OSD doesn't care who did it.
click_widget 'panel[output="HEADLESS-1"] gadget.network > button'
wait_for 'popup gadget.network device.wired.active' 1 "eth0 connected"
click_widget 'popup gadget.network device.wired > button'
wait_for 'popup gadget.network device.expanded actions button.disconnect' 1
click_widget 'popup gadget.network device.expanded actions button.disconnect'
wait_osd 2 "disconnected: the OSD"
assert_eq "$(count_widgets 'osd.network label')" 2
shot_surface osd-disconnected osd
nm_stop
