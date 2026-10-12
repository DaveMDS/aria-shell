# The Themes gadget: a left click toggles light/dark (the palette
# changes, the tray menu rules follow), a right click lists the schemes
# and the themes found; picking a theme restyles live (manjaro's clock
# cell has reversed colours), picking Base goes back.

# A right click on the first bar's button, until the menu's five items
# are there (3s): the popup is the shell's as soon as it's asked for,
# its widgets once the compositor has mapped it (up to ~1s here, longer
# while a theme loads).
open_menu() {
    click_widget_with right 'panel[output="HEADLESS-1"] gadget.themes > button'
    i=0
    while [ "$(count_widgets 'menu > item')" != 5 ] && [ $i -lt 30 ]; do
        sleep 0.1
        i=$((i + 1))
    done
    assert_surface popup
}

assert_eq "$(aria debug theme)" "style=- scheme=light" "starts light, base only"
wait_for 'gadget.themes > button' 2 "one per bar"
set -- $(widget 'panel[output="HEADLESS-1"] gadget.themes > button')
shot bar-light "$(($1 - 60)),0 160x32"

click_widget 'panel[output="HEADLESS-1"] gadget.themes > button'
assert_eq "$(aria debug theme)" "style=- scheme=dark" "left click toggles"
shot bar-dark "$(($1 - 60)),0 160x32"
# The clock cell's top left corner: the bar's background in the base.
sample() {
    set -- $(widget 'panel[output="HEADLESS-1"] gadget.clock > button')
    grim -g "$1,$2 1x1" -t ppm - | tail -c 3 | od -An -tu1 | tr -s ' '
}
base_dark=$(sample)
click_widget 'panel[output="HEADLESS-2"] gadget.themes > button'
assert_eq "$(aria debug theme)" "style=- scheme=light" "and back, from either bar"
base_light=$(sample)

# The menu: schemes, then the base and every theme in assets/themes.
open_menu
wait_for 'menu > item' 5 "Light, Dark, Base, manjaro, waybar"
wait_for 'menu > separator' 1
wait_for 'menu > item.checked:nth-child(1)' 1 "Light checked"
wait_for 'menu > item.checked:nth-child(4)' 1 "Base checked"
shot_surface menu popup

click_widget 'menu > item:nth-child(5)'
assert_no_surface popup
assert_eq "$(aria debug theme)" "style=manjaro scheme=light" "manjaro picked"
[ "$(sample)" != "$base_light" ] || { echo "manjaro's clock looks as the base's"; exit 1; }

open_menu
wait_for 'menu > item.checked:nth-child(5)' 1 "manjaro checked"
click_widget 'menu > item:nth-child(2)'
assert_no_surface popup
assert_eq "$(aria debug theme)" "style=manjaro scheme=dark" "Dark from the menu"

open_menu
click_widget 'menu > item:nth-child(4)'
assert_eq "$(aria debug theme)" "style=- scheme=dark" "Base picked"
assert_eq "$(sample)" "$base_dark" "back to the base bar"
