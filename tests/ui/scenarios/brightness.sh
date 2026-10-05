# The Brightness gadget and `aria-shell brightness`, over a fake ddcutil
# (tests/ui/bin/ddcutil: a monitor on HEADLESS-1 at 70%, one on HEADLESS-2
# at 30%, a third that doesn't answer): the screens found and read, the
# first reading silent; the command steps every screen, or the one
# `--output` names, and the OSD shows on the screens that changed; a
# step down stops short of dark; the wheel on a bar steps both; the
# popup (a slider per screen) reads the monitors again, so a change
# made with a monitor's own buttons shows, and its slider sets one;
# Settings runs settings_command.

restart_shell "$ARIA_UI_ROOT/tests/ui/config-brightness"

monitor() {
    cat "$ARIA_UI_OUT/ddc/$1"
}

# Until the fake monitor on bus $1 is at $2 (3 s): writes are async.
wait_monitor() {
    i=0
    while [ "$(monitor "$1")" != "$2" ] && [ $i -lt 30 ]; do
        sleep 0.1; i=$((i + 1))
    done
    assert_eq "$(monitor "$1")" "$2" "monitor on bus $1"
}

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

# --- found and read ------------------------------------------------------
wait_for 'gadget.brightness > button > text' 2 "the percent on both bars"
b=$(aria debug brightness)
assert_contains "$b" 'ddc:3 output=HEADLESS-1 model="Fake Monitor" 70%'
assert_contains "$b" 'ddc:4 output=HEADLESS-2 model="Fake Monitor" 30%'
assert_not_contains "$b" "ddc:5" "the monitor that doesn't answer"
assert_eq "$(count_osd)" 0 "the first reading shows nothing"
shot bars

# --- the command -----------------------------------------------------------
aria brightness up
wait_monitor 3 75
wait_monitor 4 35
wait_osd 2 "an OSD on each screen"
assert_eq "$(count_widgets 'osd.brightness meter')" 2 "with its level"
shot_surface osd-brightness osd
wait_osd 0 "gone after its duration"

aria brightness set 50 --output HEADLESS-2
wait_monitor 4 50
assert_eq "$(monitor 3)" 75 "the other screen untouched"
wait_osd 1 "on that screen only"
assert_contains "$(surfaces)" "osd HEADLESS-2"
wait_osd 0

# A step down stops short of dark; `set` goes there.
aria brightness set 3 --output HEADLESS-1
wait_monitor 3 3
aria brightness down --output HEADLESS-1
wait_monitor 3 1
aria brightness set 0 --output HEADLESS-1
wait_monitor 3 0
aria brightness set 75 --output HEADLESS-1
wait_monitor 3 75

# A burst (a held key): each step counts, the monitor ends at the last.
for i in 1 2 3 4 5 6; do aria brightness up 5 --output HEADLESS-2; done
wait_monitor 4 80

if aria brightness set 140 2> /dev/null; then
    echo "a percent over 100 was accepted"; exit 1
fi
wait_osd 0

# --- the wheel ---------------------------------------------------------------
geo=$(widget 'panel[output="HEADLESS-1"] gadget.brightness > button')
set -- $geo
pointer $(($1 + $3 / 2)) $(($2 + $4 / 2))
scroll -2
wait_monitor 3 85
wait_monitor 4 90
scroll 1
wait_monitor 3 80
wait_monitor 4 85
wait_osd 0

# --- the popup -------------------------------------------------------------
# The monitor's own buttons: unseen until the popup reads again.
echo 20 > "$ARIA_UI_OUT/ddc/3"
settle
assert_contains "$(aria debug brightness)" "ddc:3 output=HEADLESS-1 model=\"Fake Monitor\" 80%"
click_widget 'panel[output="HEADLESS-1"] gadget.brightness > button'
assert_surface popup
assert_eq "$(count_widgets 'popup gadget.brightness screen.ddc')" 2 "a row per monitor"
assert_eq "$(count_widgets 'popup gadget.brightness screen slider')" 2 "a slider each"
i=0
while ! aria debug brightness | grep -q 'ddc:3 [^;]* 20%' && [ $i -lt 30 ]; do
    sleep 0.1; i=$((i + 1))
done
assert_contains "$(aria debug brightness)" "ddc:3 output=HEADLESS-1 model=\"Fake Monitor\" 20%" "read again"
settle
shot_surface popup popup

# A slider: a click on its middle.
click_widget 'popup gadget.brightness screen[output="HEADLESS-2"] slider'
wait_monitor 4 50
assert_eq "$(monitor 3)" 20 "the other one untouched"

# Settings runs settings_command and closes the popup.
click_widget 'popup gadget.brightness button.settings'
assert_no_surface popup
assert_eq "$(cat "$ARIA_UI_OUT/brightness-settings")" "settings" "settings_command ran"
