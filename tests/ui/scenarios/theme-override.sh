# theme-override.css beside the config: the user's touches over
# whatever theme is in use, read with `reload_theme` when it appears,
# changes or goes; a broken one is left out, the theme kept.

conf=$(mktemp -d)
trap 'rm -rf "$conf"' EXIT
cp -r "$XDG_CONFIG_HOME/." "$conf"
sed -i "s|^reload_theme =.*|reload_theme = yes|" "$conf/aria-shell/aria.conf"
restart_shell "$conf"
settle 1

over="$conf/aria-shell/theme-override.css"
# The bar's thickness on the first output.
thickness() { surfaces | tr ';' '\n' | grep "panel HEADLESS-1 " | awk '{print $4}' | cut -dx -f2; }
# Until the bar is $1 thick (3 s), else fails.
wait_thickness() {
    i=0
    while [ "$(thickness)" != "$1" ] && [ $i -lt 60 ]; do
        sleep 0.05; i=$((i + 1))
    done
    assert_eq "$(thickness)" "$1" "$2"
}

base=$(thickness)
# Atomically, as editors save.
write() { printf '%s\n' "$1" > "$over.tmp" && mv "$over.tmp" "$over"; }

write 'surface.panel { min-height: 44px; }'
wait_thickness 44 "created: the bar as it says"
write 'surface.panel { min-height: 52px; }'
wait_thickness 52 "changed"
# Broken: the theme alone, the bar back to its thickness.
write 'surface.panel { min-height: 60px;'
wait_thickness "$base" "broken: left out"
rm "$over"
write 'surface.panel { min-height: 48px; }'
wait_thickness 48 "written again"
rm "$over"
wait_thickness "$base" "removed: the theme alone"
