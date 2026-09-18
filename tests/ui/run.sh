#!/bin/sh
# Runs UI scenarios against the shell inside a headless, nested Sway:
# no GPU output, no dependency on the desktop's compositor, no root.
#
#   tests/ui/run.sh              every scenario in tests/ui/scenarios/
#   tests/ui/run.sh launcher     one of them
#
# Each scenario gets its own Sway (two 1920x1080 outputs, see
# sway.conf) on its own session bus (so the shell's tray sees the
# scenario's fake item, not the desktop's, and owns the watcher name),
# a fresh shell with tests/ui/config as XDG_CONFIG_HOME, tests/ui/data
# as XDG_DATA_HOME on top of the system data dirs and an empty
# XDG_STATE_HOME (no launcher usage carried over), and writes to
# target/ui/<scenario>/: shell.log, sway.log, the screenshots it takes,
# `state/` and `status` (ok, or what failed). Input goes through
# `aria-inject` (tests/ui/inject: a virtual keyboard and pointer);
# positions come from the shell's `debug` commands; tray items through
# `aria-sni` (tests/ui/sni). See lib.sh for the vocabulary.
set -u
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)

for tool in sway swaymsg grim dbus-run-session; do
    command -v "$tool" > /dev/null || { echo "missing: $tool" >&2; exit 2; }
done
cargo build --quiet --workspace --manifest-path "$root/Cargo.toml" || exit 2

if [ $# -gt 0 ]; then
    scenarios=$*
else
    scenarios=$(ls "$here/scenarios" | sed 's/\.sh$//')
fi

# The nested compositor must not be mistaken for the desktop's.
unset HYPRLAND_INSTANCE_SIGNATURE SWAYSOCK I3SOCK NIRI_SOCKET
export WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1
export XDG_CONFIG_HOME=$here/config
export XDG_DATA_HOME=$here/data
export XDG_DATA_DIRS=${XDG_DATA_DIRS:-/usr/local/share:/usr/share}
export ARIA_UI_ROOT=$root

failed=0
for scenario in $scenarios; do
    out=$root/target/ui/$scenario
    rm -rf "$out" && mkdir -p "$out"
    export ARIA_UI_OUT=$out ARIA_UI_SCENARIO=$here/scenarios/$scenario.sh
    export XDG_STATE_HOME=$out/state
    timeout 120 dbus-run-session -- sway -c "$here/sway.conf" > "$out/sway.log" 2>&1
    status=$(cat "$out/status" 2>/dev/null || echo "the scenario didn't finish (see $out/sway.log)")
    echo "$scenario: $status"
    [ "$status" = ok ] || failed=1
done
exit $failed
