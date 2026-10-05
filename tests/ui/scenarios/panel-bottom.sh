# A bar at the bottom (a second shell on tests/ui/config-bottom): the
# clock's popup opens above it, its box meeting the bar's top edge,
# centred on the clock; on both outputs.

restart_shell "$ARIA_UI_ROOT/tests/ui/config-bottom"

for output in HEADLESS-1 HEADLESS-2; do
    assert_no_surface popup
    click_widget "panel[output=\"$output\"] gadget.clock > button"
    assert_surface popup
    shot_surface "popup-$output" popup

    set -- $(widget "panel[output=\"$output\"] gadget.clock > button")
    clock_cx=$(($1 + $3 / 2))
    set -- $(widget "popup[output=\"$output\"] calendar")
    popup_cx=$(($1 + $3 / 2))
    [ $((popup_cx - clock_cx)) -le 2 ] && [ $((clock_cx - popup_cx)) -le 2 ] \
        || { echo "popup centre $popup_cx, clock centre $clock_cx"; exit 1; }
    set -- $(widget "panel[output=\"$output\"]")
    bar_top=$2
    set -- $(widget "popup[output=\"$output\"]")
    assert_eq "$(($2 + $4))" "$bar_top" "popup bottom at the bar's top"

    click 600 300
    assert_no_surface popup
done
