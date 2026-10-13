# The network gadget: the primary connection's icon on each bar, the
# popup with the Wi‑Fi networks around, the wired device and the VPN
# profiles, driven against the fake NetworkManager (tests/ui/nm) on the
# scenario's bus, which the shell takes for the system bus.

bar='surface.panel[output="HEADLESS-1"] gadget.network > button'
list='surface.popup gadget.network'

# Nothing but scans for a moment.
nm_idle() {
    if read -r -t 1 line <&4 && [ "$line" != scan ]; then
        echo "unexpected event from NetworkManager: $line"
        return 1
    fi
}

# --- no NetworkManager yet ---------------------------------------------------
wait_for 'gadget.network > button.disconnected.muted' 2 "offline on both bars, nobody on the bus"
click_widget "$bar"
assert_surface popup
wait_for "$list empty" 1 "the popup says NetworkManager isn't running"
shot_surface network-none popup
click 600 600
assert_no_surface popup

# --- it comes up: nothing connected ------------------------------------------
nm_start
wait_for 'gadget.network > button.wifi.disconnected.muted' 2 "a Wi‑Fi device, disconnected, on both bars"

# --- the networks around -----------------------------------------------------
nm_send "ap Darma6 80 psk"
nm_send "ap Casa 55 psk"
nm_send "known Casa"
nm_send "ap Guest 20 open"
nm_send "ap Corp 60 eap"
click_widget "$bar"
assert_surface popup
assert_eq "$(nm_event)" scan "opening the popup asks for a scan"
wait_for "$list ap" 4 "four networks listed"
# The known one first (after the Wi‑Fi header), then by strength.
wait_for "$list ap:nth-child(2)[ssid=\"Casa\"].known.secured" 1 "Casa (known) first"
wait_for "$list ap:nth-child(3)[ssid=\"Darma6\"].secured" 1 "Darma6 (80%) second"
wait_for "$list ap:nth-child(4)[ssid=\"Corp\"].enterprise" 1 "Corp (60%, 802.1x) third"
wait_for "$list ap:nth-child(5)[ssid=\"Guest\"]" 1 "Guest (20%, open) last"
wait_for "$list ap.secured > button > title > icon.badge" 3 "a lock on the secured ones"
wait_for "$list header.wifi toggle.checked" 1 "the Wi‑Fi toggle on"
wait_for "$list header.wired" 1 "a wired section"
wait_for "$list device.wired.unplugged" 1 "eth0 unplugged"
wait_for "$list header.vpn" 0 "no VPN section without profiles"
# The other bar's popup lists the same.
click 600 600
click_widget 'surface.panel[output="HEADLESS-2"] gadget.network > button'
assert_eq "$(nm_event)" scan "the other bar scans too"
wait_for "$list ap" 4 "the same four on the other output"
shot_surface network-list popup
click 2500 600
assert_no_surface popup

# --- a known network: click connects ------------------------------------------
click_widget "$bar"
assert_eq "$(nm_event)" scan
wait_for "$list ap" 4
click_widget "$list ap[ssid=\"Casa\"] > button"
assert_eq "$(nm_action)" "activate Casa" "the saved profile activated"
wait_for "$list ap[ssid=\"Casa\"].connecting" 1 "Casa connecting"
wait_for 'gadget.network > button.wifi.connecting' 2 "the bars show it connecting"
nm_send finish
wait_for "$list ap[ssid=\"Casa\"].active" 1 "Casa connected"
wait_for "$list ap:nth-child(2)[ssid=\"Casa\"]" 1 "the active one stays first"
wait_for "$list ap.active > button > status" 1 "with its status"
wait_for 'gadget.network > button.wifi.connected' 2 "connected on both bars"
wait_for 'gadget.network > button.wifi.connected > label' 2 "the SSID on the bars (show_label)"
# Its details unfold on a click.
click_widget "$list ap[ssid=\"Casa\"] > button"
wait_for "$list ap.active.expanded details" 1 "the details unfolded"
wait_for "$list ap.expanded details line" 4 "IPv4, gateway, DNS, the band line"
wait_for "$list ap.expanded actions button.disconnect" 1 "Disconnect"
wait_for "$list ap.expanded actions button.forget" 1 "Forget"
shot_surface network-details popup
click_widget "$list ap[ssid=\"Casa\"] > button"
wait_for "$list ap.expanded" 0 "folded again"

# --- an unknown secured network: the password in place ---------------------
click_widget "$list ap[ssid=\"Darma6\"] > button"
wait_for "$list ap[ssid=\"Darma6\"].expanded auth input" 1 "the password field appears"
nm_idle
wait_for "$list ap.expanded auth button.connect:disabled" 1 "Connect disabled with nothing typed"
type_text secret
wait_for "$list ap.expanded auth button.connect:disabled" 0 "Connect enabled"
click_widget "$list ap.expanded auth toggle.peek"
wait_for "$list ap.expanded auth toggle.peek.checked" 1 "the password shown"
shot_surface network-password popup
key Return
assert_eq "$(nm_action)" "add-activate Darma6 psk=secret" "a profile added with the key and activated"
wait_for "$list ap[ssid=\"Darma6\"].connecting" 1 "connecting"
# A wrong key: the profile is dropped, the row says so and asks again.
nm_send "fail psk"
assert_eq "$(nm_action)" "delete Darma6" "the failed profile deleted"
wait_for "$list ap[ssid=\"Darma6\"].failed message" 1 "'Wrong password' under the row"
wait_for "$list ap[ssid=\"Darma6\"].expanded auth input" 1 "the field still there"
wait_for "$list ap[ssid=\"Darma6\"].known" 0 "still unknown"
shot_surface network-wrong popup
type_text right
key Return
assert_eq "$(nm_action)" "add-activate Darma6 psk=right" "tried again"
nm_send finish
wait_for "$list ap[ssid=\"Darma6\"].active" 1 "Darma6 connected"
wait_for "$list ap.failed" 0 "the failure gone"
wait_for "$list ap[ssid=\"Casa\"].active" 0 "Casa no longer (one connection per device)"

