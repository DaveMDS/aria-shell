# One shell per display: a second one started on the same compositor
# finds the display's lock taken and leaves, before touching anything
# (the first keeps its socket, its bars, and its autostart runs once).

before=$(surfaces)
lock="$XDG_RUNTIME_DIR/aria-shell/$WAYLAND_DISPLAY.lock"
[ -f "$lock" ] || { echo "no lock file at $lock"; exit 1; }

status=0
timeout 10 "$ARIA_UI_ROOT/target/debug/aria-shell" > "$ARIA_UI_OUT/second.log" 2>&1 || status=$?
assert_eq "$status" 1 "the second shell exits with an error"
assert_contains "$(cat "$ARIA_UI_OUT/second.log")" "already running on this display" "and says why"

assert_eq "$(aria ping)" pong "the first one still answers"
assert_eq "$(surfaces)" "$before" "with the same surfaces"
assert_eq "$(grep -c first "$ARIA_UI_OUT/autostart")" 1 "autostart ran once"

# Once the first is gone, the lock goes with it: a new shell starts.
restart_shell "$XDG_CONFIG_HOME"
assert_eq "$(aria ping)" pong "a shell again after the first ended"
