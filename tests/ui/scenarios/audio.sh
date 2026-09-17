# The audio gadget: the volume icon on each bar; the popup lists the
# mixer channels the machine has (not asserted: a headless CI has none)
# and the media players: a fake one here, whose state the popup follows
# and whose controls the popup drives; the Mixer button runs
# mixer_command.

mpris_start

button='panel[output="HEADLESS-1"] gadget.audio > button.output'
player='popup player'
assert_eq "$(count_widgets 'gadget.audio > button.output')" 2 "a volume button per bar"
assert_eq "$(count_widgets 'gadget.audio > button.input')" 2 "a microphone button per bar (show_microphone)"
assert_eq "$(count_widgets 'gadget.audio > button > text')" 4 "the percent after each icon (show_percent)"

# Until the gadget shows the player (the bus watcher is async).
wait_for() {
    i=0
    while [ "$(count_widgets "$1")" != "$2" ] && [ $i -lt 30 ]; do
        sleep 0.1; i=$((i + 1))
    done
    assert_eq "$(count_widgets "$1")" "$2" "$3"
}

click_widget "$button"
assert_surface popup
wait_for "$player" 1 "the fake player in the popup"
assert_eq "$(count_widgets 'popup player.paused[name="Aria test player"]')" 1 "paused, by its identity"
assert_eq "$(count_widgets "$player > controls > button.previous.disabled")" 1 "previous disabled (CanGoPrevious false)"
assert_eq "$(count_widgets "$player > controls > button.next.disabled")" 0 "next enabled"
shot_surface popup popup

# The controls reach the player.
click_widget "$player > controls > button.play"
assert_eq "$(mpris_event)" play-pause "play/pause asked"
click_widget "$player > controls > button.next"
assert_eq "$(mpris_event)" next "next asked"

# The player's changes reach the popup.
mpris_send "status Playing"
wait_for 'popup player.playing' 1 "now playing"
mpris_send "title Second song"
settle
assert_eq "$(count_widgets 'popup player.playing')" 1 "still there after a title change"

# The Mixer button runs mixer_command and closes the popup.
click_widget 'popup gadget.audio button.mixer'
assert_no_surface popup
i=0; while ! [ -f "$ARIA_UI_OUT/clicks" ] && [ $i -lt 20 ]; do sleep 0.1; i=$((i + 1)); done
assert_eq "$(cat "$ARIA_UI_OUT/clicks")" mixer "mixer_command ran"

# A player going away leaves the popup.
mpris_stop
click_widget "$button"
assert_surface popup
wait_for "$player" 0 "the player is gone"
click 600 600
assert_no_surface popup

# The microphone button opens the same popup, under itself.
click_widget 'panel[output="HEADLESS-1"] gadget.audio > button.input'
assert_surface popup
set -- $(widget 'panel[output="HEADLESS-1"] gadget.audio > button.input')
mic_cx=$(($1 + $3 / 2))
set -- $(surfaces | tr ';' '\n' | grep ' popup ' | head -n 1 | sed 's/.* \([0-9-]*\),\([0-9-]*\) \([0-9]*\)x\([0-9]*\)$/\1 \2 \3 \4/')
popup_cx=$(($1 + $3 / 2))
[ $((popup_cx - mic_cx)) -le 2 ] && [ $((mic_cx - popup_cx)) -le 2 ] \
    || { echo "popup centre $popup_cx, microphone centre $mic_cx"; exit 1; }
click 600 600
assert_no_surface popup
