//! Network gadget: the primary connection's icon on the bar (Wi‑Fi
//! strength, wired, connecting, no internet, Wi‑Fi off, offline; a VPN
//! badge; the name with `show_label`); a left click opens the popup:
//! the Wi‑Fi networks around (a click joins a known or open one, asks
//! the password of a secured one in place, unfolds the details of the
//! active one with Disconnect / Forget), the wired devices, the VPN
//! profiles with a toggle each, a Settings button. Right click:
//! `settings_command`; middle click: Wi‑Fi on/off.
//!
//! Holds no network state: what's around comes from `ctx.network`; what
//! the user does goes back as `Action::Network`. Its own state is the
//! popup's: which row is unfolded, the password being typed.

use iced::widget::{mouse_area, operation};
use iced::{Subscription, Task, keyboard, widget};
use iced_wayland_subscriber::OutputInfo;
use icssed::Element;

use crate::config::{RawSection, Section};
use crate::gadgets::{Action, Context, Gadget, Popup};
use crate::locale::Locale;
use crate::process;
use crate::services::network::{
    AccessPoint, ActiveState, Command, Device, DeviceKind, DeviceState, FailKey, FailReason,
    ICON_OFFLINE, ICON_WIFI_ACQUIRING, ICON_WIFI_EXCELLENT, ICON_WIFI_GOOD, ICON_WIFI_NO_ROUTE,
    ICON_WIFI_NONE, ICON_WIFI_OFF, ICON_WIFI_OFFLINE, ICON_WIFI_OK, ICON_WIFI_WEAK, ICON_WIRED,
    ICON_WIRED_ACQUIRING, ICON_WIRED_NO_ROUTE, ICON_WIRED_OFFLINE, IpConfig, Profile, Security,
    strength_icon,
};

/// Seconds between scans while the popup is open.
const SCAN_STEP: u32 = 10;

const ICON_VPN: &str = "network-vpn-symbolic";
const ICON_SECURED: &str = "channel-secure-symbolic";
const ICON_SCAN: &str = "view-refresh-symbolic";
const ICON_PEEK: &str = "view-reveal-symbolic";

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// A program (the network settings) run by the popup's button and a
    /// right click; none when empty.
    pub settings_command: String,
    /// The connection's name after the icon on the bar.
    pub show_label: bool,
    /// The badge on the bar while a VPN is up.
    pub show_vpn: bool,
}

impl Section for NetworkConfig {
    const NAME: &'static str = "Network";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            settings_command: raw.str_or("settings_command", ""),
            show_label: raw.bool_or("show_label", false),
            show_vpn: raw.bool_or("show_vpn", true),
        }
    }
}

/// A row of the popup that can be unfolded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A Wi‑Fi network, by SSID.
    Ap(String),
    /// A device, by path.
    Device(String),
}

pub struct NetworkGadget {
    config: NetworkConfig,
    popup: Popup,
    expanded: Option<Row>,
    password: String,
    peek: bool,
    input: widget::Id,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    RunSettings,
    /// The toggle in the header.
    SetWireless(bool),
    /// A middle click on the bar.
    FlipWireless,
    Scan,
    Tick,
    Expand(Row),
    Collapse,
    /// Join a network (a known or open one).
    Connect(String),
    /// Bring a wired device up.
    ConnectDevice(String),
    Password(String),
    Peek,
    /// The password typed for the unfolded network.
    Submit,
    /// Put the keyboard in the password field (once it exists: the
    /// message after the one that unfolded it).
    Focus,
    Disconnect(String),
    Forget(String),
    /// A VPN's toggle.
    Vpn(String, bool),
}

impl Gadget for NetworkGadget {
    type Config = NetworkConfig;
    type Message = Message;

