# The Power gadget, on a fake UPower and power-profiles-daemon
# (tests/ui/upower): only the eye while there's neither a battery nor
# profiles; the battery with its percent and the profile once they
# come; the popup (the battery's state and time, draw and health, a
# peripheral, the profile picker, keep awake, Settings); a profile
# picked reaches the daemon, one it switches to shows; the charger
# switches idle to its battery timeouts; UPower's low then critical
# warning is one notification, replaced, closed once charging.

upower_start
restart_shell "$ARIA_UI_ROOT/tests/ui/config-power"
status='surface.panel[output="HEADLESS-1"] gadget.power > button.status'

# Nothing to show but the eye.
assert_contains "$(aria debug power)" "battery=none"
assert_contains "$(aria debug power)" "profile=none"
wait_for 'gadget.power > button.status' 0 "no battery, no profile: no status"
wait_for 'gadget.power > button.idle' 2 "the eye on both bars"

# A battery, unplugged, and the profiles daemon.
upower_send "battery 64 discharging 7800"
upower_send "ac off"
upower_send "profiles on"
upower_send "device MX_Master 40"
assert_contains "$(aria debug power)" "battery=64% discharging"
assert_contains "$(aria debug power)" "profile=balanced available=power-saver,balanced,performance"
wait_for 'gadget.power > button.status.discharging.balanced' 2 "battery and profile on both bars"
wait_for 'gadget.power > button.status > label' 2 "the percent"
assert_contains "$(aria debug idle)" "power=battery"
assert_contains "$(aria debug idle)" "armed=lock=30m"

# The popup.
click_widget "$status"
assert_surface popup
shot_surface popup popup
wait_for 'surface.popup gadget.power battery.discharging' 1 "the battery"
wait_for 'surface.popup gadget.power details line' 2 "power draw and health"
wait_for 'surface.popup gadget.power device' 1 "the mouse"
wait_for 'surface.popup gadget.power profiles toggle' 3 "three profiles"
wait_for 'surface.popup gadget.power profiles toggle.balanced.checked' 1 "balanced is active"

# A profile picked goes to the daemon; one it picks shows.
click_widget 'surface.popup gadget.power profiles toggle.performance'
assert_eq "$(upower_event)" "set-profile performance"
settle 0.5
wait_for 'surface.popup gadget.power profiles toggle.performance.checked' 1 "performance is active"
upower_send "profile power-saver"
wait_for 'gadget.power > button.status.power-saver' 2 "the daemon's pick on the bars"
upower_send "degraded high-operating-temperature"
wait_for 'surface.popup gadget.power message' 1 "why performance is limited"

# Keep awake, from the popup.
click_widget 'surface.popup gadget.power idle toggle'
assert_contains "$(aria debug idle)" "inhibited=true"
wait_for 'gadget.power > button.idle.inhibited' 2 "held, on both bars"
click_widget 'surface.popup gadget.power idle toggle'
assert_contains "$(aria debug idle)" "inhibited=false"

# Settings runs settings_command and closes the popup.
click_widget 'surface.popup gadget.power button.settings'
assert_no_surface popup
assert_eq "$(cat "$ARIA_UI_OUT/power-settings")" "settings" "settings_command ran"

# The charger back: idle's AC timeouts.
upower_send "ac on"
assert_contains "$(aria debug idle)" "armed=lock=1h"

# Low, then critical: one notification, replaced; gone once charging.
upower_send "ac off"
upower_send "warning low"
assert_logged '"Battery low"'
wait_for 'notification' 1 "a notification"
upower_send "battery 4 discharging 600"
upower_send "warning critical"
assert_logged '"Battery critically low"'
wait_for 'notification.critical' 1 "the same one, now critical"
shot low-battery
upower_send "battery 5 charging 3600"
upower_send "warning none"
wait_for 'notification' 0 "closed once charging"

# UPower leaving: back to the eye alone.
upower_send "profiles off"
upower_send "battery none"
wait_for 'gadget.power > button.status' 0 "nothing but the eye"
upower_stop
