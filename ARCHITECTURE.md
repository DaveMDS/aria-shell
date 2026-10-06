# Architecture

How Aria Shell is built and why: the shape of the code, the decisions
behind it, facts about the crates verified in their source or on a
real session, the known risks and the internal limits. Read it before
an architectural change. What the shell does for its users, and what
it doesn't yet, is the README's checklist.

## Why iced

Aria Shell was Python + GTK4 + gtk4-layer-shell (`git log --
aria-shell-python`), rewritten because GTK's CSS is too limited for the
visual customization wanted. Qt was ruled out; EFL is effectively
dead; a real CSS engine without GTK still means WebKitGTK on Linux
(Blitz alone is too young). So: Rust + `iced` + `iced_exwlshell`
(layer shell and session lock, formerly `iced_layershell` +
`iced_sessionlock`): wgpu, actively maintained, and the `shader` widget
for more visual freedom than CSS. COSMIC (`libcosmic`, an `iced` fork)
ships every subsystem we need and is the pattern reference; we stay on
vanilla `iced` and don't depend on `libcosmic`.

The config format (`aria.conf`: INI, case-sensitive, `[Name]` /
`[Name:id]`, empty value = default) is the Python implementation's,
kept compatible on purpose; nothing of its structure (`Singleton`,
`AriaService`, `AriaModule`, `importlib` loading) is.

## Known risks (from real research, not memory)

- PAM in Rust is the weakest link: even COSMIC's official greeter has open
  production auth bugs. Ours is a hand-written `libpam` binding
  (`locker/pam.rs`), small enough to read whole; refused-password path
  exercised by a UI scenario, the accepted one only by hand.
- Raw PipeWire volume control and a GStreamer→wgpu bridge for video
  lack mature crates; expect to hand-roll them. (The idle-notifier
  protocol didn't: `wayland-protocols` has it, see `idle/wayland.rs`.)
- `iced_exwlshell` is a small, fast-moving crate: expect API churn, verify
  against its source in `~/.cargo/registry` rather than memory.
  The branch `exwlshell-0.21` (on 0.21.0-rc1: surfaces rebuilt only
  when drawn, a fraction of the CPU with the pointer moving) waits for
  the 0.21 release: merge it then, adapting to the API changes made
  after the rc.
- Transparency has 2 bits: `iced_wgpu` 0.14.0 takes the first non-sRGB
  surface format needing no feature, and Mesa's Vulkan on Wayland lists
  `Rgb10a2Unorm` before `Bgra8Unorm` (the log says `Selected format:
  Rgb10a2Unorm`), so what shows through a surface has 4 levels: a
  shadow is a hard band, the antialiased edge of a rounded surface
  jagged, an `rgba` bar banded. Measured with a red `0 0 30px` shadow:
  the colour fades, the alpha steps from 0.33 to 0. iced master excludes
  `Rgb10a2Unorm`/`Rgb10a2Uint` (a `BLACKLIST` in
  `wgpu/src/window/compositor.rs`), not in any 0.14 release, and
  `iced_exwlshell` 0.21 stays on iced 0.14. Chosen: wait for the
  release, no vendored copy; until then `base.css` gives the surfaces'
  roots neither shadows nor rounded corners (inner widgets round fine,
  on an opaque background). The room for a shadow (`Theme::shadow_room`)
  is already there for when it renders.

## Structure

Plain Elm architecture as iced defines it, nested once per layer. Every
layer is a struct with its own `Message`, `update`, `view` and
`subscription`; the parent routes by key and `.map()`s messages up.

```
AriaShell  (main.rs)        daemon; owns Config, ShellReceiver, Compositor, panels: BTreeMap<window::Id, Panel>
  Message::Shell(ShellEvent)          monitors and surfaces appearing/disappearing
  Message::Panel(window::Id, panel::Message)
  Message::Compositor(compositor::Event)   workspaces/windows changes, applied to `Compositor`
  Message::Tray(tray::Event)               status notifier items and their menus, applied to `Tray`
  Message::Notifications(notifications::Event)   notifications coming and going, applied to `Notifications`
  Message::Toast(toast::Message)           a click on a notification's surface -> notifications::Command
  Message::SysMon(sysmon::Event)           a system reading / the process table, applied to `SysMon`
  Message::Audio(audio::Event) | Network(network::Event)   the mixer / players, NetworkManager, applied to each
  Message::PanelKey(window::Id, keyboard::Event)   a key on a bar holding the keyboard for its popup
  Message::Locker(locker::Message)         routed to the lock screen while the session is locked
  + variants injected by #[to_exwlshell_message] (NewLayerShell, RemoveWindow, Lock, UnLock, ...)

Panel      (panel.rs)       one layer surface on one output; PanelConfig; gadgets: Vec<(Slot, AnyGadget)>
  Message::Gadget(index, gadget::Message) | Key(keyboard::Event)   (the latter for the popup wanting the keyboard)

AnyGadget  (gadget.rs)      closed enum over every gadget type, plus `create(name, &Config, &OutputInfo)`
  Message::Clock(clock::Message) | Message::Workspaces(..) | ...

Clock      (gadgets/clock.rs)  impl Gadget: new / update / view(ctx) / popup_view(ctx) / popup_size(ctx) / subscription
  Message::Calendar(calendar::Message)

TrayGadget (gadgets/tray.rs)   impl Gadget: a row of items from `ctx.tray`, the clicked item's menu in the popup
  Message::OpenMenu(key, n) | Menu(menu::Message) | Activate(key) | Scroll(key, delta) | ...

Themes     (gadgets/themes.rs) impl Gadget: the scheme's icon; left click toggles light/dark, right click a menu
  Message::Toggle | OpenMenu | Menu(menu::Message)   -> Action::Theme(theme::Command)

Custom     (gadgets/custom.rs) impl Gadget: icon and/or label, a program per mouse button / wheel direction
  Message::Left | Right | Middle | Scroll(delta)   -> process::run; with `exec`, Action::Script(Refresh(spec))
  script() -> Option<scripts::Spec>                the `exec` for the daemon; its output read from `ctx.scripts`

Menu       (widgets/menu.rs)  reusable component: Item tree (labels, toggles, separators, submenus unfolding in
                              place) -> update(Message) -> Event, view(theme, node, items), size(..) for the popup

Calendar   (widgets/calendar.rs)  reusable component, not a gadget: state + Message + update + view(today, theme, node)

Compositor (compositor/)    daemon-owned desktop state: workspaces, windows, active/urgent flags
  subscription()            the single IPC stream (compositor/hyprland.rs or sway.rs), yields `Event`s
  apply(Event)              patches the state
  run(Command) -> Task      sends a command (activate workspace/window, `Exit` = end the session:
                            Sway `exit`, Hyprland `dispatch hl.dsp.exit()`) to the backend

Tray       (tray/)         daemon-owned status notifier items: `items: Vec<Item>` (props + pixmap icons), loaded menus
  subscription()            one session-bus connection (tray/dbus.rs): the watcher we serve or defer to, the
                            host, one task per item; yields `Event`s (Connected, Added/Updated/Removed, Menu, ..)
  apply(Event) -> Task      patches the state; a `MenuChanged` for a loaded menu re-fetches it
  run(Command, cursor)      Activate/SecondaryActivate/ContextMenu/Scroll on an item, LoadMenu/ExpandMenu/MenuClick
                            over `com.canonical.dbusmenu` (tray/menu.rs)

Notifications (notifications/)  daemon-owned notification daemon: `items: Vec<Notification>` (newest first)
  subscription()            one session-bus connection (notifications/dbus.rs) serving
                            `org.freedesktop.Notifications` (the name re-requested when another daemon drops it);
                            yields `Event`s (Connected, Notify, Close, Expired)
  apply(Event) -> Task      patches the list; a `Notify` starts the expiry timer (serial-checked)
  run(Command) -> Task      Activate (the `default` action, then close) / Invoke(key) / Dismiss: emits
                            `ActionInvoked` / `NotificationClosed`
  toast.rs                  node(n, output) / node_under(parent, n) / view(.., Extras) / size(.., &Extras):
                            the one view of a notification, on its own layer surface (the daemon stacks them,
                            `AriaShell::sync_toasts`, from the configured corner by margin) and as a row of the
                            gadget's popup (`Extras`: the width to lay out in, the age, a ✕)
  history / dnd / unseen()  what the gadget shows: the last `history` notifications (newest first, `seen`
                            flag), do-not-disturb (no toasts but critical ones), the count not looked at

NotificationsGadget (gadgets/notifications.rs)  impl Gadget: the bell with the unseen count; left click the
                            history popup (header: do-not-disturb, clear; one `toast::view` row per entry),
                            right click do-not-disturb, middle click closes the toasts
  Message::TogglePopup(unseen ids) | ToggleDnd | DismissAll | Clear | Toast(toast::Message) | Tick
                            -> Action::Notifications(notifications::Command)

SysMon     (sysmon/)        daemon-owned system readings: `sample()` (cpu total and per core, freq, temp, load,
                            uptime, memory/swap, disks with usage and throughput, interfaces with rates, GPUs),
                            `history()` (ring buffers per series, `[SystemMonitor] history` long), `processes()`
  subscription()            one sampler stream keyed on `MonitorConfig`: every `interval` seconds a
                            `spawn_blocking` reads /proc (proc.rs: stat, meminfo, loadavg, uptime, diskstats,
                            net/dev, mounts + statvfs) and /sys (sensors.rs: cpufreq, hwmon, amdgpu; nvidia-smi
                            as a process, once probed), rates against the previous counters
  apply(Event)              stores the sample and pushes the series; the process table gets its cpu% from
                            the previous reading's ticks
  run(Command)              Processes (a `spawn_blocking` over /proc/[pid]) | Signal(pid, Terminate|Kill) (libc::kill)
  format.rs                 bytes / rate / duration / freq, and `expand("{cpu}% {rx}", sample)` for the bar text

SystemMonitor (gadgets/system_monitor.rs)  impl Gadget: instances only (`[SystemMonitor:mem]`; the base section
                            is the sampler's), one value each (`show =`, required) as text, a sparkline or a gauge (`mode =`);
                            the popup: tabs cpu (graph, per-core meters, details), mem, disk, net, gpu,
                            processes (sortable columns; a click selects a row, a bar pinned under the
                            scrolling table offers Terminate/Kill for it; the list's scrollbar is embedded,
                            `scrollable::Scrollbar::spacing`, so it doesn't cover the last column), opened
                            on the value's tab;
                            right click on the bar: `command`, else btop/htop/top in `[general] terminal`
  Message::TogglePopup | RunCommand | Tab(kind) | Tick (processes re-read while open) | Sort(Column) | Select(pid) | Signal(..)

Audio      (audio/)          daemon-owned mixer and players: `channels_of(kind)` (outputs, inputs, the streams
                            playing: label, icon hints, volume as a fraction of 100%, mute, `default`),
                            `default_output()`, `players()` (identity, status, title/artist/album, cover
                            URL, what it can do), `cover(bus)` (a `file://` cover as an `Icon`),
                            `recordings()` (the apps listening to a microphone: source outputs not corked, from
                            a source among the inputs, i.e. not a sink's monitor; `None` until the first
                            listing is in, `Event::Listed` at the end of it)
  subscription()            pulse.rs: libpulse (`pipewire-pulse` / PulseAudio) on a thread of ours owning the
                            threaded mainloop and the context; its callbacks only post `Request`s the thread
                            serves under the lock (state, subscribe changes, the daemon's commands via the
                            `Handle` carried by `Event::Connected`); introspection answers go out as
                            `Event::Channel` / `ChannelGone` / `Defaults` / `Recording` / `RecordingGone`. mpris.rs: the session bus,
                            `NameOwnerChanged` + one task per `org.mpris.MediaPlayer2.*` name
                            (`PropertiesProxy` get_all, then `PropertiesChanged`) -> `Event::Player` / `PlayerGone`
  apply(Event)              patches the lists (channels grouped by kind, the defaults flagged), loads covers
  run(Command)              SetVolume/SetMuted/SetDefault(kind, index, ..), StepDefault/SetDefaultVolume/
                            ToggleDefaultMute/SetDefaultMuted (the default device of a kind),
                            PlayPause/Next/Previous(bus) over a `#[proxy]`

AudioGadget (gadgets/audio.rs)  impl Gadget: the default output's level icon (+ the percent with
                            `show_percent`; the default input's beside it with `show_microphone`, the popup
                            hanging off whichever was clicked); left click the popup (sections
                            Output / Input / Playing with a mute button + name (a button making a device the
                            default) + percent + slider per channel; a block per player: cover or app icon,
                            title/artist/album, previous / play-pause / next; no volume, its stream is
                            in the mixer already; a Mixer button);
                            wheel: `step` on the button's device up to `max_volume`, middle: mute, right:
                            `mixer_command`
  Message::TogglePopup(kind) | ToggleMute(kind) | Scroll(kind, ..) | RunMixer | Volume(kind, index, %) | Mute | SetDefault
           | PlayPause(bus) | Previous | Next    -> Action::Audio(audio::Command)

