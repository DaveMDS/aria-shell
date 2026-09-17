# The workspaces gadget on the compositor's state (Sway here): one
# workspace per output to start with, the active one marked; windows
# appear in their workspace with the active one's title after the row;
# workspaces come and go, urgency shows, and a click on a workspace
# activates it.

ws='gadget.workspaces > workspace'
p1='panel[output="HEADLESS-1"]'
p2='panel[output="HEADLESS-2"]'

# The gadget on the first bar, with some room after it.
shot_gadget() {
    name=$1
    set -- $(widget "$p1 gadget.workspaces")
    shot "$name" "$(($1 - 4)),$2 $(($3 + 160))x$4"
}

# Until the gadget shows what the compositor has (the initial fetch is
# async), else fails.
wait_for() {
    i=0
    while [ "$(count_widgets "$1")" != "$2" ] && [ $i -lt 30 ]; do
        sleep 0.1; i=$((i + 1))
    done
    assert_eq "$(count_widgets "$1")" "$2" "$3"
}

# Sway starts with one workspace per output, the first one focused.
wait_for "$p1 $ws" 1 "one workspace on the first bar"
wait_for "$p2 $ws" 1 "one workspace on the second bar"
assert_eq "$(count_widgets "$p1 workspace.active[name=\"1\"]")" 1 "workspace 1 active on the first bar"
assert_eq "$(count_widgets "$p2 workspace.active[name=\"2\"]")" 1 "workspace 2 active on the second bar"
assert_eq "$(count_widgets 'gadget.workspaces > title')" 0 "no active window, no title"

# A window lands on the focused workspace, focused: its icon in the
# workspace, its title after the row (on that output's bar only).
open_window aria-one "Window one"
wait_for "$p1 $ws > window" 1 "the window in workspace 1"
assert_eq "$(count_widgets "$p1 workspace[name=\"1\"] > window.active[class=\"aria-one\"]")" 1 "the window is active"
assert_eq "$(count_widgets "$p1 gadget.workspaces > title[class=\"aria-one\"]")" 1 "its title after the row"
assert_eq "$(count_widgets "$p2 gadget.workspaces > title")" 0 "not on the other bar"
shot_gadget bar-window

# A title change follows.
retitle_window aria-one "Window one, renamed"
settle 0.5
set -- $(widget "$p1 gadget.workspaces > title > text")
old_w=$3
assert_eq "$(count_widgets "$p1 gadget.workspaces > title[class=\"aria-one\"]")" 1 "still the title"

# A second window: two in the workspace, the new one active.
open_window aria-two
wait_for "$p1 workspace[name=\"1\"] > window" 2 "two windows in workspace 1"
assert_eq "$(count_widgets "$p1 window.active[class=\"aria-two\"]")" 1 "the new window is active"
assert_eq "$(count_widgets "$p1 window.active")" 1 "only one active window"

# Switching to a new workspace creates it, empty, active; the title goes
# with the focus (nothing is focused there).
swaymsg workspace 3 > /dev/null
wait_for "$p1 $ws" 2 "a second workspace on the first bar"
assert_eq "$(count_widgets "$p1 workspace.active[name=\"3\"]")" 1 "workspace 3 active"
assert_eq "$(count_widgets "$p1 workspace.active")" 1 "only one active workspace per output"
assert_eq "$(count_widgets "$p1 gadget.workspaces > title")" 0 "no focused window, no title"
assert_eq "$(count_widgets "$p2 $ws")" 1 "the other bar unchanged"

# The window left behind, made urgent, marks its workspace.
swaymsg '[app_id=aria-two] urgent enable' > /dev/null
wait_for "$p1 workspace.urgent[name=\"1\"]" 1 "workspace 1 urgent"
assert_eq "$(count_widgets "$p1 window.urgent[class=\"aria-two\"]")" 1 "the window urgent"
shot_gadget bar-urgent

# A click on the urgent workspace goes there: focus returns to the
# window, urgency clears, the empty workspace 3 disappears.
click_widget "$p1 workspace[name=\"1\"]"
wait_for "$p1 $ws" 1 "the empty workspace is gone"
assert_eq "$(count_widgets "$p1 workspace.active[name=\"1\"]")" 1 "back on workspace 1"
assert_eq "$(count_widgets "$p1 workspace.urgent")" 0 "no urgency left"
assert_eq "$(count_widgets "$p1 window.active[class=\"aria-two\"]")" 1 "its window focused again"
assert_eq "$(count_widgets "$p1 gadget.workspaces > title[class=\"aria-two\"]")" 1 "and titled"

# Moving a window to the other output's workspace: it shows there.
swaymsg '[app_id=aria-one] move workspace 2' > /dev/null
wait_for "$p2 workspace[name=\"2\"] > window" 1 "the window moved to workspace 2"
assert_eq "$(count_widgets "$p1 workspace[name=\"1\"] > window")" 1 "one left in workspace 1"

# Closing windows empties the rows.
close_window aria-two
wait_for "$p1 $ws > window" 0 "workspace 1 empty"
assert_eq "$(count_widgets "$p1 gadget.workspaces > title")" 0 "no title without a focused window"
close_window aria-one
wait_for "$p2 $ws > window" 0 "workspace 2 empty"
