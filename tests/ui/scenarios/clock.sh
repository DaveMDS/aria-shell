# The clock's calendar popup: opens under the clock, shows today,
# navigates months, closes on a click outside; on both outputs.

for output in HEADLESS-1 HEADLESS-2; do
    assert_no_surface popup
    click_widget "panel[output=\"$output\"] slot.center gadget.clock > button"
    assert_surface popup
    shot_surface "popup-$output" popup
    assert_eq "$(count_widgets 'calendar > day.today')" 1 "today is shown"
    assert_eq "$(count_widgets 'calendar > weekday')" 7 "seven weekday headers"

    # The popup sits below the bar, centred on the clock.
    set -- $(widget "panel[output=\"$output\"] slot.center gadget.clock > button")
    clock_cx=$(($1 + $3 / 2))
    set -- $(widget "popup[output=\"$output\"] calendar")
    popup_cx=$(($1 + $3 / 2))
    [ $((popup_cx - clock_cx)) -le 2 ] && [ $((clock_cx - popup_cx)) -le 2 ] \
        || { echo "popup centre $popup_cx, clock centre $clock_cx"; exit 1; }

    click_widget 'calendar > header > button.next'
    assert_eq "$(count_widgets 'calendar > day.today')" 0 "next month has no today"
    click_widget 'calendar > header > button.prev'
    assert_eq "$(count_widgets 'calendar > day.today')" 1 "back to this month"

    click 600 600
    assert_no_surface popup
done

# A click on the bar itself, beside the gadgets, closes the popup too
# (Sway delivers it to us instead of dismissing the popup), and so does
# opening another gadget's popup: one at a time.
click_widget 'panel[output="HEADLESS-1"] slot.center gadget.clock > button'
assert_surface popup
click 600 16
assert_no_surface popup

click_widget 'panel[output="HEADLESS-1"] slot.center gadget.clock > button'
assert_surface popup
click_widget 'panel[output="HEADLESS-1"] gadget.notifications > button'
assert_eq "$(surfaces | tr ';' '\n' | grep -c popup)" 1 "one popup at a time"
assert_eq "$(count_widgets 'popup calendar')" 0 "the calendar's popup is gone"
click 600 600
assert_no_surface popup
