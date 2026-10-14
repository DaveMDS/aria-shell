# [panel] align: a bar as long as its gadgets in the middle of its edge
# (center), or in a corner (start, end), instead of the whole edge; it
# follows its gadgets as they grow. The centred one keeps its room from
# the windows, the cornered ones don't (layer-shell ignores a corner's
# exclusive zone).

conf=$(mktemp -d)
trap 'rm -rf "$conf"' EXIT
cp -r "$XDG_CONFIG_HOME/." "$conf"
echo short > "$conf/label"
# The test config's panels out, three of ours in.
python3 -I - "$conf/aria-shell/aria.conf" "$conf/label" << 'EOF'
import re, sys
path, label = sys.argv[1], sys.argv[2]
text = open(path).read()
text = re.sub(r"(?ms)^\[panel[^\]]*\].*?(?=^\[)", "", text)
text += f"""
[panel]
outputs = HEADLESS-1
position = top
align = center
items_center = Clock Custom:grow

[panel:start]
outputs = HEADLESS-2
position = top
align = start
items_start = Clock

[panel:end]
outputs = HEADLESS-2
position = bottom
align = end
items_end = Clock

[Custom:grow]
exec = cat {label}
interval = 1
"""
open(path, "w").write(text)
EOF
restart_shell "$conf"
settle 1
surfaces
shot top-1 "0,0 1920x40"
shot top-2 "1920,0 1920x40"
shot bottom-2 "1920,1040 1920x40"

# x y w h of the bar on an output
bar() { surfaces | tr ';' '\n' | grep "panel $1" | sed -n "${2}p" | awk '{print $3, $4}' | tr ',x' '  '; }

set -- $(bar HEADLESS-1 1)
[ "$3" -lt 1920 ] || { echo "the centred bar spans the edge: $*"; exit 1; }
assert_eq "$(($1 * 2 + $3))" 1920 "centred"
width=$3
# Its pixels: the bar where it says, the wallpaper beside it (the
# wallpaper a horizontal gradient: the same column below the bar).
sample() { grim -g "$1,$2 1x1" -t ppm - | tail -c 3 | od -An -tu1 | tr -s ' '; }
bar_at() { [ "$(sample "$1" 5)" != "$(sample "$1" 35)" ]; }
bar_at $(($1 + 2)) || { echo "no bar at its left end"; exit 1; }
! bar_at $(($1 - 3)) || { echo "a bar left of it"; exit 1; }

set -- $(bar HEADLESS-2 1)
assert_eq "$1,$2" "1920,0" "start: the top left corner"
[ "$3" -lt 1920 ] || { echo "the start bar spans the edge: $*"; exit 1; }
set -- $(bar HEADLESS-2 2)
assert_eq "$(($1 + $3)),$(($2 + $4))" "3840,1080" "end: the bottom right corner"

# Longer content: the bar grows, still centred.
echo "a label much longer than before" > "$conf/label"
sleep 2
settle 1
set -- $(bar HEADLESS-1 1)
[ "$3" -gt "$width" ] || { echo "the bar didn't grow: $width -> $3"; exit 1; }
assert_eq "$(($1 * 2 + $3))" 1920 "still centred"
bar_at $(($1 + 2)) || { echo "the grown bar isn't drawn there"; exit 1; }
shot top-1-grown "0,0 1920x40"
