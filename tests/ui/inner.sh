#!/bin/sh
# Runs inside the nested Sway (exec'd from sway.conf, so WAYLAND_DISPLAY
# and SWAYSOCK are the nested compositor's): starts the shell, runs the
# scenario with lib.sh's vocabulary, records the outcome, ends Sway.
out=$ARIA_UI_OUT
cd "$ARIA_UI_ROOT" || exit 1
. "$ARIA_UI_ROOT/tests/ui/lib.sh"

# The input devices come first and stay for the whole scenario: Sway
# resets keyboard focus when the seat gets its first keyboard, which
# would close the launcher under the first keystroke otherwise.
inject_start
# Let the second output appear before the shell looks around.
sleep 0.5
RUST_LOG=aria_shell=debug "$ARIA_UI_ROOT/target/debug/aria-shell" > "$out/shell.log" 2>&1 &
echo $! > "$out/shell.pid"

if wait_for_shell; then
    # Not as an `if` condition: `set -e` would be ignored in there.
    (set -e; . "$ARIA_UI_SCENARIO") > "$out/scenario.log" 2>&1
    if [ $? -eq 0 ]; then
        echo ok > "$out/status"
    else
        echo "failed: $(tail -n 1 "$out/scenario.log")" > "$out/status"
    fi
else
    echo "the shell didn't answer on its socket" > "$out/status"
fi

kill "$(cat "$out/shell.pid")" 2> /dev/null
inject_stop
swaymsg exit
