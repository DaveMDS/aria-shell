# The picker, `aria-shell screenshot`: both outputs frozen under a
# surface each; a drag draws an area, a corner resizes it, its inside
# moves it, Enter saves it; a click on bare desktop picks the output,
# on a window the window; Escape or a right click cancels; the toolbar
# copies, edits, takes every output. A drag stays on the output it
# started on. The gadget: a left click opens the picker, its menu
# takes the bar's screen with the menu gone. HOME is the scenario's
# output directory, as in screenshot.sh.

export HOME=$ARIA_UI_OUT/home
restart_shell "$XDG_CONFIG_HOME"
pictures=$HOME/Pictures/Screenshots

last() {
    aria debug screenshot | sed 's/.*last=//'
}

selection() {
    aria debug screenshot | sed -n 's/.*selection=\([^;]*\);.*/\1/p'
}

count_pickers() {
    surfaces | tr ';' '\n' | grep -c '^ *screenshot ' || true
}

# Until n picker surfaces are open (5 s).
wait_picker() {
    i=0
    while [ "$(count_pickers)" != "$1" ] && [ $i -lt 50 ]; do
        sleep 0.1; i=$((i + 1))
    done
    assert_eq "$(count_pickers)" "$1" "$2"
    settle
}

# Until a picture other than $1 is the last one (5 s): its `WxH`.
wait_picture() {
    i=0
    while [ "$(last)" = "$1" ] && [ $i -lt 50 ]; do
        sleep 0.1; i=$((i + 1))
    done
    [ "$(last)" != "$1" ] || { echo "no new picture"; return 1; }
    last | sed 's/.* //'
}

# --- an area: drawn, resized, moved, saved -----------------------------------
aria screenshot
wait_picker 2 "a picker surface on each output"
assert_eq "$(selection)" none
pointer 960 540
settle
shot picker
drag 100 100 500 400
assert_eq "$(selection)" "100,100 400x300" "drawn"
drag 500 400 600 450
assert_eq "$(selection)" "100,100 500x350" "the corner resizes"
drag 300 300 350 320
assert_eq "$(selection)" "150,120 500x350" "the inside moves"
shot selection
before=$(last)
key Return
wait_picker 0 "Enter takes it"
assert_eq "$(wait_picture "$before")" 500x350 "the area, saved"
assert_eq "$(ls "$pictures" | wc -l)" 1

# --- a click: the output; Escape, a right click ------------------------------
aria screenshot
wait_picker 2
click 2500 500
assert_eq "$(selection)" "1920,0 1920x1080" "bare desktop: the output"
key Escape
wait_picker 0 "Escape cancels"
aria screenshot
wait_picker 2
inject "click right"
wait_picker 0 "a right click cancels"
assert_eq "$(ls "$pictures" | wc -l)" 1 "nothing saved"

# --- a window, copied ----------------------------------------------------------
open_window red
geometry=$(swaymsg -t get_tree | python3 -c '
import json, sys
def views(n):
    if n.get("pid") and n.get("focused"):
        yield n
    for c in n.get("nodes", []) + n.get("floating_nodes", []):
        yield from views(c)
v = next(views(json.load(sys.stdin)))
r, w = v["rect"], v["window_rect"]
print("%d,%d %dx%d" % (r["x"] + w["x"], r["y"] + w["y"], w["width"], w["height"]))
')
aria screenshot
wait_picker 2
click 960 600
assert_eq "$(selection)" "$geometry" "a click on a window: the window"
before=$(last)
click_widget 'screenshot toolbar button.copy'
wait_picker 0 "Copy takes it"
assert_eq "$(wait_picture "$before")" "${geometry#* }" "copied"
assert_eq "$(wl-paste --list-types)" image/png
close_window red

# --- the toolbar: edit, every output; a drag stays on its output -------------
aria screenshot
wait_picker 2
click 2500 500
before=$(last)
click_widget 'screenshot toolbar button.edit'
wait_picker 0
assert_eq "$(wait_picture "$before")" 1920x1080
settle
assert_eq "$(cat "$ARIA_UI_OUT/edited")" "$(last | sed 's/ [0-9x]*$//')" "the editor got the file"

aria screenshot
wait_picker 2
drag 1800 500 2100 600
assert_eq "$(selection)" "1800,500 120x100" "stopped at the output's edge"
before=$(last)
click_widget 'screenshot toolbar button.all'
wait_picker 0
assert_eq "$(wait_picture "$before")" 3840x1080 "every output"

# --- the gadget ------------------------------------------------------------------
click_widget 'panel[output="HEADLESS-2"] gadget.screenshot > button'
wait_picker 2 "a left click opens the picker"
key Escape
wait_picker 0
click_widget_with right 'panel[output="HEADLESS-2"] gadget.screenshot > button'
wait_for 'menu > item' 3 "window, screen, all"
before=$(last)
click_widget 'menu > item:nth-child(2)'
assert_eq "$(wait_picture "$before")" 1920x1080 "this screen"
assert_no_surface popup
assert_logged "capturing 1920,0 1920x1080"
