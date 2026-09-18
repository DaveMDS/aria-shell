//! System monitor gadget: one value on the bar (cpu, memory, swap,
//! disk, network, gpu, temperature or load) as text and a sparkline,
//! and a popup in the spirit of btop, one tab per section: cpu history
//! and per-core meters, memory and swap, disks, network, GPUs, and the
//! process table (sort by column; a click selects a row, a bar under
//! the table offers Terminate / Kill for it).
//! The popup opens on the tab of the value shown. A right click on the
//! bar runs `command` (by default a terminal monitor: btop, htop or
//! top, in the launcher's terminal).
//!
//! Holds no data: the sample, the history and the processes come from
//! `ctx.sysmon` (`sysmon/`), whose `[SystemMonitor]` section also
//! configures the popup; reading the processes and signalling one go
//! back as `Action::SysMon`. Every instance opens the same popup.

use iced::widget::{Space, column, container, mouse_area, row, scrollable};
use iced::{Alignment, Element, Length, Subscription};
use iced_wayland_subscriber::OutputInfo;

use crate::config::{RawSection, Section};
use crate::gadget::{Action, Context, Gadget, Popup};
use crate::locale::Locale;
use crate::process;
use crate::sysmon::{
    Column, Command, MonitorConfig, Process, Sample, Signal, SysMon, Value, format,
};
use crate::theme::{self, Node, Theme};
use crate::time::aligned_ticks;
use crate::widgets::graph;

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
    /// From these values on the button is `.warning` / `.critical`
    /// (the value's own unit); `None`: 70 / 90 for a percentage, no
    /// threshold otherwise.
    pub warning: Option<f32>,
    pub critical: Option<f32>,
    /// Run on a right click; empty: a terminal monitor.
    pub command: String,
    /// `[launcher] terminal`, for the default command (set by the
    /// gadget factory, not a key of this section).
    pub terminal: String,
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
            terminal: String::new(),
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

/// Numbers sit at the right of their column.
fn column_alignment(column: Column) -> Alignment {
    match column {
        Column::Cpu | Column::Mem => Alignment::End,
        _ => Alignment::Start,
    }
}

/// Width of a table column when the theme doesn't set one on the
/// header's node (the name takes the rest).
fn default_width(column: Column) -> Option<f32> {
    match column {
        Column::Name => None,
        Column::Pid => Some(64.0),
        Column::User => Some(84.0),
        Column::Cpu => Some(60.0),
        Column::Mem => Some(80.0),
    }
}