# --- an enterprise network can't be joined here; an open one just is -------
click_widget "$list ap[ssid=\"Corp\"] > button"
wait_for "$list ap[ssid=\"Corp\"].expanded message" 1 "the enterprise hint"
nm_idle
click_widget "$list ap[ssid=\"Guest\"] > button"
assert_eq "$(nm_action)" "add-activate Guest" "an open network: a profile without a key"
nm_send finish
wait_for "$list ap[ssid=\"Guest\"].active" 1 "Guest connected"

# --- disconnect, forget --------------------------------------------------------
click_widget "$list ap[ssid=\"Guest\"] > button"
wait_for "$list ap.expanded actions button.disconnect" 1
click_widget "$list ap.expanded actions button.disconnect"
assert_eq "$(nm_action)" "disconnect wlan0" "disconnected"
wait_for "$list ap.active" 0 "nothing active"
wait_for "$list ap[ssid=\"Guest\"].known" 1 "Guest is known now"
wait_for 'gadget.network > button.wifi.disconnected' 2 "the bars offline"
# A known one that's not connected: its chevron unfolds Connect / Forget
# (a click on the row itself would join it).
click_widget "$list ap[ssid=\"Casa\"] > button button.chevron"
wait_for "$list ap[ssid=\"Casa\"].expanded actions button.connect" 1 "Connect under a known network"
wait_for "$list ap[ssid=\"Casa\"].expanded actions button.forget" 1 "and Forget"
nm_idle
shot_surface network-known popup
click_widget "$list ap.expanded actions button.forget"
assert_eq "$(nm_action)" "delete Casa" "Casa forgotten"
wait_for "$list ap[ssid=\"Casa\"].known" 0 "no longer known"
wait_for "$list ap.expanded" 0 "folded"

# --- Wi‑Fi off and on ------------------------------------------------------------
click_widget "$list header.wifi toggle"
assert_eq "$(nm_action)" "wireless false" "Wi‑Fi turned off"
wait_for "$list header.wifi.off" 1 "the header says off"
wait_for "$list ap" 0 "no networks listed"
wait_for "$list empty" 1 "'Wi‑Fi is off'"
shot_surface network-off popup
click 600 600
assert_no_surface popup
click_widget_with middle "$bar"
assert_eq "$(nm_action)" "wireless true" "a middle click turns it back on"

# --- the wired device --------------------------------------------------------------
nm_send "wired eth0-profile"
nm_send "wired-up"
wait_for 'gadget.network > button.wired.connected' 2 "wired is the primary connection now"
click_widget "$bar"
assert_eq "$(nm_event)" scan
wait_for "$list device.wired.active" 1 "eth0 connected"
click_widget "$list device.wired > button"
wait_for "$list device.expanded details line" 4 "its details"
shot_surface network-wired popup
click_widget "$list device.expanded actions button.disconnect"
assert_eq "$(nm_action)" "disconnect eth0"
wait_for "$list device.wired.active" 0 "eth0 down"
nm_send "carrier off"
wait_for "$list device.wired.unplugged" 1 "cable unplugged"

# --- a VPN profile with its toggle ---------------------------------------------
nm_send "vpn Office"
wait_for "$list vpn[name=\"Office\"]" 1 "the VPN row"
click_widget "$list vpn[name=\"Office\"] toggle"
assert_eq "$(nm_action)" "activate Office" "activated by the toggle"
wait_for "$list vpn.connecting" 1
nm_send "fail nosecrets"
wait_for "$list vpn.failed message" 1 "'Needs a password' under it"
shot_surface network-vpn popup
click_widget "$list vpn[name=\"Office\"] toggle"
assert_eq "$(nm_action)" "activate Office"
nm_send finish
wait_for "$list vpn.active" 1 "the VPN up"
wait_for "$list vpn.failed" 0 "the failure gone"
wait_for 'gadget.network > button > icon.vpn' 2 "the VPN badge on the bars"
click_widget "$list vpn[name=\"Office\"] toggle"
assert_eq "$(nm_action)" "deactivate Office"
wait_for "$list vpn.active" 0 "the VPN down"

# --- strength, settings, NetworkManager leaving --------------------------------
click_widget "$list ap[ssid=\"Guest\"] > button"
assert_eq "$(nm_action)" "activate Guest"
nm_send finish
wait_for 'gadget.network > button.wifi.connected' 2
nm_send "strength Guest 10"
settle 0.5
wait_for "$list ap[ssid=\"Guest\"].active" 1 "still connected at 10%"
click_widget "$list button.settings"
assert_no_surface popup
i=0; while ! [ -f "$ARIA_UI_OUT/clicks" ] && [ $i -lt 20 ]; do sleep 0.1; i=$((i + 1)); done
assert_eq "$(cat "$ARIA_UI_OUT/clicks")" settings "settings_command ran"
nm_stop
wait_for 'gadget.network > button.disconnected.muted' 2 "offline again once NetworkManager is gone"
wait_for 'gadget.network > button.wifi' 0 "no device"
