//! System monitor gadget: one value on the bar (cpu, memory, swap,
//! disk, network, gpu, temperature or load) as text and a sparkline,
//! and a popup in the spirit of btop, one tab per section: cpu history
//! and per-core meters, memory and swap, disks, network, GPUs, and the
//! process table (sort by column; a click selects a row, a bar under
//! the table offers Terminate / Kill for it).
//! The popup opens on the tab of the value shown. A right click on the
//! bar runs `command` (by default a terminal monitor: btop, htop or
//! top, in `[general] terminal`).
//!
//! Holds no data: the sample, the history and the processes come from
//! `ctx.sysmon` (`sysmon/`), whose `[SystemMonitor]` section also
//! configures the popup; reading the processes and signalling one go
//! back as `Action::SysMon`. Every instance opens the same popup.

use std::collections::VecDeque;

use iced::Subscription;
use iced::widget::mouse_area;
use iced_wayland_subscriber::OutputInfo;
use icssed::{Element, Tab};

use crate::config::{RawSection, Section};
use crate::gadgets::{Action, Context, Gadget, Popup};
use crate::locale::Locale;
use crate::process;
use crate::services::sysmon::{
    Column, Command, MonitorConfig, Process, Sample, Signal, SysMon, Value, format,
};
use crate::time::aligned_ticks;

/// `[SystemMonitor:<id>]`: a gadget's keys (the sampler's and the
/// popup's are `sysmon::MonitorConfig`, from the base `[SystemMonitor]`;
/// a bare `SystemMonitor` isn't a gadget).
#[derive(Debug, Clone)]
pub struct InstanceConfig {
    pub show: Value,
    pub mode: Mode,
    pub format: String,
    pub icon: String,
    /// The top of the sparkline / the full gauge; `None`: 100 for a
    /// percentage, the history's maximum otherwise.
    pub max: Option<f32>,
    /// From these values on the button is `.warning` / `.danger`
    /// (the value's own unit); `None`: 70 / 90 for a percentage, no
    /// threshold otherwise.
    pub warning: Option<f32>,
    pub critical: Option<f32>,
    /// Run on a right click; empty: a terminal monitor.
    pub command: String,
    /// `[general] terminal`, for the default command (set by the
    /// gadget factory, not a key of this section).
    pub terminal: Option<String>,
}

impl Section for InstanceConfig {
    const NAME: &'static str = "SystemMonitor";

    fn from_raw(raw: &RawSection) -> Self {
        // `show` is required (the gadget factory refuses an instance
        // without it); a wrong one is logged and shows the cpu.
        let show = raw
            .get("show")
            .map(|s| {
                Value::parse(s).unwrap_or_else(|| {
                    log::warn!("unknown value {s:?} for show, using cpu");
                    Value::Cpu
                })
            })
            .unwrap_or(Value::Cpu);
        let mode = match raw.get("mode") {
            None => Mode::Sparkline,
            Some(m) => Mode::parse(m).unwrap_or_else(|| {
                log::warn!("unknown mode {m:?}, using sparkline");
                Mode::Sparkline
            }),
        };
        let number = |key: &str| {
            raw.get(key).and_then(|v| match v.parse::<f32>() {
                Ok(n) if n >= 0.0 => Some(n),
                _ => {
                    log::warn!("invalid {key} {v:?}, using the default");
                    None
                }
            })
        };
        Self {
            show,
            mode,
            format: raw.str_or("format", show.default_format()),
            icon: raw.str_or("icon", ""),
            max: number("max").filter(|n| *n > 0.0),
            warning: number("warning"),
            critical: number("critical"),
            command: raw.str_or("command", ""),
            terminal: None,
        }
    }
}

/// How an instance draws its value on the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The text alone.
    Text,
    /// The history as a small graph, the text over it.
    Sparkline,
    /// A bar filled to the value, the text over it.
    Gauge,
}

impl Mode {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "text" => Self::Text,
            "sparkline" => Self::Sparkline,
            "gauge" => Self::Gauge,
            _ => return None,
        })
    }
}

