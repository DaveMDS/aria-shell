//! The system monitor's data: cpu, memory, disks, network, GPUs read
//! from `/proc` and `/sys` every few seconds, their recent history, and
//! the process table while a popup shows it.
//!
//! One [`SysMon`] lives in the daemon, shaped like `Tray`: the
//! [`SysMon::subscription`] is the sampler (one for the whole process,
//! off the runtime thread), the [`Event`]s it yields go through
//! [`SysMon::apply`], gadgets read the [`Sample`] and the [`History`]
//! from their view context and act with a [`Command`] the daemon runs
//! with [`SysMon::run`] (reading the processes, signalling one).

pub mod format;
pub mod proc;
pub mod sensors;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream};
use iced::stream as iced_stream;
use iced::{Subscription, Task};

use crate::config::{RawSection, Section};

/// The `[SystemMonitor]` section: the sampler's keys and the popup's
/// (the base section only; the gadgets are its instances, one value
/// each, see `gadgets/system_monitor.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MonitorConfig {
    /// Seconds between samples.
    pub interval: u64,
    /// Samples kept per series.
    pub history: usize,
    /// Mount points to show; empty: every real filesystem.
    pub disks: Vec<String>,
    /// Interfaces to show; empty: all but `lo`.
    pub interfaces: Vec<String>,
    /// The hwmon `name` for the cpu temperature; empty: the usual
    /// cpu drivers.
    pub temperature: String,
    /// Rows of the popup's process table.
    pub processes: usize,
    /// Its initial sort.
    pub sort: Column,
}

impl Section for MonitorConfig {
    const NAME: &'static str = "SystemMonitor";

    fn from_raw(raw: &RawSection) -> Self {
        let sort = match raw.get("sort") {
            None => Column::Cpu,
            Some(s) => Column::parse(s).unwrap_or_else(|| {
                log::warn!("unknown sort {s:?} for the process table, using cpu");
                Column::Cpu
            }),
        };
        Self {
            interval: raw.u64_or("interval", 2).max(1),
            history: (raw.u64_or("history", 60) as usize).max(2),
            disks: raw.list_or("disks", &[]),
            interfaces: raw.list_or("interfaces", &[]),
            temperature: raw.str_or("temperature", ""),
            processes: raw.u64_or("processes", 10) as usize,
            sort,
        }
    }
}

/// A column of the process table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Column {
    Name,
    Pid,
    User,
    Cpu,
    Mem,
}

impl Column {
    pub const ALL: [Column; 5] = [Self::Name, Self::Pid, Self::User, Self::Cpu, Self::Mem];

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "name" => Self::Name,
            "pid" => Self::Pid,
            "user" => Self::User,
            "cpu" => Self::Cpu,
            "mem" => Self::Mem,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Pid => "pid",
            Self::User => "user",
            Self::Cpu => "cpu",
            Self::Mem => "mem",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Pid => "PID",
            Self::User => "User",
            Self::Cpu => "CPU%",
            Self::Mem => "Memory",
        }
    }

    /// Numbers sort largest first, names smallest first.
    pub fn descending_by_default(self) -> bool {
        matches!(self, Self::Cpu | Self::Mem | Self::Pid)
    }
}

/// What a gadget instance shows on the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Cpu,
    Mem,
    Swap,
    Disk,
    Net,
    Gpu,
    Temp,
    Load,
}

