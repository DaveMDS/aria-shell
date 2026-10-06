# The Places gadget's network shares and the local mounts UDisks2
# doesn't know, on the scenario's own fstab and
# mountinfo (ARIA_SHELL_FSTAB, ARIA_SHELL_MOUNTINFO) and the fake mount
# / umount / fusermount3 of tests/ui/bin: fstab's network entries with
# x-gvfs-show or under the home listed (x-gvfs-name, x-gvfs-hide, a
# local filesystem and a folder nobody looks in left out), plus an
# sshfs mounted by hand; a click mounts a share and opens it, an
# unreachable server and a share in use are notified, ⏏ unmounts (the
# one mounted by hand with fusermount3, and it's gone); an encfs mounted
# by hand is listed with the devices, encrypted, and unmounted alike.

export HOME=$ARIA_UI_OUT/home
export ARIA_SHELL_FSTAB=$ARIA_UI_OUT/fstab
export ARIA_SHELL_MOUNTINFO=$ARIA_UI_OUT/mountinfo
config=$ARIA_UI_OUT/config
mkdir -p "$HOME/Shares/Docs" "$config/aria-shell" "$ARIA_UI_OUT/busy" "$ARIA_UI_OUT/unreachable"

cat > "$ARIA_SHELL_FSTAB" << EOF
# a comment
UUID=1234 / btrfs defaults 0 0
/dev/sdb1 /media/data ext4 defaults,users 0 2
nas:/volume1/Backup /media/NAS/Backup nfs users,noauto,x-gvfs-show 0 0
//nas/docs $HOME/Shares/Docs cifs users,noauto,x-gvfs-name=My%20Docs 0 0
nas:/volume1/Hidden /media/NAS/Hidden nfs users,noauto,x-gvfs-show,x-gvfs-hide 0 0
nas:/volume1/Srv /srv/nas nfs users,noauto 0 0
EOF
escaped_home=$(printf '%s' "$HOME" | sed 's/ /\\040/g')
cat > "$ARIA_SHELL_MOUNTINFO" << EOF
22 1 0:21 / / rw,relatime shared:1 - btrfs /dev/nvme0n1p7 rw
90 22 0:90 / $escaped_home/remote\\040box rw,nosuid,nodev,relatime shared:9 - fuse.sshfs dave@box:/ rw
91 22 0:91 / /srv/x rw,relatime shared:10 - nfs4 nas:/x rw
92 22 0:92 / /media/Vault rw,nosuid,nodev,relatime shared:11 - fuse.encfs encfs rw
93 22 0:93 / /media/scratch rw,relatime shared:12 - tmpfs tmpfs rw
EOF

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
show = devices network
EOF

restart_shell "$config"
button='panel[output="HEADLESS-1"] gadget.places > button'

click_widget "$button"
shot_surface network popup
assert_eq "$(count_widgets 'share')" 3 "Backup, My Docs, the sshfs"
assert_eq "$(count_widgets 'share.nfs')" 1 "Backup"
assert_eq "$(count_widgets 'share.smb')" 1 "My Docs"
assert_eq "$(count_widgets 'share.ssh.mounted')" 1 "the sshfs mounted by hand"
assert_eq "$(count_widgets 'share button.eject')" 1 "⏏ only on the mounted one"
assert_eq "$(count_widgets 'share meter')" 0 "no usage: it would wait on the server"
assert_contains "$(aria debug places)" 'label="My Docs" type=cifs mounted=false fstab=true'
assert_contains "$(aria debug places)" "share $HOME/remote box \"dave@box:/\" label=\"remote box\" type=fuse.sshfs mounted=true fstab=false"
assert_not_contains "$(aria debug places)" "Hidden" "x-gvfs-hide"
assert_not_contains "$(aria debug places)" "/srv" "where nobody looks"
assert_eq "$(count_widgets 'device')" 1 "the encfs, with the devices (not the tmpfs)"
assert_eq "$(count_widgets 'device.encrypted.mounted')" 1 "encrypted, mounted"
assert_contains "$(aria debug places)" 'mount /media/Vault "encfs" label="Vault" type=fuse.encfs mounted=true fstab=false'

# Mounted, then opened.
click_widget 'share.nfs button.open'
assert_no_surface popup
i=0
while [ ! -s "$ARIA_UI_OUT/opened" ] && [ $i -lt 20 ]; do
    sleep 0.1; i=$((i + 1))
done
assert_eq "$(cat "$ARIA_UI_OUT/mount.log")" "mount /media/NAS/Backup"
assert_eq "$(cat "$ARIA_UI_OUT/opened")" "/media/NAS/Backup" "opened"
click_widget "$button"
assert_eq "$(count_widgets 'share.nfs.mounted')" 1 "Backup, mounted"
assert_eq "$(count_widgets 'share button.eject')" 2 "⏏ on it too"

# A server that doesn't answer.
touch "$ARIA_UI_OUT/unreachable/Docs"
click_widget 'share.smb button.open'
assert_logged '"Can.t mount My Docs"'
assert_logged 'Connection timed out'

# In use: notified, still mounted.
click_widget "$button"
touch "$ARIA_UI_OUT/busy/Backup"
click_widget 'share.nfs button.eject'
assert_logged '"Can.t unmount Backup"'
assert_logged 'target is busy'
assert_surface popup
assert_eq "$(count_widgets 'share.nfs.mounted')" 1 "still mounted"

# Unmounted; the one mounted by hand with fusermount3, and gone.
rm "$ARIA_UI_OUT/busy/Backup"
click_widget 'share.nfs button.eject'
settle 0.5
assert_eq "$(count_widgets 'share.nfs.mounted')" 0 "Backup unmounted"
assert_eq "$(count_widgets 'share.nfs')" 1 "still listed: fstab has it"
click_widget 'share.ssh button.eject'
settle 0.5
assert_logged 'remote box unmounted'
assert_contains "$(cat "$ARIA_UI_OUT/mount.log")" "fusermount3 -u $HOME/remote box"
assert_eq "$(count_widgets 'share.ssh')" 0 "gone: nothing to mount it again with"

# The encfs: unmounted with fusermount3, and gone.
click_widget 'device.encrypted button.eject'
settle 0.5
assert_contains "$(cat "$ARIA_UI_OUT/mount.log")" "fusermount3 -u /media/Vault"
assert_eq "$(count_widgets 'device')" 0 "gone: nothing to mount it again with"
