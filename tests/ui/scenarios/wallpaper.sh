# The wallpaper: one background surface per output, [wallpaper] on
# HEADLESS-1 and [wallpaper:HEADLESS-2] on the other, the bar above it;
# the images are two 16x16 gradients stretched (`fit = fill`), told
# apart on the screenshot by eye and by the pixels sampled below.

assert_eq "$(surfaces | tr ';' '\n' | grep -c '^ *wallpaper ')" 2 "one wallpaper per output"
assert_contains "$(surfaces)" "wallpaper HEADLESS-1 0,0 1920x1080"
assert_contains "$(surfaces)" "wallpaper HEADLESS-2 1920,0 1920x1080"
assert_logged "wallpapers/a.png loaded (16x16)"
assert_logged "wallpapers/b.png loaded (16x16)"
settle 0.5
shot wallpaper

# The two outputs show different images: a is a horizontal gradient
# (its left and right edges differ, top and bottom don't), b a vertical
# one; a 1-pixel sample per corner, well below the bar.
sample() { grim -g "$1,$2 1x1" -t ppm - | tail -c 3 | od -An -tu1 | tr -s ' '; }
a_left=$(sample 10 600); a_right=$(sample 1900 600); a_top=$(sample 10 100)
b_top=$(sample 1930 100); b_bottom=$(sample 1930 1070); b_right=$(sample 3830 100)
[ "$a_left" != "$a_right" ] || { echo "a: no horizontal gradient ($a_left)"; exit 1; }
[ "$a_left" = "$a_top" ] || { echo "a: vertical change ($a_left vs $a_top)"; exit 1; }
[ "$b_top" != "$b_bottom" ] || { echo "b: no vertical gradient ($b_top)"; exit 1; }
[ "$b_top" = "$b_right" ] || { echo "b: horizontal change ($b_top vs $b_right)"; exit 1; }

# The launcher and the lock screen sit above it.
aria launcher show; settle 0.8
shot wallpaper-launcher "0,0 1920x1080"
key Escape
aria lock; settle 0.8
assert_surface locker
shot wallpaper-locker "0,0 1920x1080"
key Return; settle 0.8
assert_no_surface locker

# The file changing reloads it: b becomes a copy of a, the vertical
# gradient on HEADLESS-2 turns horizontal (b.png is in the tree: put
# back whatever happens; the scenario runs in its own subshell).
walls=$XDG_CONFIG_HOME/aria-shell/wallpapers
orig=$(mktemp); cp "$walls/b.png" "$orig"
trap 'cp "$orig" "$walls/b.png"; rm -f "$orig"' EXIT
cp "$walls/a.png" "$walls/b.png"
assert_logged "wallpaper file(s) changed"
settle 1
b_top=$(sample 1930 100); b_bottom=$(sample 1930 1070); b_right=$(sample 3830 100)
[ "$b_top" = "$b_bottom" ] || { echo "b after reload: still vertical ($b_top vs $b_bottom)"; exit 1; }
[ "$b_top" != "$b_right" ] || { echo "b after reload: no horizontal gradient ($b_top)"; exit 1; }
