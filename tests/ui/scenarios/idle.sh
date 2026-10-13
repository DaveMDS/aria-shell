# Idle: the screens dim after dim (the fake monitors at 15% of their
# level, no OSD), go off after screen_off and come back at the first
# input, at their level; the session locks after lock (screens off by
# then: the input wakes them on the lock screen), and nothing goes idle
# while held by the Power gadget's eye, by `aria-shell idle inhibit` or
# by a media player playing.
# tests/ui/config-idle has the timeouts (1s, 2s, 4s). No logind or UPower on
# the scenario's bus: the sleep lock and the battery timeouts aren't
# exercised here.

restart_shell "$ARIA_UI_ROOT/tests/ui/config-idle"
mpris_start
button='surface.panel[output="HEADLESS-1"] gadget.power > button.idle'
outputs_on() {
    swaymsg -t get_outputs | grep -c '"power": true'
}
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
# Until `debug idle` says $1 (3 s).
wait_idle() {
    i=0
    while ! aria debug idle | grep -q "$1" && [ $i -lt 60 ]; do
        sleep 0.05; i=$((i + 1))
    done
    assert_contains "$(aria debug idle)" "$1"
}
assert_contains "$(aria debug idle)" "armed=dim=1s,lock=4s,screen_off=2s"
wait_for 'gadget.power > button.idle' 2 "an idle button per bar"

# The monitors read (70% and 30%), then an input: the timers count
# from it.
i=0
while ! aria debug brightness | grep -q "ddc:4 .* 30%" && [ $i -lt 60 ]; do
    sleep 0.05; i=$((i + 1))
done
pointer 200 200

# The screens dim, then go off; the first input wakes them, at their
# level: no lock yet, no OSD.
wait_idle "dimmed=true"
wait_monitor 3 11
wait_monitor 4 5
assert_no_surface osd
wait_idle "screens=off"
assert_eq "$(outputs_on)" 0 "both outputs off"
assert_no_surface locker
pointer 300 300; settle 0.5
assert_contains "$(aria debug idle)" "screens=on"
assert_contains "$(aria debug idle)" "dimmed=false"
assert_eq "$(outputs_on)" 2 "both outputs back on"
wait_monitor 3 70
wait_monitor 4 30
assert_no_surface osd
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
wait_for 'gadget.power > button.idle.active' 2 "held, on both bars"
shot_surface held panel
sleep 4.5
assert_contains "$(aria debug idle)" "screens=on"
assert_no_surface locker
click_widget "$button"
assert_contains "$(aria debug idle)" "inhibited=false"
assert_contains "$(aria debug idle)" "armed=dim=1s,lock=4s,screen_off=2s"
wait_for 'gadget.power > button.idle.active' 0 "let go"

# The same from the command line.
aria idle inhibit on; settle
assert_contains "$(aria debug idle)" "inhibited=true"
aria idle inhibit; settle
assert_contains "$(aria debug idle)" "inhibited=false"
aria idle inhibit toggle; settle
assert_contains "$(aria debug idle)" "inhibited=true"
aria idle inhibit off; settle
assert_contains "$(aria debug idle)" "armed=dim=1s,lock=4s,screen_off=2s"
case "$(aria idle inhibit sideways 2>&1)" in
    *"invalid arguments for <idle>"*) ;;
    *) echo "a bad argument was accepted"; exit 1 ;;
esac

# A player playing holds too, and lets go when paused.
mpris_send "status Playing"
assert_contains "$(aria debug idle)" "playing=true"
assert_contains "$(aria debug idle)" "armed=none"
wait_for 'gadget.power > button.idle.active' 0 "the eye is the user's alone"
mpris_send "status Paused"
assert_contains "$(aria debug idle)" "armed=dim=1s,lock=4s,screen_off=2s"
mpris_stop