/// Percents from which a value is `.warning` / `.critical` (the
/// popup's meters, and the bar's percentages by default).
const WARNING: f32 = 70.0;
const CRITICAL: f32 = 90.0;
/// Icon size when the theme doesn't set `height` on `icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;
/// Popup size when the theme doesn't size `monitor`.
const DEFAULT_WIDTH: f32 = 560.0;
const DEFAULT_HEIGHT: f32 = 460.0;
/// Per-core meters per row.
const CORES_PER_ROW: usize = 8;
/// The terminal monitors tried for the default `command`.
const MONITORS: [&str; 3] = ["btop", "htop", "top"];

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
        let theme = ctx.theme;
        let sample = ctx.sysmon.sample();
        let value = self.config.show;
        let (series, auto_max) = ctx.sysmon.history().series(value);
        let level = self.level(series.back().copied(), auto_max.is_some());
        let button = ctx
            .node
            .child("button")
            .class(value.name())
            .class_if("warning", level == Some("warning"))
            .class_if("critical", level == Some("critical"));
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if !self.config.icon.is_empty() {
            let icon_node = button.child("icon");
            let style = theme.resolve(&icon_node);
            let size = px(style.height.or(style.width)).unwrap_or(DEFAULT_ICON_SIZE);
            parts.push(match ctx.icons.get_name(&self.config.icon, None) {
                Some(icon) => icon.view(size, style.color),
                None => Space::new().width(size).height(size).into(),
            });
        }
        let text = format::expand(&self.config.format, sample);
        let max = self.config.max.or(auto_max);
        parts.push(match self.config.mode {
            Mode::Text => {
                let text_node = button.child("text");
                theme
                    .container(&text_node, theme.text(&text_node, text))
                    .into()
            }
            Mode::Sparkline => graph::sparkline(
                theme,
                &button.child("graph"),
                series,
                max,
                ctx.sysmon.config().history,
                Some(&text),
            ),
            Mode::Gauge => {
                let last = series.back().copied().unwrap_or(0.0);
                let top = max
                    .unwrap_or_else(|| series.iter().copied().fold(1.0, f32::max))
                    .max(f32::EPSILON);
                graph::gauge(theme, &button.child("gauge"), last / top, Some(&text))
            }
        });
        let content = row(parts)
            .spacing(theme.resolve(&button).gap)
            .align_y(Alignment::Center);
        let button = theme
            .button(&button, content)
            .on_press(Message::TogglePopup);
        mouse_area(self.popup.anchor(button))
            .on_right_press(Message::RunCommand)
            .into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_closed(&mut self) {
        self.selected = None;
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let locale = ctx.locale;
        let monitor = ctx.node.child("monitor");
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
        let tabs_node = monitor.child("tabs");
        let tabs = SectionKind::ALL
            .iter()
            .filter(|k| **k != SectionKind::Gpu || has_gpu)
            .map(|&k| {
                let n = tabs_node
                    .child("tab")
                    .class(k.name())
                    .class_if("active", k == tab);
                theme
                    .button(&n, theme.text(&n.child("text"), locale.tr(k.title())))
                    .on_press(Message::Tab(k))
                    .into()
            });
        let tabs: Element<'a, Message> = theme
            .row(&tabs_node, tabs)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into();
        let node = monitor.child("section").class(tab.name());
        let body: Vec<Element<'a, Message>> = match (tab, sample) {
            (SectionKind::Processes, _) => self.processes(theme, locale, &node, sysmon),
            (_, None) => Vec::new(),
            (SectionKind::Cpu, Some(s)) => cpu_section(theme, locale, &node, s, sysmon),
            (SectionKind::Mem, Some(s)) => mem_section(theme, locale, &node, s, sysmon),
            (SectionKind::Disk, Some(s)) => disk_section(theme, &node, s, sysmon),
            (SectionKind::Net, Some(s)) => net_section(theme, &node, s, sysmon),
            (SectionKind::Gpu, Some(s)) => gpu_section(theme, locale, &node, s, sysmon),
        };
        let value = section_value(tab, sample);
        let header = header(theme, &node, locale.tr(tab.title()), &value);
        let section: Element<'a, Message> = theme
            .container(
                &node,
                theme
                    .column(&node, std::iter::once(header).chain(body))
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .into();
        // The action bar stays under the scrolling table.
        let footer =
            (tab == SectionKind::Processes).then(|| self.actions(theme, locale, &node, sysmon));
        // The process list always overflows: its scrollbar is embedded
        // (taking its own column) so it doesn't cover the last column;
        // the other tabs' floats, shown only when needed.
        let mut scroll = scrollable(section).width(Length::Fill).height(Length::Fill);
        if tab == SectionKind::Processes {
            scroll = scroll.direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new()
                    .width(6)
                    .scroller_width(6)
                    .spacing(6),
            ));
        }
        let scroll: Element<'a, Message> = scroll.into();
        let content = theme
            .column(&monitor, [tabs, scroll].into_iter().chain(footer))
            .width(Length::Fill)
            .height(Length::Fill);
        theme
            .container(&monitor, content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn popup_size(&self, ctx: Context<'_>) -> (u32, u32) {
        let s = ctx.theme.resolve(&ctx.node.child("monitor"));
        (
            px(s.width).unwrap_or(DEFAULT_WIDTH).max(1.0) as u32,
            px(s.height).unwrap_or(DEFAULT_HEIGHT).max(1.0) as u32,
        )
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

    /// `"critical"` / `"warning"` when the current value reaches the
    /// instance's thresholds (the defaults for a percentage).
    fn level(&self, current: Option<f32>, percent: bool) -> Option<&'static str> {
        let v = current?;
        let default = |d: f32| percent.then_some(d);
        let critical = self.config.critical.or_else(|| default(CRITICAL));
        let warning = self.config.warning.or_else(|| default(WARNING));
        if critical.is_some_and(|t| v >= t) {
            Some("critical")
        } else if warning.is_some_and(|t| v >= t) {
            Some("warning")
        } else {
            None
        }
    }

    fn run_command(&self) {
        if !self.config.command.is_empty() {
            process::run(&self.config.command);
            return;
        }
        match process::first_on_path(&MONITORS) {
            Some(program) => process::run_argv(&process::in_terminal(
                &self.config.terminal,
                vec![program.to_owned()],
            )),
            None => log::warn!("system monitor: none of {MONITORS:?} on the PATH"),
        }
    }

    /// The process table: sortable header, the top rows.
    fn processes<'a>(
        &'a self,
        theme: &'a Theme,
        locale: &Locale,
        node: &Node,
        sysmon: &'a SysMon,
    ) -> Vec<Element<'a, Message>> {
        let table = node.child("table");
        let header_node = table.child("header");
        let (sort_by, desc) = self.sort;
        let columns = Column::ALL.iter().map(|&c| {
            let n = header_node
                .child("column")
                .class(c.name())
                .class_if("sorted", c == sort_by)
                .class_if("reverse", c == sort_by && desc != c.descending_by_default());
            let label = container(theme.text(&n.child("text"), locale.tr(c.label())))
                .width(Length::Fill)
                .align_x(column_alignment(c));
            let mut b = theme.button(&n, label).on_press(Message::Sort(c));
            b = match cell_width(theme, &n, c) {
                Some(w) => b.width(Length::Fixed(w)),
                None => b.width(Length::Fill),
            };
            b.into()
        });
        let header: Element<'a, Message> = theme
            .row(&header_node, columns)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into();
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
            let r = table
                .child("row")
                .attr("pid", p.pid.to_string())
                .attr("name", p.name.clone())
                .attr("state", p.state.to_string())
                .class_if("selected", self.selected == Some(p.pid));
            let cells = Column::ALL.iter().map(|&c| {
                let n = r.child(c.name());
                let text = match c {
                    Column::Name => p.name.clone(),
                    Column::Pid => p.pid.to_string(),
                    Column::User => p.user.clone(),
                    Column::Cpu => format!("{:.1}", p.cpu),
                    Column::Mem => format::bytes(p.rss),
                };
                let cell = theme
                    .container(&n, theme.text(&n, text))
                    .align_x(column_alignment(c));
                match cell_width(theme, &header_node.child("column").class(c.name()), c) {
                    Some(w) => cell.width(Length::Fixed(w)).into(),
                    None => cell.width(Length::Fill).into(),
                }
            });
            let content = theme
                .row(&r, cells)
                .align_y(Alignment::Center)
                .width(Length::Fill);
            mouse_area(theme.container(&r, content).width(Length::Fill))
                .on_press(Message::Select(p.pid))
                .into()
        });
        vec![
            theme
                .column(&table, std::iter::once(header).chain(rows))
                .width(Length::Fill)
                .into(),
        ]
    }

    /// The bar under the (scrolling) process table: the selected
    /// process (while it's still there) and its signals, or the help
    /// and the buttons disabled.
    fn actions<'a>(
        &'a self,
        theme: &'a Theme,
        locale: &Locale,
        node: &Node,
        sysmon: &'a SysMon,
    ) -> Element<'a, Message> {
        let p = self
            .selected
            .and_then(|pid| sysmon.processes().iter().find(|p| p.pid == pid));
        let actions = node.child("actions").class_if("none", p.is_none());
        let text = actions.child("text");
        let label = match p {
            Some(p) => format!("{} ({})", p.name, p.pid),
            None => locale.tr("sysmon.select_help").to_owned(),
        };
        let button = |class: &'static str, label: &'static str, signal: Signal| {
            let b = actions.child("button").class(class);
            let mut button = theme.button(&b, theme.text(&b.child("text"), label));
            if p.is_some() {
                button = button.on_press(Message::Signal(signal));
            }
            button
        };
        let bar = theme
            .row(
                &actions,
                [
                    theme
                        .container(&text, theme.text(&text, label))
                        .width(Length::Fill)
                        .into(),
                    button(
                        "terminate",
                        locale.tr("sysmon.terminate"),
                        Signal::Terminate,
                    )
                    .into(),
                    button("kill", locale.tr("sysmon.kill"), Signal::Kill).into(),
                ],
            )
            .align_y(Alignment::Center)
            .width(Length::Fill);
        theme.container(&actions, bar).width(Length::Fill).into()
    }
}

