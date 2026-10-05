# `aria-shell screenshot`: every output side by side, one output (the
# focused one, or by name), the active window cut out at its contents;
# a rotated output comes out upright. The pictures go to
# ~/Pictures/Screenshots, HOME being the scenario's output directory;
# `--edit` opens the file in the editor (the config's says what it
# got), `--clipboard` copies the picture instead (read back with
# wl-paste), no file.

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
    case $now in
        clipboard*) wl-paste --type image/png > "$ARIA_UI_OUT/$1.png" ;;
        *) cp "${now% *}" "$ARIA_UI_OUT/$1.png" ;;
    esac
    echo "${now##* }"
}

assert_contains "$(aria debug screenshot)" "capture=ext-image-copy-capture-v1"
assert_contains "$(aria debug screenshot)" "clipboard=ext-data-control-v1"
assert_eq "$(last)" none "nothing yet"

# --- outputs ----------------------------------------------------------------
assert_eq "$(take all)" 3840x1080 "both outputs side by side"
assert_eq "$(take output)" 1920x1080 "the focused output"
assert_eq "$(take output HEADLESS-2)" 1920x1080 "an output by name"
assert_eq "$(ls "$pictures" | wc -l)" 3 "three files"

# --- the editor, the clipboard ---------------------------------------------
assert_eq "$(take all --edit)" 3840x1080 "edited too"
settle
assert_eq "$(cat "$ARIA_UI_OUT/edited")" "$(last | sed 's/ [0-9x]*$//')" "the editor got the file"
assert_not_contains "$(wl-paste --list-types 2>&1)" image/png "a file isn't copied"
assert_eq "$(take output --clipboard)" 1920x1080 "copied"
assert_eq "$(last)" "clipboard 1920x1080"
assert_eq "$(ls "$pictures" | wc -l)" 4 "no file for the clipboard"
assert_eq "$(wl-paste --list-types)" image/png
assert_eq "$(python3 -c '
import struct, sys
print("%dx%d" % struct.unpack(">II", open(sys.argv[1], "rb").read()[16:24]))
' "$ARIA_UI_OUT/output.png")" 1920x1080 "a PNG of the output"
aria screenshot window --edit --clipboard 2>&1 | grep -q -- "--edit opens the file"
wl-copy hello
settle
assert_eq "$(wl-paste)" hello "another selection replaces ours"

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