/// A tab of the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Cpu,
    Mem,
    Disk,
    Net,
    Gpu,
    Processes,
}

impl SectionKind {
    const ALL: [SectionKind; 6] = [
        Self::Cpu,
        Self::Mem,
        Self::Disk,
        Self::Net,
        Self::Gpu,
        Self::Processes,
    ];

    /// The tab a bar value belongs to.
    fn for_value(value: Value) -> Self {
        match value {
            Value::Cpu | Value::Temp | Value::Load => Self::Cpu,
            Value::Mem | Value::Swap => Self::Mem,
            Value::Disk => Self::Disk,
            Value::Net => Self::Net,
            Value::Gpu => Self::Gpu,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Mem => "mem",
            Self::Disk => "disk",
            Self::Net => "net",
            Self::Gpu => "gpu",
            Self::Processes => "processes",
        }
    }

    /// The catalogue key of the tab's title.
    fn title(self) -> &'static str {
        match self {
            Self::Cpu => "sysmon.tab.cpu",
            Self::Mem => "sysmon.tab.memory",
            Self::Disk => "sysmon.tab.disks",
            Self::Net => "sysmon.tab.network",
            Self::Gpu => "sysmon.tab.gpu",
            Self::Processes => "sysmon.tab.processes",
        }
    }
}

/// Percents from which a value is `.warning` / `.danger` (the
/// popup's meters, and the bar's percentages by default).
const WARNING: f32 = 70.0;
const CRITICAL: f32 = 90.0;
/// The terminal monitors tried for the default `command`.
const MONITORS: [&str; 3] = ["btop", "htop", "top"];
/// Per-core meters per row.
const CORES_PER_ROW: usize = 8;

pub struct SystemMonitor {
    config: InstanceConfig,
    popup: Popup,
    /// The tab shown.
    tab: SectionKind,
    sort: (Column, bool),
    /// The process row selected, whose signals the bar under the
    /// table offers.
    selected: Option<u32>,
    /// The interval the ticks follow while the popup is open.
    interval: u64,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    RunCommand,
    Tab(SectionKind),
    /// While the popup is open: time to read the processes again.
    Tick,
    Sort(Column),
    /// A click on the row of that pid: selects it, or deselects it
    /// when it was.
    Select(u32),
    /// A button of the action bar: the signal for the selected process.
    Signal(Signal),
}

impl Gadget for SystemMonitor {
    type Config = InstanceConfig;
    type Message = Message;

