# The clock's calendar popup: opens under the clock, shows today,
# navigates months, closes on a click outside; on both outputs.

# The clock shares the centre slot with the system monitor, whose memory
# gauge widens with its first reading (the percent appears), about a
# second after the start: the slot re-centres and the clock moves 4 px
# left, away from a popup opened before. The net value turning critical
# (`critical = 0`, .danger) says the reading is in, on each bar.
wait_for 'gadget#net.system-monitor > indicator.net.danger' 2 "the first system reading"

for output in HEADLESS-1 HEADLESS-2; do
    assert_no_surface popup
    click_widget "surface.panel[output=\"$output\"] slot.center gadget.clock > indicator"
    assert_surface popup
    shot_surface "popup-$output" popup
    wait_for 'calendar day.today' 1 "today is shown"
    wait_for 'calendar > weekdays > weekday' 7 "seven weekday headers"

    # The popup sits right below the bar (not the button), centred on
    # the clock.
    set -- $(widget "surface.panel[output=\"$output\"] slot.center gadget.clock > indicator")
    clock_cx=$(($1 + $3 / 2))
    set -- $(widget "surface.popup[output=\"$output\"] calendar")
    popup_cx=$(($1 + $3 / 2))
    [ $((popup_cx - clock_cx)) -le 2 ] && [ $((clock_cx - popup_cx)) -le 2 ] \
        || { echo "popup centre $popup_cx, clock centre $clock_cx"; exit 1; }
    set -- $(widget "surface.panel[output=\"$output\"]")
    bar_bottom=$(($2 + $4))
    set -- $(widget "surface.popup[output=\"$output\"]")
    assert_eq "$2" "$bar_bottom" "popup top at the bar's bottom"

    click_widget 'calendar > header > button.next'
    wait_for 'calendar day.today' 0 "next month has no today"
    click_widget 'calendar > header > button.previous'
    wait_for 'calendar day.today' 1 "back to this month"

    click 600 600
    assert_no_surface popup
done

# A click on the bar itself, beside the gadgets, closes the popup too
# (Sway delivers it to us instead of dismissing the popup), and so does
# opening another gadget's popup: one at a time.
click_widget 'surface.panel[output="HEADLESS-1"] slot.center gadget.clock > indicator'
assert_surface popup
click 600 16
assert_no_surface popup

click_widget 'surface.panel[output="HEADLESS-1"] slot.center gadget.clock > indicator'
assert_surface popup
click_widget 'surface.panel[output="HEADLESS-1"] gadget.notifications > indicator'
assert_eq "$(surfaces | tr ';' '\n' | grep -c popup)" 1 "one popup at a time"
wait_for 'surface.popup calendar' 0 "the calendar's popup is gone"
click 600 600
assert_no_surface popup

# The pointer resting on it: the tip in `tooltip_format`, centred on
# it; away, gone. Clock:2 has no `tooltip_format`: no tip.
pointer 960 600
assert_no_surface tooltip
set -- $(widget 'surface.panel[output="HEADLESS-1"] slot.center gadget.clock > indicator')
clock_cx=$(($1 + $3 / 2))
pointer $clock_cx $(($2 + $4 / 2))
assert_surface tooltip
set -- $(widget 'surface.tooltip')
[ $(($1 + $3 / 2 - clock_cx)) -le 1 ] && [ $((clock_cx - $1 - $3 / 2)) -le 1 ] || { echo "the tip not centred on the clock ($clock_cx): $*"; exit 1; }
shot tip "$(($1 - 30)),$(($2 - 20)) $(($3 + 60))x$(($4 + 60))"
pointer 960 600
assert_no_surface tooltip
set -- $(widget 'surface.panel[output="HEADLESS-1"] slot.end gadget.clock > indicator')
pointer $(($1 + $3 / 2)) $(($2 + $4 / 2))
settle 1
assert_no_surface tooltip
pointer 960 600
