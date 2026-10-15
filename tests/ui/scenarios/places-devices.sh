# The Places gadget's devices, on a fake UDisks2 (tests/ui/udisks): the
# root filesystem (⏏ disabled), a USB stick and a locked LUKS volume
# listed, an internal disk not mounted left out; a click mounts the
# stick and opens it, ⏏ fails while it's busy (a notification), then
# unmounts it and powers it off (gone); polkit refusing a mount is
# notified.

export HOME=$ARIA_UI_OUT/home
# No mounts but UDisks2's: the desktop's (an encfs, a share) stay out.
export ARIA_SHELL_FSTAB=$ARIA_UI_OUT/fstab
export ARIA_SHELL_MOUNTINFO=$ARIA_UI_OUT/mountinfo
config=$ARIA_UI_OUT/config
mkdir -p "$HOME" "$config/aria-shell"
: > "$ARIA_SHELL_FSTAB"
: > "$ARIA_SHELL_MOUNTINFO"

# In the middle of the bar: at an edge the compositor slides the popup
# onto the screen, where `debug surfaces` doesn't know it is.
cat > "$config/aria-shell/aria.conf" << EOF
[general]
language = en
file_manager = sh -c 'echo "\$1" >> "$ARIA_UI_OUT/opened"' sh

[panel]
outputs = all
items_center = Places

[Places]
show = devices
EOF

udisks_start
restart_shell "$config"
button='surface.panel[output="HEADLESS-1"] gadget.places > indicator'

# No device yet.
click_widget "$button"
wait_for 'device' 0 "no device"
wait_for 'list > empty' 1 "the popup says so"
click_widget "$button"
assert_no_surface popup

udisks_send "internal root CachyOS /"
udisks_send "internal win Windows -"
udisks_send "stick cos COS_2026"
udisks_send "luks vault"
assert_contains "$(aria debug places)" 'label="CachyOS" kind=harddisk mounted=/ '
assert_not_contains "$(aria debug places)" "Windows" "an internal disk not mounted"

click_widget "$button"
shot_surface devices popup
wait_for 'device' 3 "the root, the stick, the LUKS volume"
wait_for 'device.harddisk.mounted' 1 "the root"
wait_for 'device.thumb' 1 "the stick"
wait_for 'device.usb.locked' 1 "the LUKS volume"
wait_for 'device button.eject' 1 "⏏ only on the mounted one"
wait_for 'device button.eject:disabled' 1 "the root's, disabled"
wait_for 'device meter' 1 "how full the root is"

# Mounted, then opened.
click_widget 'device.thumb button.open'
assert_eq "$(udisks_event)" "mount cos"
assert_no_surface popup
i=0
while [ ! -s "$ARIA_UI_OUT/opened" ] && [ $i -lt 20 ]; do
    sleep 0.1; i=$((i + 1))
done
assert_eq "$(cat "$ARIA_UI_OUT/opened")" "$ARIA_UI_OUT/media/COS_2026" "opened where it went"

click_widget "$button"
wait_for 'device.thumb.mounted' 1 "the stick, mounted"
wait_for 'device button.eject' 2 "⏏ on it too"
wait_for 'device meter' 2 "how full it is"

# In use: notified, still there.
udisks_send "busy cos on"
click_widget 'device.thumb button.eject'
assert_eq "$(udisks_event)" "unmount-busy cos"
assert_logged '"Can.t eject COS_2026"'
assert_logged 'target is busy'
assert_surface popup
wait_for 'device.thumb.mounted' 1 "still mounted"

# Unmounted, powered off: gone.
udisks_send "busy cos off"
click_widget 'device.thumb button.eject'
assert_eq "$(udisks_event)" "unmount cos"
assert_eq "$(udisks_event)" "power-off cos"
settle 1
wait_for 'device.thumb' 0 "gone with its drive"
wait_for 'device' 2 "the root and the LUKS volume"

# Polkit says no.
udisks_send "stick other OTHER"
udisks_send "deny other on"
click_widget 'device.thumb button.open'
assert_logged '"Can.t mount OTHER"'
assert_logged 'Not authorized'

udisks_stop