    fn new(config: InstanceConfig, _output: &OutputInfo) -> Self {
        Self::with_config(config)
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::TogglePopup => {
                if self.popup.is_open() {
                    return self.popup.close();
                }
                self.tab = SectionKind::for_value(self.config.show);
                Action::Many(vec![
                    Action::SysMon(Command::Processes),
                    self.popup.toggle(),
                ])
            }
            Message::RunCommand => {
                self.run_command();
                Action::None
            }
            Message::Tab(tab) => {
                self.tab = tab;
                self.selected = None;
                Action::None
            }
            Message::Tick => Action::SysMon(Command::Processes),
            Message::Sort(column) => {
                self.sort = if self.sort.0 == column {
                    (column, !self.sort.1)
                } else {
                    (column, column.descending_by_default())
                };
                Action::None
            }
            Message::Select(pid) => {
                self.selected = if self.selected == Some(pid) {
                    None
                } else {
                    Some(pid)
                };
                Action::None
            }
            Message::Signal(signal) => match self.selected {
                Some(pid) => Action::SysMon(Command::Signal(pid, signal)),
                None => Action::None,
            },
        }
    }

    fn icon_names(&self) -> Vec<String> {
        if self.config.icon.is_empty() {
            Vec::new()
        } else {
            vec![self.config.icon.clone()]
        }
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let sample = ctx.sysmon.sample();
        let value = self.config.show;
        let (series, auto_max) = ctx.sysmon.history().series(value);
        let level = self.level(series.back().copied(), auto_max.is_some());
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if !self.config.icon.is_empty() {
            parts.push(ctx.icon(self.config.icon.as_str()).into());
        }
        let text = format::expand(ctx.locale, &self.config.format, sample);
        let max = self.config.max.or(auto_max);
        parts.push(match self.config.mode {
            Mode::Text => icssed::text(text).into(),
            Mode::Sparkline => {
                let mut graph = icssed::graph(ctx.sysmon.config().history)
                    .series(series.iter().copied())
                    .label(text);
                if let Some(max) = max {
                    graph = graph.max(max);
                }
                graph.into()
            }
            Mode::Gauge => {
                let last = series.back().copied().unwrap_or(0.0);
                let top = max
                    .unwrap_or_else(|| series.iter().copied().fold(1.0, f32::max))
                    .max(f32::EPSILON);
                icssed::meter(last / top).label(text).into()
            }
        });
        let button = icssed::button(parts)
            .kind("indicator")
            .class(value.name())
            .class_if("warning", level == Some("warning"))
            .class_if("danger", level == Some("danger"))
            .on_press(Message::TogglePopup)
            .id(self.popup.anchor());
        let button = mouse_area(button).on_right_press(Message::RunCommand);
        icssed::row![button].into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_closed(&mut self) {
        self.selected = None;
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let locale = ctx.locale;
        let sysmon = ctx.sysmon;
        let sample = sysmon.sample();
        let has_gpu = sample.is_some_and(|s| !s.gpus.is_empty());
        // A gpu tab only with a gpu; the tab shown falls back to the
        // cpu's when it's the gpu's and there is none.
        let tab = if self.tab == SectionKind::Gpu && !has_gpu {
            SectionKind::Cpu
        } else {
            self.tab
        };
        let tabs = SectionKind::ALL
            .iter()
            .filter(|k| **k != SectionKind::Gpu || has_gpu)
            .map(|&k| {
                Tab::new(locale.tr(k.title()), Message::Tab(k))
                    .active(k == tab)
                    .class(k.name())
            });
        let mut body: Vec<Element<'a, Message>> = vec![header(
            locale.tr(tab.title()),
            section_value(locale, tab, sample),
        )];
        body.extend(match (tab, sample) {
            (SectionKind::Processes, _) => vec![self.processes(locale, sysmon)],
            (_, None) => Vec::new(),
            (SectionKind::Cpu, Some(s)) => cpu_section(locale, s, sysmon),
            (SectionKind::Mem, Some(s)) => mem_section(locale, s, sysmon),
            (SectionKind::Disk, Some(s)) => disk_section(locale, s, sysmon),
            (SectionKind::Net, Some(s)) => net_section(locale, s, sysmon),
            (SectionKind::Gpu, Some(s)) => gpu_section(locale, s, sysmon),
        });
        let mut parts: Vec<Element<'a, Message>> = vec![
            icssed::tabs(tabs).into(),
            icssed::scroll(icssed::column(body).kind("section").class(tab.name())).into(),
        ];
        // The action bar stays under the scrolling table.
        if tab == SectionKind::Processes {
            parts.push(self.actions(locale, sysmon));
        }
        icssed::column(parts).kind("monitor").into()
    }

    fn subscription(&self) -> Subscription<Message> {
        if !self.popup.is_open() {
            return Subscription::none();
        }
        Subscription::run_with(self.interval as u32, |step| aligned_ticks(*step))
            .map(|_| Message::Tick)
    }
}

impl SystemMonitor {
    fn with_config(config: InstanceConfig) -> Self {
        let tab = SectionKind::for_value(config.show);
        Self {
            config,
            popup: Popup::new(),
            tab,
            sort: (Column::Cpu, true),
            selected: None,
            interval: 2,
        }
    }

    /// What the base section says about the popup: the initial sort of
    /// the process table, and the sampler's interval for the ticks
    /// (set by the factory).
    pub fn set_monitor(&mut self, monitor: &MonitorConfig) {
        self.sort = (monitor.sort, monitor.sort.descending_by_default());
        self.interval = monitor.interval.max(1);
    }