    fn new(config: NetworkConfig, _output: &OutputInfo) -> Self {
        Self {
            config,
            popup: Popup::new(),
            expanded: None,
            password: String::new(),
            peek: false,
            input: widget::Id::unique(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::TogglePopup => {
                let opening = !self.popup.is_open();
                let toggle = self.popup.toggle();
                if opening {
                    Action::Many(vec![toggle, Action::Network(Command::Scan)])
                } else {
                    toggle
                }
            }
            Message::RunSettings => {
                if !self.config.settings_command.is_empty() {
                    process::run(&self.config.settings_command);
                }
                self.popup.close()
            }
            Message::SetWireless(on) => Action::Network(Command::SetWireless(on)),
            Message::FlipWireless => Action::Network(Command::ToggleWireless),
            Message::Scan | Message::Tick => Action::Network(Command::Scan),
            Message::Expand(row) => {
                self.expanded = Some(row);
                self.password.clear();
                self.peek = false;
                Action::Run(Task::done(Message::Focus))
            }
            Message::Focus => Action::Run(operation::focus(self.input.clone())),
            Message::Collapse => {
                self.collapse();
                Action::None
            }
            Message::Connect(ssid) => {
                self.collapse();
                Action::Network(Command::Connect { ssid })
            }
            Message::ConnectDevice(device) => Action::Network(Command::ConnectDevice { device }),
            Message::Password(p) => {
                self.password = p;
                Action::None
            }
            Message::Peek => {
                self.peek = !self.peek;
                Action::Run(operation::focus(self.input.clone()))
            }
            Message::Submit => match (&self.expanded, self.password.is_empty()) {
                (Some(Row::Ap(ssid)), false) => {
                    let command = Command::ConnectWithPassword {
                        ssid: ssid.clone(),
                        password: std::mem::take(&mut self.password),
                    };
                    Action::Network(command)
                }
                _ => Action::None,
            },
            Message::Disconnect(device) => {
                self.collapse();
                Action::Network(Command::Disconnect { device })
            }
            Message::Forget(uuid) => {
                self.collapse();
                Action::Network(Command::Forget { uuid })
            }
            Message::Vpn(uuid, on) => Action::Network(if on {
                Command::Activate { uuid }
            } else {
                Command::Deactivate { uuid }
            }),
        }
    }

    fn icon_names(&self) -> Vec<String> {
        [
            ICON_WIFI_NONE,
            ICON_WIFI_WEAK,
            ICON_WIFI_OK,
            ICON_WIFI_GOOD,
            ICON_WIFI_EXCELLENT,
            ICON_WIFI_ACQUIRING,
            ICON_WIFI_NO_ROUTE,
            ICON_WIFI_OFF,
            ICON_WIFI_OFFLINE,
            ICON_WIRED,
            ICON_WIRED_ACQUIRING,
            ICON_WIRED_NO_ROUTE,
            ICON_WIRED_OFFLINE,
            ICON_OFFLINE,
            ICON_VPN,
            ICON_SECURED,
            ICON_SCAN,
            ICON_PEEK,
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let summary = ctx.network.summary();
        let running = ctx.network.running();
        let wireless = ctx.network.wireless_enabled();
        let mut parts: Vec<Element<'a, Message>> =
            vec![ctx.icon(summary.icon_name(running, wireless)).into()];
        if summary.vpn && self.config.show_vpn {
            parts.push(ctx.icon(ICON_VPN).class("vpn").into());
        }
        if self.config.show_label && !summary.label.is_empty() {
            parts.push(icssed::text(summary.label.clone()).into());
        }
        let kind = summary.kind.filter(|_| running);
        let button = icssed::button(parts)
            .kind("indicator")
            .class_if("wifi", kind == Some(DeviceKind::Wifi))
            .class_if("wired", kind == Some(DeviceKind::Wired))
            .class(if summary.connected {
                "connected"
            } else if summary.connecting {
                "connecting"
            } else {
                "disconnected"
            })
            .class_if("warning", summary.connected && summary.limited)
            .class_if("muted", !summary.connected && !summary.connecting)
            .on_press(Message::TogglePopup)
            .id(self.popup.anchor());
        let button = mouse_area(button)
            .on_middle_press(Message::FlipWireless)
            .on_right_press(Message::RunSettings);
        icssed::row![button].into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        icssed::scroll(icssed::column(self.rows(ctx)).kind("list"))
            .class("flat")
            .into()
    }

    fn popup_closed(&mut self) {
        self.collapse();
    }

    fn popup_key(&mut self, event: keyboard::Event) -> Action<Message> {
        use keyboard::key::{Key, Named};
        let keyboard::Event::KeyPressed {
            key,
            text,
            modifiers,
            ..
        } = event
        else {
            return Action::None;
        };
        if !matches!(self.expanded, Some(Row::Ap(_))) {
            return Action::None;
        }
        match key {
            Key::Named(Named::Enter) => self.update(Message::Submit),
            Key::Named(Named::Escape) => self.update(Message::Collapse),
            Key::Named(Named::Backspace) => {
                self.password.pop();
                Action::None
            }
            _ => {
                if let Some(text) = text
                    && !modifiers.command()
                    && !modifiers.control()
                {
                    self.password
                        .extend(text.chars().filter(|c| !c.is_control()));
                }
                Action::None
            }
        }
    }

    fn popup_keyboard(&self) -> bool {
        // Any row may unfold into the password field; the keyboard has
        // to be ours before the popup maps (the compositor won't move
        // it to a popup afterwards).
        true
    }

    fn subscription(&self) -> Subscription<Message> {
        if !self.popup.is_open() {
            return Subscription::none();
        }
        // From the opening, not wall-clock aligned: a scan was just
        // asked, the next comes a full step later.
        Subscription::run_with(SCAN_STEP, |step| {
            let step = std::time::Duration::from_secs(u64::from(*step));
            iced::futures::stream::unfold((), move |()| async move {
                tokio::time::sleep(step).await;
                Some(((), ()))
            })
        })
        .map(|()| Message::Tick)
    }
}

/// Mb/s, Gb/s.
fn speed(locale: &Locale, mbps: u32) -> String {
    if mbps >= 1000 {
        let g = mbps as f32 / 1000.0;
        if g.fract() == 0.0 {
            format!("{} Gb/s", g as u32)
        } else {
            format!("{} Gb/s", locale.decimal(g.into(), 1))
        }
    } else {
        format!("{mbps} Mb/s")
    }
}

impl NetworkGadget {
    fn collapse(&mut self) {
        self.expanded = None;
        self.password.clear();
        self.peek = false;
    }

    /// Every row of the popup.
    fn rows<'a>(&'a self, ctx: Context<'a>) -> Vec<Element<'a, Message>> {
        let net = ctx.network;
        let mut rows: Vec<Element<'a, Message>> = Vec::new();
        let empty = |text: &'a str| super::note(text).kind("empty").into();
        if !net.running() {
            rows.push(empty(ctx.locale.tr("network.not_running")));
        } else {
            let wifi_devices: Vec<&Device> = net.devices_of(DeviceKind::Wifi).collect();
            if !wifi_devices.is_empty() {
                rows.push(wifi_header(ctx));
                if !net.wireless_enabled() {
                    rows.push(empty(ctx.locale.tr("network.wifi_off")));
                } else if net.access_points().is_empty() {
                    rows.push(empty(ctx.locale.tr("network.no_networks")));
                } else {
                    for ap in net.access_points() {
                        rows.push(self.ap_row(ctx, ap));
                    }
                }
            }
            let wired: Vec<&Device> = net.devices_of(DeviceKind::Wired).collect();
            if !wired.is_empty() {
                rows.push(header("wired", ctx.locale.tr("network.wired")));
                for d in wired {
                    rows.push(self.device_row(ctx, d));
                }
            }
            let vpns: Vec<&Profile> = net.vpns().collect();
            if !vpns.is_empty() {
                rows.push(header("vpn", ctx.locale.tr("network.vpn")));
                for p in vpns {
                    rows.push(vpn_row(ctx, p));
                }
            }
            if rows.is_empty() {
                rows.push(empty(ctx.locale.tr("network.no_devices")));
            }
        }
        if !self.config.settings_command.is_empty() {
            rows.push(
                icssed::button![icssed::text(ctx.locale.tr("network.settings"))]
                    .class("settings")
                    .on_press(Message::RunSettings)
                    .into(),
            );
        }
        rows
    }

    /// A Wi‑Fi network: the strength icon, the name, the lock, the
    /// status; unfolded, its details or the password field below.
    fn ap_row<'a>(&'a self, ctx: Context<'a>, ap: &'a AccessPoint) -> Element<'a, Message> {
        let locale = ctx.locale;
        let key = FailKey::Ssid(ap.ssid.clone());
        let failure = ctx.network.failure(&key);
        let attempting = ctx.network.attempting(&key);
        let expanded = self.expanded == Some(Row::Ap(ap.ssid.clone()));
        let status = if ap.active {
            Some(locale.tr("network.connected"))
        } else if ap.connecting || attempting {
            Some(locale.tr("network.connecting"))
        } else {
            None
        };
        let wrong_password = matches!(failure.map(|f| &f.reason), Some(FailReason::WrongPassword));
        let fold = if expanded {
            Message::Collapse
        } else {
            Message::Expand(Row::Ap(ap.ssid.clone()))
        };
        // What a click on the row does: fold/unfold the active one, join
        // a known or open one, ask the password (or say it can't)
        // otherwise. The chevron unfolds the active and the known ones
        // (Disconnect / Forget live there).
        let on_press = if expanded || ap.active || ap.security == Security::Enterprise {
            fold.clone()
        } else if (ap.known.is_some() || !ap.security.secured()) && !wrong_password {
            Message::Connect(ap.ssid.clone())
        } else {
            fold.clone()
        };
        let mut parts: Vec<Element<'a, Message>> = vec![row_button(
            ctx,
            RowLine {
                icon: strength_icon(ap.strength),
                name: &ap.ssid,
                badge: ap.security.secured().then_some(ICON_SECURED),
                status,
                chevron: (expanded || ap.active || ap.known.is_some()).then_some((expanded, fold)),
            },
            on_press,
        )];
        if expanded {
            if ap.active {
                let device = ctx.network.devices().iter().find(|d| d.path == ap.device);
                let ip4 = device.and_then(|d| d.ip4.as_ref());
                let ip6 = device.and_then(|d| d.ip6.as_ref());
                let mut lines = ip_lines(locale, ip4, ip6);
                let mut extra = vec![
                    ap.band().to_owned(),
                    speed(locale, ap.max_bitrate / 1000),
                    ap.security.label().to_owned(),
                ];
                if let Some(d) = device {
                    extra.push(d.iface.clone());
                }
                lines.push(extra.join(" · "));
                parts.push(details(lines));
                let mut actions: Vec<(&'static str, &'a str, Message)> = Vec::new();
                if let Some(d) = device {
                    actions.push((
                        "disconnect",
                        locale.tr("network.disconnect"),
                        Message::Disconnect(d.path.clone()),
                    ));
                }
                if let Some(uuid) = &ap.known {
                    actions.push((
                        "forget",
                        locale.tr("network.forget"),
                        Message::Forget(uuid.clone()),
                    ));
                }
                parts.push(actions_row(actions));
            } else if ap.security == Security::Enterprise {
                parts.push(
                    super::note(locale.tr("network.enterprise"))
                        .kind("message")
                        .into(),
                );
            } else {
                // A known one whose key was refused asks it again; the
                // rest of the known ones offer to join or to be
                // forgotten; an unknown one asks its key.
                let mut actions: Vec<(&'static str, &'a str, Message)> = Vec::new();
                if ap.known.is_none() || wrong_password {
                    parts.push(self.auth(ctx, attempting));
                } else {
                    actions.push((
                        "connect",
                        locale.tr("network.connect"),
                        Message::Connect(ap.ssid.clone()),
                    ));
                }
                if let Some(uuid) = &ap.known {
                    actions.push((
                        "forget",
                        locale.tr("network.forget"),
                        Message::Forget(uuid.clone()),
                    ));
                }
                if !actions.is_empty() {
                    parts.push(actions_row(actions));
                }
            }
        }
        if let Some(f) = failure {
            let text = match &f.reason {
                FailReason::WrongPassword => locale.tr("network.wrong_password"),
                FailReason::NoSecrets => locale.tr("network.needs_password"),
                FailReason::Refused(_) | FailReason::Other => locale.tr("network.failed"),
            };
            parts.push(super::note(text).kind("message").into());
        }
        icssed::column(parts)
            .kind("ap")
            .class_if("active", ap.active)
            .class_if("connecting", ap.connecting || attempting)
            .class_if("known", ap.known.is_some())
            .class_if("secured", ap.security.secured())
            .class_if("enterprise", ap.security == Security::Enterprise)
            .class_if("expanded", expanded)
            .class_if("failed", failure.is_some())
            .attr("ssid", &ap.ssid)
            .into()
    }

    /// A wired device: plugged and connected (unfold: the details and
    /// Disconnect), unplugged, or down (a click brings it up).
    fn device_row<'a>(&'a self, ctx: Context<'a>, d: &'a Device) -> Element<'a, Message> {
        let locale = ctx.locale;
        let active = d.state == DeviceState::Activated;
        let expanded = self.expanded == Some(Row::Device(d.path.clone()));
        let status = if !d.carrier {
            locale.tr("network.unplugged")
        } else if active {
            locale.tr("network.connected")
        } else if d.state.is_connecting() {
            locale.tr("network.connecting")
        } else {
            locale.tr("network.not_connected")
        };
        let fold = if expanded {
            Message::Collapse
        } else {
            Message::Expand(Row::Device(d.path.clone()))
        };
        let on_press = if expanded || active {
            Some(fold.clone())
        } else if d.carrier && d.state == DeviceState::Disconnected {
            Some(Message::ConnectDevice(d.path.clone()))
        } else {
            None
        };
        let connection = ctx.network.active_of(d);
        let name = connection.map_or(d.iface.as_str(), |a| a.id.as_str());
        let line = RowLine {
            icon: if active {
                ICON_WIRED
            } else {
                ICON_WIRED_OFFLINE
            },
            name,
            badge: None,
            status: Some(status),
            chevron: (expanded || active).then_some((expanded, fold)),
        };
        let mut parts: Vec<Element<'a, Message>> =
            vec![row_button(ctx, line, on_press.unwrap_or(Message::Collapse))];
        if expanded && active {
            let mut lines = ip_lines(locale, d.ip4.as_ref(), d.ip6.as_ref());
            let mut extra = Vec::new();
            if d.speed > 0 {
                extra.push(speed(locale, d.speed));
            }
            extra.push(d.iface.clone());
            if !d.hw_address.is_empty() {
                extra.push(d.hw_address.clone());
            }
            lines.push(extra.join(" · "));
            parts.push(details(lines));
            parts.push(actions_row(vec![(
                "disconnect",
                locale.tr("network.disconnect"),
                Message::Disconnect(d.path.clone()),
            )]));
        }
        icssed::column(parts)
            .kind("device")
            .class("wired")
            .class_if("active", active)
            .class_if("connecting", d.state.is_connecting())
            .class_if("unplugged", !d.carrier)
            .class_if("expanded", expanded)
            .attr("name", &d.iface)
            .into()
    }

    /// The password field with its eye and the Connect button.
    fn auth<'a>(&'a self, ctx: Context<'a>, busy: bool) -> Element<'a, Message> {
        let locale = ctx.locale;
        let eye = icssed::toggle(self.peek)
            .on_toggle(|_| Message::Peek)
            .class("peek")
            .icon(ctx.named(ICON_PEEK));
        let field = icssed::input(locale.tr("network.password"), &self.password)
            .disabled_if(busy)
            .id(self.input.clone())
            .secure(!self.peek)
            .on_input(Message::Password)
            .on_submit(Message::Submit)
            .trailing(eye);
        let ready = !busy && !self.password.is_empty();
        let connect = icssed::button![icssed::text(locale.tr("network.connect"))]
            .class("connect")
            .disabled_if(!ready)
            .on_press(Message::Submit);
        icssed::row![field, connect]
            .kind("auth")
            .class_if("busy", busy)
            .into()
    }
}

