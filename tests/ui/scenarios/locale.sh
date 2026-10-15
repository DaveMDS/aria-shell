# The UI language: the shared config pins English; a second shell on
# tests/ui/config-locale is Italian. `debug widgets` doesn't show
# texts, so the asserted part is `debug locale`; the screenshots are
# for the eye (the lock screen, the calendar, the notifications and
# system monitor popups in Italian).

assert_eq "$(aria debug locale)" "lang=en dates=en_US" "the scenarios run in English"

restart_shell "$ARIA_UI_ROOT/tests/ui/config-locale"
assert_eq "$(aria debug locale)" "lang=it dates=it_IT" "the second shell is Italian"

# The calendar: month and weekday names.
click_widget 'surface.panel[output="HEADLESS-1"] gadget.clock > indicator'
assert_surface popup
shot_surface locale-calendar popup
click 300 700

# The notifications popup: title, buttons, "no notifications".
click_widget 'surface.panel[output="HEADLESS-1"] gadget.notifications > indicator'
assert_surface popup
shot_surface locale-notifications popup
click 300 700

# The system monitor: tabs and details.
click_widget 'surface.panel[output="HEADLESS-1"] gadget#cpu.system-monitor > indicator'
assert_surface popup
shot_surface locale-sysmon popup
click 300 700

# The launcher: the test entry's Name[it]/Comment[it] are what shows,
# and what the search matches ("prova" is only in the Italian texts).
aria launcher show; wait_keyboard launcher
type_text "prova"
wait_for 'surface.launcher item' 1 "the Italian name matches"
shot_surface locale-launcher launcher
key Escape
assert_no_surface launcher

# The lock screen.
aria lock; wait_keyboard locker
assert_surface locker
shot locale-locker
key Return; settle 0.8
assert_no_surface locker