Network    (network/)        daemon-owned NetworkManager state: `devices()` (managed wired / Wi‑Fi: state, carrier,
                            ip4/ip6, the AP a Wi‑Fi one is on), `access_points()` (the networks around merged by
                            SSID — the strongest BSSID —, `known` (a profile's uuid), `active`, `connecting`; the
                            active first, then the known, then by strength), `vpns()` (profiles of type vpn /
                            wireguard), `active_by_uuid`, `summary()` (the bar's: the primary connection's kind,
                            connected/connecting/limited, label, strength, a VPN up), `failure(key)` /
                            `attempting(key)` (an attempt the gadget started: by SSID or profile uuid), `describe()`
  subscription()            nm.rs: the system bus (`Connection::system`, `DBUS_SYSTEM_BUS_ADDRESS` honoured),
                            `NameOwnerChanged` on org.freedesktop.NetworkManager (`Event::Running`), one match rule
                            on every signal under its path; any signal marks a snapshot due, re-read 200 ms after
                            the last one (`snapshot()`: `GetAll` per object, the lists concurrently) -> `Event::Snapshot`;
                            `Device.StateChanged` to Failed / `Connection.Active.StateChanged` to Deactivated carry
                            the reason -> `Event::DeviceFailed` / `ActiveFailed`
  apply(Event) -> (changed, Task)   replaces the snapshot and rebuilds the merged list; a failure of the current
                            attempt becomes a `Failure` (NO_SECRETS / supplicant reasons on a device = wrong
                            password; reason 9 on a VPN = needs a password), and the profile the attempt added
                            (`AddAndActivateConnection`) is deleted so a wrong key leaves nothing behind
  run(Command) -> Task      SetWireless / ToggleWireless, Scan (every Wi‑Fi device), Connect { ssid } (a known
                            network: `ActivateConnection(profile, device, ap)`; an open one: `AddAndActivate`;
                            a secured unknown one is the gadget's to ask first), ConnectWithPassword (a new
                            profile: `802-11-wireless-security` wpa-psk / sae / wep, `connection.permissions =
                            user:<login>:` so `settings.modify.own` is enough; a known one's old profile deleted
                            first), ConnectDevice (`ActivateConnection("/", device, "/")`), Disconnect (device),
                            Forget (profile), Activate / Deactivate (a profile, VPN)

NetworkGadget (gadgets/network.rs)  impl Gadget: the primary connection's icon (`show_label`: the name;
                            `show_vpn`: a badge); left click the popup: header Wi‑Fi (scan button, on/off
                            toggle), one row per network (click: join a known / open one, unfold the
                            password field of a secured one — Enter / Connect; a chevron button on the active
                            and the known ones: the active one's details with Disconnect / Forget, a known
                            idle one's Connect / Forget; an 802.1x one's hint), the wired devices (details, Disconnect,
                            a click brings a plugged one up), the VPN profiles with a toggle each, Settings
                            (`settings_command`); middle click Wi‑Fi on/off, right click `settings_command`;
                            a scan when it opens and every 10 s while open
  Message::TogglePopup | SetWireless | FlipWireless | Scan | Tick | Expand(Row) | Collapse | Focus | Connect
           | ConnectDevice | Password | Peek | Submit | Disconnect | Forget | Vpn(uuid, on)   -> Action::Network
  popup_keyboard() / popup_key()   the popup takes typed text (see "Popups and the keyboard" below)