/// What a network's or a device's line shows: icon, name, an optional
/// badge icon, a status text, a chevron on rows that unfold
/// (`(unfolded, what its click does)`).
struct RowLine<'a> {
    icon: &'a str,
    name: &'a str,
    badge: Option<&'a str>,
    status: Option<&'a str>,
    chevron: Option<(bool, Message)>,
}

/// The clickable line of a network or device row, a `button.flat`: the
/// name and its badge in a `title` taking the room left, so the status
/// and the chevron sit at the end; the chevron is a button of its own
/// inside it, which then doesn't fire.
fn row_button<'a>(ctx: Context<'a>, line: RowLine<'a>, on_press: Message) -> Element<'a, Message> {
    let mut name: Vec<Element<'a, Message>> = vec![icssed::text(line.name).kind("name").into()];
    if let Some(badge) = line.badge {
        name.push(ctx.icon(badge).class("badge").into());
    }
    let mut parts: Vec<Element<'a, Message>> = vec![
        ctx.icon(line.icon).into(),
        icssed::row(name).kind("title").into(),
    ];
    if let Some(status) = line.status {
        parts.push(icssed::text(status).kind("status").into());
    }
    if let Some((expanded, toggle)) = line.chevron {
        parts.push(
            icssed::button![icssed::mark().kind("arrow")]
                .class("chevron")
                .class("flat")
                .class_if("active", expanded)
                .on_press(toggle)
                .into(),
        );
    }
    icssed::button(parts)
        .class("flat")
        .on_press(on_press)
        .into()
}

