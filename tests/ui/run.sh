#!/bin/sh
# Runs UI scenarios against the shell inside a headless, nested Sway:
# no display output, no dependency on the desktop's compositor, no root.
#
#   tests/ui/run.sh              every scenario in tests/ui/scenarios/
#   tests/ui/run.sh launcher     one of them
#   tests/ui/run.sh -j 4 ...     four at a time (default: one per CPU)
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
#
# Nothing is shared between two scenarios (the shell's socket is named
# after the nested Sway's display), so they run side by side, each line
# printed as its scenario ends.
set -u
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)

# One scenario (`run.sh --one <name>`): what the run below starts, a few
# at a time, in the environment it set up. How long it took goes in
# `seconds`, for the next run's order.
if [ "${1:-}" = --one ]; then
    scenario=$2
    out=$root/target/ui/$scenario
    rm -rf "$out" && mkdir -p "$out"
    start=$(date +%s)
    export ARIA_UI_OUT=$out ARIA_UI_SCENARIO=$here/scenarios/$scenario.sh
    export XDG_STATE_HOME=$out/state
    timeout 120 dbus-run-session -- sway -c "$here/sway.conf" > "$out/sway.log" 2>&1
    echo $(($(date +%s) - start)) > "$out/seconds"
    status=$(cat "$out/status" 2>/dev/null || echo "the scenario didn't finish (see $out/sway.log)")
    echo "$scenario: $status"
    exit
fi

jobs=$(nproc)
if [ "${1:-}" = -j ]; then
    jobs=$2
    shift 2
fi

for tool in sway swaymsg grim dbus-run-session wl-paste wl-copy python3; do
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
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1
# Sway composes on the GPU when there is one: under pixman the shell's
# wgpu falls back to llvmpipe, a core busy at rest and popups taking
# seconds on a laptop. Without a render node, or with WLR_RENDERER set,
# that's what it gets.
if [ -z "${WLR_RENDERER:-}" ]; then
    WLR_RENDERER=pixman
    for node in /dev/dri/renderD*; do
        [ -e "$node" ] && WLR_RENDERER=gles2
    done
fi
export WLR_RENDERER
export XDG_CONFIG_HOME=$here/config
export XDG_DATA_HOME=$here/data
export XDG_DATA_DIRS=${XDG_DATA_DIRS:-/usr/local/share:/usr/share}
export ARIA_UI_ROOT=$root

# The longest first, as long as they took last time: the last to start
# shouldn't be one that keeps the others waiting.
for scenario in $scenarios; do
    echo "$(cat "$root/target/ui/$scenario/seconds" 2> /dev/null || echo 0) $scenario"
done | sort -rn | cut -d ' ' -f 2 | xargs -n 1 -P "$jobs" "$here/run.sh" --one

failed=
for scenario in $scenarios; do
    [ "$(cat "$root/target/ui/$scenario/status" 2> /dev/null)" = ok ] || failed="$failed $scenario"
done
[ -z "$failed" ] || { echo "failed:$failed"; exit 1; }
