# Borders by side (`border-top`, `border-bottom`, ...), on a theme of
# the scenario's own: they take room as in CSS (the content sits inside
# the border, then the padding), are drawn sharp on the pixel grid
# wherever the bar puts the box, and a button's follow its `:hover`
# (a transparent side keeps the room of the one shown on hover, the
# button doesn't move). Inset shadows: bands inside the box taking no
# room, a child's over its parent's (waybar's underline). Told apart by
# the pixels sampled below (pure colours, opaque, the bar black).

conf=$(mktemp -d)
trap 'rm -rf "$conf"' EXIT
cp -r "$XDG_CONFIG_HOME/." "$conf"
cat > "$conf/borders.css" << 'EOF'
panel {
  background: #000000;
  border-top: 2px solid #0000ff;
  box-shadow: inset 0 -2px #00ffff;
}
gadget.screenshot > button {
  height: fill;
  background: #000000;
  box-shadow: inset 0 -3px #ff00ff;
}
gadget.themes > button {
  background: #000000;
  padding: 0 4px;
  border-bottom: 4px solid #ff0000;
  border-left: 2px solid transparent;
}
gadget.themes > button:hover {
  background: #ffffff;
  border-bottom: 4px solid #00ff00;
  border-left: 2px solid #ffff00;
}
EOF
sed -i "s|^style =.*|style = $conf/borders.css|" "$conf/aria-shell/aria.conf"
restart_shell "$conf"
settle 1

sample() { grim -g "$1,$2 1x1" -t ppm - | tail -c 3 | od -An -tu1 | tr -s ' ' | sed 's/^ //'; }
black="0 0 0"; red="255 0 0"; green="0 255 0"; blue="0 0 255"; yellow="255 255 0"
cyan="0 255 255"; magenta="255 0 255"

# The content area of the button: its box reaches 4 px (padding) + 2 px
# (the left border) further left, 4 px (the bottom border) further down.
set -- $(widget 'panel[output="HEADLESS-1"] gadget.themes > button')
x=$1; y=$2; w=$3; h=$4
cx=$((x + w / 2))
shot bar "$((x - 60)),0 $((w + 120))x$((y + h + 10))"

assert_eq "$(sample "$cx" 0)" "$blue" "the panel's top border"
assert_eq "$(sample "$cx" 1)" "$blue" "2 px of it"
assert_eq "$(sample "$cx" 2)" "$black" "no more"
assert_eq "$(sample "$cx" $((y + h)))" "$red" "the button's bottom border, below the content"
assert_eq "$(sample "$cx" $((y + h + 3)))" "$red" "4 px of it"
assert_eq "$(sample "$cx" $((y + h + 4)))" "$black" "no more"
assert_eq "$(sample $((x - 6)) $((y + h / 2)))" "$black" "a transparent left side"

# Hovered: the bottom turns green, the left side yellow. Sharp: the row
# across the left edge is the bar, then two whole yellow pixels, then
# the button's white (the widget's position is a whole pixel down from
# the layout's, the box snapped to the nearest: the side is found, not
# expected at x - 6). The corner where they meet is the bottom's.
pointer "$cx" $((y + h / 2))
settle 0.5
shot bar-hover "$((x - 60)),0 $((w + 120))x$((y + h + 10))"
assert_eq "$(widget 'panel[output="HEADLESS-1"] gadget.themes > button')" "$x $y $w $h" "nothing moves"
assert_eq "$(sample "$cx" $((y + h)))" "$green" "hover: the bottom border"
row=""; left=""
for i in 9 8 7 6 5 4 3 2; do
    case $(sample $((x - i)) $((y + h / 2))) in
        "$black") row="${row}k" ;;
        "$yellow") row="${row}y"; [ -n "$left" ] || left=$((x - i)) ;;
        "255 255 255") row="${row}w" ;;
        *) row="${row}?" ;;
    esac
done
echo "$row" | grep -qx 'k*yyw*' || { echo "left edge on hover: $row (k bar, y border, w button)"; exit 1; }
assert_eq "$(sample "$left" $((y + h + 3)))" "$green" "the corner is the bottom's"

pointer 1000 600
settle 0.5
assert_eq "$(sample "$cx" $((y + h)))" "$red" "back from hover"
assert_eq "$(sample $((x - 6)) $((y + h / 2)))" "$black" "the left border transparent again"

# The bar's inset shadow along its bottom, where nothing covers it (the
# button above is shorter than the bar); the screenshot button, as tall
# as the bar, draws its own over it, and takes no room for it.
set -- $(surfaces | tr ';' '\n' | grep 'panel HEADLESS-1 ')
bar_h=${4#*x}
assert_eq "$(sample "$cx" $((bar_h - 1)))" "$cyan" "the bar's inset shadow"
assert_eq "$(sample "$cx" $((bar_h - 2)))" "$cyan" "2 px of it"
assert_eq "$(sample "$cx" $((bar_h - 3)))" "$black" "no more"
set -- $(widget 'panel[output="HEADLESS-1"] gadget.screenshot > button')
sx=$(($1 + $3 / 2))
shot bar-inset "$(($1 - 40)),0 $(($3 + 80))x$bar_h"
assert_eq "$(($2 + $4))" "$bar_h" "the button's content down to the bar's bottom"
assert_eq "$(sample "$sx" $((bar_h - 1)))" "$magenta" "the button's inset shadow over the bar's"
assert_eq "$(sample "$sx" $((bar_h - 3)))" "$magenta" "3 px of it"
assert_eq "$(sample "$sx" $((bar_h - 4)))" "$black" "no more"
