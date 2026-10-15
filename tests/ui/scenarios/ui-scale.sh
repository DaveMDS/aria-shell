# [general] scale: the whole UI drawn that much bigger, every surface
# that much bigger for the compositor (a bar, its popup, the launcher,
# the OSD), the pointer where it's drawn (a click on a widget lands on
# it), the picker's selection in the outputs' pixels; changed while the
# shell runs, the surfaces follow.

# `w h` of the first surface of a kind on the first output.
size_of() { surfaces | tr ';' '\n' | grep "^ *$1 HEADLESS-1 " | head -n 1 | awk '{print $4}' | tr x ' '; }
# $1 is $2 times $3, within a pixel (rounding).
assert_times() {
    want=$(python3 -I -c "print(round($2 * $3))")
    [ $(($1 - want)) -le 1 ] && [ $((want - $1)) -le 1 ] ||
        { echo "$4: $1, not $2 x $3 = $want"; return 1; }
}
clock='surface.panel[output="HEADLESS-1"] slot.center gadget.clock > indicator'

# At 1: what the theme says.
set -- $(size_of panel)
bar=$2
click_widget "$clock"
assert_surface popup
set -- $(size_of popup)
popup_w=$1 popup_h=$2
click 300 900
assert_no_surface popup
aria launcher show
assert_surface launcher
set -- $(size_of launcher)
launcher_w=$1 launcher_h=$2
aria launcher hide
assert_no_surface launcher

conf=$(mktemp -d)
trap 'rm -rf "$conf"' EXIT
cp -r "$XDG_CONFIG_HOME/." "$conf"
sed -i -e 's/^\[general\]$/[general]\nscale = 1.5/' -e 's/^reload_config =.*/reload_config = yes/' \
    "$conf/aria-shell/aria.conf"
restart_shell "$conf"
settle 1

# At 1.5: all of it half as big again.
set -- $(size_of panel)
assert_times "$2" "$bar" 1.5 "the bar"
# Widgets reported where they're drawn: the click opens the clock's.
click_widget "$clock"
assert_surface popup
set -- $(size_of popup)
assert_times "$1" "$popup_w" 1.5 "the popup's width"
assert_times "$2" "$popup_h" 1.5 "the popup's height"
# Under its clock, centred.
set -- $(widget "$clock")
centre=$(($1 + $3 / 2))
set -- $(surfaces | tr ';' '\n' | grep "^ *popup " | awk '{print $3, $4}' | tr ',x' '  ')
assert_times "$(($1 + $3 / 2))" "$centre" 1 "the popup under its clock"
shot popup "0,0 1920x700"
click 300 900
assert_no_surface popup
aria launcher show
assert_surface launcher
set -- $(size_of launcher)
assert_times "$1" "$launcher_w" 1.5 "the launcher's width"
assert_times "$2" "$launcher_h" 1.5 "the launcher's height"
shot launcher
aria launcher hide
assert_no_surface launcher

# The picker: a drag in the outputs' pixels, whatever the UI's scale.
aria screenshot
i=0
while [ "$(surfaces | tr ';' '\n' | grep -c '^ *screenshot ')" -lt 2 ] && [ $i -lt 60 ]; do
    sleep 0.05; i=$((i + 1))
done
# The pointer on it first, as a hand would: the picker sees it come.
pointer 960 540
settle
drag 100 100 500 400
i=0
while ! aria debug screenshot | grep -q "selection=100,100 400x300" && [ $i -lt 60 ]; do
    sleep 0.05; i=$((i + 1))
done
assert_contains "$(aria debug screenshot)" "selection=100,100 400x300" "the picker's selection"
key Escape
assert_no_surface screenshot

# Changed while running: the bars follow.
sed -i 's/^scale = 1.5$/scale = 2/' "$conf/aria-shell/aria.conf"
want=$((bar * 2))
i=0
while [ "$(size_of panel | awk '{print $2}')" != "$want" ] && [ $i -lt 60 ]; do
    sleep 0.05; i=$((i + 1))
done
set -- $(size_of panel)
assert_times "$2" "$bar" 2 "the bar, the scale changed"
