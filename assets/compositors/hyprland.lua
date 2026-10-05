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

-- Volume: the default output (--input: the microphone), by [Audio] step
-- up to max_volume; whatever changes it, the shell shows its OSD by itself
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd("aria-shell volume up"),           { locked = true, repeating = true })
hl.bind("XF86AudioLowerVolume", hl.dsp.exec_cmd("aria-shell volume down"),         { locked = true, repeating = true })
hl.bind("XF86AudioMute",        hl.dsp.exec_cmd("aria-shell volume mute"),         { locked = true })
hl.bind("XF86AudioMicMute",     hl.dsp.exec_cmd("aria-shell volume mute --input"), { locked = true })

-- Brightness: every screen (the laptop's panel, the monitors over
-- DDC/CI); the shell shows its OSD on each. `--output eDP-1` for one
hl.bind("XF86MonBrightnessUp",   hl.dsp.exec_cmd("aria-shell brightness up"),   { locked = true, repeating = true })
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd("aria-shell brightness down"), { locked = true, repeating = true })

-- Screenshots: the picker (a window, a screen or an area), the focused
-- screen, the active window; a file in [Screenshot] directory, or
-- `--clipboard` to copy instead
hl.bind("Print",         hl.dsp.exec_cmd("aria-shell screenshot"))
hl.bind("SHIFT + Print", hl.dsp.exec_cmd("aria-shell screenshot output"))
hl.bind("ALT + Print",   hl.dsp.exec_cmd("aria-shell screenshot window"))
