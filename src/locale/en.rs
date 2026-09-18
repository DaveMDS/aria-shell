//! English: the reference catalogue, the fallback under every other.
//! Keys are stable ids grouped by component; change a text here
//! without touching the other languages.

use super::Catalogue;

pub const CATALOGUE: Catalogue = &[
    // launcher
    ("launcher.search", "Search applications…"),
    // locker
    ("locker.enter_password", "Enter password"),
    ("locker.auth_failed", "Authentication failed"),
    ("locker.unlocking", "Unlocking…"),
    ("locker.unlock", "Unlock"),
    ("locker.show_password", "show"),
    ("locker.hide_password", "hide"),
    // notifications
    ("notifications.title", "Notifications"),
    ("notifications.dnd", "Do not disturb"),
    ("notifications.clear", "Clear"),
    ("notifications.empty", "No notifications"),
    ("notifications.age.now", "now"),
    ("notifications.age.minutes", "{n} min"),
    ("notifications.age.hours", "{n} h"),
    ("notifications.age.yesterday", "yesterday"),
    // audio
    ("audio.output", "Output"),
    ("audio.input", "Input"),
    ("audio.playing", "Playing"),
    ("audio.mixer", "Mixer"),
    ("audio.empty", "No audio devices"),
    // system monitor
    ("sysmon.tab.cpu", "CPU"),
    ("sysmon.tab.memory", "Memory"),
    ("sysmon.tab.disks", "Disks"),
    ("sysmon.tab.network", "Network"),
    ("sysmon.tab.gpu", "GPU"),
    ("sysmon.tab.processes", "Processes"),
    ("sysmon.column.name", "Name"),
    ("sysmon.column.pid", "PID"),
    ("sysmon.column.user", "User"),
    ("sysmon.column.cpu", "CPU%"),
    ("sysmon.column.memory", "Memory"),
    ("sysmon.terminate", "Terminate"),
    ("sysmon.kill", "Kill"),
    ("sysmon.select_help", "Select a process to signal it"),
    (
        "sysmon.cpu.load",
        "Load: {load1}  {load5}  {load15}   Up: {uptime}",
    ),
    ("sysmon.cpu.frequency", "Frequency: {freq}"),
    ("sysmon.cpu.temperature", "Temperature: {temp}°C"),
    (
        "sysmon.mem.used",
        "Used {used} of {total}  ·  cached {cached}  ·  available {available}",
    ),
    ("sysmon.mem.swap", "Swap {used} of {total}"),
    ("sysmon.gpu.vram", "VRAM {used} / {total}"),
    // themes
    ("themes.light", "Light"),
    ("themes.dark", "Dark"),
    ("themes.base", "Base"),
];
