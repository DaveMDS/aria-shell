# `aria-shell open terminal`: [general] terminal alone (no `-e` without
# a program); `open file-manager [dir]`: [general] file_manager on the
# home, or on the dir, made absolute by the client.

aria open terminal
assert_logged 'ran \["true"\]'

aria open file-manager
mkdir -p "$ARIA_UI_OUT/my dir"
(cd "$ARIA_UI_OUT" && aria open file-manager "my dir")
aria open file-manager /tmp
i=0
while [ "$(wc -l < "$ARIA_UI_OUT/opened" 2> /dev/null)" != 3 ] && [ $i -lt 20 ]; do
    sleep 0.1; i=$((i + 1))
done
# (the three race, as programs do)
assert_eq "$(sort "$ARIA_UI_OUT/opened")" "$(printf '%s\n' "$HOME" "$ARIA_UI_OUT/my dir" /tmp | sort)" \
    "the home, the relative dir from the client's cwd, an absolute one"

if aria open browser 2> /dev/null; then
    echo "open browser accepted"
    exit 1
fi
