# The Themes gadget: a left click toggles light/dark (the palette
# changes, the tray menu rules follow), a right click lists the schemes
# and the themes found; picking a theme restyles live (manjaro's last
# gadget is in the accent), picking Base goes back.

# A right click on the first bar's button, until the menu's seven items
# are there (3s): the popup is the shell's as soon as it's asked for,
# its widgets once the compositor has mapped it (up to ~1s here, longer
# while a theme loads).
open_menu() {
    click_widget_with right 'surface.panel[output="HEADLESS-1"] gadget.themes > button'
    i=0
    while [ "$(count_widgets 'menu > item')" != 7 ] && [ $i -lt 30 ]; do
        sleep 0.1
        i=$((i + 1))
    done
    assert_surface popup
}

assert_eq "$(aria debug theme)" "theme=- scheme=light" "starts light, base only"
wait_for 'gadget.themes > button' 2 "one per bar"
set -- $(widget 'surface.panel[output="HEADLESS-1"] gadget.themes > button')
shot bar-light "$(($1 - 60)),0 160x32"

click_widget 'surface.panel[output="HEADLESS-1"] gadget.themes > button'
assert_eq "$(aria debug theme)" "theme=- scheme=dark" "left click toggles"
shot bar-dark "$(($1 - 60)),0 160x32"
# The last gadget's top left corner: the bar's background in the base.
sample() {
    set -- $(widget 'surface.panel[output="HEADLESS-1"] gadget.screenshot > button')
    grim -g "$1,$2 1x1" -t ppm - | tail -c 3 | od -An -tu1 | tr -s ' '
}
base_dark=$(sample)
click_widget 'surface.panel[output="HEADLESS-2"] gadget.themes > button'
assert_eq "$(aria debug theme)" "theme=- scheme=light" "and back, from either bar"
base_light=$(sample)

# The menu: schemes, then the base and every theme in assets/themes,
# a folder (e17) or a file, then the theme's settings.
open_menu
wait_for 'menu > item' 7 "Light, Dark, Base, e17, manjaro, sway, the theme's settings"
wait_for 'menu > separator' 2
wait_for 'menu > item.checked:nth-child(1)' 1 "Light checked"
wait_for 'menu > item.checked:nth-child(4)' 1 "Base checked"
shot_surface menu popup

click_widget 'menu > item:nth-child(6)'
assert_no_surface popup
assert_eq "$(aria debug theme)" "theme=manjaro scheme=light" "manjaro picked"
[ "$(sample)" != "$base_light" ] || { echo "manjaro's last gadget looks as the base's"; exit 1; }

open_menu
wait_for 'menu > item.checked:nth-child(6)' 1 "manjaro checked"
click_widget 'menu > item:nth-child(2)'
assert_no_surface popup
assert_eq "$(aria debug theme)" "theme=manjaro scheme=dark" "Dark from the menu"

open_menu
click_widget 'menu > item:nth-child(4)'
assert_eq "$(aria debug theme)" "theme=- scheme=dark" "Base picked"
assert_eq "$(sample)" "$base_dark" "back to the base bar"
