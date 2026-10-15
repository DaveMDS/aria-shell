# The OSD: `aria-shell osd show` puts a bar at the bottom of every
# output, a second one updates it in place and starts the timer again,
# and it goes `duration` after the last (1 s in the test config). What
# it watches shows by itself, whoever changed it: the network going up
# and down, Wi‑Fi on and off, a VPN (the fake NetworkManager), keep
# awake (`aria-shell idle inhibit`), the charger and the power profile
# (the fake UPower); the first reading of each shows nothing. The
# volume and the microphone are the machine's own mixer here: not
# driven, not asserted.

count_osd() {
    surfaces | tr ';' '\n' | grep -c '^ *osd ' || true
}

# Until `$2` ms have gone since `$1` (`date +%s%N`).
wait_since() {
    while [ $((($(date +%s%N) - $1) / 1000000)) -lt "$2" ]; do sleep 0.05; done
}

# Until n OSD surfaces are open (3 s).
wait_osd() {
    i=0
    while [ "$(count_osd)" != "$1" ] && [ $i -lt 30 ]; do
        sleep 0.1; i=$((i + 1))
    done
    assert_eq "$(count_osd)" "$1" "$2"
}

# --- from a script -------------------------------------------------------------
assert_eq "$(count_osd)" 0 "no OSD at rest"
aria osd show --icon audio-volume-high-symbolic --value 40 Test
first=$(date +%s%N)
wait_osd 2 "one OSD per output"
# 320x56 (aria.css), centred, 100 px above the bottom edge.
assert_contains "$(surfaces)" "osd HEADLESS-1 800,924 320x56"
assert_contains "$(surfaces)" "osd HEADLESS-2 2720,924 320x56"
assert_eq "$(widget 'surface.osd[output="HEADLESS-1"]')" "800 924 320 56" "the box fills it"
wait_for 'surface.osd.custom icon' 2 "the icon"
wait_for 'surface.osd.custom meter' 2 "the level"
wait_for 'surface.osd.custom label' 2 "the text"
wait_for 'surface.osd.custom value' 2 "the percent"
shot_surface osd-custom osd

# Another one while it shows, half a second after the first: the same
# surfaces, the new content. 0.7 s after it, the first one's second is
# over, the second one's isn't: counted, the asserts in between take
# what they take.
wait_since "$first" 500
aria osd show --value 75
second=$(date +%s%N)
settle 0.2
assert_eq "$(count_osd)" 2 "still one per output"
wait_for 'surface.osd.custom label' 0 "no text this time"
wait_for 'surface.osd.custom meter' 2
wait_since "$second" 700
assert_eq "$(count_osd)" 2 "the timer started again with the second"
wait_osd 0 "gone after its duration"

# Nothing to show is refused.
if aria osd show 2> /dev/null; then
    echo "osd show with nothing to show was accepted"; exit 1
fi
assert_eq "$(count_osd)" 0

# --- the network -----------------------------------------------------------------
nm_start
wait_for 'gadget.network > indicator.disconnected' 2 "NetworkManager known, nothing connected"
settle 0.5
assert_eq "$(count_osd)" 0 "its first reading shows nothing"
nm_send "wired Office"
nm_send "wired-up"
wait_osd 2 "connected: the OSD"
wait_for 'surface.osd.network label' 2 "with the connection's name"
wait_for 'surface.osd.network meter' 0 "and no level"
shot_surface osd-network osd
wait_osd 0

# Down, from the gadget's popup: the OSD doesn't care who did it.
click_widget 'surface.panel[output="HEADLESS-1"] gadget.network > indicator'
wait_for 'surface.popup gadget.network device.wired.active' 1 "eth0 connected"
click_widget 'surface.popup gadget.network device.wired > button'
wait_for 'surface.popup gadget.network device.expanded actions button.disconnect' 1
click_widget 'surface.popup gadget.network device.expanded actions button.disconnect'
assert_eq "$(nm_action)" "disconnect eth0"
wait_osd 2 "disconnected: the OSD"
wait_for 'surface.osd.network label' 2
shot_surface osd-disconnected osd
click 600 600
wait_osd 0

# Wi‑Fi off and on again, from the gadget's middle click.
click_widget_with middle 'surface.panel[output="HEADLESS-1"] gadget.network > indicator'
assert_eq "$(nm_action)" "wireless false"
wait_for 'surface.osd.wifi label' 2 "Wi‑Fi off: the OSD"
shot_surface osd-wifi osd
wait_osd 0
click_widget_with middle 'surface.panel[output="HEADLESS-1"] gadget.network > indicator'
assert_eq "$(nm_action)" "wireless true"
wait_for 'surface.osd.wifi label' 2 "Wi‑Fi on: the OSD"
wait_osd 0

# A VPN up, from the popup's toggle.
nm_send "vpn Office"
click_widget 'surface.panel[output="HEADLESS-1"] gadget.network > indicator'
wait_for 'surface.popup gadget.network vpn[name="Office"]' 1 "the VPN row"
click_widget 'surface.popup gadget.network vpn[name="Office"] toggle'
assert_eq "$(nm_action)" "activate Office"
nm_send finish
wait_for 'surface.osd.vpn label' 2 "VPN connected: the OSD"
shot_surface osd-vpn osd
click 600 600
wait_osd 0
nm_stop

# --- keep awake ------------------------------------------------------------------
aria idle inhibit on
wait_for 'surface.osd.idle label' 2 "keep awake on: the OSD"
aria idle inhibit off
wait_for 'surface.osd.idle label' 2 "and off"
wait_osd 0

# --- the charger and the power profile ----------------------------------------------
# Off the charger before the battery shows: the charger is unknown
# without one, so the battery coming is its first reading.
upower_start
upower_send "ac off"
upower_send "battery 64 discharging 7800"
upower_send "profiles on"
settle 0.5
assert_eq "$(count_osd)" 0 "UPower's first reading shows nothing"
assert_eq "$(grep -c 'osd: Content { kind: \(Charger\|Profile\)' "$ARIA_UI_OUT/shell.log")" 0 \
    "not even briefly"
upower_send "ac on"
wait_for 'surface.osd.charger label' 2 "charger connected: the OSD, with the charge"
shot_surface osd-charger osd
wait_osd 0
upower_send "profile power-saver"
wait_for 'surface.osd.profile label' 2 "another profile: the OSD"
shot_surface osd-profile osd
upower_stop