/// A section title.
fn header<'a>(class: &str, title: &'a str) -> Element<'a, Message> {
    icssed::row![icssed::text(title).kind("title")]
        .kind("header")
        .class(class)
        .into()
}

/// The Wi‑Fi title with the scan button and the on/off toggle.
fn wifi_header<'a>(ctx: Context<'a>) -> Element<'a, Message> {
    let on = ctx.network.wireless_enabled();
    let mut parts: Vec<Element<'a, Message>> = vec![
        icssed::text(ctx.locale.tr("network.wifi"))
            .kind("title")
            .into(),
    ];
    if on {
        parts.push(
            icssed::button![ctx.icon(ICON_SCAN)]
                .class("scan")
                .on_press(Message::Scan)
                .into(),
        );
    }
    parts.push(
        icssed::toggle(on)
            .on_toggle(Message::SetWireless)
            .switch()
            .into(),
    );
    icssed::row(parts)
        .kind("header")
        .class("wifi")
        .class_if("off", !on)
        .into()
}

/// A VPN profile with its toggle.
fn vpn_row<'a>(ctx: Context<'a>, p: &'a Profile) -> Element<'a, Message> {
    let locale = ctx.locale;
    let key = FailKey::Uuid(p.uuid.clone());
    let active = ctx.network.active_by_uuid(&p.uuid);
    let on = active.is_some_and(|a| a.state == ActiveState::Activated);
    let connecting =
        ctx.network.attempting(&key) || active.is_some_and(|a| a.state == ActiveState::Activating);
    let failure = ctx.network.failure(&key);
    let uuid = p.uuid.clone();
    let mut head: Vec<Element<'a, Message>> = vec![
        ctx.icon(ICON_VPN).into(),
        icssed::text(p.id.as_str()).kind("name").into(),
    ];
    if connecting {
        head.push(
            icssed::text(locale.tr("network.connecting"))
                .kind("status")
                .into(),
        );
    }
    head.push(
        icssed::toggle(on || connecting)
            .on_toggle(move |v| Message::Vpn(uuid.clone(), v))
            .switch()
            .into(),
    );
    let mut parts: Vec<Element<'a, Message>> = vec![icssed::row(head).kind("line").into()];
    if let Some(f) = failure {
        let text = match &f.reason {
            FailReason::NoSecrets => locale.tr("network.needs_password"),
            FailReason::WrongPassword => locale.tr("network.wrong_password"),
            _ => locale.tr("network.failed"),
        };
        parts.push(super::note(text).kind("message").into());
    }
    icssed::column(parts)
        .kind("vpn")
        .class_if("active", on)
        .class_if("connecting", connecting)
        .class_if("failed", failure.is_some())
        .attr("name", &p.id)
        .into()
}

