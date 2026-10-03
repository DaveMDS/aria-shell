# Idle: the screens go off after screen_off and come back at the first
# input, the session locks after lock (screens off by then: the input
# wakes them on the lock screen), and nothing goes idle while held by
# the Power gadget's eye, by `aria-shell idle inhibit` or by a media
# player playing.
# tests/ui/config-idle has the timeouts (2s, 4s). No logind or UPower on
# the scenario's bus: the sleep lock and the battery timeouts aren't
# exercised here.

restart_shell "$ARIA_UI_ROOT/tests/ui/config-idle"
mpris_start
button='panel[output="HEADLESS-1"] gadget.power > button.idle'
outputs_on() {
    swaymsg -t get_outputs | grep -c '"power": true'
}
assert_contains "$(aria debug idle)" "armed=lock=4s,screen_off=2s"
assert_eq "$(count_widgets 'gadget.power > button.idle')" 2 "an idle button per bar"

# The screens go off, then the first input wakes them: no lock yet.
sleep 2.5
assert_contains "$(aria debug idle)" "screens=off"
assert_eq "$(outputs_on)" 0 "both outputs off"
assert_no_surface locker
pointer 300 300; settle 0.5
assert_contains "$(aria debug idle)" "screens=on"
assert_eq "$(outputs_on)" 2 "both outputs back on"
assert_no_surface locker

# Left alone: screens off, then the lock on them; an input shows it.
sleep 4.5
assert_logged "idle: lock reached"
assert_surface locker
pointer 400 400; settle 0.5
assert_eq "$(outputs_on)" 2 "both outputs back on"
assert_surface locker
key Return; settle 0.5
assert_no_surface locker

# Held by the eye: no timer armed, nothing happens; held is drawn.
click_widget "$button"
assert_contains "$(aria debug idle)" "inhibited=true"
assert_contains "$(aria debug idle)" "armed=none"
assert_eq "$(count_widgets 'gadget.power > button.idle.inhibited')" 2 "held, on both bars"
shot_surface held panel
sleep 4.5
assert_contains "$(aria debug idle)" "screens=on"
assert_no_surface locker
click_widget "$button"
assert_contains "$(aria debug idle)" "inhibited=false"
assert_contains "$(aria debug idle)" "armed=lock=4s,screen_off=2s"
assert_eq "$(count_widgets 'gadget.power > button.idle.inhibited')" 0 "let go"

# The same from the command line.
aria idle inhibit on; settle
assert_contains "$(aria debug idle)" "inhibited=true"
aria idle inhibit; settle
assert_contains "$(aria debug idle)" "inhibited=false"
aria idle inhibit toggle; settle
assert_contains "$(aria debug idle)" "inhibited=true"
aria idle inhibit off; settle
assert_contains "$(aria debug idle)" "armed=lock=4s,screen_off=2s"
case "$(aria idle inhibit sideways 2>&1)" in
    *"invalid arguments for <idle>"*) ;;
    *) echo "a bad argument was accepted"; exit 1 ;;
esac

# A player playing holds too, and lets go when paused.
mpris_send "status Playing"
assert_contains "$(aria debug idle)" "playing=true"
assert_contains "$(aria debug idle)" "armed=none"
assert_eq "$(count_widgets 'gadget.power > button.idle.playing')" 2 "held by the player"
mpris_send "status Paused"
assert_contains "$(aria debug idle)" "armed=lock=4s,screen_off=2s"
mpris_stop
