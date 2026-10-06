//! Numbers as the bar and the popup show them, and the `format` of a
//! gadget instance expanded with a sample's values.

use std::time::Duration;

use super::{Sample, Value};
use crate::locale::Locale;

/// Bytes with a binary unit: `512 B`, `1.5 KiB`, `12.3 GiB` (the
/// decimal point the locale's).
pub fn bytes(locale: &Locale, n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else {
        format!("{} {}", locale.decimal(v, 1), UNITS[i])
    }
}

/// A throughput with a decimal unit: `0 B/s`, `12.3 MB/s`.
pub fn rate(locale: &Locale, bps: f32) -> String {
    const UNITS: [&str; 5] = ["B/s", "kB/s", "MB/s", "GB/s", "TB/s"];
    let mut v = f64::from(bps.max(0.0));
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 || v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else {
        format!("{} {}", locale.decimal(v, 1), UNITS[i])
    }
}

/// `3d 4h`, `4h 12m`, `12m`, `45s`.
pub fn duration(d: Duration) -> String {
    let s = d.as_secs();
    let (days, hours, mins) = (s / 86400, (s / 3600) % 24, (s / 60) % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else if mins > 0 {
        format!("{mins}m")
    } else {
        format!("{s}s")
    }
}

/// `3.8 GHz` / `800 MHz`.
pub fn freq(locale: &Locale, mhz: f32) -> String {
    if mhz >= 1000.0 {
        format!("{} GHz", locale.decimal(f64::from(mhz) / 1000.0, 1))
    } else {
        format!("{mhz:.0} MHz")
    }
}

/// `format` with its `{placeholder}`s replaced by the sample's values
/// (see [`placeholder`]); `{name:width}` right-aligns a number in
/// `width` characters; unknown names stay as written. Without a
/// sample yet, the numbers are `-`.
pub fn expand(locale: &Locale, format: &str, sample: Option<&Sample>) -> String {
    let mut out = String::with_capacity(format.len());
    let mut rest = format;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('}') else {
            out.push_str(tail);
            return out;
        };
        let spec = &tail[1..end];
        let (name, width) = match spec.split_once(':') {
            Some((n, w)) => (n, w.parse::<usize>().ok()),
            None => (spec, None),
        };
        match placeholder(locale, name, sample) {
            Some(value) => match width {
                Some(w) => out.push_str(&format!("{value:>w$}")),
                None => out.push_str(&value),
            },
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The value of one placeholder name; `None` for an unknown one.
pub fn placeholder(locale: &Locale, name: &str, sample: Option<&Sample>) -> Option<String> {
    let pct = |v: Option<f32>| v.map_or("-".to_owned(), |v| format!("{v:.0}"));
    let Some(s) = sample else {
        return Value::parse(name)
            .map(|_| "-".to_owned())
            .or_else(|| KNOWN.contains(&name).then(|| "-".to_owned()));
    };
    let mem_pct = |used: u64, total: u64| (total > 0).then(|| used as f32 * 100.0 / total as f32);
    Some(match name {
        "cpu" => pct(Some(s.cpu.total)),
        "mem" => pct(mem_pct(s.mem.used, s.mem.total)),
        "mem_used" => bytes(locale, s.mem.used),
        "mem_total" => bytes(locale, s.mem.total),
        "mem_available" => bytes(locale, s.mem.available),
        "swap" => pct(mem_pct(s.mem.swap_used, s.mem.swap_total)),
        "swap_used" => bytes(locale, s.mem.swap_used),
        "rx" => rate(locale, s.net.iter().map(|i| i.rx_bps).sum()),
        "tx" => rate(locale, s.net.iter().map(|i| i.tx_bps).sum()),
        "read" => rate(locale, s.disks.iter().map(|d| d.read_bps).sum()),
        "write" => rate(locale, s.disks.iter().map(|d| d.write_bps).sum()),
        "disk" => pct(s.disks.first().and_then(|d| mem_pct(d.used, d.total))),
        "temp" => pct(s.cpu.temp_c),
        "freq" => s.cpu.freq_mhz.map_or("-".to_owned(), |f| freq(locale, f)),
        "load1" => locale.decimal(s.load.0.into(), 2),
        "load5" => locale.decimal(s.load.1.into(), 2),
        "load15" => locale.decimal(s.load.2.into(), 2),
        "uptime" => duration(s.uptime),
        "gpu" => pct(s.gpus.first().and_then(|g| g.busy)),
        "gpu_temp" => pct(s.gpus.first().and_then(|g| g.temp_c)),
        "vram" => pct(s
            .gpus
            .first()
            .and_then(|g| mem_pct(g.vram_used?, g.vram_total?))),
        _ => return None,
    })
}

/// Every placeholder name, for the no-sample-yet case.
const KNOWN: &[&str] = &[
    "cpu",
    "mem",
    "mem_used",
    "mem_total",
    "mem_available",
    "swap",
    "swap_used",
    "rx",
    "tx",
    "read",
    "write",
    "disk",
    "temp",
    "freq",
    "load1",
    "load5",
    "load15",
    "uptime",
    "gpu",
    "gpu_temp",
    "vram",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sysmon::{Cpu, Iface, Mem};

    fn sample() -> Sample {
        Sample {
            cpu: Cpu {
                total: 42.4,
                cores: vec![10.0, 74.8],
                freq_mhz: Some(3800.0),
                temp_c: Some(55.6),
            },
            load: (0.5, 1.0, 1.5),
            uptime: Duration::from_secs(90061),
            mem: Mem {
                total: 16 * 1024 * 1024 * 1024,
                used: 4 * 1024 * 1024 * 1024,
                available: 12 * 1024 * 1024 * 1024,
                cached: 1024,
                swap_total: 0,
                swap_used: 0,
            },
            disks: Vec::new(),
            net: vec![Iface {
                name: "eth0".into(),
                rx_bps: 1_234_567.0,
                tx_bps: 12.0,
                rx_total: 0,
                tx_total: 0,
            }],
            gpus: Vec::new(),
        }
    }

    #[test]
    fn units() {
        let en = Locale::new("en");
        assert_eq!(bytes(&en, 512), "512 B");
        assert_eq!(bytes(&en, 1536), "1.5 KiB");
        assert_eq!(bytes(&en, 200 * 1024 * 1024), "200 MiB");
        assert_eq!(rate(&en, 0.0), "0 B/s");
        assert_eq!(rate(&en, 1_234_567.0), "1.2 MB/s");
        assert_eq!(duration(Duration::from_secs(90061)), "1d 1h");
        assert_eq!(duration(Duration::from_secs(4000)), "1h 6m");
        assert_eq!(duration(Duration::from_secs(45)), "45s");
        assert_eq!(freq(&en, 3800.0), "3.8 GHz");
        assert_eq!(freq(&en, 800.0), "800 MHz");
        let it = Locale::new("it");
        assert_eq!(bytes(&it, 1536), "1,5 KiB");
        assert_eq!(rate(&it, 1_234_567.0), "1,2 MB/s");
        assert_eq!(freq(&it, 3800.0), "3,8 GHz");
    }

    #[test]
    fn expansion() {
        let en = Locale::new("en");
        let s = sample();
        assert_eq!(expand(&en, "{cpu}%", Some(&s)), "42%");
        assert_eq!(expand(&en, "[{cpu:3}%]", Some(&s)), "[ 42%]");
        assert_eq!(expand(&en, "{mem}% {mem_used}", Some(&s)), "25% 4.0 GiB");
        assert_eq!(expand(&en, "{rx} {tx}", Some(&s)), "1.2 MB/s 12 B/s");
        assert_eq!(
            expand(&en, "{temp}° {freq} {load1}", Some(&s)),
            "56° 3.8 GHz 0.50"
        );
        assert_eq!(expand(&en, "{swap}% {gpu}%", Some(&s)), "-% -%");
        assert_eq!(expand(&en, "{nope} {cpu", Some(&s)), "{nope} {cpu");
        assert_eq!(expand(&en, "{cpu}% up {uptime}", None), "-% up -");
        assert_eq!(
            expand(&Locale::new("it"), "{load1} {mem_used}", Some(&s)),
            "0,50 4,0 GiB"
        );
    }
}
