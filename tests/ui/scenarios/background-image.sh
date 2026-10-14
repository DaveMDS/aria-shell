# Icons over a bar's background image: svgs, drawn after the picture,
# stay over it on wgpu (iced_wgpu draws a merged layer's svgs before its
# rasters: paint.rs keeps the layers apart). A bar of the scenario's own,
# its colour under the picture as a theme's, nothing over it that could
# keep the layers apart instead (a quad); its icon green, told apart by
# the pixels over a red picture.

conf=$ARIA_UI_OUT/conf
mkdir -p "$conf/aria-shell"
cat > "$conf/aria-shell/aria.conf" <<CONF
[general]
language = en
theme = $conf/bg.css
reload_theme = no
reload_config = no

[panel]
outputs = all
items_center = Custom:icon

[Custom:icon]
icon = system-shutdown-symbolic

[Brightness]
backlight = no
CONF
cat > "$conf/bg.css" <<'CSS'
surface.panel {
  background-image: url(bg.png);
}
surface.panel icon {
  height: 24px;
  color: #00ff00;
}
CSS
# A red picture, 4x4 (a png, written by hand).
python3 - "$conf/bg.png" <<'PY'
import struct, sys, zlib
def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
rows = b"".join(b"\0" + b"\xff\0\0" * 4 for _ in range(4))
png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 4, 4, 8, 2, 0, 0, 0))
       + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))
open(sys.argv[1], "wb").write(png)
PY
restart_shell "$conf"
settle 1

# How many of the icon's pixels are green, and how many of the bar's
# are red: the picture is there, the icon over it.
count() {
    grim -g "$1" -t ppm - | python3 -c '
import sys
data = sys.stdin.buffer.read()
w, h = (int(v) for v in data.split(b"\n")[1].split())
px = data[-w * h * 3:]
want = {"green": (0, 255, 0), "red": (255, 0, 0)}[sys.argv[1]]
print(sum(1 for i in range(0, len(px), 3)
          if all(abs(px[i + k] - want[k]) < 60 for k in range(3))))' "$2"
}
set -- $(widget 'surface.panel[output="HEADLESS-1"] gadget.custom > button > icon')
icon="$1,$2 ${3}x$4"
shot_surface bar panel
[ "$(count "$icon" red)" -gt 0 ] || { echo "no picture under the icon"; exit 1; }
[ "$(count "$icon" green)" -gt 20 ] || { echo "the icon is under the picture"; exit 1; }