/// The facts about a connection, one line each.
fn details<'a>(lines: Vec<String>) -> Element<'a, Message> {
    let lines = lines
        .into_iter()
        .map(|l| icssed::text(l).kind("line").into());
    icssed::column(lines).kind("details").into()
}

/// A row of text buttons (Disconnect, Forget).
fn actions_row<'a>(actions: Vec<(&'static str, &'a str, Message)>) -> Element<'a, Message> {
    let buttons = actions.into_iter().map(|(class, label, message)| {
        icssed::button![icssed::text(label)]
            .class(class)
            .on_press(message)
            .into()
    });
    icssed::row(buttons).kind("actions").into()
}

/// The IP lines of the details: addresses, gateway, DNS.
fn ip_lines(locale: &Locale, ip4: Option<&IpConfig>, ip6: Option<&IpConfig>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(ip) = ip4 {
        if !ip.addresses.is_empty() {
            lines.push(format!("IPv4 {}", ip.addresses.join(", ")));
        }
        if !ip.gateway.is_empty() {
            lines.push(format!("{} {}", locale.tr("network.gateway"), ip.gateway));
        }
        if !ip.dns.is_empty() {
            lines.push(format!("DNS {}", ip.dns.join(", ")));
        }
    }
    if let Some(ip) = ip6
        && !ip.addresses.is_empty()
    {
        lines.push(format!("IPv6 {}", ip.addresses.join(", ")));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let raw = RawSection::default();
        let c = NetworkConfig::from_raw(&raw);
        assert_eq!(c.settings_command, "");
        assert!(!c.show_label);
        assert!(c.show_vpn);
    }

    #[test]
    fn speeds() {
        let en = Locale::new("en");
        assert_eq!(speed(&en, 100), "100 Mb/s");
        assert_eq!(speed(&en, 1000), "1 Gb/s");
        assert_eq!(speed(&en, 2500), "2.5 Gb/s");
        assert_eq!(speed(&Locale::new("it"), 2500), "2,5 Gb/s");
    }
}