/// Width of a table column: the theme's on the header column node,
/// else the default; `None` fills.
fn cell_width(theme: &Theme, node: &Node, column: Column) -> Option<f32> {
    px(theme.resolve(node).width).or(default_width(column))
}

fn px(l: Option<theme::Length>) -> Option<f32> {
    match l {
        Some(theme::Length::Px(px)) => Some(px),
        _ => None,
    }
}

fn pct(used: u64, total: u64) -> f32 {
    if total > 0 {
        used as f32 * 100.0 / total as f32
    } else {
        0.0
    }
}

/// The number shown in a section's header.
fn section_value(kind: SectionKind, sample: Option<&Sample>) -> String {
    let Some(s) = sample else {
        return String::new();
    };
    match kind {
        SectionKind::Cpu => format!("{:.0}%", s.cpu.total),
        SectionKind::Mem => format!("{:.0}%", pct(s.mem.used, s.mem.total)),
        SectionKind::Disk => format!(
            "{} / {}",
            format::rate(s.disks.iter().map(|d| d.read_bps).sum()),
            format::rate(s.disks.iter().map(|d| d.write_bps).sum())
        ),
        SectionKind::Net => format!(
            "↓ {}  ↑ {}",
            format::rate(s.net.iter().map(|i| i.rx_bps).sum()),
            format::rate(s.net.iter().map(|i| i.tx_bps).sum())
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

fn header<'a, M: 'a>(theme: &Theme, node: &Node, title: &str, value: &str) -> Element<'a, M> {
    let h = node.child("header");
    theme
        .row(
            &h,
            [
                theme
                    .text(&h.child("title"), title.to_owned())
                    .width(Length::Fill)
                    .into(),
                theme.text(&h.child("value"), value.to_owned()).into(),
            ],
        )
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
}

fn text_line<'a, M: 'a>(theme: &Theme, node: &Node, text: String) -> Element<'a, M> {
    theme.text(node, text).into()
}

fn cpu_section<'a, M: 'a>(
    theme: &Theme,
    locale: &Locale,
    node: &Node,
    s: &Sample,
    sysmon: &'a SysMon,
) -> Vec<Element<'a, M>> {
    let cap = sysmon.config().history;
    let h = sysmon.history();
    let mut out = vec![graph::graph(
        theme,
        &node.child("graph"),
        &[&h.cpu],
        Some(100.0),
        cap,
        graph::Unit::Percent,
    )];
    let cores_node = node.child("cores");
    let gap = theme.resolve(&cores_node).gap;
    let core_rows = s.cores_chunks().map(|chunk| {
        let mut cores: Vec<Element<'a, M>> = chunk
            .iter()
            .map(|(i, v)| {
                let c = cores_node
                    .child("core")
                    .class_if("warning", (WARNING..CRITICAL).contains(v))
                    .class_if("critical", *v >= CRITICAL)
                    .nth(*i, s.cpu.cores.len());
                let meter = graph::meter(theme, &c.child("meter"), v / 100.0);
                let label = theme.text(&c.child("text"), format!("{v:.0}%"));
                theme
                    .container(
                        &c,
                        column![meter, label].spacing(2).align_x(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .into()
            })
            .collect();
        // A short last row keeps the cells' width.
        while cores.len() < CORES_PER_ROW {
            cores.push(Space::new().width(Length::Fill).into());
        }
        row(cores).spacing(gap).width(Length::Fill).into()
    });
    out.push(
        theme
            .column(&cores_node, core_rows)
            .width(Length::Fill)
            .into(),
    );
    let d = node.child("details");
    let t = d.child("text");
    let mut lines = vec![locale.fmt(
        "sysmon.cpu.load",
        &[
            ("load1", &format_args!("{:.2}", s.load.0)),
            ("load5", &format_args!("{:.2}", s.load.1)),
            ("load15", &format_args!("{:.2}", s.load.2)),
            ("uptime", &format::duration(s.uptime)),
        ],
    )];
    let mut hw = Vec::new();
    if let Some(f) = s.cpu.freq_mhz {
        hw.push(locale.fmt("sysmon.cpu.frequency", &[("freq", &format::freq(f))]));
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
    out.push(
        theme
            .column(&d, lines.into_iter().map(|l| text_line(theme, &t, l)))
            .width(Length::Fill)
            .into(),
    );
    out
}

impl Sample {
    /// The cores as rows of [`CORES_PER_ROW`], indexed.
    fn cores_chunks(&self) -> impl Iterator<Item = Vec<(usize, f32)>> + '_ {
        let indexed: Vec<(usize, f32)> = self.cpu.cores.iter().copied().enumerate().collect();
        (0..indexed.len().div_ceil(CORES_PER_ROW)).map(move |r| {
            indexed
                .iter()
                .skip(r * CORES_PER_ROW)
                .take(CORES_PER_ROW)
                .copied()
                .collect()
        })
    }
}

fn mem_section<'a, M: 'a>(
    theme: &Theme,
    locale: &Locale,
    node: &Node,
    s: &Sample,
    sysmon: &'a SysMon,
) -> Vec<Element<'a, M>> {
    let cap = sysmon.config().history;
    let h = sysmon.history();
    let used = pct(s.mem.used, s.mem.total);
    let mut out = vec![
        text_line(
            theme,
            &node.child("text"),
            locale.fmt(
                "sysmon.mem.used",
                &[
                    ("used", &format::bytes(s.mem.used)),
                    ("total", &format::bytes(s.mem.total)),
                    ("cached", &format::bytes(s.mem.cached)),
                    ("available", &format::bytes(s.mem.available)),
                ],
            ),
        ),
        graph::meter(
            theme,
            &node
                .child("meter")
                .class("used")
                .class_if("warning", (WARNING..CRITICAL).contains(&used))
                .class_if("critical", used >= CRITICAL),
            used / 100.0,
        ),
    ];
    if s.mem.swap_total > 0 {
        let swap = pct(s.mem.swap_used, s.mem.swap_total);
        out.push(text_line(
            theme,
            &node.child("text"),
            locale.fmt(
                "sysmon.mem.swap",
                &[
                    ("used", &format::bytes(s.mem.swap_used)),
                    ("total", &format::bytes(s.mem.swap_total)),
                ],
            ),
        ));
        out.push(graph::meter(
            theme,
            &node
                .child("meter")
                .class("swap")
                .class_if("warning", (WARNING..CRITICAL).contains(&swap))
                .class_if("critical", swap >= CRITICAL),
            swap / 100.0,
        ));
    }
    out.push(graph::graph(
        theme,
        &node.child("graph"),
        &[&h.mem, &h.swap],
        Some(100.0),
        cap,
        graph::Unit::Percent,
    ));
    out
}

fn disk_section<'a, M: 'a>(
    theme: &Theme,
    node: &Node,
    s: &Sample,
    sysmon: &'a SysMon,
) -> Vec<Element<'a, M>> {
    let cap = sysmon.config().history;
    let h = sysmon.history();
    let count = s.disks.len();
    let mut out: Vec<Element<'a, M>> = s
        .disks
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let used = pct(d.used, d.total);
            let r = node
                .child("disk")
                .attr("mount", d.mount.clone())
                .nth(i, count);
            let top = row![
                theme
                    .text(
                        &r.child("name"),
                        format!("{}  ({}, {})", d.mount, d.name, d.fs)
                    )
                    .width(Length::Fill),
                theme.text(
                    &r.child("text"),
                    format!(
                        "{} / {}   ↓ {}  ↑ {}",
                        format::bytes(d.used),
                        format::bytes(d.total),
                        format::rate(d.read_bps),
                        format::rate(d.write_bps)
                    )
                ),
            ]
            .width(Length::Fill);
            theme
                .container(
                    &r,
                    column![
                        top,
                        graph::meter(
                            theme,
                            &r.child("meter")
                                .class_if("warning", (WARNING..CRITICAL).contains(&used))
                                .class_if("critical", used >= CRITICAL),
                            used / 100.0
                        )
                    ]
                    .spacing(2),
                )
                .width(Length::Fill)
                .into()
        })
        .collect();
    out.push(graph::graph(
        theme,
        &node.child("graph"),
        &[&h.disk_read, &h.disk_write],
        None,
        cap,
        graph::Unit::Rate,
    ));
    out
}

