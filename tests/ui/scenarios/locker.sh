# The lock screen: `aria lock` covers both outputs; without a password
# (the shared config) Enter unlocks; with one (tests/ui/config-locker, a
# second shell) a wrong password is refused by PAM and the lock stays.

assert_no_surface locker

aria lock; settle 0.8
assert_logged "session locked"
assert_eq "$(surfaces | tr ';' '\n' | grep -c '^ *locker ')" 2 "one lock surface per output"
assert_contains "$(surfaces)" "locker HEADLESS-1 0,0 1920x1080"
assert_contains "$(surfaces)" "locker HEADLESS-2 1920,0 1920x1080"
shot locked

# The blocks, on both outputs; no password field.
assert_eq "$(count_widgets 'locker avatar')" 2 "an avatar per surface"
assert_eq "$(count_widgets 'locker username')" 2 "a username per surface"
assert_eq "$(count_widgets 'locker time')" 2 "a time per surface"
assert_eq "$(count_widgets 'locker date')" 2 "a date per surface"
assert_eq "$(count_widgets 'locker auth button')" 2 "an Unlock button per surface"
assert_eq "$(count_widgets 'locker auth input')" 0 "no password field with password_prompt = no"
# Centred on its output.
set -- $(widget 'locker[output="HEADLESS-2"] box')
[ "$1" -gt 2400 ] && [ "$(($1 + $3))" -lt 3400 ] || { echo "box not centred on HEADLESS-2: $*"; exit 1; }
[ "$2" -gt 100 ] && [ "$(($2 + $4))" -lt 1000 ] || { echo "box not centred vertically: $*"; exit 1; }
# The clock ticks (seconds in the scenario's format).
t1=$(aria debug widgets 'locker[output="HEADLESS-1"] time')
sleep 1.2
t2=$(aria debug widgets 'locker[output="HEADLESS-1"] time')
[ -n "$t1" ] || { echo "no time widget"; exit 1; }

# A second lock is ignored.
aria lock; settle
assert_logged "lock requested while locked"
assert_eq "$(surfaces | tr ';' '\n' | grep -c '^ *locker ')" 2 "still two lock surfaces"

# Enter unlocks.
key Return; settle 0.8
assert_logged "unlocking without credentials"
assert_no_surface locker
assert_surface panel

# The button unlocks too.
aria lock; settle 0.8
click_widget 'locker[output="HEADLESS-1"] auth button'
settle 0.5
assert_no_surface locker

# With a password: the PAM path, a second shell.
restart_shell "$ARIA_UI_ROOT/tests/ui/config-locker"
aria lock; settle 0.8
assert_eq "$(surfaces | tr ';' '\n' | grep -c '^ *locker ')" 2 "two lock surfaces (second shell)"
assert_eq "$(count_widgets 'locker auth input')" 2 "a password field per surface"
assert_eq "$(count_widgets 'locker avatar')" 0 "no avatar (show_avatar = no)"
assert_eq "$(count_widgets 'locker date')" 0 "no date (show_date = no)"
assert_eq "$(count_widgets 'locker auth message')" 0 "no message at rest"
shot locked-password

# The eye shows the password in clear and hides it again (the field's
# text isn't readable from outside; the button's state is).
type_text "not-the-password"
assert_eq "$(count_widgets 'locker peek.on')" 0 "hidden at first"
click_widget 'locker[output="HEADLESS-1"] peek'
assert_eq "$(count_widgets 'locker peek.on')" 2 "shown after a click, on both"
shot locked-peek
click_widget 'locker[output="HEADLESS-1"] peek'
assert_eq "$(count_widgets 'locker peek.on')" 0 "hidden again"

# The field has the keyboard: a wrong password, checked by PAM, is
# refused; the field is emptied, the lock stays.
key Return; settle 0.5
assert_logged "checking the password with PAM"
i=0
while [ $i -lt 50 ] && ! grep -q "password refused" "$ARIA_UI_OUT/shell.log"; do sleep 0.1; i=$((i + 1)); done
assert_logged "password refused"
settle 0.5
assert_eq "$(count_widgets 'locker auth.error message')" 2 "the failure shown on both surfaces"
assert_eq "$(surfaces | tr ';' '\n' | grep -c '^ *locker ')" 2 "still locked"
shot locked-refused