    /// `"danger"` / `"warning"` when the current value reaches the
    /// instance's thresholds (the defaults for a percentage).
    fn level(&self, current: Option<f32>, percent: bool) -> Option<&'static str> {
        let v = current?;
        let default = |d: f32| percent.then_some(d);
        let critical = self.config.critical.or_else(|| default(CRITICAL));
        let warning = self.config.warning.or_else(|| default(WARNING));
        if critical.is_some_and(|t| v >= t) {
            Some("danger")
        } else if warning.is_some_and(|t| v >= t) {
            Some("warning")
        } else {
            None
        }
    }

    /// The process table: sortable header, the top rows.
    fn processes<'a>(&'a self, locale: &Locale, sysmon: &'a SysMon) -> Element<'a, Message> {
        let (sort_by, desc) = self.sort;
        let columns = Column::ALL.iter().map(|&c| {
            icssed::button![icssed::text(locale.tr(c.label()))]
                .kind("column")
                .class(c.name())
                .class_if("sorted", c == sort_by)
                .class_if("reverse", c == sort_by && desc != c.descending_by_default())
                .on_press(Message::Sort(c))
                .into()
        });
        let mut procs: Vec<&Process> = sysmon.processes().iter().collect();
        procs.sort_by(|a, b| {
            let ord = match sort_by {
                Column::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Column::Pid => a.pid.cmp(&b.pid),
                Column::User => a.user.cmp(&b.user),
                Column::Cpu => a.cpu.total_cmp(&b.cpu),
                Column::Mem => a.rss.cmp(&b.rss),
            };
            if desc { ord.reverse() } else { ord }
        });
        let rows = procs.into_iter().take(sysmon.config().processes).map(|p| {
            let cells = Column::ALL.iter().map(|&c| {
                let text = match c {
                    Column::Name => p.name.clone(),
                    Column::Pid => p.pid.to_string(),
                    Column::User => p.user.clone(),
                    Column::Cpu => locale.decimal(p.cpu.into(), 1),
                    Column::Mem => format::bytes(locale, p.rss),
                };
                icssed::text(text).kind(c.name()).into()
            });
            icssed::row(cells)
                .attr("pid", &p.pid.to_string())
                .attr("name", &p.name)
                .attr("state", &p.state.to_string())
                .class_if("selected", self.selected == Some(p.pid))
                .on_press(Message::Select(p.pid))
                .into()
        });
        let header = icssed::row(columns).kind("header").into();
        icssed::column(std::iter::once(header).chain(rows))
            .kind("table")
            .into()
    }

    /// The bar under the (scrolling) process table: the selected
    /// process (while it's still there) and its signals, or the help
    /// and the buttons disabled.
    fn actions<'a>(&'a self, locale: &'a Locale, sysmon: &'a SysMon) -> Element<'a, Message> {
        let p = self
            .selected
            .and_then(|pid| sysmon.processes().iter().find(|p| p.pid == pid));
        let label = match p {
            Some(p) => format!("{} ({})", p.name, p.pid),
            None => locale.tr("sysmon.select_help").to_owned(),
        };
        let button = |class: &str, label: &'a str, signal: Signal| {
            icssed::button![icssed::text(label)]
                .class(class)
                .class_if("danger", class == "kill")
                .disabled_if(p.is_none())
                .on_press(Message::Signal(signal))
        };
        icssed::row![
            icssed::text(label),
            button(
                "terminate",
                locale.tr("sysmon.terminate"),
                Signal::Terminate,
            ),
            button("kill", locale.tr("sysmon.kill"), Signal::Kill),
        ]
        .kind("actions")
        .class_if("none", p.is_none())
        .into()
    }

    fn run_command(&self) {
        if !self.config.command.is_empty() {
            process::run(&self.config.command);
            return;
        }
        let Some(terminal) = &self.config.terminal else {
            log::warn!("system monitor: no terminal for the monitor ([general] terminal)");
            return;
        };
        match process::first_on_path(&MONITORS) {
            Some(program) => {
                process::run_argv(&process::in_terminal(terminal, &[program.to_owned()]))
            }
            None => log::warn!("system monitor: none of {MONITORS:?} on the PATH"),
        }
    }
}

