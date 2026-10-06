# The Places gadget: the home, the XDG folders (`user-dirs.dirs`: one
# set to the home is disabled, a missing one left out), the trash
# (full here), GTK's and KDE's bookmarks (Dolphin's own places, hidden
# and system items left out; the home and duplicates not repeated; a
# remote one by its host); a click opens one in [general] file_manager.

export HOME=$ARIA_UI_OUT/home
export XDG_DATA_HOME=$ARIA_UI_OUT/data
config=$ARIA_UI_OUT/config
mkdir -p "$HOME/Scaricati" "$HOME/Musica" "$HOME/My Projects" "$HOME/Work" \
    "$config/aria-shell" "$config/gtk-3.0" "$XDG_DATA_HOME/Trash/files"
touch "$XDG_DATA_HOME/Trash/files/old.txt"

# In the middle of the bar: at an edge the compositor slides the popup
# onto the screen, where `debug surfaces` doesn't know it is.
cat > "$config/aria-shell/aria.conf" << EOF
[general]
file_manager = sh -c 'echo "\$1" >> "$ARIA_UI_OUT/opened"' sh

[panel]
outputs = all
items_center = Places

[Places]
show = places bookmarks
EOF

cat > "$config/user-dirs.dirs" << 'EOF'
# a comment
XDG_DESKTOP_DIR="$HOME/"
XDG_DOWNLOAD_DIR="$HOME/Scaricati"
XDG_MUSIC_DIR="$HOME/Musica"
XDG_VIDEOS_DIR="$HOME/Video"
XDG_TEMPLATES_DIR="$HOME/Musica"
EOF

cat > "$config/gtk-3.0/bookmarks" << EOF
file://$HOME Home
file://$HOME/My%20Projects
sftp://dave@server.lan/srv
file://$HOME/Gone
EOF

cat > "$XDG_DATA_HOME/user-places.xbel" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xbel>
<xbel xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
 <bookmark href="file://$HOME"><title>Home</title>
  <info><metadata owner="http://www.kde.org"><isSystemItem>true</isSystemItem></metadata></info>
 </bookmark>
 <bookmark href="trash:/"><title>Trash</title>
  <info><metadata owner="http://www.kde.org"><isSystemItem>true</isSystemItem></metadata></info>
 </bookmark>
 <bookmark href="file://$HOME/Work"><title>Work &amp; stuff</title></bookmark>
 <bookmark href="file://$HOME/My%20Projects"><title>Projects again</title></bookmark>
 <bookmark href="file://$HOME/Musica"><title>Hidden</title>
  <info><metadata owner="http://www.kde.org"><IsHidden>true</IsHidden></metadata></info>
 </bookmark>
</xbel>
EOF

restart_shell "$config"

click_widget 'panel[output="HEADLESS-1"] gadget.places button'
assert_surface popup
shot_surface places popup
assert_eq "$(count_widgets 'header')" 2 "two sections"
assert_eq "$(count_widgets 'item')" 7 "home, Scaricati, Musica, trash; My Projects, server.lan, Work & stuff"
assert_eq "$(count_widgets 'item.trash.full')" 1 "the trash is full"
assert_eq "$(count_widgets 'item.remote')" 1 "the sftp bookmark"

click_widget 'item.remote'
assert_no_surface popup
click_widget 'panel[output="HEADLESS-1"] gadget.places button'
click_widget 'item.download'
click_widget 'panel[output="HEADLESS-1"] gadget.places button'
click_widget 'item.trash'
i=0
while [ "$(wc -l < "$ARIA_UI_OUT/opened" 2> /dev/null)" != 3 ] && [ $i -lt 20 ]; do
    sleep 0.1; i=$((i + 1))
done
assert_eq "$(sort "$ARIA_UI_OUT/opened")" "$(printf '%s\n' "$HOME/Scaricati" sftp://dave@server.lan/srv trash:/// | sort)" \
    "the remote URI, the folder, the trash"