Idle       (idle/)          daemon-owned idle stages: `IdleConfig::load` (`[Idle]`, `[Idle:battery]` over its
                            timeouts), `inhibited()` / `held_by_player()` for the gadget, `describe()`
  subscription()            wayland.rs: a Wayland connection of our own (its fd polled by tokio next to the
                            daemon's requests; libwayland takes an empty socket as success, so a `poll` says
                            `WouldBlock` once drained): `ext-idle-notify-v1` timers -> `Event::Idled/Resumed(Stage)`,
                            `wlr-output-power-management-v1` for the screens; logind.rs: `PrepareForSleep` with a
                            delay inhibitor (`Event::Sleeping(SleepLock)`, released on `ShellEvent::Locked`),
                            the session's `Lock` -> `Event::LockRequested`
  apply(Event, Locker) -> (lock?, Task)   the daemon locks when told; `set_on_battery` (from `Power`),
                            `set_playing` (from `Audio`), `run(Command)` (the user's hold) re-arm the timers
                            (all of them, from now; none while held)

Power      (power/)         daemon-owned UPower and power profiles: `battery()` (the display device: percent,
                            state, times, rate, `WarningLevel`, UPower's icon; health from the laptop battery),
                            `devices()` (peripherals), `profiles()`, `on_battery()`, `describe()`
  subscription()            upower.rs: the system bus, `NameOwnerChanged` for both names and one match rule on
                            every signal under /org/freedesktop/UPower (the profiles' path is under it), re-read
                            200 ms after the last one -> `Event::UPower` / `Event::Profiles` (`None`: not running)
  apply(Event) -> (changed, Option<Low>, Task)   a battery newly at UPower's low / critical warning is a `Low`
                            the daemon words (`notify_low`) and sends over the session bus as any app would
                            (`replaces_id`: one notification, closed once the warning is gone)
  run(Command) -> Task      SetProfile -> the `ActiveProfile` property

PowerGadget (gadgets/power.rs)  impl Gadget: `button.status` (UPower's battery icon, the percent, the profile's
                            icon; each optional, absent where there is none) opens the popup, `button.idle` (the
                            eye) holds idle; middle click holds idle, right click `settings_command`. Popup: the
                            battery, details, the peripherals, the profile picker, "Keep awake", Settings
  Message::TogglePopup | ToggleIdle | SetIdle(bool) | SetProfile | RunSettings   -> Action::Power / Action::Idle

Brightness (brightness/)    daemon-owned screens: `displays()` (id `backlight:<device>` / `ddc:<bus>`, kind, the
                            output's connector, the monitor's model, `Level { value, max }` raw, `None` until
                            read), `on_output(connector)`, `describe()`
  subscription()            worker.rs, keyed on `[Brightness] backlight`: finds the screens
                            (backlight.rs: one of /sys/class/backlight, firmware > platform > raw as GNOME, tied
                            to its output by its `device` link `card0-eDP-1`, else the one internal panel connected;
                            ddc.rs: `ddcutil detect --terse` when it's on the PATH (else a warning, and no
                            monitors), the `DRM connector` `card1-HDMI-A-1`), again a
                            second after outputs come or go; one task per screen, whose writes coalesce to the
                            latest (`setvcp 10 <n> --noverify` ~100 ms each; a backlight through logind's
                            `Session.SetBrightness`, no root); a thread `poll(POLLPRI)`s the backlight's
                            `actual_brightness` -> `Event::Level`. DDC has no notification: read when found and
                            on `Command::Refresh` (the popup opening)
  run(Command) -> bool      Set(target, %) / Step { target, up, by } (target: All | Output(connector) |
                            Display(id)): the new raw level is the state at once, the write goes to the
                            worker; a step stops at 1% going down and moves one raw unit when the percent
                            rounds back (a backlight with few levels). Readings other than the last level asked
                            are ignored until `Event::Written` (a backlight reports every write on the way)

BrightnessGadget (gadgets/brightness.rs)  impl Gadget: `button` (the icon, the bar's screen's percent with
                            `show_percent`); wheel: `step` on every screen or the bar's (`wheel = all | output`),
                            right click `settings_command`; popup: a row per screen (icon, name, connector,
                            percent, slider), Settings; opening it asks `Refresh`
  Message::TogglePopup | Scroll | Set(id, %) | RunSettings   -> Action::Brightness

Screenshot (screenshot/)    daemon-owned screenshots: `[Screenshot]` (directory, editor, the gadget's icon);
                            `run(Command { target, destination })`: target Pick | Window | Output(name) | All,
                            destination File { edit } | Clipboard. A job per capture: the global logical rect
                            to cut (Window: the active one of `Compositor::shown_windows()`, asked then), the
                            outputs it touches captured -> `pixels.rs` (pure, unit-tested: shm format -> RGBA,
                            the output's transform undone, `compose` the rect out of the shots at the largest
                            scale among them, PNG) in `spawn_blocking` -> `Event::Taken` (file written, or
                            the PNG to the clipboard; the editor run on the file with `--edit`: its path for
                            `%f`, last without one, `process::on_file`; `auto` = the first of `EDITORS`
                            on the PATH, at load)
  subscription()            wayland.rs, the idle/wayland.rs pattern (its own connection, fd polled by tokio,
                            `Handle` + `Request`s): `ext-image-copy-capture-v1` on an
                            `ext-output-image-capture-source` per output, one frame each into a memfd shm
                            buffer read back; `ext-data-control-v1` for the clipboard (a source offering
                            `image/png` until `cancelled`, each `send` written from a thread, other clients'
                            offers destroyed at once); plus the picker's events while it's open
  apply(Event, outputs, &Compositor) -> (Task, Surfaces)   `Surfaces { open, close, redraw }`: the daemon opens
                            and closes the picker's layer surfaces and redraws those it names
                            (`Message::Screenshot` is `Scope::None`)
  picker.rs                 Pick: every output and the shown windows asked together, the frames made upright
                            (`Event::Frozen`), then `Picker::open`: one `Layer::Overlay` surface per output
                            (`exclusive_zone -1`, all `OnDemand`), `stack![image(frozen), canvas(Overlay),
                            toolbar]`. State in the global logical space (surface-local + the output's
                            origin): `Drag::New` (a click picks the topmost window under it or the output, a
                            drag draws; clamped to the output it started on) / `Move` / `Resize { edges }`
                            (`grip`: within 10 px of an edge, two at a corner). The overlay's canvas draws
                            the theme's shade around the selection, the hover, the outline and the handles,
                            sets the cursor (crosshair, resize, grab), and publishes the presses with their
                            position; moves, the release, Escape/Enter, a right click come from
                            `listen_with`. `Look` before/after a message names the surfaces to redraw.
                            Enter takes the command's destination, the toolbar's buttons theirs, All every
                            output; the picture is cut from the frozen shots

ScreenshotGadget (gadgets/screenshot.rs)  impl Gadget: an icon button; left click the picker, right click a
                            menu (the active window, the bar's screen, every screen)
  Message::Pick | OpenMenu | Menu  -> Action::Screenshot (with the popup's close: the daemon waits 150 ms
                            before capturing, so the menu is off the screen)

Places     (places/)        daemon-owned places of a file manager's sidebar: `places()` (the home, the XDG
                            folders of `user-dirs.dirs` that exist, the trash), `bookmarks()` (GTK's
                            `gtk-3.0/bookmarks`, then KDE's `user-places.xbel` without Dolphin's system,
                            hidden and device entries; no duplicates, no local folder that's gone; a
                            `file://` one a `Target::Path`, any other scheme a `Target::Uri`), `trash_full()`,
                            `devices()` (UDisks2's volumes as GVfs chooses them: not `HintIgnore`, not swap; a
                            `HintSystem` one only mounted or in fstab under /media, /run/media, /mnt or the
                            home, or `x-gvfs-show`, or mounted at `/`; `x-gvfs-hide` hides any. Kind (icons
                            best first, as libudisks names them), mount point (`/` for the root's btrfs
                            subvolumes), usage by `statvfs`, removable, what an eject does to the drive),
                            `shares()` / `local_mounts()` (mounts.rs: what UDisks2 doesn't know, the mounts
                            without a block device: fstab's entries with `x-gvfs-show` or where a user
                            looks, not `x-gvfs-hide`, then those mounted by hand there, from
                            /proc/self/mountinfo, the system's types (tmpfs, proc, the portals, ...) left
                            out; `x-gvfs-name`, `x-gvfs-[symbolic-]icon`; a network one has no usage,
                            `statvfs` would wait on the server, a local one (encfs, bindfs) has;
                            `ARIA_SHELL_FSTAB` / `ARIA_SHELL_MOUNTINFO` name other files, for tests/ui),
                            `busy(key)` (a device's path, a mount's `Mount::key`)
  subscription()            udisks.rs: the system bus, `NameOwnerChanged` + every signal under
                            /org/freedesktop/UDisks2, debounced 200 ms -> `GetManagedObjects` read into plain
                            `Block`s and `Drive`s -> `Event::Objects`
  apply(Event, file_manager) -> (changed, Option<Failure>)   the devices filtered; `Mounted` opens the
                            mount point; `Failed` goes back to the daemon, which notifies it
                            (`notifications::client`, as any app; polkit's refusal worded: no agent)
  run(Command, file_manager) -> Task   Refresh (everything read again: a few small files, no watcher;
                            the usage) | Open(target): `[general] file_manager` on the path or the URI
                            (`trash:///`) | Mount(path) | Eject(path): unmount, lock a LUKS volume, then
                            eject (optical) or power off a removable drive with nothing else mounted
                            | MountDir(dir): `mount <dir>` (fstab's `user`/`users`, the setuid `mount`)
                            | UnmountDir(dir): `umount <dir>`, `fusermount3 -u` for a FUSE mount by hand;
                            their stderr is the failure's message

PlacesGadget (gadgets/places.rs)  impl Gadget: an icon (+ `label`) button; the popup: a header and a button
                            per place for each section of `[Places] show` (required), in its order, a
                            bookmark of a listed place left out; the devices then the local mounts, the
                            shares (`volume()`, one row for all): `button.open` (icon, label, the usage
                            meter when known) and, while mounted,
                            `button.eject` (disabled on `/`; nodes built disabled in `view`, so the icon takes
                            `button:disabled`'s colour); sized from the widest row
  Message::TogglePopup  -> Action::Places(Refresh) before the popup opens | Open(target) -> Places(Open), closed
           | Mount(path) -> Places(Mount), closed | Eject(path) -> Places(Eject), the popup stays
           | MountDir(dir) -> Places(MountDir), closed | UnmountDir(dir) -> Places(UnmountDir)

graph      (widgets/graph.rs)  canvas programs: `Sparkline` (one series, a `Label` over it), `Gauge` (a bar
                            filled to a fraction, label over it), `Graph` (up to two series, grid lines);
                            `sparkline()` / `gauge()` / `graph()` build them from a theme node; `meter()` is
                            containers (`meter > fill` with `FillPortion`), so the theme styles it

Scripts    (scripts.rs)     daemon-owned programs feeding gadgets (`[Custom] exec`): `Spec` (argv, interval,
                            return_type) -> last `Output` (text, icon, classes); one run per distinct spec
  subscription(specs)       from `Panel::scripts()` every time: one runner per spec, restarted by `Refresh`
                            (the generation is part of its identity); yields `Event::Ran(spec, output)`
  apply(Event) / run(Command::Refresh)

Locale     (locale.rs)      daemon-owned UI language, in `Shared` as `ctx.locale`: `tr("locker.unlock")`,
                            `fmt("notifications.age.minutes", &[("n", &5)])`, `date(&dt, "%A %d %B")` (chrono
                            `format_localized`); `[general] language`, else LC_ALL / LC_MESSAGES / LANG
  locale/en.rs, it.rs       one `CATALOGUE: &[(key, text)]` per language, compiled in, stable dotted keys;
                            `en` is the fallback under every other; a unit test scans `src/` and fails on a
                            key without an English text, an English text nobody uses, or a language whose
                            keys differ from English's

time       (time.rs)        `aligned_ticks(step)` (a wall-clock-aligned tick stream) and `shows_seconds(format)`,
                            for the Clock, the locker, the notifications' ages, the sysmon sampler; nothing
                            is imported from `gadgets/` by anything but `gadget.rs`

process    (process.rs)     split_words (shell-like quoting, no shell), command(line), spawn_detached, run(line):
                            the config's command lines and the launcher's desktop entries; `aria-shell` as
                            the program is this very binary. The preferred programs (`[general] terminal`,
                            `file_manager`, `[Screenshot] editor`): `chosen` (`auto` = `first_installed` of
                            a list of command lines, `none`/`off`, else the line), `filled` (a placeholder
                            word, `%c` the program a terminal runs, `%f` a path, else appended last;
                            `in_terminal`, `on_file`). Each child is moved (systemd's
                            `StartTransientUnit` with its pid, on the session bus, best effort) into
                            `app-aria\x2dshell-<app>-<pid>.scope` in `app.slice`: out of our cgroup, so a
                            `systemctl --user restart aria-shell` (the unit kills its whole cgroup) spares
                            them. Moved after the spawn: a fork made before the move stays with us

Theme      (theme/)         daemon-owned styling: base.css + the user's theme, parsed once for one Scheme
  load / try_load(&Config, style, scheme)   css.rs (scanner) -> selector.rs + value.rs (typed rules)
  scheme() / name()         what's loaded; `available()` lists the themes/*.css of every theme dir
  Command                   ToggleScheme | SetScheme | SetStyle: from a gadget (Action::Theme), the daemon
                            keeps `style`/`scheme` at runtime and reloads (no persistence)
  resolve(&Node) -> Style   cascade for one element path; container()/button()/text()/row() helpers

Icons      (icons/)         daemon-owned app icons: window class -> `Icon` (iced svg/image handle)
  load() -> Task            builds `Index` (icons/theme.rs theme chain + icons/desktop.rs .desktop db) off-thread
  apply(Event::Loaded)      installs it; resolve(class) fills the per-class cache; get(class) in `view`
  index() -> Arc<Index>     the desktop db is also what the launcher searches; desktop::launch runs an entry

Dialog     (dialog.rs)      the modal surface the launcher and the exit menu share: one Overlay layer surface
                            centred on the focused output with the keyboard, a transparent grab surface per
                            output under it; `open(namespace, output, size, outputs)` -> the surfaces to
                            create, `resize`, `windows()`, `rect(output)`; `pointer(window, event)` says when
                            a click outside happened (a press *ignored* by the widget tree on any of its
                            windows, then its release: `dialog::content` wraps the content in a `mouse_area`
                            so every press inside is captured); `pointer_events()` the subscription

Launcher   (launcher.rs)    a component the daemon owns while open: `launcher: Option<(Dialog, Launcher)>`
  Message / update -> Action { Run(Task) | Close | Exit(name) }, view(Shared), subscription() for Up/Down/Esc
  actions                   `[launcher] actions = all | none | names`: the exit menu's buttons as a row of icons
                            above the search field; a click goes through the exit menu's flow (Exit(name))

Exiter     (exiter.rs)      the exit menu, `aria-shell exiter toggle`: `exiter: Option<(Dialog, Exiter)>`
  ExiterConfig              `[exiter]`: columns, ask_confirm, confirm_timeout, `buttons = ...` in order, each
                            `<name> = [!]<command line>` (`!` = confirm; `auto` = the compositor's own exit),
                            `<name>_icon`, `<name>_label`; the six standard ones have defaults, icons with
                            fallbacks (Adwaita lacks suspend/hibernate) and labels from the catalogue
  Message / update -> Action { Run(Task) | Perform(Command) | Close }; the grid (`columns` per row, arrows
                            + Enter), or the confirmation in place (Cancel / the action, a countdown that
                            runs it by itself); `size(theme, locale)` measured from the content, the daemon
                            resizes the dialog after every update (`sync_exiter`); `confirming(config, name)`
                            opens straight on a confirmation (the launcher's row)

Locker     (locker/)        a component the daemon owns from `aria-shell lock` to the unlock: `locker: Option<Locker>`
  Message / update -> Action { Run(Task) | Unlock }, view(shared, root), subscription() (the clock tick;
                            Enter when there is no password field)
  windows                   the lock surfaces the runtime made (one per output, `ShellEvent::NewShell` of type
                            `SessionLock`), all drawing the one state; `Message::Lock` / `UnLock` to the runtime
  pam.rs                    `authenticate(user, password)`: a direct libpam binding, blocking (spawn_blocking)

Wallpapers (wallpaper.rs)   the desktop background: `WallpaperConfig::for_output(config, connector)` picks
                            `[wallpaper:<connector>]` (with a source) over `[wallpaper]`; `source` through
                            `Config::resolve_path` (`~`, absolute, else relative to aria.conf's dir), `fit` =
                            CSS object-fit (cover default, contain, fill, none, scale-down -> iced ContentFit);
                            the daemon opens one `Layer::Background` surface per output (`wallpapers:
                            BTreeMap<Id, Wallpaper>`, with the panels, closed with the output); `images:
                            Wallpapers` decodes each file once off-thread (`load` -> `Event::Loaded`, by
                            content like the avatar), shared by path, reloaded when the watcher sees the
                            file change; `view` is `image(handle).content_fit(..)` in a `wallpaper` root
                            container (the theme's background shows around a `contain`ed image)

Osd        (osd.rs)         daemon-owned, display only: `[osd]` (`show` = what to watch, duration, position,
                            margin); `observe(&Audio, &Network, &Power, &Idle, &Brightness)` after every change of
                            those (and of the user's hold on idle) reads a `Watched` (the default output's and
                            input's device/percent/mute, each screen's brightness, whether some app records,
                            Wi‑Fi enabled, the network
                            connected + label, the VPN up, the charger plugged with a battery, the power
                            profile, keep awake; each `None` until known, so the first reading after start or
                            a source coming back shows nothing; "connecting" keeps the last reading) and
                            `change(old, new)` (pure, unit-tested) says what to show, the first that changed
                            in `Watch`'s order (Wi‑Fi turned off before the disconnection it brings). Whoever
                            made the change (a keybind, a gadget, another app) is not its business. `show(Content)` from there or from
                            `Command::Osd` (`aria-shell osd show`); the daemon opens one `Layer::Overlay`
                            surface per output (`Content::outputs`: only those, each with its own percent, the
                            others' surfaces closed: a brightness change shows on the screens that changed;
                            `events_transparent`, sized by the theme's `osd`, anchored by
                            `position`), redraws the open ones, and a timer per show sends `OsdExpired(serial)`:
                            only the last show's closes them, so a held volume key keeps the bar up

commands::listen()          (commands.rs) the command socket as a Subscription; `Command::Launcher(ToggleCommand)`,
                            `Command::Exiter(ToggleCommand)` (toggle | show | hide), `Command::Lock`,
                            `Command::Osd(Content)`, `Command::Brightness(brightness::Command)`,
                            `Command::Volume(VolumeCommand)` (made an `audio::Command` by the daemon with the
                            Audio gadget's `[Audio] step` / `max_volume`, so keys and wheel agree),
                            `Command::Screenshot(screenshot::Command)`, `Command::Open(OpenCommand)` (Terminal
                            | FileManager(dir): `[general] terminal` alone, `file_manager` on the dir or the
                            home, run by the daemon)
commands::send(args)        the client: `aria-shell launcher toggle` is the same binary with arguments; the
                            dir of `open file-manager` made absolute first (the shell's cwd isn't ours)
commands::single_instance() first thing in `main` for the shell: an exclusive `flock` on
                            `<WAYLAND_DISPLAY>.lock` next to the socket, held until exit, or exit 1 (a
                            second shell would run the autostart again and take the socket); a lock,
                            not a `ping`: atomic for two shells started together, gone with a crash

watch::watch(paths)         (watch.rs) one `notify` subscription for aria.conf, theme files and the
                            icon/applications dirs; yields `Changed(paths)`: config -> rebuild panels,
                            theme -> reload it, anything else -> rebuild the icon index
```

Two things flow between the daemon and the gadgets besides messages:

- **`gadget::Context`** goes *down*, into `view`. It holds
  `gadget::Shared` (`&Compositor`, `&Theme`, `&Icons`, `&Tray`,
  `&Audio`, `&Network`, ...): daemon-owned, read-only, plus the gadget's own `theme::Node`. A
  gadget that shows shared state keeps no copy of it, it filters the
  context in `view`.
- **`gadget::Action`** comes *up*, out of `update`, in place of a bare
  `Task`: `Action::Run(Task)` for the gadget's own async work,
  `Action::Compositor(Command)` / `Action::Tray(Command)` (and later
  `Action::Audio(..)`, ...) for things only the daemon can do,
  `Action::OpenPopup`/`ClosePopup` for a popup surface, `Action::Many`
  for several at once. `Panel::update` turns it into the concrete
  `panel::Action` (same variants, popup bookkeeping done) and
  `AriaShell::perform` into a `Task`. Gadgets never hold an IPC handle.

- **Popups** are xdg popups parented to the panel's layer surface. A
  gadget with one keeps a `gadget::Popup` field, exposes it through
  `Gadget::popup()`, wraps the widget the popup hangs from in
  `popup.anchor(..)` (a `container` tagged with a `widget::Id` unique to
  the `Popup`; `anchor_nth(n, ..)` / `toggle_nth(n)` when several widgets
  may host it, one per tray item) and returns `popup.toggle()` from
  `update`; the content is `Gadget::popup_view`. **Its size is a
  function of the state**: `Gadget::popup_size(ctx)` (a Wayland surface
  needs it up front, iced can't size it from the content). The daemon
  asks for it when the popup opens and again after every shared-state
  or gadget update (`sync_popups`); a different answer is sent as
  `PopUpReposition` with the same anchor, which is how a tray menu
  grows when a submenu unfolds. `Theme::measure(node, text)` /
  `line_height(node)` exist for that (cosmic-text through
  `iced::advanced::graphics::text::Paragraph`). Under the hood `toggle`
  yields `Action::OpenPopup { anchor }` / `ClosePopup(id)`; the panel
  mints the `window::Id`, remembers `popup -> gadget index` and records
  it in the gadget's `Popup`. The daemon keeps `popup -> (panel, anchor
  rect, size)`, asks the widget tree for the anchor's bounds with a
  custom `Operation` (`widget_bounds` in `main.rs`) and sends `NewPopUp`
  placed by `panel::popup_settings` (centred on the anchor, its box
  meeting the bar's edge whatever the anchor's height: the anchor rect
  is stretched to the bar's whole thickness). `view(popup_id)` routes to
  `Panel::popup_view` -> `Gadget::popup_view`. Whoever closes it (the
  gadget, or the compositor on a click outside), it ends in
  `ShellEvent::Closed(id)` -> `Panel::popup_closed` -> the gadget's
  `Popup` is marked closed and `Gadget::popup_closed` runs.

- **Popups and the keyboard.** A popup of a bar never gets keyboard
  focus by itself: on Sway (wlroots) an xdg popup's grab doesn't move
  the keyboard, and the bar's layer surface has
  `KeyboardInteractivity::None`. So a gadget whose popup shows a text
  field says so (`Gadget::popup_keyboard`), and the daemon makes the
  bar `Exclusive` *before* the popup maps (`sync_keyboard`, first in
  the batch of `Message::Panel`: a change after the grab started does
  nothing) and `None` again when the popup closes. The compositor then
  sends the keys to the *bar's* window (verified: the popup's window
  never sees them), and the daemon forwards every keyboard event
  arriving on such a bar to the gadget (`Message::PanelKey` ->
  `panel::Message::Key` -> `Gadget::popup_key`), which edits its own
  field (the `text_input` is controlled state anyway): characters,
  Backspace, Enter submits, Escape folds. No cursor blinks in the
  field and there is no paste; a compositor that does focus the popup
  would let the widget handle the keys itself. `operation::focus` is
  still run, one message after the field appears (an operation runs
  on the widget tree as it is, the field isn't in it yet).
- **Light/dark**: a theme is loaded for a `theme::Scheme` (`[general]
  color_scheme`, default light; the `Themes` gadget switches it at
  runtime, in memory only — following or setting the desktop's scheme is
  for later, per DE). `:root.light { }` / `:root.dark { }` blocks hold
  the scheme's variables, folded per file over the plain `:root` ones
  (`base :root`, `base :root.<scheme>`, `user :root`, `user
  :root.<scheme>`: a user theme's plain variable still beats the base's
  scheme one); a theme that only sets `:root` looks the same in both.
  `Theme::resolve` gives the root node of every path the scheme as a
  class, so `panel.dark { }` rules work without touching the nodes
  gadgets build. `base.css` carries both palettes (Catppuccin-like
  Latte / Mocha). `debug theme` prints `style=<name|-> scheme=<..>`.
- **Programs, not shells**: every command line in the config
  (`[Custom] command*`, `command_wheel_*`, `exec`) is a program and its
  arguments, split with shell-like quoting (`process::split_words`) and
  run directly, detached; the user writes `sh -c '...'` when a shell
  is wanted. The shell's own commands are its CLI (`command =
  aria-shell launcher toggle`), with that name resolved to the running
  binary so a dev build works the same; no `aria ` prefix magic as the
  Python one had. `exec` programs are run by the daemon (`scripts.rs`),
  once per distinct spec however many panels show the gadget: two
  monitors don't run `checkupdates` twice (it fails when they do), and
  the output is shared state read from `ctx.scripts`; a gadget asking
  for a fresh run after a click returns `Action::Script(Refresh)`.
- **Styling is a CSS-like theme file**, resolved per widget in `view`.
  `assets/base.css` (compiled in, always first) documents the element
  tree and the supported properties for theme authors; `[general] style`
  names a user theme loaded on top (`themes/<name>.css` in the config
  dirs, the XDG data dirs, then `assets/`; or a path). Both are parsed
  and type-checked at load (`theme/css.rs` scanner, `selector.rs`,
  `value.rs`); bad selectors/declarations are logged with `file:line:col`
  and skipped, a syntax error rejects the file (and on hot reload keeps
  the last good theme). `:root { --x: v }` variables are substituted
  textually after all files are merged, so a user theme can override a
  variable the base uses. Cascade is specificity then source order.
  In `view`, every widget has a `theme::Node`: its element path
  (`panel.top#2 > slot.start > gadget.workspaces > workspace.active`),
  an immutable `Arc` list so style closures can own one. `Theme::resolve`
  walks root→leaf applying matching rules and inheriting `color` and
  `font-*`; it only tries, at each node, the rules whose rightmost
  compound names that node's type plus those naming none (indexed at
  load, merged back into cascade order): trying all ~280 on every node
  and ancestor was 46% of the CPU while the pointer moved over a bar,
  as every message rebuilds every surface (optimized build, perf);
  `Theme::button/container/text/row` do that and return plain
  iced widgets. A button's style closure re-resolves with
  `node.status(status)`, which is how `:hover`/`:active` work. Text gets
  an explicit colour only when a rule set it on the text node itself;
  otherwise iced's own cascade (container/button `text_color`) carries
  it, so `workspace:hover { color }` reaches the label. Class names are
  the Rust roles (`panel`, `slot`, `gadget.clock`, `workspace`, ...),
  not the Python/GTK ones. The bar thickness is the theme's `min-height`
  on `panel` (layer-shell needs it before layout); a reload that changes
  it sends `LayoutChange` + `ExclusiveZoneChange`. Surfaces are created
  transparent (`daemon(..).style`), the theme's `panel`/`popup`
  background is what shows, so `rgba` bars and rounded popups work.
  Gadgets must not hard-code colours/paddings/spacing: derive nodes from
  `ctx.node` and go through the theme helpers.
- **App icons** (`icons/`) are a daemon-owned source with the usual
  verbs. Resolution is `class` -> desktop entry (by id, `StartupWMClass`,
  `Exec` basename, reverse-DNS suffix) -> `Icon=` -> theme lookup, then
  the `[apps_class_map]` override, the class as an icon name, and
  `application-x-executable`. The theme lookup is an **in-memory index**:
  every directory the `index.theme` chain lists (theme, `Inherits`,
  `hicolor`, then `pixmaps/`) is read once into a name -> (dir, ext) map,
  so a lookup is a hash probe plus the spec's size choice (exact match,
  svg first, else closest; scale-1 dirs only; no xpm), with no `stat`
  per candidate. Built with `spawn_blocking` from the boot `Task`, so the
  bars show dots first and icons a few ms later. Warm timings in release
  on this machine: Adwaita+AdwaitaLegacy+hicolor (2k icons, 700 dirs)
  ≈ 10 ms, breeze+hicolor (8k names, 18.7k files) ≈ 23 ms, 80 desktop
  entries ≈ 2 ms. The `applications/` dirs and every indexed theme dir
  are watched (≈ 725 inotify watches here, the limit is 524k): an
  install or removal rebuilds the index after the burst settles and the
  daemon re-resolves the classes it shows (the Python version missed
  this). `Icons::resolve` is called by the daemon after every compositor
  event, never from `view`; handles are created once and cloned, since
  iced caches decoded images by handle id. Icon size and tint come from
  the theme (`window { height; color }`; only `-symbolic` svgs are
  tinted). Not done: `icon-theme.cache` (GTK's mmap cache) as a
  zero-scan fast path, worth it only if cold starts turn out slow.
- **The launcher is a component, not a gadget**: it has its own layer
  surface and the keyboard, one instance at a time, on the focused
  output (`Compositor::focused_output`, from Hyprland's `j/monitors[].focused`
  and the `focusedmonv2` event; the first output if unknown). Opened by
  `aria-shell launcher toggle|show|hide` over the command socket
  (`$XDG_RUNTIME_DIR/aria-shell/cmd.sock`, the Python line protocol:
  `OK ...` / `ERR ...` per line; parsing is in the listener, the daemon
  only sees valid `Command`s). The surface is `Layer::Overlay`, no
  anchors (the compositor centres it), sized by the theme's `launcher {
  width; height }`, `KeyboardInteractivity::Exclusive` (`OnDemand`
  works on Hyprland but not on Sway, whose `arrange_layers`, run on
  any layer-surface commit on the output, our grab surfaces mapping
  included, unfocuses a non-exclusive focused layer: the launcher
  closed within a second. Hyprland routes every pointer event to an
  exclusive layer, so there the outside click comes tagged with the
  launcher's window and is told from the coordinates being outside
  its size: verified on Hyprland 0.56, an outside click arrives as
  e.g. `(-400, 570)` on a 520x420 launcher, one on the other output as
  `(-2320, 370)`); the search
  field is focused with `operation::focus` on the `ShellEvent::NewShell`
  of that surface (earlier, the widget tree doesn't exist yet). A click
  outside closes it as it does for a popup: while it's open the daemon
  keeps one full-screen transparent surface per output on `Layer::Top`
  (above the bars, below the launcher, `exclusive_zone: -1`) whose
  `mouse_area` yields `Close`; the click is swallowed. Any of those
  surfaces closing (`ShellEvent::Closed`) closes the rest. It also
  closes when the keyboard leaves its surface
  (`window::Event::Unfocused`: a keybind moved focus, a click on a
  window), which is how cosmic-launcher closes; on its own that isn't
  enough here because a click on a bar or on an empty desktop doesn't
  move keyboard focus on Hyprland/Sway. It searches
  the desktop db already in `icons::Index` (an `Arc` snapshot, replaced
  when the index is rebuilt), scoring as the Python did (exact 10,
  prefix 8, substring 6 over id/name/comment, empty query lists all);
  `NoDisplay` and entries without `Exec` are skipped. Icons are
  resolved by the daemon for the listed ids (`Icons::resolve` accepts
  a desktop id, `for_class` tries `by_id` first) after every launcher
  update, never in `view`. Launching is hand-rolled in
  `icons::desktop::launch` (the Python used `gtk-launch`): spec
  quoting, field codes, `Path=`, `Terminal=true` via `[general]
  terminal` (`process::in_terminal`: the program for `%c`, else `-e` and the program; `auto`
  = `$TERMINAL`, else the first of `process::TERMINALS` on the PATH; `none` refuses), own process
  group, stdio to null, reaped by a thread. Not done: `DBusActivatable`,
  startup notification, other providers (the Python had only apps too).
  The Python `[launcher]` keys `width/height/icon_size/opacity` are
  theme matters here (`launcher`, `launcher icon { height }`), not
  config.
- **The exit menu** (`exiter.rs`) is the third component on a `Dialog`
  (`dialog.rs`, the launcher's surface-plus-grabs mechanics pulled out
  of `main.rs`). Its buttons are explicit config (`buttons = ...`, one
  `<name> = [!]command` each, `_icon`/`_label`), not free-form keys as
  the Python had; the confirmation replaces the grid on the same
  surface (no second window) with a countdown from `confirm_timeout`
  that runs the action by itself; `logout = auto` goes through the
  compositor IPC we already hold (`compositor::Command::Exit`) instead
  of a program; `grab_display` and `opacity` are gone (the grabs are
  always there, opacity is the theme's `exiter { background }`). The
  surface is sized from the content (`Exiter::size`, the popups'
  measuring) and resized on every change (`LayoutChange`), which is
  what forced the click-outside rework below. The launcher shows the
  same buttons as a row of icons (`[launcher] actions`) and hands a
  click to the same flow: run, or open the exit menu on that button's
  confirmation.
- **A click outside a dialog is an *ignored press***, not a position
  check. The launcher first compared the last `CursorMoved` with the
  surface size (Hyprland routes every pointer event to the exclusive
  layer, with surface-local coordinates past the edges). That broke
  once a dialog could resize: a button press shrinks the exit menu
  (its confirmation) before the release is reported, and in the same
  event batch iced delivers the button's message (from the UI update)
  before the subscriptions' `CursorMoved`, so the position was judged
  against the wrong size; on Sway the resize is applied late and a
  pointer heading for a button crosses the grab surface meanwhile.
  Now `dialog::content` wraps the content in a `mouse_area` that
  captures presses, and `dialog::pointer_events` reports presses with
  `Status::Ignored` (nothing under the cursor: a grab, the dialog past
  its edges, or a cursor that never entered it, which is the bar button
  clicked twice) and releases; a press outside marks the dialog, the
  release closes it. Same on both compositors, no size bookkeeping.
- **The lock screen is a component too** (`locker/`), the launcher's
  shape: the daemon holds `Option<Locker>` from the `lock` command to
  the unlock. The Wayland side is entirely the runtime's
  (`ext-session-lock-v1` in `exwlshellev`): `Message::Lock` asks the
  compositor for the lock and makes one lock surface per output (and
  one for any output plugged in meanwhile), announced as
  `ShellEvent::NewShell` with `ShellType::SessionLock` then
  `WindowOutputChanged`; `Locked` confirms, `LockDenied` (no protocol,
  or refused: the locker is dropped), `LockedFinished` (the compositor
  ended it). `Message::UnLock` tears the surfaces down (`Closed` each).
  The daemon's `view(id)` draws the one `Locker` state on every lock
  window inside `locker[output=..]`: the password typed on whichever
  surface has the keyboard (the compositor's choice) is the password;
  the field's `widget::Id` is shared so one `operation::focus` on
  `NewShell` focuses it everywhere. Behaviour as the Python: `[locker]`
  keys, avatar (`~/.face`, `~/.face.icon`, AccountsService; decoded by
  content since `image::open` trusts only the extension and `.face` has
  none) → name (gecos, else login, `getpwuid_r`) → time → date →
  password field (+ an eye button showing it in clear, `secure(!peek)`;
  the click takes the keyboard from the field, so the toggle refocuses
  it) + message + Unlock; `password_prompt = no` unlocks on Enter/click
  without PAM. PAM is `locker/pam.rs`: `pam_start` /
  `pam_authenticate` / `pam_acct_mgmt` / `pam_end` declared by hand
  (`#[link(name = "pam")]`, as `libc::kill` rather than a crate), a
  conversation answering the password to `PAM_PROMPT_ECHO_OFF` (responses
  `calloc`/`strdup`ed, PAM frees them), run in
  `spawn_blocking` (a refusal sleeps ~2 s in `pam_faildelay`). Service:
  `[locker] pam_service`, else `aria-shell` when `/etc/pam.d/aria-shell`
  exists (`assets/pam.d/` has it: `include login`), else `login`, which
  every distro has and `swaylock`'s own file includes; `other` is
  `pam_deny` on Arch, so an unknown name refuses everything. `pam_unix`
  goes through the setuid `unix_chkpwd`, so no privilege is needed. The
  conversation keeps PAM's `TEXT_INFO`/`ERROR_MSG` texts and the locker
  shows them in place of "Authentication failed" when a refusal came
  with some; `pam_faillock` sends its "account is locked" as a
  `TEXT_INFO` (`pam_info`) and only without `PAM_SILENT`, so the flags
  are 0. **Learned the
  hard way**: `system-auth` has `pam_faillock` (`deny = 3`, `unlock_time
  = 600` by default): three refusals on a real service lock the account
  for ten minutes, the right password included, with only "Authentication
  failure" from `pam_strerror` (the reason comes as an `ERROR_MSG`). One
  `cargo test` plus two scenario runs did that to the developer's
  account on the first desktop try; so the scenario's config names a
  service with no file (`pam_service = aria-shell-ui-test`: `other`
  refuses at once, no tally) and the ignored unit test does the same.
  `faillock --user <name>` shows the tally, `--reset` clears it (root).
  Not ported: the shake
  (no animations in the theme), the spinner (a "Unlocking…" text), a
  wallpaper behind (the theme's `locker { background }` for now).
- **The tray** (`tray/`) is the first DBus source, the shape notifications
  and MPRIS will copy: one `zbus::Connection` (tokio feature) opened in
  the subscription's stream, handed to the daemon as
  `Event::Connected(conn)` so `Tray::run` can call methods with it; the
  stream runs the host loop. The
  `org.kde.StatusNotifierWatcher` object is always served
  (`#[interface]`, item keys are `<bus name><path>`, an item is
  unregistered when its name leaves the bus via `NameOwnerChanged`);
  the name is requested with `DoNotQueue`, and if another bar owns it
  (noctalia on this desktop) we act as a plain host of that watcher,
  which uses the same proxy and signals as talking to our own, and
  retry when the owner goes away. One tokio task per item: `GetAll`
  on `org.freedesktop.DBus.Properties`, then `NewIcon`/`NewStatus`/...
  re-read the properties they cover (`PropertiesChanged` too, for the
  apps that emit it; proxies are built with `CacheProperties::No`,
  most items never emit it). `IconPixmap` (`a(iiay)`, ARGB32 network
  order) is picked (largest <= 64px) and converted to RGBA once per
  change into an `image::Handle` kept in the `Item`; `IconName` goes
  through `Icons::resolve_name` (the item's `IconThemePath` searched
  first, then the theme, then the generic fallback). Menus:
  `com.canonical.dbusmenu` `GetLayout(0, -1)` after `AboutToShow`
  (apps build their menus on it: nm-applet's VPN submenu appears
  then), parsed into a `Menu` tree (invisible items dropped, `_`
  mnemonics stripped), shown by the gadget as a column of buttons with
  submenus unfolding in place; a click sends `Event(id, "clicked")` and
  closes the popup; `LayoutUpdated`/`ItemsPropertiesUpdated` re-fetch
  a loaded menu. Left click: `Activate`, or the menu if `ItemIsMenu`;
  right: the menu, or `ContextMenu` when there's none; middle:
  `SecondaryActivate`; wheel: `Scroll(clicks, orientation)`, positive
  up as KDE sends it. The click methods get the pointer's global
  position as the daemon estimates it. No tooltips (a 32px surface
  would clip them), no overlay icons, no menu icons or shortcuts.
- **Texts and dates go through `Locale`** (`locale.rs`), never a
  literal in a view: `ctx.locale.tr("audio.output")` with stable dotted
  keys (a changed English wording touches `en.rs` only), catalogues
  compiled in as static slices (`locale/<lang>.rs`, poured into one
  `HashMap` at startup with English underneath), no files to install,
  no globals, no macros. Dates: `locale.date(&dt, fmt)` over chrono's
  `unstable-locales` (`format_localized`; the `Locale` enum knows
  `xx_YY` names only, so a bare `language = it` maps to `it_IT` via the
  catalogue's default region). Messages from libpam come translated by
  gettext already. Desktop entries carry their own translations:
  `Locale::languages()` (`it_IT`, `it`, the spec's order without
  modifiers) goes into `Icons::new`, and `desktop::parse` keeps the
  best-ranked `Name[..]`/`Comment[..]` over the plain key (only those
  two: `Icon[it]` is skipped), so the launcher lists and searches the
  localized texts; the index is rebuilt on a config reload, which
  covers a language change. Untranslated on purpose: config keys, theme
  selectors, the log, the socket replies, units. The completeness test
  is the translator's tool: `cargo test` names the missing keys.
- **No global state.** `Config` is loaded in `AriaShell::new` and passed
  by `&` down to gadget construction. This is what makes hot-reload
  possible later (replace the value, rebuild panels) and what makes the
  config layer unit-testable.
- **Multi-monitor from day one.** The daemon starts in
  `StartMode::Background` (no surface). It subscribes to the shell
  broadcast (`iced_wayland_subscriber`); on `OutputAdded` it opens one
  layer surface per `[panel*]` section that wants that output
  (`NewLayerShellSettings { output_option: OutputOption::GlobalName(id) }`),
  on `OutputRemoved` it removes them. The broadcast replays current
  outputs to late subscribers, so startup and hot-plug are the same path.
- **External event sources are `Subscription`s**, not services. A timer
  is a stream; the compositor IPC socket is a stream (`compositor::hyprland::events`);
  a DBus connection will be one too. Shared connections live in the
  daemon's subscription, their events fan out through `update`, never a
  mutex-guarded static.
- **Subscription identity.** iced dedups subscriptions by hash. Every
  level keys its children's subscriptions with `.with(key)` (gadget index
  in `Panel`, `window::Id` in `AriaShell`) so two identical gadgets on two
  outputs keep separate streams.
- **Closed gadget set.** No plugins, no `dyn`. Adding a gadget: one file
  under `gadgets/`, one variant in `AnyGadget` and `gadget::Message`, one
  arm in each `match` in `gadget.rs`.
- **Reusable widgets live in `widgets/`** (`Calendar` so far), as plain
  Elm components: a host embeds one as a field, calls `update` with the
  widget's `Message` and `.map()`s its `view`. Nothing in iced or a
  third-party crate fit (`iced_aw::date_picker` is a modal picker, and
  would pin our iced version); COSMIC's calendar is inside `libcosmic`.
- **Config sections** implement `config::Section` by hand
  (`const NAME` + `from_raw(&RawSection)`). `RawSection` has the typed
  accessors (`str_or`, `bool_or`, `list_or`; add `int_or` when a section
  needs it).
- **Shared state is owned by the daemon**, one struct per source
  (`Compositor` now; audio, tray, notifications later), each with the same
  three verbs: `subscription()` (one stream for the whole process),
  `apply(Event)`, `run(Command) -> Task`. Gadgets read it through
  `Context` and change it through `Action`. This is the shape to copy for
  the next shared source; don't give gadgets their own connection.

### Facts about the crates, verified in source (v0.20.1 / iced 0.14)

- zbus 5 with `default-features = false, features = ["tokio"]` runs on
  iced's tokio runtime (`tokio::spawn` from a subscription works). A
  `#[proxy]` caches properties by default and only invalidates them on
  `PropertiesChanged`, which SNI items rarely emit: build item proxies
  with `.cache_properties(CacheProperties::No)`.
  `request_name_with_flags(.., DoNotQueue)` returns
  `Err(Error::NameTaken)`, not `Ok(RequestNameReply::Exists)`, when
  another connection owns the name. A `#[interface]` method gets the
  caller with `#[zbus(header)] h: Header<'_>` (`h.sender()`) and emits
  signals through `#[zbus(signal_emitter)]`; from outside a method,
  `SignalEmitter::new(&conn, path)`. Signal streams from different
  signals are different types: merge them with `stream::select_all`
  over `BoxStream`s. `zvariant` converts an `OwnedValue` to a tuple /
  `Vec` / `String` with `try_from`; a `Structure` inside an `av` comes
  back as a tuple the same way.
- `Message::PopUpReposition { settings: IcedNewPopupSettings, id }`
  resizes and re-places a mapped popup (xdg_popup v3 `reposition`;
  Hyprland and Sway have it, older compositors log and ignore); the
  runtime updates the surface size on the following configure.
- One wheel click reaches iced as **several** `WheelScrolled` events
  in `iced_exwlshell`: `Pixels { 0, 0 }` (from `axis_source`), `Lines
  { y }` (from `axis_value120`/`axis_discrete`) and `Pixels { y: 15 }`
  (from `axis`), signs negated (up is positive). Touchpads send only
  `Pixels`. The tray counts `Lines` as clicks, accumulates `Pixels`
  into clicks of 15 and drops a `Pixels` arriving within 100ms of a
  `Lines` (the same click); `-0.0.signum()` is `-1.0`, so a zero delta
  must be filtered before taking its sign.
- A widget's on-screen bounds for `debug widgets` are the container's
  outer bounds; `Theme::button` puts the node's padding on the button,
  so the row inside must not get it again (`theme.row(&node, ..)`
  would: the Workspaces gadget does that, doubling `workspace`
  padding; left as is since the shipped themes are tuned to it).
- Theme selectors are global: `gadget.tray item` matched the menu
  rows under `popup > gadget.tray > menu > item` too and won on
  specificity, so base.css uses `gadget.tray > item` for the bar.
- A layer surface's `margin: Some((top, right, bottom, left))` and
  `Message::MarginChange { id, margin }` place it from its anchored
  edges; `exclusive_zone: None` keeps it out of the bars' zones (Sway
  and wlroots arrange exclusive surfaces of every layer first, so an
  overlay toast still sits under a `top`-layer bar). That's how the
  notifications stack: one surface each, the daemon recomputes the
  margins on every change.
- A surface is only drawn inside its bounds: one the size of its box
  cut the box's `box-shadow` flat (and transparency has 2 bits for now,
  see the known risks) (iced draws it `blur` past the box
  moved by the offset, `quad/solid.wgsl`). So the boxed surfaces
  (popups, toasts, the OSD, the dialogs) are their box plus
  `Theme::shadow_room(root)`, the box drawn inside it
  (`Theme::surface`), and placed so the box stays put: margins less the
  room, a popup's anchor rectangle shortened on the bar's side
  (xdg-shell has no offset; a moved rectangle could leave the bar). A
  click on the shadow is the surface's: lost on a popup or a toast,
  "outside" on a dialog (the room is outside its `mouse_area`).
- iced's `container` and `button` lay their content out inside the
  padding only: the border is drawn over it and takes no room, so a
  surface measured for its content adds padding, not `border-width`.
  Wrapped text is measured with `Paragraph::with_text` bounded on the
  width (`Theme::measure_in`); it matches `text(..).wrapping(Word)` in
  a container of that width to the pixel.
- `gdbus call` infers `[255, 0]` as `ai`: an `image-data` hint from
  the shell needs `@ay [..]` in the tuple.
- `debug widgets` only reports what the theme helpers tag (containers
  and buttons): a bare `Theme::text` or `canvas` isn't found, wrap it
  in a `Theme::container` of its node when a scenario needs it. Its
  rectangles are layout coordinates: inside a `scrollable`, what's
  below the fold is reported where it would be unscrolled, so a
  scenario can only click what fits in the popup (one tab at a time
  does).
- iced `canvas` (feature `canvas`): a `Program` with `draw(&self,
  state, renderer, theme, bounds, cursor) -> Vec<Geometry>`, a
  `Frame::new(renderer, size)`, `Path::new(|b| ..)` with
  `move_to`/`line_to`/`close`, `frame.fill(&path, Color)` and
  `frame.stroke(&path, Stroke::default().with_color(..).with_width(..))`.
  The canvas has no id of its own; a themed container around it gives
  it background, border and a `debug widgets` entry.
- `#[to_layer_message(multi)]` doesn't add `Lock`/`UnLock`;
  `#[to_exwlshell_message]` is the same set plus those two. In daemon
  mode `Message::Lock` is refused with a log line while a lock is
  pending or held (`RequestLock` checks `LockLifecycle`), so the daemon
  keeps its own `locker.is_some()` guard to answer `lock` twice.
- `image::open(path)` (0.25, what `image::Handle::from_path` uses)
  picks the decoder from the extension only: a file without one
  (`~/.face`) fails with "format could not be determined"; decode by
  content (`ImageReader::new(..).with_guessed_format()`) and
  `Handle::from_rgba`.
- chrono `unstable-locales` (pure-rust-locales 0.8): `Locale::try_from`
  accepts `xx_YY` / `xx_YY@variant` and `POSIX`, not a bare language;
  `%a`/`%A`/`%b`/`%B` follow it, numbers don't. The calendar's weekday
  header is `%a` of a Monday..Sunday (three letters in most languages,
  the fixed cell still fits).
- `text_input` takes no `widget::Id` for `debug widgets`:
  `Theme::tag(node, input)` wraps it in a bare container with the path.
- `libc::statvfs` for a filesystem's size and `libc::kill` for a
  signal, no `nix`/`sysinfo`; `/proc/mounts` on btrfs lists one entry
  per subvolume of the same device, so the default disk list keeps one
  mount per device (`/` first).
  wider than the room left at the edge of the output is slid back by
  the compositor but not by the `debug surfaces` estimate, so the
  test config keeps gadgets with wide popups (Notifications: 380px)
  away from the ends of the bar.
- `iced::time::every` needs the `tokio` feature on `iced`. We use
  `tokio::time::sleep` directly for the wall-clock-aligned clock tick.
- `LayerSize::FILL` with only `Anchor::Top` fills the whole output height;
  always set `LayerSize::fill_width(h)` for a bar.
- `exclusive_zone` is a plain pixel count; there is no "auto from content".
- `Anchor` is a bitflag re-export of the protocol type.
- `OutputInfo` (sctk) carries `id` (`wl_registry` global name, what
  `OutputOption::GlobalName` wants) and `name: Option<String>` (connector,
  e.g. `HDMI-A-1`, what the `outputs =` config key matches).
- `Subscription::with(v)` includes `v` in the recipe hash; `.map(f)` only
  includes `TypeId::of::<F>()`.
- `configparser` strips inline comments from the first `#` anywhere in a
  value (Python needs leading whitespace). We disable inline comments
  entirely so `#ff0000` survives.
- `configparser::Ini::new_cs()` is case-sensitive on sections and keys.
- `configparser` stores sections in a `HashMap` unless its `indexmap`
  feature is on; we need it, `Config::instances` (and so the order of
  `[panel:*]` bars and of gadgets' sections) is file order.
- libpulse (`libpulse-binding` 2.30): the threaded mainloop runs
  libpulse's loop on its own thread and hands out `lock()`/`unlock()`;
  `Context`/`Mainloop` are `!Send`, so a thread of ours owns them and
  everything else reaches them as messages. `connect()` reports its
  first state change synchronously: a state callback that borrows the
  context (`Rc<RefCell<Context>>`, the crate's docs' pattern) panics
  right there, hence callbacks that only post to a channel. Sink
  inputs' `application.icon_name` / sinks' `device.icon_name` are
  rarely in an icon theme; a stream's icon comes from its
  `application.name` matched against the desktop entries. Volumes are
  cubic-mapped `u32`s, `Volume::NORMAL` = 100%, settable above (up to
  `Volume::MAX`); one `ChannelVolumes` per object, so setting keeps the
  channel count and loses the balance (as the Python did).
- MPRIS over zbus: `Metadata` is `a{sv}` and comes as a `Dict` of
  `Value::Value` boxes (nested once more when built by hand). The
  player's `Volume` isn't shown: its stream is in the mixer already,
  and Firefox ignores writes to it.
- NetworkManager over zbus: `Connection::system()` honours
  `DBUS_SYSTEM_BUS_ADDRESS` (`address/mod.rs`), which is how the UI
  scenarios put a fake NetworkManager on their session bus. One
  `MatchRule` with `sender` = the well-known name and `path_namespace`
  = `/org/freedesktop/NetworkManager` (`MessageStream::for_match_rule`)
  catches every signal of every object; `PropertiesProxy::get_all`
  per object and interface is one round trip each (a snapshot of a
  laptop with ~30 access points is ~50 of them, concurrent per list,
  well under the 200 ms debounce). `Device.StateChanged(new, old,
  reason)` and `Connection.Active.StateChanged(state, reason)` are the
  only signals whose bodies are read. AP `Ssid` is `ay`; `AddressData`
  / `NameserverData` are `aa{sv}` with a `Value::Value` box per entry.
  polkit on Arch: `settings.modify.system` is `auth_admin_keep`,
  `modify.own` is `yes`, so profiles the shell creates carry
  `connection.permissions = user:<login>:` (as nm-applet does); Forget
  on a system-wide profile is refused (logged). A `#[interface]` with
  a property `state` and a signal `StateChanged` clash on the generated
  `state_changed`: name the signal fn differently with
  `#[zbus(signal, name = "StateChanged")]`.
- Brightness, verified in the kernel's `drivers/video/backlight/backlight.c`
  (master, 2026-10): `brightness_store` -> `backlight_device_set_brightness`
  -> `backlight_generate_event`, which sends a `change` uevent and
  `sysfs_notify(.., "actual_brightness")`; the firmware's hotkeys come
  the same way (`BACKLIGHT_UPDATE_HOTKEY`). So a `poll(POLLPRI | POLLERR)`
  on `actual_brightness`, re-armed by reading it from offset 0, sees
  every change, logind's writes included (inotify sees nothing on
  sysfs). `brightness` and `max_brightness` are readable by anyone, the
  former writable by root only: logind's `SetBrightness("backlight",
  name, value)` on `/org/freedesktop/login1/session/auto` is the user's
  way, what `brightnessctl` does when built with logind. Tried on a
  laptop: the keys the firmware handles show the OSD too.
- `ddcutil` 2.2 (tried on two Dell P2314H over HDMI): `detect --terse`
  prints `Display N` blocks (`I2C bus: /dev/i2c-0`, `DRM connector:
  card1-HDMI-A-1`, `Monitor: DEL:DELL P2314H:<serial>`) and `Invalid
  display` ones for what doesn't answer; `getvcp 10 --bus 0 --terse`
  is `VCP 10 C 75 100` (~0.1-0.35 s), `setvcp 10 75 --bus 0 --noverify`
  ~0.15 s, exit 1 with `No monitor detected on bus ...` on a bus without
  one. `/dev/i2c-*` are `root:i2c` with an ACL for the seat's user here
  (`i2c-dev` loaded); without access, `detect` finds nothing. The DRM
  connector is the Wayland output's name, which is how a monitor is tied
  to its bar.
- `iced::widget::toggler` styled through `toggler::Style` (track
  background/border, `foreground` the knob, `border_radius: None` for
  round); `Theme::toggler(node, on, f)` is a container-tagged one
  with `.on` as a class the caller sets (its status only tells
  hovered/disabled).
- Hyprland 0.56 (Lua config) changed the IPC dispatch syntax: the command
  socket takes `dispatch hl.dsp.focus({ workspace = 3 })` /
  `dispatch hl.dsp.focus({ window = "address:0x..." })`; the old
  `dispatch workspace 3` is an error. `j/workspaces`, `j/clients`,
  `j/monitors`, `j/activewindow` and the `.socket2.sock` event names are
  unchanged. Dispatcher names: `/usr/share/hypr/stubs/hl.meta.lua`.
- Hyprland's "active workspace" is one per monitor (`j/monitors[].activeWorkspace`),
  and `workspacev2` only tells you the focused monitor switched. `Compositor`
  models it that way: `ActiveWorkspace(id)` clears the flag only among
  workspaces on the same output.
- `j/workspaces` comes in creation order and includes special workspaces
  (negative ids); we sort by id and drop those.
- Sway (`compositor/sway.rs`, the i3 IPC on `$SWAYSOCK`: `i3-ipc` +
  native-endian u32 length and type + JSON): a con id doesn't select a
  workspace (criteria only match views), so workspaces are identified
  by name, unique in Sway, and activated with `workspace
  --no-auto-back-and-forth "<name>"` (a bar click means "go there", not
  "go back"); windows by con id, `[con_id=N] focus`. The window list
  is a walk of `get_tree` (views are the nodes with a `pid`; the
  `__i3` output is the hidden scratchpad), since the `window` events
  carry the container but not its workspace; `get_workspaces` gives the
  list (`num` order, named ones last) plus `visible` (active per
  output) and `focused` (the focused output). A `workspace focus`
  event on an empty workspace comes with no `window focus`: the active
  window is taken from the event's `current` node (its focused
  descendant, or none).
- A `tooltip` on a 32px layer surface would be clipped to the surface, so
  the Workspaces gadget has none (Python showed name/title tooltips).
- A popup's size is asked twice: at `OpenPopup` (to have one) and
  again when the anchor's bounds come back from the widget tree
  (`PopupAnchor`), since the state may have moved on in between: the
  tray's menu loads over the bus faster than a render pass, and a
  popup created with the size of the "…" placeholder stayed that size
  until some unrelated event resized it (the tray scenario flaked once
  audio events existed to do so, a second too late).
- `Message::NewPopUp { settings: IcedNewPopupSettings, id }` (added by the
  macro). `IcedNewPopupSettings::new(parent, size, anchor_pos, anchor_size)`
  then `.anchor(PopupAnchor::Bottom).gravity(PopupGravity::Bottom)` puts
  the popup centred under the anchor rect; defaults flip/slide it back on
  screen. The runtime takes the grab serial from the last pointer button
  itself, so a popup opened from a click gets the implicit grab: a click
  outside dismisses it (`xdg_popup.popup_done` -> `ShellEvent::Closed`)
  and the compositor swallows that click. On Hyprland. wlroots' popup
  grab (Sway) only dismisses on a click outside *the client's*
  surfaces: a click on our own bar is delivered to the bar. So the
  daemon also closes the popups on a button press no widget took on
  any non-popup window of ours (`panel::presses_outside`, `Status::
  Ignored`: a press on a gadget's button is that gadget's business),
  and opening a popup closes the others (`AriaShell::close_popups`,
  telling the panel itself since no `Closed` will).
- A widget's on-screen bounds are only known to the widget tree: query
  them with a custom `widget::Operation` via `iced::advanced::widget::operate`
  (needs the `advanced` feature). The runtime runs the operation on every
  window and can't tell which answered, so tag widgets with
  `widget::Id::unique()` per instance, never a fixed name.
  `container::visible_bounds` from iced 0.13 is gone in 0.14.
- `container.center(Length::Fill)` sets width *and* height, overriding a
  fixed size set before it; use `align_x`/`align_y` for a fixed cell.
- **Driving the app from the CLI**, without compositor-specific tools
  (Sway and others are next): input with `ydotool` (uinput, so it works
  under any compositor; keys, clicks, `mousemove -a` absolute moves
  which land exactly with a flat pointer profile), positions from the
  shell's own `debug` socket commands. Setup done on this machine:
  `uinput` module loaded at boot (`/etc/modules-load.d/uinput.conf`),
  udev rule `KERNEL=="uinput", GROUP="input", MODE="0660"`, user in
  `input`, `ydotoold` as a systemd user service (`newgrp` isn't enough
  for the user manager, a re-login was). Alternatives weighed:
  Hyprland's `hl.dsp.cursor.move` / `hl.dsp.focus` (no click,
  Hyprland-only), `wtype` (keyboard only, `zwp_virtual_keyboard_v1`,
  which Sway, Hyprland and cosmic-comp all have), `wlr-virtual-pointer`
  (not universal: cosmic-comp lacks it).
  - On Hyprland a button press sent right after an absolute uinput
    warp (`ydotool mousemove -a`) is dropped more often than not, and
    `ydotool click` (press and release a few ms apart) is lost too:
    warp near the target, then a few small relative moves onto it,
    then press and release ~80ms apart, lands every time. A real mouse
    does both by itself. Once, after a Terminate from the processes
    tab and an outside click closing the popup, the bar got no pointer
    events until another surface of ours (the launcher) was mapped;
    not reproduced since.
  - `aria-shell debug surfaces` → `panel HDMI-A-1 0,0 1920x30; grab
    ...; launcher HDMI-A-1 700,330 520x420`: every surface with the
    global rectangle it *asked for* (`OutputInfo::logical_position/
    logical_size` from xdg-output plus anchor and size; popups are the
    requested placement, before any slide). Wayland doesn't tell a
    client where it really is: another client's exclusive zone shifts
    a bar (noctalia's bar reserves 35px here, so our top bar sits at
    y=35 while `debug surfaces` says 0). Exact in a harness where only
    the shell runs.
  - `aria-shell debug cursor` → `panel HDMI-A-1 local 960,15 global
    960,15`: where the pointer was last seen over one of our surfaces
    (the daemon tracks `CursorMoved`). The driver knows where it put the
    pointer, so `asked - local` is the surface's real origin: the
    calibration for the case above (asked 960,50, local 960,15 → y=35).
  - A session: `aria-shell launcher show; ydotool type 'term'; ydotool
    key 108:1 108:0` (evdev codes: 103 Up, 108 Down, 28 Enter, 1 Esc);
    `grim -g "700,330 520x420" shot.png` with the rectangle from `debug
    surfaces`; `ydotool mousemove -a -x 900 -y 461; ydotool click 0xC0`
    on a row; the log says `launched "kitty"` and `debug surfaces` no
    longer lists the launcher. Kill what you launched.
  - `aria-shell debug widgets [selector]` → `launcher > list >
    item.selected:nth-child(1) 720,388 480x41; ...`: every themed widget
    (the theme helpers tag containers, and buttons' content, with the
    node path as `widget::Id`; the calendar cells go through the theme
    for this) with its global rectangle, filtered by a theme selector
    (`:nth-child(n)` was added for it). A driver says "the third row"
    instead of measuring a screenshot.
- **UI scenarios** (`tests/ui/`, see AGENTS.md "Verifying"): `run.sh`
  starts a headless nested Sway (`WLR_BACKENDS=headless`,
  `WLR_RENDERER=gles2` when there's a render node, two 1920x1080
  outputs, `sway.conf` execs
  `inner.sh` so everything inherits the nested `WAYLAND_DISPLAY`), the
  shell with `tests/ui/config` and `tests/ui/data` (a harmless
  `aria-test.desktop` with `Exec=true`), then each scenario with
  `lib.sh`'s vocabulary; `target/ui/<scenario>/` gets status, logs and
  screenshots. Facts learned building it:
  - Under a pixman Sway (the default before, and still without a
    render node) the shell's wgpu loads the Vulkan driver but ends up
    drawing on llvmpipe (Mesa's software GL): with the test config's
    clock in seconds the shell sat at a core at rest and ~4.5 cores
    during a scenario, menus took 0.5-1s to show and `debug` answered
    in ~130ms, enough to fail fixed `settle` pauses on a laptop. Under a
    `gles2` Sway it renders on the GPU: ~9% at rest, the suite a
    quarter faster. A GPU-less CI still gets llvmpipe (slow) or would
    need iced's `tiny-skia` fallback, untested.
  - A popup that would overflow the output is slid back by the
    compositor (Sway and Hyprland alike), and `debug surfaces` /
    `widgets` report the requested placement: a scenario clicking in a
    popup keeps its gadget away from the bar's ends (the audio gadget
    sits in `items_center` for that); on the desktop `debug cursor`
    over the popup gives the real origin (local vs global).
  - `aria-inject` also opens plain xdg-shell windows (`window`, `title`,
    `close`) so the workspaces scenario needs no real application
    (`tests/ui/scenarios/workspaces.sh` exercises the Sway backend).
  - The nested shell must not talk to the desktop: `run.sh` unsets
    `HYPRLAND_INSTANCE_SIGNATURE`/`SWAYSOCK`, and the command socket is
    per display (`$XDG_RUNTIME_DIR/aria-shell/<WAYLAND_DISPLAY>.sock`)
    since the first run took over the desktop shell's socket.
  - Sway's seat has no devices in headless mode: the deprecated `swaymsg
    seat - cursor set/press` do nothing for clients (no pointer
    capability) and `wtype` creates a virtual keyboard per run, and the
    seat getting its *first* keyboard makes Sway reset keyboard focus
    (three `Unfocused` on the launcher, which closes). Hence
    `tests/ui/inject`: a tiny `wayland-client` program holding one
    `zwp_virtual_keyboard_v1` and one `zwlr_virtual_pointer_v1` for the
    whole scenario, driven over fifos (`move X Y` absolute over the
    layout, `click`, `key Down`, `type text`), with its own generated
    keymap (one keycode per keysym, Unicode `Uxxxx` names, as `wtype`
    does). Virtual pointer absolute motion maps to the whole layout
    when the pointer isn't bound to an output.
  - `set -e` is ignored inside a subshell used as an `if` condition
    (bash): the scenario runs as a plain command and its status is read
    after.
  - `restart_shell <config dir>` (lib.sh) ends the shell and starts one
    with that `XDG_CONFIG_HOME` (the pid in `shell.pid`, which inner.sh
    kills at the end), for a scenario needing a config the shared one
    can't carry (`tests/ui/config-locker`: a password prompt). The
    nested Sway 1.12 (headless) serves `ext-session-lock-v1`; `grim`
    captures the lock surfaces; the virtual keyboard types into the
    focused one.
- COSMIC as reference (checked in `cosmic-launcher`, `cosmic-panel`,
  `cosmic-applets`, `cosmic-comp`, `libcosmic`, `cosmic-settings-daemon`
  at 2026-09): **no automated UI tests anywhere**, only unit tests in
  the libraries and a manual QA checklist (`cosmic-launcher/TESTING.md`).
  cosmic-comp has `winit`/`x11` backends to run nested for development,
  no headless test harness. cosmic-launcher: a separate process,
  toggled by DBus activation (`org.freedesktop.Application`,
  `run_single_instance`), `KeyboardInteractivity::Exclusive` on a
  top-anchored layer surface, closed on `LayerEvent::Unfocused` (their
  compositor doesn't force the pointer onto exclusive layers), a dummy
  layer surface + their `overlap-notify` protocol to keep clear of the
  panel. Popups in the applets are xdg popups with the grab, as ours.
- `button.style(impl Fn(&Theme, Status) -> Style + 'a)` /
  `container.style(Fn(&Theme) -> Style + 'a)`: the closures can own data
  with the view's lifetime, which is what lets them hold a `theme::Node`
  and a `&'a Theme`. `button::Style::text_color` is a plain `Color`
  (falls back to `theme.palette().text`); `container::Style::text_color`
  and `text::Style::color` are `Option`s and `None` inherits at draw time.
- `iced::font::Family::Name` wants a `&'static str`: theme font names are
  leaked once each (`theme::intern`). Fonts come from the system via
  cosmic-text's fontdb (`FontSystem::new_with_fonts` loads system fonts);
  `Font::with_name("JetBrainsMono NF")` works for an installed font. A
  `Font` is one family, so a CSS `font-family` list is resolved at theme
  load to the first installed name, asking
  `iced::advanced::graphics::text::font_system()` (`.raw().db().faces()`).
  `text` defaults to `Shaping::Basic`, which does **no per-glyph font
  fallback** (an icon font as the family shows boxes for digits);
  `Theme::text` sets `Shaping::Advanced`.
- `daemon(..).style(|state, theme| iced::theme::Style)` is one background
  for every window (no window id); per-surface looks are done by the
  view's root container over a transparent background.
- `Padding::horizontal(x)`/`vertical(y)` are *setters* in iced 0.14, not
  the sums (`left + right`).
- iced image caches (`iced_wgpu/src/image/{vector,raster}.rs`):
  `svg::Handle::from_path` id is the path hash (stable), the file is read
  and parsed with usvg on the render thread at first draw, rasters are
  cached per (handle, size, color); `image::Handle::from_rgba` gets a
  *new* id per call. When a new entry lands, entries not drawn that
  frame are evicted, so pre-warming icons that aren't on screen is
  pointless. `svg::Style { color }` tints (for symbolic icons).
  `iced` feature `image-without-codecs` + our own `image = { features
  = ["png"] }` keeps only the PNG decoder in the binary.
- The boot closure of `iced_exwlshell::daemon` may return `(State,
  Task)`: that's where the icon index build starts.
- After every batch of messages `iced_exwlshell` rebuilds the widget
  tree of **every** surface (not ours to change), and asks a frame of
  the surfaces its redraw policy names, asked per message *before*
  `update`; without a `Daemon::redraw_scope` that's all of them, and a
  frame is always a full draw and present (no "nothing changed" check,
  no damage with wgpu). With the default a clock tick or a pointer move
  redrew both wallpapers, and the shell spent ~1 core at rest in the
  nested pixman Sway. Ours is `Message::redraw_scope`: `None` for what
  is only answered (`debug`), `Window(id)` for a panel's own messages
  and for the pointer's surface (see below), `None` for the
  shared-state events, whose
  `update` sends `Message::Redraw(Some(id))` for each surface showing
  `Shared` (all but the wallpapers), and `All` for the rest. A gadget's
  popup is another surface: a `Panel` message with a local action
  (`panel::Action::is_local`) redraws its popups the same way, any
  other action everything. Measured in the nested Sway (2 bars, 2
  wallpapers, test config, an optimized build), shell CPU before →
  after: under pixman (software drawing, a weak GPU's case) at rest
  112% → 6%, pointer moving on a bar 962% → 43%, system monitor popup
  open 198% → 53%; under gles2 at rest 2.7% either way, pointer 21% →
  19%, popup within the noise (7-10%): on a GPU drawing a wallpaper is
  cheap, and each `Redraw` batch is one more widget-tree rebuild of
  every surface (the shared events as `All` instead measured the same
  on gles2, 60/99/160% on pixman). Frames at rest 8.4/s → 4/s, with
  the pointer moving 82/s → 26/s (the pointer's bar only), wallpapers
  none. A debug build costs 4-5× the CPU on gles2 (~10% at rest).
  A new message that changes what some surface shows must either be
  `All` or be followed by `Redraw` of that surface: the scenarios read
  the widget tree, which is always fresh, so a stale frame only shows
  on screen (`tests/ui/scenarios/redraw.sh` compares screenshots).
- iced 0.14's widgets keep their status (hovered, pressed, ...) in the
  widget itself (`button.rs`: `status: Option<Status>`), so it's lost
  with every rebuild and set again only on a `RedrawRequested`; while
  it's `None` a hover change asks no frame. A surface rebuilt (every
  message, above) but not drawn ignores the pointer until its next
  frame, and entering a surface gives `CursorEntered` with no
  `CursorMoved`: the hover showed up to a second late. Hence the
  pointer's own events (`Cursor`, `PointerCrossed`) redraw the surface
  they happen on. Over an empty desktop that surface was the wallpaper:
  a frame of it, and a rebuild of everything, per pointer motion (64%
  CPU in a debug build on gles2, 460% on pixman). The wallpapers are
  `events_transparent` (an empty input region), so the pointer there
  reaches nobody (11% / 15%, the same as at rest); a click on the bare
  desktop still closes an open popup, by the compositor's popup grab
  (verified on Sway 1.12 by every scenario's `click 600 600`;
  Hyprland's grab does it too).
- `notify` 8: `recommended_watcher(handler)` runs its own thread; watch
  the parent directory (editors save by rename) and filter on paths.
  `futures::mpsc::UnboundedReceiver::try_next` is deprecated for
  `try_recv`.
- A layer surface with `Anchor::empty()` and `LayerSize::px(w, h)` is
  centred on its output by Hyprland; `exclusive_zone: Some(-1)` on a
  full-size surface covers the bars too.
- Hyprland routes **all pointer input** (every output) to a layer
  surface with `KeyboardInteractivity::Exclusive` while it's mapped:
  the click-catching surfaces never saw a click (`listen_with` showed
  every press on the launcher's window). `OnDemand` gets keyboard
  focus on map just the same and leaves the pointer alone.
- `iced::keyboard::listen()` only yields *ignored* key events, and a
  focused `text_input` captures Escape (and drops its focus); use
  `iced::event::listen_with` (gets every event with its status and
  `window::Id`; takes a plain `fn`, so filter on the window afterwards)
  for keys the launcher wants regardless. Up/Down aren't captured by
  `text_input`.
- The focus/scroll tasks live at `iced::widget::operation::{focus,
  snap_to, scroll_to}`; they run on every window, so the widget ids
  must be `Id::unique()`. The launcher first used `snap_to(id,
  RelativeOffset { y: i / (n-1) })` to keep item `i` of `n` in view:
  it needs no row height but scrolls on every arrow, from the very
  first. Now it keeps the row in view the way a list should: on
  Up/Down it asks the widget tree (`widgets::bounds`, the popups'
  anchor operation) for the selected row and the list container
  (`Theme::tag`), both in layout coordinates (unscrolled), so `row.y -
  list.y` is the row's offset in the content; with the current offset
  from `scrollable::on_scroll` it scrolls (`scroll_to`, absolute) only
  when the row is above or below the viewport, by the least amount;
  the row located is the one *past* the selection in the direction of
  travel, so the next candidate is already visible before it's picked.
  `Task<Option<T>>::and_then` short-circuits on `None`.
- `tokio::spawn` inside a `stream::channel` subscription works (iced's
  tokio executor runs it on the runtime) but needs the `rt` feature.
- Hyprland `j/monitors[].focused` / `focusedmonv2>>NAME,WSID` give the
  focused monitor by connector name.
- `hyprctl dispatch 'hl.dsp.focus({ monitor = "HDMI-A-2" })'` moves
  focus to a monitor, handy to test per-output behaviour.
- Screen capture: Hyprland 0.56 and Sway 1.12 (headless too) serve
  `ext-image-copy-capture-v1`, `ext-output-image-capture-source-v1`
  and `ext-data-control-v1` (Hyprland also `ext-foreign-toplevel-
  image-capture-source`, `zwlr_screencopy`, its own toplevel export).
  A session sends `buffer_size`, the `shm_format`s and `done`, then a
  frame with our buffer attached and fully damaged gets `transform`
  and `ready`. The buffer is in the output's own orientation: Sway's
  `transform 90` comes back with `transform` 90 and `rotate90`
  (clockwise) makes it upright (the panel on top: `screenshot.sh`).
  wl-clipboard 2.3 reads an `ext-data-control` selection in the
  nested Sway, where nothing has the keyboard.
- `ShellEvent::OutputUpdated` (a rotated, moved, rescaled output) was
  ignored, and the daemon's outputs kept their first geometry: it now
  replaces the stored `OutputInfo` (sctk's has no `PartialEq`).
- Window geometry for screenshots: Sway's `get_tree` views carry
  `rect` (global, borders included), `window_rect` (the contents,
  relative to `rect`) and `visible`; the last of `floating_nodes` is on
  top. Hyprland's `j/clients` carry `at` / `size` (global, the
  contents), `hidden`, `floating`, `focusHistoryID` (0: the most
  recent); a monitor's open special workspace is
  `j/monitors[].specialWorkspace.id` (0 when none).
- iced reports a pointer entering a surface as `CursorEntered`, with
  no position: a surface mapped under a still pointer doesn't know
  where it is until it moves. A `canvas::Program::update` gets the
  cursor's position with every event, so the picker takes its presses
  there. `stack` hands events to its layers top first and stops at
  the first that captures (a toolbar button keeps its click from the
  canvas under it).
- Keyboard on the picker's surfaces: with one `OnDemand` and the
  others `None`, a click on another output in Sway focuses that
  output's workspace and the picker loses Escape/Enter; with all
  `OnDemand` the clicked one takes the keyboard. On Hyprland Escape
  works right after opening, no click needed (tried live).

## Internal limits

What users don't see in the README's checklist but a change may run
into: screenshots from the gadget's menu wait a fixed 150 ms for the
menu to leave the screen (no frame callback to wait on); the picker
holds every output twice (the RGBA shots, the image handles' copies);
`:hover` only on buttons (iced containers have no hover state; needs a
`mouse_area` wrapper: the system monitor's table rows, the
notification popup rows), iced's default scrollbar (not themed), the
panel height from the theme's `min-height` rather than from the
content, `columns` of the exit menu from the config rather than the
theme, the PAM conversation refusing visible prompts (no second prompt
such as a one-time code), the GPU readers (amdgpu, nvidia) never run
on a machine that has one (this one is Intel).

## Verified where

Every interactive piece has a scenario in `tests/ui/scenarios/`, run in
the nested Sway. Also tried on the real Hyprland session: the panels,
workspaces, clock, theming and config hot reload, window icons, the
launcher, the tray (nm-applet, MEGAsync), Custom, Themes, the system
monitor, audio, the lock screen (PAM accepting the right password; the
monitor hot-plugged while locked), idle (`loginctl lock-session`,
the lock before a suspend, the battery timeouts), screenshots (the
commands, the clipboard, the editor, the picker, the gadget; not yet
with a HiDPI screen, only the unit tests mix scales).
Only in the nested Sway so far: the notifications and their gadget,
the exit menu, the wallpaper, the OSD (its volume side only by the
unit tests: the nested Sway's mixer is the desktop's own), the Power gadget (with `tests/ui/upower`,
a fake UPower and power-profiles-daemon), the Brightness gadget (with `tests/ui/bin/ddcutil`; the
backlight side by its unit tests, and by hand on a laptop). Never exercised by a scenario: the
logind and UPower side of idle (the nested Sway's bus has neither).