/// A section's title and the number it shows.
fn header<'a>(title: &'a str, value: String) -> Element<'a, Message> {
    icssed::row![
        icssed::text(title).kind("title"),
        icssed::text(value).kind("value"),
    ]
    .kind("header")
    .into()
}

/// The top of a graph: its given maximum, else the largest value of
/// its series (at least 1, so a flat zero line stays at the bottom).
fn scale(series: &[&VecDeque<f32>], max: Option<f32>) -> f32 {
    max.unwrap_or_else(|| {
        series
            .iter()
            .flat_map(|s| s.iter().copied())
            .fold(1.0, f32::max)
    })
}

/// A section's graph: up to two series (the second `series.second`),
/// scaled to `max` (else the largest value), its top written as `label`.
fn graph<'a>(
    locale: &Locale,
    sysmon: &'a SysMon,
    series: [Option<&'a VecDeque<f32>>; 2],
    max: Option<f32>,
    rate: bool,
) -> Element<'a, Message> {
    let present: Vec<&VecDeque<f32>> = series.iter().flatten().copied().collect();
    let top = scale(&present, max);
    let label = if rate {
        format::rate(locale, top)
    } else {
        format!("{top:.0}%")
    };
    let mut graph = icssed::graph(sysmon.config().history).max(top);
    if let Some(first) = series[0] {
        graph = graph.series(first.iter().copied());
    }
    if let Some(second) = series[1] {
        graph = graph.series_class("second", second.iter().copied());
    }
    graph.label(label).into()
}

/// A fraction's meter, `.warning` / `.danger` from its percent.
fn level_meter<'a>(percent: f32) -> icssed::Meter<'a, Message> {
    icssed::meter(percent / 100.0)
        .class_if("warning", (WARNING..CRITICAL).contains(&percent))
        .class_if("danger", percent >= CRITICAL)
}

fn cpu_section<'a>(locale: &Locale, s: &Sample, sysmon: &'a SysMon) -> Vec<Element<'a, Message>> {
    let h = sysmon.history();
    let mut out = vec![graph(
        locale,
        sysmon,
        [Some(&h.cpu), None],
        Some(100.0),
        false,
    )];
    let groups = s.cpu.cores.chunks(CORES_PER_ROW).map(|chunk| {
        let cells = chunk.iter().map(|v| {
            let meter = icssed::meter(v / 100.0).class("thin");
            icssed::column![meter, icssed::text(format!("{v:.0}%"))]
                .kind("core")
                .class_if("warning", (WARNING..CRITICAL).contains(v))
                .class_if("danger", *v >= CRITICAL)
                .into()
        });
        icssed::row(cells).kind("group").into()
    });
    out.push(icssed::column(groups).kind("cores").into());
    let mut lines = vec![locale.fmt(
        "sysmon.cpu.load",
        &[
            ("load1", &locale.decimal(s.load.0.into(), 2)),
            ("load5", &locale.decimal(s.load.1.into(), 2)),
            ("load15", &locale.decimal(s.load.2.into(), 2)),
            ("uptime", &format::duration(s.uptime)),
        ],
    )];
    let mut hw = Vec::new();
    if let Some(f) = s.cpu.freq_mhz {
        hw.push(locale.fmt(
            "sysmon.cpu.frequency",
            &[("freq", &format::freq(locale, f))],
        ));
    }
    if let Some(t) = s.cpu.temp_c {
        hw.push(locale.fmt(
            "sysmon.cpu.temperature",
            &[("temp", &format_args!("{t:.0}"))],
        ));
    }
    if !hw.is_empty() {
        lines.push(hw.join("   "));
    }
    let lines = lines
        .into_iter()
        .map(|l| icssed::text(l).kind("line").into());
    out.push(icssed::column(lines).kind("details").into());
    out
}

