# A theme as a folder: `themes/<name>/<name>.css` found by its name, its
# images beside it (url() from the theme's directory), the Themes menu
# listing it; and with `reload_theme`, an image of the folder rewritten
# reloads the theme, the new picture drawn.

conf=$(mktemp -d)
trap 'rm -rf "$conf"' EXIT
cp -r "$XDG_CONFIG_HOME/." "$conf"
theme="$conf/aria-shell/themes/tile"
mkdir -p "$theme"
cat > "$theme/tile.css" << 'EOF'
gadget.themes > indicator {
  background: url(tile.png);
}
EOF
# A 4x4 PNG of one colour, `r g b`.
png() {
    python3 -I - "$@" << 'EOF'
import struct, sys, zlib
r, g, b, path = int(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))
rows = b"".join(b"\0" + bytes([r, g, b]) * 4 for _ in range(4))
with open(path + ".tmp", "wb") as f:
    f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 4, 4, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))
EOF
    mv "$4.tmp" "$4"
}
png 255 0 0 "$theme/tile.png"
sed -i -e "s|^theme =.*|theme = tile|" -e "s|^reload_theme =.*|reload_theme = yes|" \
    "$conf/aria-shell/aria.conf"
restart_shell "$conf"
settle 1

assert_eq "$(aria debug theme)" "theme=tile scheme=light" "found by its name"
sample() {
    set -- $(widget 'surface.panel[output="HEADLESS-1"] gadget.themes > indicator')
    grim -g "$(($1 + 1)),$(($2 + 1)) 1x1" -t ppm - | tail -c 3 | od -An -tu1 | tr -s ' ' | sed 's/^ //'
}
assert_eq "$(sample)" "255 0 0" "its image, beside it"

# The menu: Light, Dark, Base, then the themes, tile (ours) among them.
click_widget_with right 'surface.panel[output="HEADLESS-1"] gadget.themes > indicator'
assert_surface popup
wait_for 'menu > item:checked' 2 "Light and tile checked"
shot_surface menu popup
click 600 600
assert_no_surface popup

png 0 0 255 "$theme/tile.png"
assert_logged "theme file changed, reloading"
settle 1
assert_eq "$(sample)" "0 0 255" "the image rewritten, drawn again"