impl Value {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "cpu" => Self::Cpu,
            "mem" => Self::Mem,
            "swap" => Self::Swap,
            "disk" => Self::Disk,
            "net" => Self::Net,
            "gpu" => Self::Gpu,
            "temp" => Self::Temp,
            "load" => Self::Load,
            _ => return None,
        })
    }

    /// The class the bar button carries.
    pub fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Mem => "mem",
            Self::Swap => "swap",
            Self::Disk => "disk",
            Self::Net => "net",
            Self::Gpu => "gpu",
            Self::Temp => "temp",
            Self::Load => "load",
        }
    }

    /// The `format` when the config doesn't set one.
    pub fn default_format(self) -> &'static str {
        match self {
            Self::Cpu => "{cpu}%",
            Self::Mem => "{mem}%",
            Self::Swap => "{swap}%",
            Self::Disk => "{read} {write}",
            Self::Net => "{rx} {tx}",
            Self::Gpu => "{gpu}%",
            Self::Temp => "{temp}°",
            Self::Load => "{load1}",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cpu {
    /// Busy percent, 0..100.
    pub total: f32,
    /// One per core.
    pub cores: Vec<f32>,
    pub freq_mhz: Option<f32>,
    pub temp_c: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mem {
    pub total: u64,
    pub used: u64,
    pub available: u64,
    pub cached: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Disk {
    /// The kernel device name (`nvme0n1p2`).
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub total: u64,
    pub used: u64,
    pub read_bps: f32,
    pub write_bps: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Iface {
    pub name: String,
    pub rx_bps: f32,
    pub tx_bps: f32,
    pub rx_total: u64,
    pub tx_total: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub busy: Option<f32>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    pub temp_c: Option<f32>,
}

/// One reading of everything.
#[derive(Debug, Clone)]
pub struct Sample {
    pub cpu: Cpu,
    pub load: (f32, f32, f32),
    pub uptime: Duration,
    pub mem: Mem,
    pub disks: Vec<Disk>,
    pub net: Vec<Iface>,
    pub gpus: Vec<Gpu>,
}

/// A process as read from `/proc`, before the cpu percentage.
#[derive(Debug, Clone)]
pub struct RawProcess {
    pub pid: u32,
    pub name: String,
    pub state: char,
    /// User + system jiffies so far.
    pub ticks: u64,
    pub rss: u64,
    pub uid: u32,
}

/// A process as the table shows it.
#[derive(Debug, Clone)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    pub user: String,
    pub state: char,
    /// Percent of one core since the previous reading.
    pub cpu: f32,
    pub rss: u64,
}

/// The recent values of every series the graphs draw, oldest first.
#[derive(Debug, Default)]
pub struct History {
    pub cpu: VecDeque<f32>,
    pub mem: VecDeque<f32>,
    pub swap: VecDeque<f32>,
    pub disk_read: VecDeque<f32>,
    pub disk_write: VecDeque<f32>,
    pub net_rx: VecDeque<f32>,
    pub net_tx: VecDeque<f32>,
    pub gpu: VecDeque<f32>,
    pub temp: VecDeque<f32>,
    pub load: VecDeque<f32>,
}

impl History {
    fn push(&mut self, s: &Sample, cap: usize) {
        fn add(series: &mut VecDeque<f32>, v: f32, cap: usize) {
            series.push_back(v);
            while series.len() > cap {
                series.pop_front();
            }
        }
        let pct = |used: u64, total: u64| {
            if total > 0 {
                used as f32 * 100.0 / total as f32
            } else {
                0.0
            }
        };
        add(&mut self.cpu, s.cpu.total, cap);
        add(&mut self.mem, pct(s.mem.used, s.mem.total), cap);
        add(&mut self.swap, pct(s.mem.swap_used, s.mem.swap_total), cap);
        add(
            &mut self.disk_read,
            s.disks.iter().map(|d| d.read_bps).sum(),
            cap,
        );
        add(
            &mut self.disk_write,
            s.disks.iter().map(|d| d.write_bps).sum(),
            cap,
        );
        add(&mut self.net_rx, s.net.iter().map(|i| i.rx_bps).sum(), cap);
        add(&mut self.net_tx, s.net.iter().map(|i| i.tx_bps).sum(), cap);
        add(
            &mut self.gpu,
            s.gpus.first().and_then(|g| g.busy).unwrap_or(0.0),
            cap,
        );
        add(&mut self.temp, s.cpu.temp_c.unwrap_or(0.0), cap);
        add(&mut self.load, s.load.0, cap);
    }

    /// The series a bar value's sparkline draws, and its scale (100
    /// for percentages, `None` for rates: the series' own maximum).
    pub fn series(&self, value: Value) -> (&VecDeque<f32>, Option<f32>) {
        match value {
            Value::Cpu => (&self.cpu, Some(100.0)),
            Value::Mem => (&self.mem, Some(100.0)),
            Value::Swap => (&self.swap, Some(100.0)),
            Value::Disk => (&self.disk_read, None),
            Value::Net => (&self.net_rx, None),
            Value::Gpu => (&self.gpu, Some(100.0)),
            Value::Temp => (&self.temp, Some(100.0)),
            Value::Load => (&self.load, None),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Sample(Box<Sample>),
    Processes(Vec<RawProcess>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Terminate,
    Kill,
}

#[derive(Debug, Clone)]
pub enum Command {
    /// Read the process table now.
    Processes,
    Signal(u32, Signal),
}

pub struct SysMon {
    config: MonitorConfig,
    last: Option<Sample>,
    history: History,
    processes: Vec<Process>,
    /// Ticks and time of the previous process reading, by pid.
    proc_prev: HashMap<u32, (u64, Instant)>,
    users: HashMap<u32, String>,
    clock_ticks: u64,
}

impl SysMon {
    pub fn new(config: MonitorConfig) -> Self {
        Self {
            config,
            last: None,
            history: History::default(),
            processes: Vec::new(),
            proc_prev: HashMap::new(),
            users: std::fs::read_to_string("/etc/passwd")
                .map(|t| proc::users(&t))
                .unwrap_or_default(),
            clock_ticks: proc::clock_ticks(),
        }
    }

    /// A config reload: the sampler restarts if its keys changed (the
    /// subscription is keyed on them); the history is kept.
    pub fn set_config(&mut self, config: MonitorConfig) {
        self.config = config;
    }

    pub fn config(&self) -> &MonitorConfig {
        &self.config
    }

    pub fn sample(&self) -> Option<&Sample> {
        self.last.as_ref()
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// The process table, as last read.
    pub fn processes(&self) -> &[Process] {
        &self.processes
    }

    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::run_with(self.config.clone(), |config| sampler(config.clone()))
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Sample(s) => {
                self.history.push(&s, self.config.history);
                self.last = Some(*s);
            }
            Event::Processes(raw) => {
                let now = Instant::now();
                let mut prev = HashMap::with_capacity(raw.len());
                self.processes = raw
                    .into_iter()
                    .map(|p| {
                        let cpu = match self.proc_prev.get(&p.pid) {
                            Some((ticks, at)) => {
                                let secs = now.duration_since(*at).as_secs_f32();
                                if secs > 0.0 {
                                    p.ticks.saturating_sub(*ticks) as f32 * 100.0
                                        / self.clock_ticks as f32
                                        / secs
                                } else {
                                    0.0
                                }
                            }
                            None => 0.0,
                        };
                        prev.insert(p.pid, (p.ticks, now));
                        Process {
                            pid: p.pid,
                            name: p.name,
                            user: self
                                .users
                                .get(&p.uid)
                                .cloned()
                                .unwrap_or_else(|| p.uid.to_string()),
                            state: p.state,
                            cpu,
                            rss: p.rss,
                        }
                    })
                    .collect();
                self.proc_prev = prev;
            }
        }
    }

    pub fn run(&self, command: Command) -> Task<Event> {
        match command {
            Command::Processes => Task::perform(
                async { tokio::task::spawn_blocking(|| proc::processes(Path::new("/proc"))).await },
                |r| Event::Processes(r.unwrap_or_default()),
            ),
            Command::Signal(pid, signal) => {
                if pid <= 1 {
                    log::warn!("sysmon: refusing to signal pid {pid}");
                    return Task::none();
                }
                let sig = match signal {
                    Signal::Terminate => libc::SIGTERM,
                    Signal::Kill => libc::SIGKILL,
                };
                // SAFETY: a plain syscall on a pid we don't own.
                let r = unsafe { libc::kill(pid as libc::pid_t, sig) };
                if r == 0 {
                    log::info!("sysmon: sent {signal:?} to {pid}");
                } else {
                    log::warn!(
                        "sysmon: {signal:?} to {pid}: {}",
                        std::io::Error::last_os_error()
                    );
                }
                Task::none()
            }
        }
    }

    /// `debug sysmon`: the numbers a scenario checks.
    pub fn describe(&self) -> String {
        let Some(s) = &self.last else {
            return "no sample yet".to_owned();
        };
        let pct = |used: u64, total: u64| (used * 100).checked_div(total).unwrap_or(0).to_string();
        format!(
            "cpu={:.0}% cores={} mem={}% swap={}% disks={} net={} gpu={} procs={} samples={}",
            s.cpu.total,
            s.cpu.cores.len(),
            pct(s.mem.used, s.mem.total),
            pct(s.mem.swap_used, s.mem.swap_total),
            s.disks
                .iter()
                .map(|d| d.mount.as_str())
                .collect::<Vec<_>>()
                .join(","),
            s.net
                .iter()
                .map(|i| i.name.as_str())
                .collect::<Vec<_>>()
                .join(","),
            s.gpus.len(),
            self.processes.len(),
            self.history.cpu.len(),
        )
    }
}

// --- the sampler ------------------------------------------------------------

/// What one reading carries over to the next: the counters, and what
/// was probed once.
struct Sampler {
    config: MonitorConfig,
    counters: proc::Counters,
    at: Instant,
    temp_sensor: Option<PathBuf>,
    nvidia: bool,
}

fn sampler(config: MonitorConfig) -> impl Stream<Item = Event> {
    iced_stream::channel(4, async move |mut out: mpsc::Sender<Event>| {
        let interval = Duration::from_secs(config.interval);
        let mut state = Sampler {
            temp_sensor: sensors::cpu_temp_sensor(&config.temperature),
            nvidia: sensors::has_nvidia_smi(),
            config,
            counters: proc::Counters::default(),
            at: Instant::now(),
        };
        log::debug!(
            "sysmon: sampling every {}s (temperature from {:?}, nvidia-smi: {})",
            state.config.interval,
            state.temp_sensor,
            state.nvidia
        );
        // The first reading only primes the counters.
        state = tokio::task::spawn_blocking(move || read(state).0)
            .await
            .expect("sampler");
        let mut ticks = tokio::time::interval(interval);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticks.tick().await;
        loop {
            ticks.tick().await;
            let (next, mut sample) = tokio::task::spawn_blocking(move || read(state))
                .await
                .expect("sampler");
            state = next;
            if state.nvidia {
                match tokio::process::Command::new("nvidia-smi")
                    .args(sensors::NVIDIA_SMI_ARGS)
                    .output()
                    .await
                {
                    Ok(o) if o.status.success() => sample
                        .gpus
                        .extend(sensors::nvidia_gpus(&String::from_utf8_lossy(&o.stdout))),
                    _ => {
                        log::warn!("sysmon: nvidia-smi failed, not asking again");
                        state.nvidia = false;
                    }
                }
            }
            if out.send(Event::Sample(Box::new(sample))).await.is_err() {
                return;
            }
        }
    })
}

/// One reading of `/proc` and `/sys` against the previous counters.
fn read(mut state: Sampler) -> (Sampler, Sample) {
    let read_file = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    let now = Instant::now();
    let elapsed = now.duration_since(state.at).as_secs_f32();
    let cpu_now = proc::cpu_times(&read_file("/proc/stat"));
    let usage = proc::cpu_usage(&state.counters.cpu, &cpu_now);
    let disk_now = proc::diskstats(&read_file("/proc/diskstats"));
    let net_now = proc::netdev(&read_file("/proc/net/dev"));
    let mounts = proc::mounts(&read_file("/proc/mounts"), &state.config.disks);
    let mut gpus = sensors::amd_gpus();
    gpus.truncate(4);
    let sample = Sample {
        cpu: Cpu {
            total: usage.first().copied().flatten().unwrap_or(0.0),
            cores: usage.iter().skip(1).map(|u| u.unwrap_or(0.0)).collect(),
            freq_mhz: sensors::cpu_freq_mhz(),
            temp_c: state.temp_sensor.as_deref().and_then(sensors::temp_c),
        },
        load: proc::loadavg(&read_file("/proc/loadavg")),
        uptime: proc::uptime(&read_file("/proc/uptime")),
        mem: proc::meminfo(&read_file("/proc/meminfo")),
        disks: proc::disks(&mounts, &state.counters.disks, &disk_now, elapsed),
        net: proc::ifaces(
            &state.config.interfaces,
            &state.counters.net,
            &net_now,
            elapsed,
        ),
        gpus,
    };
    state.counters = proc::Counters {
        cpu: cpu_now,
        disks: disk_now,
        net: net_now,
    };
    state.at = now;
    (state, sample)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(cpu: f32) -> Box<Sample> {
        Box::new(Sample {
            cpu: Cpu {
                total: cpu,
                cores: vec![cpu],
                freq_mhz: None,
                temp_c: None,
            },
            load: (0.0, 0.0, 0.0),
            uptime: Duration::ZERO,
            mem: Mem {
                total: 100,
                used: 50,
                ..Mem::default()
            },
            disks: Vec::new(),
            net: Vec::new(),
            gpus: Vec::new(),
        })
    }

    #[test]
    fn history_is_capped() {
        let config = crate::config::Config::parse("[SystemMonitor]\nhistory = 3\n").section(None);
        let mut m = SysMon::new(config);
        for i in 0..5 {
            m.apply(Event::Sample(sample(i as f32 * 10.0)));
        }
        assert_eq!(
            m.history().cpu.iter().copied().collect::<Vec<_>>(),
            [20.0, 30.0, 40.0]
        );
        assert_eq!(m.history().mem.len(), 3);
        assert_eq!(m.sample().unwrap().cpu.total, 40.0);
        let (series, scale) = m.history().series(Value::Cpu);
        assert_eq!((series.len(), scale), (3, Some(100.0)));
        assert!(m.describe().starts_with("cpu=40% cores=1 mem=50%"));
    }

    #[test]
    fn process_cpu_between_readings() {
        let config = crate::config::Config::parse("").section(None);
        let mut m = SysMon::new(config);
        m.clock_ticks = 100;
        let raw = |ticks| RawProcess {
            pid: 42,
            name: "x".into(),
            state: 'R',
            ticks,
            rss: 4096,
            uid: 0,
        };
        m.apply(Event::Processes(vec![raw(100)]));
        assert_eq!(m.processes()[0].cpu, 0.0, "nothing to compare to yet");
        // Fake an earlier reading one second ago with 50 fewer ticks.
        m.proc_prev
            .insert(42, (100, Instant::now() - Duration::from_secs(1)));
        m.apply(Event::Processes(vec![raw(150)]));
        let cpu = m.processes()[0].cpu;
        assert!((45.0..=50.0).contains(&cpu), "cpu {cpu}");
        assert_eq!(m.processes()[0].user, "root");
    }

    #[test]
    fn config_and_values() {
        let c = MonitorConfig::from_raw(&RawSection::default());
        assert_eq!((c.interval, c.history), (2, 60));
        assert!(c.disks.is_empty());
        assert_eq!((c.processes, c.sort), (10, Column::Cpu));
        assert_eq!(Column::parse("mem"), Some(Column::Mem));
        assert!(Column::Pid.descending_by_default() && !Column::Name.descending_by_default());
        assert_eq!(Value::parse("net"), Some(Value::Net));
        assert_eq!(Value::parse("x"), None);
        assert_eq!(Value::Temp.default_format(), "{temp}°");
    }
}