fn mem_section<'a>(locale: &Locale, s: &Sample, sysmon: &'a SysMon) -> Vec<Element<'a, Message>> {
    let h = sysmon.history();
    let used = pct(s.mem.used, s.mem.total);
    let mut out = vec![
        icssed::text(locale.fmt(
            "sysmon.mem.used",
            &[
                ("used", &format::bytes(locale, s.mem.used)),
                ("total", &format::bytes(locale, s.mem.total)),
                ("cached", &format::bytes(locale, s.mem.cached)),
                ("available", &format::bytes(locale, s.mem.available)),
            ],
        ))
        .kind("line")
        .into(),
        level_meter(used).class("used").into(),
    ];
    if s.mem.swap_total > 0 {
        let swap = pct(s.mem.swap_used, s.mem.swap_total);
        out.push(
            icssed::text(locale.fmt(
                "sysmon.mem.swap",
                &[
                    ("used", &format::bytes(locale, s.mem.swap_used)),
                    ("total", &format::bytes(locale, s.mem.swap_total)),
                ],
            ))
            .kind("line")
            .into(),
        );
        out.push(level_meter(swap).class("swap").into());
    }
    out.push(graph(
        locale,
        sysmon,
        [Some(&h.mem), Some(&h.swap)],
        Some(100.0),
        false,
    ));
    out
}

fn disk_section<'a>(locale: &Locale, s: &Sample, sysmon: &'a SysMon) -> Vec<Element<'a, Message>> {
    let h = sysmon.history();
    let mut out: Vec<Element<'a, Message>> = s
        .disks
        .iter()
        .map(|d| {
            icssed::column![
                icssed::row![
                    icssed::text(format!("{}  ({}, {})", d.mount, d.name, d.fs)).kind("name"),
                    icssed::text(format!(
                        "{} / {}   ↓ {}  ↑ {}",
                        format::bytes(locale, d.used),
                        format::bytes(locale, d.total),
                        format::rate(locale, d.read_bps),
                        format::rate(locale, d.write_bps)
                    ))
                    .kind("line"),
                ]
                .kind("top"),
                level_meter(pct(d.used, d.total)),
            ]
            .kind("disk")
            .attr("mount", &d.mount)
            .into()
        })
        .collect();
    out.push(graph(
        locale,
        sysmon,
        [Some(&h.disk_read), Some(&h.disk_write)],
        None,
        true,
    ));
    out
}

fn net_section<'a>(locale: &Locale, s: &Sample, sysmon: &'a SysMon) -> Vec<Element<'a, Message>> {
    let h = sysmon.history();
    let mut out: Vec<Element<'a, Message>> = s
        .net
        .iter()
        .map(|iface| {
            icssed::row![
                icssed::text(iface.name.clone()).kind("name"),
                icssed::text(format!(
                    "↓ {}  ↑ {}   ({} / {} total)",
                    format::rate(locale, iface.rx_bps),
                    format::rate(locale, iface.tx_bps),
                    format::bytes(locale, iface.rx_total),
                    format::bytes(locale, iface.tx_total)
                ))
                .kind("line"),
            ]
            .kind("iface")
            .attr("name", &iface.name)
            .into()
        })
        .collect();
    out.push(graph(
        locale,
        sysmon,
        [Some(&h.net_rx), Some(&h.net_tx)],
        None,
        true,
    ));
    out
}

fn gpu_section<'a>(locale: &Locale, s: &Sample, sysmon: &'a SysMon) -> Vec<Element<'a, Message>> {
    let h = sysmon.history();
    let mut out: Vec<Element<'a, Message>> = s
        .gpus
        .iter()
        .map(|g| {
            let mut info = Vec::new();
            if let (Some(u), Some(t)) = (g.vram_used, g.vram_total) {
                info.push(locale.fmt(
                    "sysmon.gpu.vram",
                    &[
                        ("used", &format::bytes(locale, u)),
                        ("total", &format::bytes(locale, t)),
                    ],
                ));
            }
            if let Some(t) = g.temp_c {
                info.push(format!("{t:.0}°C"));
            }
            icssed::column![
                icssed::row![
                    icssed::text(g.name.clone()).kind("name"),
                    icssed::text(info.join("   ")).kind("line"),
                ]
                .kind("top"),
                level_meter(g.busy.unwrap_or(0.0)),
            ]
            .kind("gpu")
            .into()
        })
        .collect();
    out.push(graph(
        locale,
        sysmon,
        [Some(&h.gpu), None],
        Some(100.0),
        false,
    ));
    out
}

