# [autostart]: the shared config's programs ran once at start, in file
# order (as the log says: the programs themselves race), the one without
# a command skipped with a warning; `=` and `:` inside a command line
# are the command's.

assert_logged "autostart second"
order=$(grep -o 'autostart [a-z]*$' "$ARIA_UI_OUT/shell.log" | tr '\n' ' ')
assert_eq "$order" "autostart first autostart second " "started once each, in file order"
assert_logged "\[autostart\] nothing: no command line"
i=0
while [ "$(wc -l < "$ARIA_UI_OUT/autostart" 2> /dev/null)" != 2 ] && [ $i -lt 20 ]; do
    sleep 0.1; i=$((i + 1))
done
assert_eq "$(sort "$ARIA_UI_OUT/autostart")" "first
second a=b: c" "both ran, their command lines whole"