fn net_section<'a, M: 'a>(
    theme: &Theme,
    node: &Node,
    s: &Sample,
    sysmon: &'a SysMon,
) -> Vec<Element<'a, M>> {
    let cap = sysmon.config().history;
    let h = sysmon.history();
    let count = s.net.len();
    let mut out: Vec<Element<'a, M>> = s
        .net
        .iter()
        .enumerate()
        .map(|(i, iface)| {
            let r = node
                .child("iface")
                .attr("name", iface.name.clone())
                .nth(i, count);
            theme
                .container(
                    &r,
                    row![
                        theme
                            .text(&r.child("name"), iface.name.clone())
                            .width(Length::Fill),
                        theme.text(
                            &r.child("text"),
                            format!(
                                "↓ {}  ↑ {}   ({} / {} total)",
                                format::rate(iface.rx_bps),
                                format::rate(iface.tx_bps),
                                format::bytes(iface.rx_total),
                                format::bytes(iface.tx_total)
                            )
                        ),
                    ]
                    .width(Length::Fill),
                )
                .width(Length::Fill)
                .into()
        })
        .collect();
    out.push(graph::graph(
        theme,
        &node.child("graph"),
        &[&h.net_rx, &h.net_tx],
        None,
        cap,
        graph::Unit::Rate,
    ));
    out
}

