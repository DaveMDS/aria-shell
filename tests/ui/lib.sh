# Vocabulary for the UI scenarios (sourced by inner.sh, then by each
# scenario with `set -e`: any failing command fails the scenario, and
# the last line of scenario.log says which).
#
# Positions come from the shell (`aria debug surfaces|widgets`), global
# coordinates in the nested Sway, where nothing else reserves space so
# they are exact. Widgets are picked with theme selectors
# (`launcher item:nth-child(3)`, `panel[output="HEADLESS-2"] gadget.clock
# > button`): `widget <selector>` prints the first match as `x y w h`,
# `click_widget <selector>` clicks its centre.

aria() {
    "$ARIA_UI_ROOT/target/debug/aria-shell" "$@"
}

# Until the shell answers `ping` (5s), else fails.
wait_for_shell() {
    i=0
    while [ $i -lt 50 ]; do
        aria ping > /dev/null 2>&1 && return 0
        sleep 0.1
        i=$((i + 1))
    done
    return 1
}

# Give the shell time to act on the last input and redraw.
settle() {
    sleep "${1:-0.3}"
}

# --- looking -------------------------------------------------------------

surfaces() {
    aria debug surfaces
}

# `x y w h` of the first widget the selector matches; fails when there
# is none.
widget() {
    line=$(aria debug widgets "$1" | tr ';' '\n' | head -n 1 | sed 's/^ *//')
    [ -n "$line" ] || { echo "no widget matches '$1'"; return 1; }
    echo "$line" | sed 's/.* \([0-9-]*\),\([0-9-]*\) \([0-9]*\)x\([0-9]*\)$/\1 \2 \3 \4/'
}

# How many widgets the selector matches.
count_widgets() {
    aria debug widgets "$1" | tr ';' '\n' | grep -c .
}

# Screenshot of the whole layout, or of `x,y wxh`, into the output dir.
shot() {
    if [ $# -gt 1 ]; then
        grim -g "$2" "$ARIA_UI_OUT/$1.png"
    else
        grim "$ARIA_UI_OUT/$1.png"
    fi
}

# Screenshot of the surface of the given kind (first match).
shot_surface() {
    geo=$(surfaces | tr ';' '\n' | grep " $2 " | head -n 1 | awk '{print $3" "$4}')
    [ -n "$geo" ] || { echo "no $2 surface to screenshot"; return 1; }
    grim -g "$geo" "$ARIA_UI_OUT/$1.png"
}

# --- asserting -----------------------------------------------------------

assert_eq() {
    [ "$1" = "$2" ] || { echo "${3:-assertion}: expected '$2', got '$1'"; return 1; }
}

assert_contains() {
    case "$1" in
        *"$2"*) ;;
        *) echo "${3:-assertion}: '$2' not found in: $1"; return 1 ;;
    esac
}

assert_not_contains() {
    case "$1" in
        *"$2"*) echo "${3:-assertion}: '$2' unexpectedly in: $1"; return 1 ;;
    esac
}

assert_surface() {
    assert_contains "$(surfaces)" "$1 " "surface $1 open"
}

assert_no_surface() {
    assert_not_contains "$(surfaces)" "$1 " "surface $1 closed"
}

# The shell log contains the text (retrying for a moment: launching is
# async).
assert_logged() {
    i=0
    while [ $i -lt 20 ]; do
        grep -q -- "$1" "$ARIA_UI_OUT/shell.log" && return 0
        sleep 0.1
        i=$((i + 1))
    done
    echo "'$1' not in shell.log"
    return 1
}

# --- acting --------------------------------------------------------------
# Input goes through `aria-inject` (tests/ui/inject), one process for the
# whole scenario holding a virtual keyboard and pointer, talked to over
# two fifos: a command line in, `ok`/`err ...` out.

inject_start() {
    inject_dir=$(mktemp -d)
    mkfifo "$inject_dir/in" "$inject_dir/out"
    "$ARIA_UI_ROOT/target/debug/aria-inject" < "$inject_dir/in" > "$inject_dir/out" &
    inject_pid=$!
    exec 7> "$inject_dir/in" 8< "$inject_dir/out"
    inject "layout 3840 1080"
}

inject_stop() {
    exec 7>&- 8<&-
    kill "$inject_pid" 2> /dev/null
    rm -rf "$inject_dir"
}

# One injector command; fails on `err`.
inject() {
    echo "$1" >&7
    read -r reply <&8
    [ "$reply" = ok ] || { echo "inject '$1': $reply"; return 1; }
}

# Pointer to global x,y.
pointer() {
    inject "move $1 $2"
}

click() {
    pointer "$1" "$2"
    inject click
    settle
}

click_widget() {
    set -- $(widget "$1")
    click $(($1 + $3 / 2)) $(($2 + $4 / 2))
}

type_text() {
    inject "type $1"
    settle
}

# A named key, an xkb keysym: Down Up Return Escape BackSpace Tab ...
key() {
    inject "key $1"
    settle
}
