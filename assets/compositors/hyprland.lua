-- Aria Shell on Hyprland (0.56 and later, Lua config): start it and bind
-- its keys. Copy what you need into your config; `aria-shell` must be on
-- the PATH, or write its full path.

-- Start: as a systemd user service with the session (UWSM:
-- `systemctl --user enable aria-shell.service`, nothing here), or here,
-- with the compositor:
hl.on("hyprland.start", function ()
    hl.exec_cmd("aria-shell")
end)

-- The launcher, the exit menu, the lock screen
hl.bind("SUPER + Space",  hl.dsp.exec_cmd("aria-shell launcher toggle"))
hl.bind("SUPER + Escape", hl.dsp.exec_cmd("aria-shell exiter toggle"))
hl.bind("SUPER + L",      hl.dsp.exec_cmd("aria-shell lock"))

-- Volume: whatever changes it, the shell shows its OSD by itself
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 5%+"), { locked = true, repeating = true })
hl.bind("XF86AudioLowerVolume", hl.dsp.exec_cmd("wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-"),      { locked = true, repeating = true })
hl.bind("XF86AudioMute",        hl.dsp.exec_cmd("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"),     { locked = true })
hl.bind("XF86AudioMicMute",     hl.dsp.exec_cmd("wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle"),   { locked = true })

-- Brightness: the OSD from the script, with brightnessctl's new percentage
local brightness = "aria-shell osd show --icon display-brightness-symbolic --value $(brightnessctl -m set %s | cut -d, -f4 | tr -d %%)"
hl.bind("XF86MonBrightnessUp",   hl.dsp.exec_cmd(string.format(brightness, "5%+")), { locked = true, repeating = true })
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd(string.format(brightness, "5%-")), { locked = true, repeating = true })
