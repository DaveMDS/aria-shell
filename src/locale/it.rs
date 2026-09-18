//! Italiano. Stesse chiavi di `en.rs`, nello stesso ordine.

use super::Catalogue;

pub const CATALOGUE: Catalogue = &[
    // launcher
    ("launcher.search", "Cerca applicazioni…"),
    // locker
    ("locker.enter_password", "Inserisci la password"),
    ("locker.auth_failed", "Autenticazione fallita"),
    ("locker.unlocking", "Sblocco…"),
    ("locker.unlock", "Sblocca"),
    ("locker.show_password", "mostra"),
    ("locker.hide_password", "nascondi"),
    // notifications
    ("notifications.title", "Notifiche"),
    ("notifications.dnd", "Non disturbare"),
    ("notifications.clear", "Cancella"),
    ("notifications.empty", "Nessuna notifica"),
    ("notifications.age.now", "adesso"),
    ("notifications.age.minutes", "{n} min"),
    ("notifications.age.hours", "{n} h"),
    ("notifications.age.yesterday", "ieri"),
    // audio
    ("audio.output", "Uscita"),
    ("audio.input", "Ingresso"),
    ("audio.playing", "In riproduzione"),
    ("audio.mixer", "Mixer"),
    ("audio.empty", "Nessun dispositivo audio"),
    // system monitor
    ("sysmon.tab.cpu", "CPU"),
    ("sysmon.tab.memory", "Memoria"),
    ("sysmon.tab.disks", "Dischi"),
    ("sysmon.tab.network", "Rete"),
    ("sysmon.tab.gpu", "GPU"),
    ("sysmon.tab.processes", "Processi"),
    ("sysmon.column.name", "Nome"),
    ("sysmon.column.pid", "PID"),
    ("sysmon.column.user", "Utente"),
    ("sysmon.column.cpu", "CPU%"),
    ("sysmon.column.memory", "Memoria"),
    ("sysmon.terminate", "Termina"),
    ("sysmon.kill", "Kill"),
    (
        "sysmon.select_help",
        "Seleziona un processo per inviargli un segnale",
    ),
    (
        "sysmon.cpu.load",
        "Carico: {load1}  {load5}  {load15}   Uptime: {uptime}",
    ),
    ("sysmon.cpu.frequency", "Frequenza: {freq}"),
    ("sysmon.cpu.temperature", "Temperatura: {temp}°C"),
    (
        "sysmon.mem.used",
        "Usata {used} di {total}  ·  cache {cached}  ·  disponibile {available}",
    ),
    ("sysmon.mem.swap", "Swap {used} di {total}"),
    ("sysmon.gpu.vram", "VRAM {used} / {total}"),
    // exiter
    ("exiter.lock", "Blocca"),
    ("exiter.suspend", "Sospendi"),
    ("exiter.hibernate", "Iberna"),
    ("exiter.logout", "Esci"),
    ("exiter.reboot", "Riavvia"),
    ("exiter.shutdown", "Spegni"),
    ("exiter.confirm", "{action}?"),
    ("exiter.countdown", "Automaticamente tra {n} s"),
    ("exiter.cancel", "Annulla"),
    // themes
    ("themes.light", "Chiaro"),
    ("themes.dark", "Scuro"),
    ("themes.base", "Base"),
];
