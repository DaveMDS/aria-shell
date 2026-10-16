# A theme's own config (`<name>.conf` beside its `.css`), read over the
# user's with `[general] theme_config = yes`: its keys win, the rest
# stays the user's; its panels replace every panel of the user's, its
# wallpaper every wallpaper section (`[wallpaper:HEADLESS-2]` too); `./`
# is the theme's folder. The Themes menu's check stops reading it (the
# user's panels back), and starts again; picking another theme leaves
# it.

conf=$(mktemp -d)
trap 'rm -rf "$conf"' EXIT
cp -r "$XDG_CONFIG_HOME/." "$conf"
theme="$conf/aria-shell/themes/shelf"
mkdir -p "$theme"
touch "$theme/shelf.css"
cp "$XDG_CONFIG_HOME/aria-shell/wallpapers/b.png" "$theme/bg.png"
cat > "$theme/shelf.conf" << 'EOF'
[general]
color_scheme = dark

[panel]
position = bottom
items_center = Themes Clock

[Clock]
format = shelf

[wallpaper]
source = ./bg.png
fit = fill
EOF
sed -i -e "s|^theme =.*|theme = shelf\ntheme_config = yes|" "$conf/aria-shell/aria.conf"
restart_shell "$conf"
settle 1

assert_eq "$(aria debug theme)" "theme=shelf scheme=dark config=$theme/shelf.conf" \
    "the theme's config read, its scheme"
assert_contains "$(surfaces)" "panel HEADLESS-1 0,1052 1920x28" "its panel, at the bottom"
wait_for 'gadget' 4 "its two gadgets on each output, nothing of the user's"
wait_for 'gadget.clock > indicator > label' 2
assert_eq "$(aria debug wallpaper | grep -o "\[wallpaper[^]]*\]" | wc -l)" 1 "one wallpaper section, the user's HEADLESS-2 one left out"
assert_contains "$(aria debug wallpaper)" "outputs=HEADLESS-1,HEADLESS-2 source=$theme/bg.png" "./ in the theme's folder"
assert_contains "$(aria debug locale)" "lang=en" "the user's language kept"

# The menu's last item: stop reading it.
click_widget_with right 'surface.panel[output="HEADLESS-1"] gadget.themes > indicator'
assert_surface popup
wait_for 'menu > item:checked' 3 "Dark, shelf, the theme's settings"
shot_surface menu popup
click_widget 'menu > item:last-child'
assert_no_surface popup
settle 1
assert_eq "$(aria debug theme)" "theme=shelf scheme=light" "the user's config alone, its scheme"
assert_contains "$(surfaces)" "panel HEADLESS-1 0,0 1920x28" "the user's panel, at the top"
assert_eq "$(aria debug wallpaper | grep -o "\[wallpaper[^]]*\]" | wc -l)" 2 "the user's two wallpapers"

# And back on, then another theme: the user's config again.
click_widget_with right 'surface.panel[output="HEADLESS-1"] gadget.themes > indicator'
assert_surface popup
click_widget 'menu > item:last-child'
settle 1
assert_eq "$(aria debug theme)" "theme=shelf scheme=dark config=$theme/shelf.conf" "read again"
click_widget_with right 'surface.panel[output="HEADLESS-1"] gadget.themes > indicator'
assert_surface popup
click_widget 'menu > item:nth-child(4)'
settle 1
assert_eq "$(aria debug theme)" "theme=- scheme=light" "Base: no theme, no theme config"
assert_contains "$(surfaces)" "panel HEADLESS-1 0,0 1920x28" "the user's panel"
