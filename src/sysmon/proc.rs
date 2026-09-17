//! The `/proc` readers: each parses the text of one file (unit-tested
//! on captured text) and, for the counters, turns two readings into
//! rates.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use super::{Disk, Iface, Mem, RawProcess};

/// The jiffies of one cpu line of `/proc/stat`: total and idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuTimes {
    pub total: u64,
    pub idle: u64,
}

/// The raw counters a sample compares to the previous one.
#[derive(Debug, Clone, Default)]
pub struct Counters {
    /// The `cpu` line first, then one per core.
    pub cpu: Vec<CpuTimes>,
    /// Sectors read / written, by device name.
    pub disks: HashMap<String, (u64, u64)>,
    /// Bytes received / sent, by interface.
    pub net: HashMap<String, (u64, u64)>,
}

/// The cpu lines of `/proc/stat`: user nice system idle iowait irq
/// softirq steal ... ; idle is idle + iowait.
pub fn cpu_times(stat: &str) -> Vec<CpuTimes> {
    stat.lines()
        .filter(|l| l.starts_with("cpu"))
        .map(|l| {
            let v: Vec<u64> = l
                .split_whitespace()
                .skip(1)
                .filter_map(|f| f.parse().ok())
                .collect();
            CpuTimes {
                total: v.iter().sum(),
                idle: v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0),
            }
        })
        .collect()
}

/// Busy percentages between two readings, one per line of
/// [`cpu_times`] (the total first); `None` for a cpu that didn't tick.
pub fn cpu_usage(prev: &[CpuTimes], now: &[CpuTimes]) -> Vec<Option<f32>> {
    now.iter()
        .enumerate()
        .map(|(i, n)| {
            let p = prev.get(i)?;
            let total = n.total.checked_sub(p.total)?;
            if total == 0 {
                return None;
            }
            let idle = n.idle.saturating_sub(p.idle).min(total);
            Some((total - idle) as f32 * 100.0 / total as f32)
        })
        .collect()
}

/// `/proc/meminfo`, in bytes (the file is in kB).
pub fn meminfo(text: &str) -> Mem {
    let kb = |key: &str| -> u64 {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
            .unwrap_or(0)
            * 1024
    };
    let total = kb("MemTotal");
    let available = kb("MemAvailable");
    let cached = kb("Cached") + kb("SReclaimable") + kb("Buffers");
    let swap_total = kb("SwapTotal");
    let swap_free = kb("SwapFree");
    Mem {
        total,
        used: total.saturating_sub(available),
        available,
        cached,
        swap_total,
        swap_used: swap_total.saturating_sub(swap_free),
    }
}

/// `/proc/loadavg`: the three averages.
pub fn loadavg(text: &str) -> (f32, f32, f32) {
    let mut f = text
        .split_whitespace()
        .map(|s| s.parse::<f32>().unwrap_or(0.0));
    (
        f.next().unwrap_or(0.0),
        f.next().unwrap_or(0.0),
        f.next().unwrap_or(0.0),
    )
}

/// `/proc/uptime`: the first number, seconds.
pub fn uptime(text: &str) -> Duration {
    let secs = text
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    Duration::from_secs_f64(secs.max(0.0))
}

/// `/proc/diskstats`: sectors read (field 6) and written (field 10) by
/// device name (field 3), partitions included.
pub fn diskstats(text: &str) -> HashMap<String, (u64, u64)> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let name = f.get(2)?;
            let read = f.get(5)?.parse().ok()?;
            let written = f.get(9)?.parse().ok()?;
            Some(((*name).to_owned(), (read, written)))
        })
        .collect()
}

/// `/proc/net/dev`: bytes received (first field) and sent (ninth) by
/// interface.
pub fn netdev(text: &str) -> HashMap<String, (u64, u64)> {
    text.lines()
        .skip(2)
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let f: Vec<&str> = rest.split_whitespace().collect();
            let rx = f.first()?.parse().ok()?;
            let tx = f.get(8)?.parse().ok()?;
            Some((name.trim().to_owned(), (rx, tx)))
        })
        .collect()
}

/// A mount of `/proc/mounts` worth showing: device, mount point and
/// filesystem type, for the filesystems that live on a disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub device: String,
    pub point: String,
    pub fs: String,
}

