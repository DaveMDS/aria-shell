# The system monitor: one gadget instance per value on both bars (text
# and a sparkline once the history has two readings); the popup opens
# on the tab of the instance's value, with the cpu (graph, one meter
# per core), memory, disks, network and process tabs; the table sorts
# by column; a right click on a row
# offers Terminate/Kill, and Terminate ends the `sleep` we started; a
# right click on the bar runs `command`, or the terminal monitor.

bell='panel[output="HEADLESS-1"] gadget#cpu.system-monitor > button.cpu'
first_pid() {
    aria debug widgets 'table row' | tr ';' '\n' | head -n 1 | sed 's/.*row\[pid="\([0-9]*\)".*/\1/'
}
row_pids() {
    aria debug widgets 'table row' | tr ';' '\n' | sed 's/.*row\[pid="\([0-9]*\)".*/\1/'
}

# The instances, and what the sampler read.
assert_eq "$(count_widgets 'gadget.system-monitor > button')" 8 "four instances per bar"
assert_eq "$(count_widgets 'gadget#cpu.system-monitor > button.cpu')" 2 "the instance id is the node's"
assert_eq "$(count_widgets 'gadget#mem.system-monitor > button.mem')" 2 "show = mem"
assert_eq "$(count_widgets 'gadget#net.system-monitor > button.net')" 2
assert_eq "$(count_widgets 'gadget#disk.system-monitor > button.disk')" 2
settle 2.5
info=$(aria debug sysmon)
assert_contains "$info" "cores=$(nproc) " "every core read"
assert_contains "$info" " mem=" "memory read"
assert_contains "$info" "disks=/" "the root filesystem first"
assert_not_contains "$info" "disks= " "at least one disk"
case "$info" in
    *"samples=1 "*|*"samples=0 "*) echo "too few samples: $info"; exit 1 ;;
esac
set -- $(widget 'gadget.system-monitor > button.cpu graph')
assert_eq "$3 $4" "40 14" "the sparkline sized by the theme (the text 'cpu' fits)"
set -- $(widget 'gadget#mem.system-monitor > button.mem gauge')
[ "$3" -ge 80 ] && [ "$4" = 14 ] || { echo "gauge ${3}x$4: a long format widens it"; exit 1; }
assert_eq "$(count_widgets 'gadget#net.system-monitor > button.net text')" 2 "mode text: a label"
assert_eq "$(count_widgets 'gadget#net.system-monitor > button.net graph')" 0 "and no graph"
assert_eq "$(count_widgets 'gadget#mem.system-monitor > button.mem text')" 0 "the gauge holds the text"
# Thresholds: the net instance is critical from 0, the disk one a
# warning from 0, the cpu one only from the percentage defaults.
assert_eq "$(count_widgets 'gadget#net.system-monitor > button.net.critical')" 2 "critical = 0"
assert_eq "$(count_widgets 'gadget#disk.system-monitor > button.disk.warning')" 2 "warning = 0"
assert_eq "$(count_widgets 'gadget#disk.system-monitor > button.disk.critical')" 0
assert_eq "$(count_widgets 'gadget#cpu.system-monitor > button.cpu.critical')" 0 "an idle cpu"
shot bar "$(($(widget "$bell" | cut -d' ' -f1) - 20)),0 520x32"

# The popup: the tabs, opened on the value's; one section at a time.
click_widget "$bell"
assert_surface popup
settle 1
for tab in cpu mem disk net processes; do
    assert_eq "$(count_widgets "monitor tabs tab.$tab")" 1 "tab $tab"
done
case "$info" in
    *"gpu=0 "*) assert_eq "$(count_widgets 'monitor tabs tab.gpu')" 0 "no gpu, no tab" ;;
    *) assert_eq "$(count_widgets 'monitor tabs tab.gpu')" 1 "a gpu tab" ;;
esac
assert_eq "$(count_widgets 'monitor tabs tab.cpu.active')" 1 "the cpu instance opens on the cpu tab"
assert_eq "$(count_widgets 'monitor section')" 1 "one section shown"
assert_eq "$(count_widgets 'section.cpu cores core')" "$(nproc)" "one meter per core"
assert_eq "$(count_widgets 'section.cpu graph')" 1
shot_surface popup-cpu popup
click_widget 'monitor tabs tab.mem'
assert_eq "$(count_widgets 'monitor tabs tab.mem.active')" 1
assert_eq "$(count_widgets 'section.mem meter.used')" 1
assert_eq "$(count_widgets 'section.cpu')" 0 "the cpu section is gone"
click_widget 'monitor tabs tab.disk'
[ "$(count_widgets 'section.disk disk')" -ge 1 ] || { echo "no disk row"; exit 1; }
assert_eq "$(count_widgets 'section.disk disk[mount="/"]')" 1 "the root mount"
click_widget 'monitor tabs tab.net'
[ "$(count_widgets 'section.net iface')" -ge 1 ] || { echo "no interface row"; exit 1; }
click_widget 'monitor tabs tab.processes'
settle 1
assert_eq "$(count_widgets 'table row')" 5 "processes = 5"
assert_eq "$(count_widgets 'table header column.cpu.sorted')" 1 "sorted by cpu at first"
shot_surface popup-processes popup

