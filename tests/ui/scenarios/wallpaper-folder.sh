# The wallpaper from a folder: its images in turn (by hand with
# `aria-shell wallpaper next`, and every `interval`), images coming and
# going while it's shown; `source = auto`, the user's backgrounds folder
# winning over the system's as soon as it has an image; `none`. One
# section for both outputs: they show the same image. The images are
# gradients, horizontal (a) or vertical (b), told apart by sampling.

walls=$ARIA_UI_OUT/walls
mkdir -p "$walls"
# The first one big: iced_wgpu uploads a raster of 2 MiB or more on a
# worker thread and asks no frame when it's done, the wallpaper stayed
# blank (a 1024x1024 horizontal gradient, 4 MiB as RGBA).
python3 - "$walls/1.png" << 'PY'
import struct, sys, zlib
w = h = 1024
row = b"\0" + bytes(c for x in range(w) for c in (x // 4, 64, 255 - x // 4))
def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(row * h)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
PY
cp "$ARIA_UI_ROOT/tests/ui/config/aria-shell/wallpapers/b.png" "$walls/2.png"

# The shared config, its [wallpaper*] sections replaced by stdin.
config_with() {
    dir=$ARIA_UI_OUT/config-$1
    mkdir -p "$dir/aria-shell"
    awk '/^\[/ { skip = ($0 ~ /^\[wallpaper/) } !skip' \
        "$ARIA_UI_ROOT/tests/ui/config/aria-shell/aria.conf" > "$dir/aria-shell/aria.conf"
    cat >> "$dir/aria-shell/aria.conf"
    echo "$dir"
}

# How many surfaces of a kind (all outputs), and waiting for a count.
count() {
    surfaces | tr ';' '\n' | grep -c "^ *$1 ${2:-}" || true
}
wait_count() {
    i=0
    while [ "$(count "$1" "$2")" != "$3" ] && [ $i -lt 30 ]; do
        sleep 0.1; i=$((i + 1))
    done
    assert_eq "$(count "$1" "$2")" "$3" "$4"
}

sample() { grim -g "$1,$2 1x1" -t ppm - | tail -c 3 | od -An -tu1 | tr -s ' '; }
# Which gradient output `$1` (0 or 1920 across) shows: a (horizontal)
# or b (vertical).
showing() {
    left=$(sample $(($1 + 10)) 600); right=$(sample $(($1 + 1900)) 600)
    if [ "$left" != "$right" ]; then echo a; else echo b; fi
}
# Until `aria debug wallpaper` contains `$1` (5 s), else fails.
wait_wallpaper() {
    i=0
    while ! aria debug wallpaper | grep -qF -- "$1" && [ $i -lt 50 ]; do
        sleep 0.1; i=$((i + 1))
    done
    assert_contains "$(aria debug wallpaper)" "$1"
}

# --- a folder, by hand ---------------------------------------------------
restart_shell "$(config_with folder << EOF
[wallpaper]
source = $walls
fit = fill
interval = 1h
EOF
)"
wait_wallpaper "shown=$walls/1.png"
assert_contains "$(aria debug wallpaper)" "outputs=HEADLESS-1,HEADLESS-2"
assert_contains "$(aria debug wallpaper)" "images=2 wanted=1"
assert_eq "$(count wallpaper)" 2 "a wallpaper per output"
settle 0.5
assert_eq "$(showing 0)" a "the first image, by name, drawn at once"
assert_eq "$(showing 1920)" a "the same image on the other output"
shot folder-first

aria wallpaper next
wait_wallpaper "shown=$walls/2.png"
settle 0.5
assert_eq "$(showing 0)" b "the next image"
assert_eq "$(showing 1920)" b "the next image on the other output too"
aria wallpaper next
wait_wallpaper "shown=$walls/1.png"
assert_contains "$(aria debug wallpaper)" "wanted=1 "

# An image coming: in the list, the one shown stays.
cp "$walls/2.png" "$walls/0.png"
wait_wallpaper "images=3"
assert_contains "$(aria debug wallpaper)" "shown=$walls/1.png"
# The one shown going: the one after it comes.
rm "$walls/1.png"
wait_wallpaper "shown=$walls/2.png"
assert_contains "$(aria debug wallpaper)" "images=2"
# A sub-folder counts.
mkdir "$walls/more" && cp "$walls/2.png" "$walls/more/3.png"
wait_wallpaper "images=3"
# Not an image: not counted.
echo "<background/>" > "$walls/slides.xml"
settle 0.5
assert_contains "$(aria debug wallpaper)" "images=3"
# Every image gone: no wallpaper; one back: the wallpaper back.
rm -r "$walls"/*
wait_count wallpaper "" 0 "no image, no wallpaper"
cp "$ARIA_UI_ROOT/tests/ui/config/aria-shell/wallpapers/b.png" "$walls/9.png"
wait_count wallpaper "" 2 "an image again, a wallpaper per output again"
wait_wallpaper "shown=$walls/9.png"

# --- every interval ------------------------------------------------------
cp "$walls/9.png" "$walls/8.png"
restart_shell "$(config_with interval << EOF
[wallpaper]
source = $walls
interval = 2s
EOF
)"
wait_wallpaper "shown=$walls/8.png"
i=0
while ! aria debug wallpaper | grep -qF "shown=$walls/9.png" && [ $i -lt 40 ]; do
    sleep 0.1; i=$((i + 1))
done
assert_contains "$(aria debug wallpaper)" "shown=$walls/9.png"
assert_contains "$(aria debug wallpaper)" "interval=2s"

# --- auto ----------------------------------------------------------------
# The system's backgrounds (in a sub-folder, as distributions have
# them) while the user's folder doesn't exist; the user's when it has
# an image.
data=$ARIA_UI_OUT/data
system=$ARIA_UI_OUT/system
mkdir -p "$data" "$system/backgrounds/distro"
cp "$ARIA_UI_ROOT/tests/ui/config/aria-shell/wallpapers/a.png" "$system/backgrounds/distro/default.png"
auto=$(config_with auto << EOF
[wallpaper]
EOF
)
(
    export XDG_DATA_HOME=$data XDG_DATA_DIRS=$system:$XDG_DATA_DIRS
    restart_shell "$auto"
)
wait_wallpaper "shown=$system/backgrounds/distro/default.png"
assert_contains "$(aria debug wallpaper)" "source=auto root=$system/backgrounds "
mkdir "$data/backgrounds"
settle 0.5
assert_contains "$(aria debug wallpaper)" "root=$system/backgrounds "
cp "$ARIA_UI_ROOT/tests/ui/config/aria-shell/wallpapers/b.png" "$data/backgrounds/mine.png"
wait_wallpaper "shown=$data/backgrounds/mine.png"
assert_contains "$(aria debug wallpaper)" "root=$data/backgrounds "

# --- none ----------------------------------------------------------------
restart_shell "$(config_with none << EOF
[wallpaper]
source = none
EOF
)"
assert_eq "$(count wallpaper)" 0 "source = none: no wallpaper"
assert_eq "$(aria debug wallpaper)" none "no show"