/// Pseudo and memory-backed filesystems, not shown unless asked for.
const VIRTUAL_FS: &[&str] = &[
    "proc",
    "sysfs",
    "tmpfs",
    "devtmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "overlay",
    "squashfs",
    "autofs",
    "mqueue",
    "hugetlbfs",
    "debugfs",
    "tracefs",
    "securityfs",
    "pstore",
    "efivarfs",
    "bpf",
    "configfs",
    "binfmt_misc",
    "fusectl",
    "ramfs",
    "rpc_pipefs",
    "nsfs",
];

/// The mounts of `/proc/mounts`: those on `wanted` mount points when
/// given, else every real filesystem (one per mount point, the last
/// mount wins), `/` first then by mount point.
pub fn mounts(text: &str, wanted: &[String]) -> Vec<Mount> {
    let mut by_point: HashMap<String, Mount> = HashMap::new();
    for l in text.lines() {
        let mut f = l.split_whitespace();
        let (Some(device), Some(point), Some(fs)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        // Octal escapes in mount points (`\040` for a space).
        let point = unescape(point);
        let keep = if wanted.is_empty() {
            !VIRTUAL_FS.contains(&fs) && !fs.starts_with("fuse") && device.starts_with('/')
        } else {
            wanted.contains(&point)
        };
        if keep {
            by_point.insert(
                point.clone(),
                Mount {
                    device: device.to_owned(),
                    point,
                    fs: fs.to_owned(),
                },
            );
        }
    }
    let mut list: Vec<Mount> = by_point.into_values().collect();
    list.sort_by(|a, b| {
        (a.point != "/")
            .cmp(&(b.point != "/"))
            .then(a.point.cmp(&b.point))
    });
    if wanted.is_empty() {
        // One entry per device: btrfs subvolumes and bind mounts share
        // their filesystem, the first mount point (`/` first) stands
        // for it.
        let mut seen = std::collections::HashSet::new();
        list.retain(|m| seen.insert(m.device.clone()));
    }
    list
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let digits: String = chars.clone().take(3).collect();
            if digits.len() == 3
                && let Ok(code) = u8::from_str_radix(&digits, 8)
            {
                out.push(code as char);
                chars.nth(2);
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Total and used bytes of the filesystem at `point` (`statvfs`).
pub fn usage(point: &str) -> Option<(u64, u64)> {
    let c_point = std::ffi::CString::new(point).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a valid C string and a zeroed struct the call fills.
    if unsafe { libc::statvfs(c_point.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let frsize = st.f_frsize as u64;
    let total = st.f_blocks as u64 * frsize;
    let free = st.f_bfree as u64 * frsize;
    Some((total, total.saturating_sub(free)))
}

/// The disks of a sample: the mounts with their usage and the
/// throughput of their device since `prev` (a partition's counters are
/// its own; `/dev/mapper/x` and friends resolve to the kernel name
/// through `/sys/class/block` symlinks, else no rates).
pub fn disks(
    mounts: &[Mount],
    prev: &HashMap<String, (u64, u64)>,
    now: &HashMap<String, (u64, u64)>,
    elapsed: f32,
) -> Vec<Disk> {
    mounts
        .iter()
        .filter_map(|m| {
            let (total, used) = usage(&m.point)?;
            let dev = device_name(&m.device);
            let (read_bps, write_bps) = match (prev.get(&dev), now.get(&dev)) {
                (Some(p), Some(n)) if elapsed > 0.0 => (
                    n.0.saturating_sub(p.0) as f32 * 512.0 / elapsed,
                    n.1.saturating_sub(p.1) as f32 * 512.0 / elapsed,
                ),
                _ => (0.0, 0.0),
            };
            Some(Disk {
                name: dev,
                mount: m.point.clone(),
                fs: m.fs.clone(),
                total,
                used,
                read_bps,
                write_bps,
            })
        })
        .collect()
}

/// `/dev/sda1` -> `sda1`; `/dev/mapper/vg-root` -> `dm-0` (the link
/// target's name); `/dev/disk/by-uuid/..` likewise.
fn device_name(device: &str) -> String {
    let path = Path::new(device);
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    resolved
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(device)
        .to_owned()
}

/// Interfaces skipped by default: the loopback and the container
/// plumbing.
fn virtual_iface(name: &str) -> bool {
    name == "lo"
        || ["veth", "br-", "docker", "virbr", "vnet"]
            .iter()
            .any(|p| name.starts_with(p))
}

/// The interfaces of a sample: every one in `now` (but the loopback
/// and the virtual ones, unless asked for), with their rates since
/// `prev`.
pub fn ifaces(
    wanted: &[String],
    prev: &HashMap<String, (u64, u64)>,
    now: &HashMap<String, (u64, u64)>,
    elapsed: f32,
) -> Vec<Iface> {
    let mut list: Vec<Iface> = now
        .iter()
        .filter(|(name, _)| {
            if wanted.is_empty() {
                !virtual_iface(name)
            } else {
                wanted.contains(name)
            }
        })
        .map(|(name, (rx, tx))| {
            let (rx_bps, tx_bps) = match prev.get(name) {
                Some((prx, ptx)) if elapsed > 0.0 => (
                    rx.saturating_sub(*prx) as f32 / elapsed,
                    tx.saturating_sub(*ptx) as f32 / elapsed,
                ),
                _ => (0.0, 0.0),
            };
            Iface {
                name: name.clone(),
                rx_bps,
                tx_bps,
                rx_total: *rx,
                tx_total: *tx,
            }
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

// --- processes ------------------------------------------------------------

/// One `/proc/[pid]/stat` line: the name is in parentheses and may
/// contain spaces and parentheses itself, so it's cut at the last `)`.
/// Fields after it: state (1), ..., utime (12), stime (13), ..., rss
/// (22, pages).
pub fn stat_line(line: &str) -> Option<(String, char, u64, u64)> {
    let start = line.find('(')?;
    let end = line.rfind(')')?;
    let name = line[start + 1..end].to_owned();
    let f: Vec<&str> = line[end + 1..].split_whitespace().collect();
    let state = f.first()?.chars().next()?;
    let utime: u64 = f.get(11)?.parse().ok()?;
    let stime: u64 = f.get(12)?.parse().ok()?;
    let rss_pages: u64 = f.get(21)?.parse().ok()?;
    Some((name, state, utime + stime, rss_pages))
}

/// Every process under `proc` (`/proc`): what the table needs, raw
/// (cpu as jiffies so far, to be compared to the previous reading).
pub fn processes(proc: &Path) -> Vec<RawProcess> {
    let page = page_size();
    let Ok(dir) = fs::read_dir(proc) else {
        return Vec::new();
    };
    dir.filter_map(|e| {
        let e = e.ok()?;
        let pid: u32 = e.file_name().to_str()?.parse().ok()?;
        let stat = fs::read_to_string(e.path().join("stat")).ok()?;
        let (name, state, ticks, rss_pages) = stat_line(&stat)?;
        let uid = fs::read_to_string(e.path().join("status"))
            .ok()
            .and_then(|s| {
                s.lines()
                    .find_map(|l| l.strip_prefix("Uid:"))?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            })
            .unwrap_or(0);
        Some(RawProcess {
            pid,
            name,
            state,
            ticks,
            rss: rss_pages * page,
            uid,
        })
    })
    .collect()
}

pub fn page_size() -> u64 {
    // SAFETY: no arguments, no side effects.
    let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if n > 0 { n as u64 } else { 4096 }
}

/// Jiffies per second, for the process cpu percentages.
pub fn clock_ticks() -> u64 {
    // SAFETY: as above.
    let n = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if n > 0 { n as u64 } else { 100 }
}

/// `/etc/passwd`: uid -> user name.
pub fn users(text: &str) -> HashMap<u32, String> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?;
            let uid = f.nth(1)?.parse().ok()?;
            Some((uid, name.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT_A: &str = "cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0 0 0\ncpu1 50 0 50 400 0 0 0 0 0 0\nintr 1 2 3\n";
    const STAT_B: &str = "cpu  200 0 200 1000 0 0 0 0 0 0\ncpu0 150 0 150 400 0 0 0 0 0 0\ncpu1 50 0 50 600 0 0 0 0 0 0\n";

    #[test]
    fn cpu_percentages_between_readings() {
        let a = cpu_times(STAT_A);
        let b = cpu_times(STAT_B);
        assert_eq!(a.len(), 3);
        assert_eq!(
            a[0],
            CpuTimes {
                total: 1000,
                idle: 800
            }
        );
        let usage = cpu_usage(&a, &b);
        // total: 400 more, 200 idle -> 50%; cpu0: 200 more, all busy; cpu1: idle only.
        assert_eq!(usage[0].map(|u| u.round()), Some(50.0));
        assert_eq!(usage[1].map(|u| u.round()), Some(100.0));
        assert_eq!(usage[2].map(|u| u.round()), Some(0.0));
        // No tick: unknown.
        assert_eq!(cpu_usage(&a, &a), vec![None, None, None]);
    }

    #[test]
    fn meminfo_in_bytes() {
        let m = meminfo(
            "MemTotal:       16000 kB\nMemFree:         1000 kB\nMemAvailable:    6000 kB\nBuffers:          500 kB\nCached:          3000 kB\nSwapTotal:       8000 kB\nSwapFree:        7000 kB\nSReclaimable:     500 kB\n",
        );
        assert_eq!(m.total, 16000 * 1024);
        assert_eq!(m.used, 10000 * 1024);
        assert_eq!(m.cached, 4000 * 1024);
        assert_eq!(m.swap_used, 1000 * 1024);
    }

    #[test]
    fn load_and_uptime() {
        assert_eq!(loadavg("0.52 0.58 0.59 1/1234 5678\n"), (0.52, 0.58, 0.59));
        assert_eq!(
            uptime("12345.67 98765.43\n"),
            Duration::from_secs_f64(12345.67)
        );
    }

    #[test]
    fn disk_and_net_counters() {
        let d = diskstats(
            " 259       0 nvme0n1 1000 0 80000 0 2000 0 160000 0 0 0 0 0 0 0 0 0 0\n 259       1 nvme0n1p1 10 0 800 0 20 0 1600 0 0 0 0 0 0 0 0 0 0\n",
        );
        assert_eq!(d["nvme0n1"], (80000, 160000));
        assert_eq!(d["nvme0n1p1"], (800, 1600));
        let n = netdev(
            "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    lo: 1000 10 0 0 0 0 0 0 1000 10 0 0 0 0 0 0\n  eth0: 5000 50 0 0 0 0 0 0 3000 30 0 0 0 0 0 0\n",
        );
        assert_eq!(n["eth0"], (5000, 3000));
        let mut later = n.clone();
        later.insert("eth0".into(), (7000, 3500));
        let list = ifaces(&[], &n, &later, 2.0);
        assert_eq!(list.len(), 1, "lo skipped");
        assert!(virtual_iface("veth1a2b") && virtual_iface("docker0") && !virtual_iface("wlan0"));
        assert_eq!(list[0].name, "eth0");
        assert_eq!(list[0].rx_bps, 1000.0);
        assert_eq!(list[0].tx_bps, 250.0);
        let only_lo = ifaces(&["lo".to_owned()], &n, &later, 2.0);
        assert_eq!(only_lo[0].name, "lo");
    }

    #[test]
    fn real_mounts_only() {
        let text = "proc /proc proc rw 0 0\n/dev/nvme0n1p2 / ext4 rw 0 0\ntmpfs /tmp tmpfs rw 0 0\n/dev/sda1 /mnt/big\\040disk ext4 rw 0 0\nnfs:/x /net nfs rw 0 0\n/dev/nvme0n1p2 / ext4 rw,remount 0 0\n/dev/nvme0n1p2 /home btrfs subvol=@home 0 0\n";
        let m = mounts(text, &[]);
        assert_eq!(
            m.iter().map(|m| m.point.as_str()).collect::<Vec<_>>(),
            ["/", "/mnt/big disk"],
            "one per device, / first"
        );
        let m = mounts(text, &["/home".to_owned(), "/".to_owned()]);
        assert_eq!(
            m.iter().map(|m| m.point.as_str()).collect::<Vec<_>>(),
            ["/", "/home"]
        );
        let m = mounts(text, &["/tmp".to_owned()]);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].fs, "tmpfs");
    }

    #[test]
    fn stat_line_with_odd_name() {
        let line = "1234 (Web Content (x)) S 1 1234 1234 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 30 0 1000 200000 1500 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0 0 0 0 0 0 0 0 0";
        let (name, state, ticks, rss) = stat_line(line).unwrap();
        assert_eq!(name, "Web Content (x)");
        assert_eq!(state, 'S');
        assert_eq!(ticks, 300);
        assert_eq!(rss, 1500);
    }

    #[test]
    fn passwd_users() {
        let u = users("root:x:0:0:root:/root:/bin/bash\ndave:x:1000:1000::/home/dave:/bin/fish\n");
        assert_eq!(u[&1000], "dave");
        assert_eq!(u[&0], "root");
    }

    #[test]
    fn this_machine() {
        // Smoke on the live /proc: the parsers accept the real files.
        let m = meminfo(&fs::read_to_string("/proc/meminfo").unwrap());
        assert!(m.total > 0);
        let c = cpu_times(&fs::read_to_string("/proc/stat").unwrap());
        assert!(c.len() >= 2);
        assert!(usage("/").is_some());
        assert!(!processes(Path::new("/proc")).is_empty());
    }
}
