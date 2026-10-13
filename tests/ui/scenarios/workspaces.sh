# The workspaces gadget on the compositor's state (Sway here): one
# workspace per output to start with, the active one marked; windows
# appear in their workspace with the active one's title after the row;
# workspaces come and go, urgency shows, and a click on a workspace
# activates it.

ws='gadget.workspaces > button.workspace'
p1='surface.panel[output="HEADLESS-1"]'
p2='surface.panel[output="HEADLESS-2"]'

# The gadget on the first bar, with some room after it.
shot_gadget() {
    name=$1
    set -- $(widget "$p1 gadget.workspaces")
    shot "$name" "$(($1 - 4)),$2 $(($3 + 160))x$4"
}

# Sway starts with one workspace per output, the first one focused.
wait_for "$p1 $ws" 1 "one workspace on the first bar"
wait_for "$p2 $ws" 1 "one workspace on the second bar"
wait_for "$p1 button.workspace.active[name=\"1\"]" 1 "workspace 1 active on the first bar"
wait_for "$p2 button.workspace.active[name=\"2\"]" 1 "workspace 2 active on the second bar"
wait_for 'gadget.workspaces > title' 0 "no active window, no title"

# A window lands on the focused workspace, focused: its icon in the
# workspace, its title after the row (on that output's bar only).
open_window aria-one "Window one"
wait_for "$p1 $ws > window" 1 "the window in workspace 1"
wait_for "$p1 button.workspace[name=\"1\"] > window.active[class=\"aria-one\"]" 1 "the window is active"
wait_for "$p1 gadget.workspaces > title[class=\"aria-one\"]" 1 "its title after the row"
wait_for "$p2 gadget.workspaces > title" 0 "not on the other bar"
shot_gadget bar-window

# A title change follows.
retitle_window aria-one "Window one, renamed"
settle 0.5
set -- $(widget "$p1 gadget.workspaces > title > label")
old_w=$3
wait_for "$p1 gadget.workspaces > title[class=\"aria-one\"]" 1 "still the title"

# A title longer than the room left: one line all the same, cut where
# the start slot (a third of the bar inside its padding) ends.
set -- $(widget "$p1 slot.start")
slot_end=$(($1 + $3))
set -- $(widget "$p1 gadget.workspaces > title")
short_h=$4
retitle_window aria-one "$(printf 'A very long window title %.0s' 1 2 3 4 5 6 7 8)"
settle 0.5
set -- $(widget "$p1 gadget.workspaces > title")
assert_eq "$4" "$short_h" "a long title on one line"
[ "$(($1 + $3))" -le "$slot_end" ] || { echo "the long title past the start slot ($slot_end): $*"; exit 1; }
shot_gadget bar-long-title
retitle_window aria-one "Window one, renamed"

# A second window: two in the workspace, the new one active.
open_window aria-two
wait_for "$p1 button.workspace[name=\"1\"] > window" 2 "two windows in workspace 1"
wait_for "$p1 window.active[class=\"aria-two\"]" 1 "the new window is active"
wait_for "$p1 window.active" 1 "only one active window"

# Switching to a new workspace creates it, empty, active; the title goes
# with the focus (nothing is focused there).
swaymsg workspace 3 > /dev/null
wait_for "$p1 $ws" 2 "a second workspace on the first bar"
wait_for "$p1 button.workspace.active[name=\"3\"]" 1 "workspace 3 active"
wait_for "$p1 button.workspace.active" 1 "only one active workspace per output"
wait_for "$p1 gadget.workspaces > title" 0 "no focused window, no title"
wait_for "$p2 $ws" 1 "the other bar unchanged"

# The window left behind, made urgent, marks its workspace.
swaymsg '[app_id=aria-two] urgent enable' > /dev/null
wait_for "$p1 button.workspace.urgent[name=\"1\"]" 1 "workspace 1 urgent"
wait_for "$p1 window.urgent[class=\"aria-two\"]" 1 "the window urgent"
shot_gadget bar-urgent

# A click on the urgent workspace goes there: focus returns to the
# window, urgency clears, the empty workspace 3 disappears.
click_widget "$p1 button.workspace[name=\"1\"]"
wait_for "$p1 $ws" 1 "the empty workspace is gone"
wait_for "$p1 button.workspace.active[name=\"1\"]" 1 "back on workspace 1"
wait_for "$p1 button.workspace.urgent" 0 "no urgency left"
wait_for "$p1 window.active[class=\"aria-two\"]" 1 "its window focused again"
wait_for "$p1 gadget.workspaces > title[class=\"aria-two\"]" 1 "and titled"

# Moving a window to the other output's workspace: it shows there.
swaymsg '[app_id=aria-one] move workspace 2' > /dev/null
wait_for "$p2 button.workspace[name=\"2\"] > window" 1 "the window moved to workspace 2"
wait_for "$p1 button.workspace[name=\"1\"] > window" 1 "one left in workspace 1"

# Closing windows empties the rows.
close_window aria-two
wait_for "$p1 $ws > window" 0 "workspace 1 empty"
wait_for "$p1 gadget.workspaces > title" 0 "no title without a focused window"
close_window aria-one
wait_for "$p2 $ws > window" 0 "workspace 2 empty"