# Sort by memory: the first row is one of the two fattest processes
# (the readings aren't simultaneous).
click_widget 'table header column.mem'
assert_eq "$(count_widgets 'table header column.mem.sorted')" 1
top2=$(ps -eo pid --sort=-rss | sed -n '2,3p' | tr -d ' ')
assert_contains "$top2" "$(first_pid)" "the fattest process first"
# By pid, descending by default; a second click flips it.
click_widget 'table header column.pid'
assert_eq "$(count_widgets 'table header column.pid.sorted')" 1
first=$(first_pid)
for p in $(row_pids); do
    [ "$p" -le "$first" ] || { echo "not descending: $p > $first"; exit 1; }
done
click_widget 'table header column.pid'
assert_eq "$(count_widgets 'table header column.pid.sorted.reverse')" 1 "flipped"
first=$(first_pid)
for p in $(row_pids); do
    [ "$p" -ge "$first" ] || { echo "not ascending: $p < $first"; exit 1; }
done

# A process of ours, started now so that by pid descending it's among
# the newest (the five rows); its menu, then Terminate ends it.
sleep 1000 &
victim=$!
click_widget 'table header column.pid'
i=0
while [ "$(count_widgets 'table row[pid="'"$victim"'"][name="sleep"]')" != 1 ]; do
    i=$((i + 1))
    [ $i -le 5 ] || { echo "our sleep ($victim) never showed on the table"; exit 1; }
    settle 1
done
# The table is re-read every second and our own `aria` clients show
# up at the top (newest pids), so a click may land on a shifted row:
# try until the right row is the selected one.
i=0
while [ "$(count_widgets 'table row.selected[pid="'"$victim"'"] menu > item')" != 2 ]; do
    i=$((i + 1))
    [ $i -le 5 ] || { echo "couldn't open the sleep's menu"; exit 1; }
    # The row may be out for a tick (a burst of our own clients).
    click_widget_with right 'table row[pid="'"$victim"'"]' || settle 1
done
shot_surface popup-menu popup
# A left click on a row puts the menu away; the menu again for Terminate.
click_widget 'table row.selected[pid="'"$victim"'"]'
assert_eq "$(count_widgets 'table row menu')" 0 "a left click closes the menu"
i=0
while [ "$(count_widgets 'table row.selected[pid="'"$victim"'"] menu > item')" != 2 ]; do
    i=$((i + 1))
    [ $i -le 5 ] || { echo "couldn't reopen the sleep's menu"; exit 1; }
    click_widget_with right 'table row[pid="'"$victim"'"]' || settle 1
done
kill -0 "$victim" 2> /dev/null || { echo "the sleep died on its own"; exit 1; }
i=0
while kill -0 "$victim" 2> /dev/null; do
    i=$((i + 1))
    [ $i -le 5 ] || { echo "the sleep survived Terminate"; exit 1; }
    if [ "$(count_widgets 'table row.selected[pid="'"$victim"'"] menu > item')" = 2 ]; then
        click_widget 'table row.selected menu > item:nth-child(1)' || :
    else
        click_widget_with right 'table row[pid="'"$victim"'"]' || :
    fi
    sleep 0.3
done
wait "$victim" 2> /dev/null || :
assert_logged "sent Terminate to $victim"
assert_eq "$(count_widgets 'table row menu')" 0 "the menu is gone"

# A click outside closes the popup; the mem instance opens on its tab.
click 300 900
assert_no_surface popup
assert_eq "$(count_widgets 'monitor')" 0
click_widget 'panel[output="HEADLESS-1"] gadget#mem.system-monitor > button.mem'
assert_surface popup
assert_eq "$(count_widgets 'monitor tabs tab.mem.active')" 1 "the mem instance opens on the mem tab"
assert_eq "$(count_widgets 'section.mem')" 1
click 300 900
assert_no_surface popup

# Right click on the bar: the configured command, or a terminal
# monitor in the launcher's terminal.
click_widget_with right "$bell"
settle 0.5
assert_eq "$(cat "$ARIA_UI_OUT/sysmon")" run "command ran"
click_widget_with right 'panel[output="HEADLESS-1"] gadget#mem.system-monitor > button.mem'
settle 0.5
monitor=$(command -v btop || command -v htop || command -v top)
assert_logged "ran [\"true\", \"-e\", \"$(basename "$monitor")\"]"
