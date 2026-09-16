# The Themes gadget: a left click toggles light/dark (the palette
# changes, the tray menu rules follow), a right click lists the schemes
# and the themes found; picking a theme restyles live (manjaro's bar is
# 28px, the base one 32px), picking Base goes back.

assert_eq "$(aria debug theme)" "style=- scheme=light" "starts light, base only"
assert_eq "$(count_widgets 'gadget.themes > button')" 2 "one per bar"
set -- $(widget 'panel[output="HEADLESS-1"] gadget.themes > button')
shot bar-light "$(($1 - 60)),0 160x32"

click_widget 'panel[output="HEADLESS-1"] gadget.themes > button'
assert_eq "$(aria debug theme)" "style=- scheme=dark" "left click toggles"
shot bar-dark "$(($1 - 60)),0 160x32"
click_widget 'panel[output="HEADLESS-2"] gadget.themes > button'
assert_eq "$(aria debug theme)" "style=- scheme=light" "and back, from either bar"

# The menu: schemes, then the base and every theme in assets/themes.
click_widget_with right 'panel[output="HEADLESS-1"] gadget.themes > button'
assert_surface popup
assert_eq "$(count_widgets 'menu > item')" 5 "Light, Dark, Base, manjaro, waybar"
assert_eq "$(count_widgets 'menu > separator')" 1
assert_eq "$(count_widgets 'menu > item.checked:nth-child(1)')" 1 "Light checked"
assert_eq "$(count_widgets 'menu > item.checked:nth-child(4)')" 1 "Base checked"
shot_surface menu popup

click_widget 'menu > item:nth-child(5)'
assert_no_surface popup
assert_eq "$(aria debug theme)" "style=manjaro scheme=light" "manjaro picked"
assert_contains "$(surfaces)" "panel HEADLESS-1 0,0 1920x28" "manjaro's 28px bar"

click_widget_with right 'panel[output="HEADLESS-1"] gadget.themes > button'
assert_surface popup
assert_eq "$(count_widgets 'menu > item.checked:nth-child(5)')" 1 "manjaro checked"
click_widget 'menu > item:nth-child(2)'
assert_no_surface popup
assert_eq "$(aria debug theme)" "style=manjaro scheme=dark" "Dark from the menu"

click_widget_with right 'panel[output="HEADLESS-1"] gadget.themes > button'
click_widget 'menu > item:nth-child(4)'
assert_eq "$(aria debug theme)" "style=- scheme=dark" "Base picked"
assert_contains "$(surfaces)" "panel HEADLESS-1 0,0 1920x32" "back to the base bar"