fn pct(used: u64, total: u64) -> f32 {
    if total > 0 {
        used as f32 * 100.0 / total as f32
    } else {
        0.0
    }
}

/// The number shown in a section's header.
fn section_value(locale: &Locale, kind: SectionKind, sample: Option<&Sample>) -> String {
    let Some(s) = sample else {
        return String::new();
    };
    match kind {
        SectionKind::Cpu => format!("{:.0}%", s.cpu.total),
        SectionKind::Mem => format!("{:.0}%", pct(s.mem.used, s.mem.total)),
        SectionKind::Disk => format!(
            "{} / {}",
            format::rate(locale, s.disks.iter().map(|d| d.read_bps).sum()),
            format::rate(locale, s.disks.iter().map(|d| d.write_bps).sum())
        ),
        SectionKind::Net => format!(
            "↓ {}  ↑ {}",
            format::rate(locale, s.net.iter().map(|i| i.rx_bps).sum()),
            format::rate(locale, s.net.iter().map(|i| i.tx_bps).sum())
        ),
        SectionKind::Gpu => s
            .gpus
            .first()
            .and_then(|g| g.busy)
            .map(|b| format!("{b:.0}%"))
            .unwrap_or_default(),
        SectionKind::Processes => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let c = InstanceConfig::from_raw(&RawSection::default());
        assert_eq!(c.show, Value::Cpu);
        assert_eq!(c.format, "{cpu}%");
        assert_eq!((c.mode, c.max), (Mode::Sparkline, None));
        assert_eq!((c.warning, c.critical), (None, None));
        let c: InstanceConfig = crate::config::Config::parse(
            "[SystemMonitor:x]\nshow = net\nmode = gauge\nmax = 90\nwarning = 70\ncritical = 85\n",
        )
        .section(Some("SystemMonitor:x"));
        assert_eq!(c.show, Value::Net);
        assert_eq!(c.format, "{rx} {tx}");
        assert_eq!((c.mode, c.max), (Mode::Gauge, Some(90.0)));
        assert_eq!((c.warning, c.critical), (Some(70.0), Some(85.0)));
        let c: InstanceConfig = crate::config::Config::parse(
            "[SystemMonitor:x]\nshow = cpu\nmode = pie\nmax = -1\nwarning = x\n",
        )
        .section(Some("SystemMonitor:x"));
        assert_eq!((c.mode, c.max, c.warning), (Mode::Sparkline, None, None));
        assert_eq!(SectionKind::for_value(Value::Swap), SectionKind::Mem);
        assert_eq!(SectionKind::for_value(Value::Load), SectionKind::Cpu);
    }

    #[test]
    fn levels() {
        let mut g = SystemMonitor::with_config(InstanceConfig::from_raw(&RawSection::default()));
        // A percentage: the defaults.
        assert_eq!(g.level(Some(69.0), true), None);
        assert_eq!(g.level(Some(70.0), true), Some("warning"));
        assert_eq!(g.level(Some(90.0), true), Some("danger"));
        // A rate: nothing unless configured.
        assert_eq!(g.level(Some(1e9), false), None);
        g.config.warning = Some(1000.0);
        assert_eq!(g.level(Some(1e9), false), Some("warning"));
        g.config.critical = Some(1e6);
        assert_eq!(g.level(Some(1e9), false), Some("danger"));
        assert_eq!(g.level(None, true), None);
    }

    #[test]
    fn sorting_flips() {
        let mut g = SystemMonitor::with_config(InstanceConfig::from_raw(&RawSection::default()));
        g.set_monitor(&MonitorConfig::from_raw(&RawSection::default()));
        assert_eq!(g.sort, (Column::Cpu, true));
        let _ = g.update(Message::Sort(Column::Name));
        assert_eq!(g.sort, (Column::Name, false));
        let _ = g.update(Message::Sort(Column::Name));
        assert_eq!(g.sort, (Column::Name, true));
        let _ = g.update(Message::Sort(Column::Mem));
        assert_eq!(g.sort, (Column::Mem, true));
    }
}