fn gpu_section<'a, M: 'a>(
    theme: &Theme,
    locale: &Locale,
    node: &Node,
    s: &Sample,
    sysmon: &'a SysMon,
) -> Vec<Element<'a, M>> {
    let cap = sysmon.config().history;
    let h = sysmon.history();
    let count = s.gpus.len();
    let mut out: Vec<Element<'a, M>> = s
        .gpus
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let r = node.child("gpu").nth(i, count);
            let busy = g.busy.unwrap_or(0.0);
            let mut info = Vec::new();
            if let (Some(u), Some(t)) = (g.vram_used, g.vram_total) {
                info.push(locale.fmt(
                    "sysmon.gpu.vram",
                    &[("used", &format::bytes(u)), ("total", &format::bytes(t))],
                ));
            }
            if let Some(t) = g.temp_c {
                info.push(format!("{t:.0}°C"));
            }
            theme
                .container(
                    &r,
                    column![
                        row![
                            theme
                                .text(&r.child("name"), g.name.clone())
                                .width(Length::Fill),
                            theme.text(&r.child("text"), info.join("   ")),
                        ]
                        .width(Length::Fill),
                        graph::meter(
                            theme,
                            &r.child("meter")
                                .class_if("warning", (WARNING..CRITICAL).contains(&busy))
                                .class_if("critical", busy >= CRITICAL),
                            busy / 100.0
                        ),
                    ]
                    .spacing(2),
                )
                .width(Length::Fill)
                .into()
        })
        .collect();
    out.push(graph::graph(
        theme,
        &node.child("graph"),
        &[&h.gpu],
        Some(100.0),
        cap,
        graph::Unit::Percent,
    ));
    out
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
        assert_eq!(g.level(Some(90.0), true), Some("critical"));
        // A rate: nothing unless configured.
        assert_eq!(g.level(Some(1e9), false), None);
        g.config.warning = Some(1000.0);
        assert_eq!(g.level(Some(1e9), false), Some("warning"));
        g.config.critical = Some(1e6);
        assert_eq!(g.level(Some(1e9), false), Some("critical"));
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
