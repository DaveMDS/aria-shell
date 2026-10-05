# `aria-shell screenshot`: every output side by side, one output (the
# focused one, or by name), the active window cut out at its contents;
# a rotated output comes out upright. The pictures go to
# ~/Pictures/Screenshots, HOME being the scenario's output directory.

export HOME=$ARIA_UI_OUT/home
restart_shell "$XDG_CONFIG_HOME"
pictures=$HOME/Pictures/Screenshots

last() {
    aria debug screenshot | sed 's/.*last=//'
}

# Run `aria screenshot $@` and wait (5 s) for the new picture: prints
# its `WxH`, copied to $1.png in the output directory.
take() {
    before=$(last)
    aria screenshot "$@" > /dev/null
    i=0
    while [ "$(last)" = "$before" ] && [ $i -lt 50 ]; do
        sleep 0.1; i=$((i + 1))
    done
    now=$(last)
    [ "$now" != "$before" ] || { echo "no picture for: screenshot $*"; return 1; }
    cp "${now% *}" "$ARIA_UI_OUT/$1.png"
    echo "${now##* }"
}

assert_contains "$(aria debug screenshot)" "capture=ext-image-copy-capture-v1"
assert_eq "$(last)" none "nothing yet"

# --- outputs ----------------------------------------------------------------
assert_eq "$(take all)" 3840x1080 "both outputs side by side"
assert_eq "$(take output)" 1920x1080 "the focused output"
assert_eq "$(take output HEADLESS-2)" 1920x1080 "an output by name"
assert_eq "$(ls "$pictures" | wc -l)" 3 "three files"
aria screenshot output NOPE
settle
assert_logged "screenshot: no output"

# --- the active window ------------------------------------------------------
open_window red
geometry=$(swaymsg -t get_tree | python3 -c '
import json, sys
def views(n):
    if n.get("pid") and n.get("focused"):
        yield n
    for c in n.get("nodes", []) + n.get("floating_nodes", []):
        yield from views(c)
v = next(views(json.load(sys.stdin)))
print("%dx%d" % (v["window_rect"]["width"], v["window_rect"]["height"]))
')
assert_eq "$(take window)" "$geometry" "the window's contents"
close_window red
aria screenshot window
settle
assert_logged "screenshot: no active window"

# --- a rotated output ---------------------------------------------------------
swaymsg output HEADLESS-2 transform 90 > /dev/null
settle 1
assert_eq "$(take output HEADLESS-2)" 1080x1920 "upright"
swaymsg output HEADLESS-2 transform normal > /dev/null
