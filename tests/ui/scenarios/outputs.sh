# Outputs and the config changing under the shell, for every kind of
# surface: an output going takes its panel, wallpaper and OSD with it
# and leaves the other output's; coming back brings its panel and
# wallpaper back, once. The launcher on it closes with its grabs, the
# screenshot picker on every output closes. A config reload closes them
# all and opens each again, once.

config=$ARIA_UI_OUT/config
cp -r "$ARIA_UI_ROOT/tests/ui/config" "$config"
# Reloads on a change, and an OSD that stays for the whole scenario.
sed -i -e 's/^reload_config = no$/reload_config = yes/' \
    -e '/^\[osd\]$/,/^$/s/^duration = .*/duration = 60/' "$config/aria-shell/aria.conf"
restart_shell "$config"

# How many surfaces of a kind, on one output (all of them without one).
count() {
    surfaces | tr ';' '\n' | grep -c "^ *$1 ${2:-}" || true
}

# Until `count $1 $2` is `$3` (3 s), else fails with `$4`.
wait_count() {
    i=0
    while [ "$(count "$1" "$2")" != "$3" ] && [ $i -lt 30 ]; do
        sleep 0.1; i=$((i + 1))
    done
    assert_eq "$(count "$1" "$2")" "$3" "$4"
}

assert_eq "$(count panel)" 2 "a panel per output"
assert_eq "$(count wallpaper)" 2 "a wallpaper per output"
aria osd show --value 40 Test
wait_count osd "" 2 "an OSD per output"

swaymsg output HEADLESS-2 disable > /dev/null
wait_count panel HEADLESS-2 0 "the panel went with its output"
assert_eq "$(count wallpaper HEADLESS-2)" 0 "the wallpaper went with its output"
assert_eq "$(count osd HEADLESS-2)" 0 "the OSD went with its output"
assert_eq "$(count panel HEADLESS-1)" 1 "the other output's panel stays"
assert_eq "$(count wallpaper HEADLESS-1)" 1 "the other output's wallpaper stays"
assert_eq "$(count osd HEADLESS-1)" 1 "the other output's OSD stays"

swaymsg output HEADLESS-2 enable > /dev/null
wait_count panel HEADLESS-2 1 "the panel came back with its output"
wait_count wallpaper HEADLESS-2 1 "the wallpaper came back with its output"
assert_eq "$(count panel)" 2 "still a panel per output"
assert_eq "$(count wallpaper)" 2 "still a wallpaper per output"

# The launcher on the output that goes: it closes, and its grabs on the
# other outputs with it, or they'd swallow every click there.
swaymsg focus output HEADLESS-2 > /dev/null
aria launcher show; settle 0.8
assert_contains "$(surfaces)" "launcher HEADLESS-2"
swaymsg output HEADLESS-2 disable > /dev/null
wait_count launcher "" 0 "the launcher closed with its output"
assert_eq "$(count grab)" 0 "its grabs closed"
# The pointer's space is HEADLESS-1 alone now.
inject "layout 1920 1080"
click_widget 'panel[output="HEADLESS-1"] slot.center gadget.clock > button'
assert_surface popup "a click reaches the other output's bar"
swaymsg output HEADLESS-2 enable > /dev/null
inject "layout 3840 1080"
wait_count panel HEADLESS-2 1 "the panel came back again"

# The screenshot picker on every output: one going cancels it, its
# pictures are of the outputs as they were.
aria screenshot
i=0
while [ "$(count screenshot)" != 2 ] && [ $i -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
assert_eq "$(count screenshot)" 2 "the picker on every output"
swaymsg output HEADLESS-2 disable > /dev/null
wait_count screenshot "" 0 "the picker closed on every output"
assert_contains "$(aria debug screenshot)" "picker=closed"
swaymsg output HEADLESS-2 enable > /dev/null
wait_count panel HEADLESS-2 1 "the panel came back once more"

# A config change: everything reopens (the OSD closes, its position may
# have changed).
echo "# touched" >> "$config/aria-shell/aria.conf"
assert_logged "config changed, rebuilding panels"
wait_count osd "" 0 "the OSD closed on reload"
wait_count panel "" 2 "a panel per output after the reload"
wait_count wallpaper "" 2 "a wallpaper per output after the reload"
aria osd show --value 60 Again
wait_count osd "" 2 "the OSD shows again"
