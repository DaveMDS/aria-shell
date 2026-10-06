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

use iced::widget::{Space, column, mouse_area, operation, row, scrollable, text};
use iced::{Alignment, Element, Length, Subscription, Task, keyboard, widget};
use iced_wayland_subscriber::OutputInfo;

use crate::config::{RawSection, Section};
use crate::gadgets::{Action, Context, Gadget, Popup};
use crate::locale::Locale;
use crate::process;
use crate::services::network::{
    AccessPoint, Command, Device, DeviceKind, DeviceState, FailKey, FailReason, ICON_OFFLINE,
    ICON_WIFI_ACQUIRING, ICON_WIFI_EXCELLENT, ICON_WIFI_GOOD, ICON_WIFI_NO_ROUTE, ICON_WIFI_NONE,
    ICON_WIFI_OFF, ICON_WIFI_OFFLINE, ICON_WIFI_OK, ICON_WIFI_WEAK, ICON_WIRED,
    ICON_WIRED_ACQUIRING, ICON_WIRED_NO_ROUTE, ICON_WIRED_OFFLINE, IpConfig, Profile, Security,
    strength_icon,
};
use crate::ui::theme::{self, Node};

/// Icon size when the theme doesn't set `height` on an `icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;
/// Popup width and height cap when the theme doesn't size `list`.
const DEFAULT_LIST_WIDTH: f32 = 380.0;
const DEFAULT_LIST_HEIGHT: f32 = 600.0;
/// Seconds between scans while the popup is open.
const SCAN_STEP: u32 = 10;

const ICON_VPN: &str = "network-vpn-symbolic";
const ICON_SECURED: &str = "channel-secure-symbolic";
const ICON_SCAN: &str = "view-refresh-symbolic";
const ICON_PEEK: &str = "view-reveal-symbolic";
const ICON_CONCEAL: &str = "view-conceal-symbolic";

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
            ICON_CONCEAL,
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let summary = ctx.network.summary();
        let running = ctx.network.running();
        let wireless = ctx.network.wireless_enabled();
        let button = ctx
            .node
            .child("button")
            .class(match summary.kind {
                _ if !running => "none",
                Some(DeviceKind::Wifi) => "wifi",
                Some(DeviceKind::Wired) => "wired",
                None => "none",
            })
            .class(if summary.connected {
                "connected"
            } else if summary.connecting {
                "connecting"
            } else {
                "disconnected"
            })
            .class_if("limited", summary.limited)
            .class_if("vpn", summary.vpn && self.config.show_vpn)
            .class_if("off", running && !wireless);
        let name = summary.icon_name(running, wireless);
        let mut parts: Vec<Element<'a, Message>> =
            vec![self.icon(&ctx, &button.child("icon"), name)];
        if summary.vpn && self.config.show_vpn {
            parts.push(self.icon(&ctx, &button.child("icon").class("vpn"), ICON_VPN));
        }
        if self.config.show_label && !summary.label.is_empty() {
            let t = button.child("text");
            parts.push(
                theme
                    .container(&t, theme.text(&t, summary.label.clone()))
                    .into(),
            );
        }
        let content = row(parts)
            .spacing(theme.resolve(&button).gap)
            .align_y(Alignment::Center);
        let button = theme
            .button(&button, content)
            .on_press(Message::TogglePopup);
        mouse_area(self.popup.anchor(button))
            .on_middle_press(Message::FlipWireless)
            .on_right_press(Message::RunSettings)
            .into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let list = ctx.node.child("list");
        let (rows, capped) = self.rows(&ctx);
        let rows: Vec<Element<'a, Message>> = rows.into_iter().map(|(e, _)| e).collect();
        let content = theme.column(&list, rows).width(Length::Fill);
        if capped {
            scrollable(content).height(Length::Fill).into()
        } else {
            content.into()
        }
    }

    fn popup_size(&self, ctx: Context<'_>) -> (u32, u32) {
        let width = self.list_width(&ctx);
        let (height, _) = self.list_height(&ctx);
        (width.ceil().max(1.0) as u32, height.ceil().max(1.0) as u32)
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

/// A row's height: its content over the node's padding.
fn padded(theme: &theme::Theme, node: &Node, content: f32) -> f32 {
    let s = theme.resolve(node);
    content + s.padding.top + s.padding.bottom
}

/// A themed row of the popup and its height, built together so the
/// popup's size and its content can't disagree.
type Block<'a> = (Element<'a, Message>, f32);

impl NetworkGadget {
    fn collapse(&mut self) {
        self.expanded = None;
        self.password.clear();
        self.peek = false;
    }

    /// A themed icon by name, at the node's `height` (its `width` if
    /// there's no height), or a blank of that size.
    fn icon<'a>(&'a self, ctx: &Context<'a>, node: &Node, name: &str) -> Element<'a, Message> {
        let style = ctx.theme.resolve(node);
        let size = px(style.height.or(style.width)).unwrap_or(DEFAULT_ICON_SIZE);
        let icon = match ctx.icons.get_name(name, None) {
            Some(icon) => icon.view(size, style.color),
            None => Space::new().width(size).height(size).into(),
        };
        ctx.theme.container(node, icon).into()
    }

    fn icon_size(&self, ctx: &Context<'_>, node: &Node) -> f32 {
        let style = ctx.theme.resolve(node);
        px(style.height.or(style.width)).unwrap_or(DEFAULT_ICON_SIZE)
    }

    /// An icon's height, its padding included.
    fn icon_height(&self, ctx: &Context<'_>, node: &Node) -> f32 {
        padded(ctx.theme, node, self.icon_size(ctx, node))
    }

    /// A button with an icon: its height is the icon's plus paddings.
    fn icon_button_height(&self, ctx: &Context<'_>, node: &Node) -> f32 {
        padded(ctx.theme, node, self.icon_height(ctx, &node.child("icon")))
    }

    /// A button with a text label.
    fn text_button_height(&self, ctx: &Context<'_>, node: &Node) -> f32 {
        let t = node.child("text");
        padded(
            ctx.theme,
            node,
            padded(ctx.theme, &t, ctx.theme.line_height(&t)),
        )
    }

    fn list_width(&self, ctx: &Context<'_>) -> f32 {
        px(ctx.theme.resolve(&ctx.node.child("list")).width).unwrap_or(DEFAULT_LIST_WIDTH)
    }

    /// The popup height: the rows with the gaps, within the list's
    /// padding, capped by its `height`; whether it was capped (then
    /// the content scrolls).
    fn list_height(&self, ctx: &Context<'_>) -> (f32, bool) {
        let (rows, capped) = self.rows(ctx);
        let list = ctx.node.child("list");
        let s = ctx.theme.resolve(&list);
        let height = rows.iter().map(|(_, h)| h).sum::<f32>()
            + s.gap * rows.len().saturating_sub(1) as f32
            + s.padding.top
            + s.padding.bottom;
        let cap = px(s.height).unwrap_or(DEFAULT_LIST_HEIGHT);
        (height.min(cap), capped)
    }

    /// Every row of the popup with its height; whether the whole is
    /// taller than the list's cap.
    fn rows<'a>(&'a self, ctx: &Context<'a>) -> (Vec<Block<'a>>, bool) {
        let theme = ctx.theme;
        let list = ctx.node.child("list");
        let net = ctx.network;
        let mut rows: Vec<Block<'a>> = Vec::new();
        if !net.running() {
            rows.push(self.empty(ctx, &list, ctx.locale.tr("network.not_running")));
        } else {
            let wifi_devices: Vec<&Device> = net.devices_of(DeviceKind::Wifi).collect();
            if !wifi_devices.is_empty() {
                rows.push(self.wifi_header(ctx, &list));
                if !net.wireless_enabled() {
                    rows.push(self.empty(ctx, &list, ctx.locale.tr("network.wifi_off")));
                } else if net.access_points().is_empty() {
                    rows.push(self.empty(ctx, &list, ctx.locale.tr("network.no_networks")));
                } else {
                    let count = net.access_points().len();
                    for (i, ap) in net.access_points().iter().enumerate() {
                        rows.push(self.ap_row(ctx, &list, ap, (i, count)));
                    }
                }
            }
            let wired: Vec<&Device> = net.devices_of(DeviceKind::Wired).collect();
            if !wired.is_empty() {
                rows.push(self.header(ctx, &list, "wired", ctx.locale.tr("network.wired")));
                for d in wired {
                    rows.push(self.device_row(ctx, &list, d));
                }
            }
            let vpns: Vec<&Profile> = net.vpns().collect();
            if !vpns.is_empty() {
                rows.push(self.header(ctx, &list, "vpn", ctx.locale.tr("network.vpn")));
                for p in vpns {
                    rows.push(self.vpn_row(ctx, &list, p));
                }
            }
            if rows.is_empty() {
                rows.push(self.empty(ctx, &list, ctx.locale.tr("network.no_devices")));
            }
        }
        if !self.config.settings_command.is_empty() {
            let b = list.child("button").class("settings");
            let label = iced::widget::container(
                theme.text(&b.child("text"), ctx.locale.tr("network.settings")),
            )
            .width(Length::Fill)
            .align_x(Alignment::Center);
            rows.push((
                theme
                    .button(&b, label)
                    .on_press(Message::RunSettings)
                    .width(Length::Fill)
                    .into(),
                self.text_button_height(ctx, &b),
            ));
        }
        let s = theme.resolve(&list);
        let total = rows.iter().map(|(_, h)| h).sum::<f32>()
            + s.gap * rows.len().saturating_sub(1) as f32
            + s.padding.top
            + s.padding.bottom;
        let capped = total > px(s.height).unwrap_or(DEFAULT_LIST_HEIGHT);
        (rows, capped)
    }

    /// A centred line of text: "Wi‑Fi is off", "No networks", ...
    fn empty<'a>(&'a self, ctx: &Context<'a>, list: &Node, text: &'a str) -> Block<'a> {
        let theme = ctx.theme;
        let empty = list.child("empty");
        let height = padded(
            theme,
            &empty,
            theme
                .measure(&empty, text)
                .height
                .max(theme.line_height(&empty)),
        );
        (
            theme
                .container(&empty, theme.text(&empty, text))
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .into(),
            height,
        )
    }

    /// A section title.
    fn header<'a>(
        &'a self,
        ctx: &Context<'a>,
        list: &Node,
        class: &'static str,
        title: &'a str,
    ) -> Block<'a> {
        let theme = ctx.theme;
        let header = list.child("header").class(class);
        let t = header.child("title");
        let content = theme
            .container(&t, theme.text(&t, title))
            .width(Length::Fill);
        let height = padded(theme, &t, theme.line_height(&t));
        (
            theme.container(&header, content).width(Length::Fill).into(),
            padded(theme, &header, height),
        )
    }

    /// The Wi‑Fi title with the scan button and the on/off toggle.
    fn wifi_header<'a>(&'a self, ctx: &Context<'a>, list: &Node) -> Block<'a> {
        let theme = ctx.theme;
        let on = ctx.network.wireless_enabled();
        let header = list.child("header").class("wifi").class_if("off", !on);
        let t = header.child("title");
        let scan = header.child("button").class("scan");
        let toggle = header.child("toggle");
        let mut parts: Vec<Element<'a, Message>> = vec![
            theme
                .container(&t, theme.text(&t, ctx.locale.tr("network.wifi")))
                .width(Length::Fill)
                .into(),
        ];
        let mut height = padded(theme, &t, theme.line_height(&t));
        if on {
            parts.push(
                theme
                    .button(&scan, self.icon(ctx, &scan.child("icon"), ICON_SCAN))
                    .on_press(Message::Scan)
                    .into(),
            );
            height = height.max(self.icon_button_height(ctx, &scan));
        }
        parts.push(theme.toggler(&toggle, on, Message::SetWireless).into());
        let ts = theme.resolve(&toggle);
        height = height.max(px(ts.height).unwrap_or(16.0));
        let content = row(parts)
            .spacing(theme.resolve(&header).gap)
            .align_y(Alignment::Center);
        (
            theme.container(&header, content).width(Length::Fill).into(),
            padded(theme, &header, height),
        )
    }

    /// A Wi‑Fi network: the strength icon, the name, the lock, the
    /// status; unfolded, its details or the password field below.
    fn ap_row<'a>(
        &'a self,
        ctx: &Context<'a>,
        list: &Node,
        ap: &'a AccessPoint,
        (i, count): (usize, usize),
    ) -> Block<'a> {
        let theme = ctx.theme;
        let locale = ctx.locale;
        let key = FailKey::Ssid(ap.ssid.clone());
        let failure = ctx.network.failure(&key);
        let attempting = ctx.network.attempting(&key);
        let expanded = self.expanded == Some(Row::Ap(ap.ssid.clone()));
        let node = list
            .child("ap")
            .nth(i, count)
            .class_if("active", ap.active)
            .class_if("connecting", ap.connecting || attempting)
            .class_if("known", ap.known.is_some())
            .class_if("secured", ap.security.secured())
            .class_if("enterprise", ap.security == Security::Enterprise)
            .class_if("expanded", expanded)
            .class_if("failed", failure.is_some())
            .attr("ssid", ap.ssid.clone());
        let button = node.child("button");
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
        let (head, head_height) = self.row_button(
            ctx,
            &button,
            strength_icon(ap.strength),
            &ap.ssid,
            ap.security.secured().then_some(ICON_SECURED),
            status,
            (expanded || ap.active || ap.known.is_some()).then_some((expanded, fold)),
            on_press,
        );
        let mut parts: Vec<Element<'a, Message>> = vec![head];
        let mut height = head_height;
        let s = theme.resolve(&node);
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
                let (details, h) = self.details(ctx, &node, lines);
                parts.push(details);
                height += s.gap + h;
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
                let (row, h) = self.actions(ctx, &node, actions);
                parts.push(row);
                height += s.gap + h;
            } else if ap.security == Security::Enterprise {
                let (m, h) = self.message(ctx, &node, locale.tr("network.enterprise"));
                parts.push(m);
                height += s.gap + h;
            } else {
                // A known one whose key was refused asks it again; the
                // rest of the known ones offer to join or to be
                // forgotten; an unknown one asks its key.
                let mut actions: Vec<(&'static str, &'a str, Message)> = Vec::new();
                if ap.known.is_none() || wrong_password {
                    let (auth, h) = self.auth(ctx, &node, attempting);
                    parts.push(auth);
                    height += s.gap + h;
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
                    let (row, h) = self.actions(ctx, &node, actions);
                    parts.push(row);
                    height += s.gap + h;
                }
            }
        }
        if let Some(f) = failure {
            let text = match &f.reason {
                FailReason::WrongPassword => locale.tr("network.wrong_password"),
                FailReason::NoSecrets => locale.tr("network.needs_password"),
                FailReason::Refused(_) | FailReason::Other => locale.tr("network.failed"),
            };
            let (m, h) = self.message(ctx, &node, text);
            parts.push(m);
            height += s.gap + h;
        }
        (
            theme
                .container(&node, column(parts).spacing(s.gap).width(Length::Fill))
                .width(Length::Fill)
                .into(),
            padded(theme, &node, height),
        )
    }

    /// A wired device: plugged and connected (unfold: the details and
    /// Disconnect), unplugged, or down (a click brings it up).
    fn device_row<'a>(&'a self, ctx: &Context<'a>, list: &Node, d: &'a Device) -> Block<'a> {
        let theme = ctx.theme;
        let locale = ctx.locale;
        let active = d.state == DeviceState::Activated;
        let expanded = self.expanded == Some(Row::Device(d.path.clone()));
        let node = list
            .child("device")
            .class("wired")
            .class_if("active", active)
            .class_if("connecting", d.state.is_connecting())
            .class_if("unplugged", !d.carrier)
            .class_if("expanded", expanded)
            .attr("name", d.iface.clone());
        let button = node.child("button");
        let status = if !d.carrier {
            locale.tr("network.unplugged")
        } else if active {
            locale.tr("network.connected")
        } else if d.state.is_connecting() {
            locale.tr("network.connecting")
        } else {
            locale.tr("network.not_connected")
        };
        let on_press = if expanded {
            Some(Message::Collapse)
        } else if active {
            Some(Message::Expand(Row::Device(d.path.clone())))
        } else if d.carrier && d.state == DeviceState::Disconnected {
            Some(Message::ConnectDevice(d.path.clone()))
        } else {
            None
        };
        let connection = ctx.network.active_of(d);
        let name = connection.map_or(d.iface.as_str(), |a| a.id.as_str());
        let (head, head_height) = self.row_button(
            ctx,
            &button,
            if active {
                ICON_WIRED
            } else {
                ICON_WIRED_OFFLINE
            },
            name,
            None,
            Some(status),
            (expanded || active).then_some((
                expanded,
                if expanded {
                    Message::Collapse
                } else {
                    Message::Expand(Row::Device(d.path.clone()))
                },
            )),
            on_press.unwrap_or(Message::Collapse),
        );
        let mut parts: Vec<Element<'a, Message>> = vec![head];
        let mut height = head_height;
        let s = theme.resolve(&node);
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
            let (details, h) = self.details(ctx, &node, lines);
            parts.push(details);
            height += s.gap + h;
            let (row, h) = self.actions(
                ctx,
                &node,
                vec![(
                    "disconnect",
                    locale.tr("network.disconnect"),
                    Message::Disconnect(d.path.clone()),
                )],
            );
            parts.push(row);
            height += s.gap + h;
        }
        (
            theme
                .container(&node, column(parts).spacing(s.gap).width(Length::Fill))
                .width(Length::Fill)
                .into(),
            padded(theme, &node, height),
        )
    }

    /// A VPN profile with its toggle.
    fn vpn_row<'a>(&'a self, ctx: &Context<'a>, list: &Node, p: &'a Profile) -> Block<'a> {
        let theme = ctx.theme;
        let locale = ctx.locale;
        let key = FailKey::Uuid(p.uuid.clone());
        let active = ctx.network.active_by_uuid(&p.uuid);
        let on =
            active.is_some_and(|a| a.state == crate::services::network::ActiveState::Activated);
        let connecting = ctx.network.attempting(&key)
            || active.is_some_and(|a| a.state == crate::services::network::ActiveState::Activating);
        let failure = ctx.network.failure(&key);
        let node = list
            .child("vpn")
            .class_if("active", on)
            .class_if("connecting", connecting)
            .class_if("failed", failure.is_some())
            .attr("name", p.id.clone());
        let name = node.child("name");
        let toggle = node.child("toggle");
        let uuid = p.uuid.clone();
        let mut head: Vec<Element<'a, Message>> = vec![
            self.icon(ctx, &node.child("icon"), ICON_VPN),
            theme
                .container(&name, theme.text(&name, &p.id))
                .width(Length::Fill)
                .into(),
        ];
        let mut height = self.icon_height(ctx, &node.child("icon")).max(padded(
            theme,
            &name,
            theme.line_height(&name),
        ));
        if connecting {
            let st = node.child("status");
            head.push(
                theme
                    .container(&st, theme.text(&st, locale.tr("network.connecting")))
                    .into(),
            );
        }
        head.push(
            theme
                .toggler(&toggle, on || connecting, move |v| {
                    Message::Vpn(uuid.clone(), v)
                })
                .into(),
        );
        height = height.max(px(theme.resolve(&toggle).height).unwrap_or(16.0));
        let s = theme.resolve(&node);
        let mut parts: Vec<Element<'a, Message>> = vec![
            row(head)
                .spacing(s.gap)
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .into(),
        ];
        if let Some(f) = failure {
            let text = match &f.reason {
                FailReason::NoSecrets => locale.tr("network.needs_password"),
                FailReason::WrongPassword => locale.tr("network.wrong_password"),
                _ => locale.tr("network.failed"),
            };
            let (m, h) = self.message(ctx, &node, text);
            parts.push(m);
            height += s.gap + h;
        }
        (
            theme
                .container(&node, column(parts).spacing(s.gap).width(Length::Fill))
                .width(Length::Fill)
                .into(),
            padded(theme, &node, height),
        )
    }

    /// The clickable line of a network or device row: icon, name, an
    /// optional badge icon, a status text, a chevron on rows that
    /// unfold (`Some((unfolded, what its click does))`: a button of
    /// its own inside the row's, which then doesn't fire).
    #[allow(clippy::too_many_arguments)]
    fn row_button<'a>(
        &'a self,
        ctx: &Context<'a>,
        button: &Node,
        icon: &str,
        name: &'a str,
        badge: Option<&str>,
        status: Option<&'a str>,
        chevron: Option<(bool, Message)>,
        on_press: Message,
    ) -> Block<'a> {
        let theme = ctx.theme;
        let n = button.child("name");
        let mut parts: Vec<Element<'a, Message>> = vec![
            self.icon(ctx, &button.child("icon"), icon),
            theme
                .container(
                    &n,
                    theme
                        .text(&n.child("text"), name)
                        .wrapping(text::Wrapping::None),
                )
                .into(),
        ];
        let mut height = self.icon_height(ctx, &button.child("icon")).max(padded(
            theme,
            &n,
            theme.line_height(&n.child("text")),
        ));
        if let Some(badge) = badge {
            let b = button.child("icon").class("badge");
            parts.push(self.icon(ctx, &b, badge));
            height = height.max(self.icon_height(ctx, &b));
        }
        parts.push(Space::new().width(Length::Fill).into());
        if let Some(status) = status {
            let st = button.child("status");
            parts.push(theme.container(&st, theme.text(&st, status)).into());
            height = height.max(padded(theme, &st, theme.line_height(&st)));
        }
        if let Some((expanded, toggle)) = chevron {
            let c = button.child("chevron").class_if("open", expanded);
            let glyph = theme.text(&c, if expanded { "▴" } else { "▾" });
            parts.push(theme.button(&c, glyph).on_press(toggle).into());
            height = height.max(padded(theme, &c, theme.line_height(&c)));
        }
        let content = row(parts)
            .spacing(theme.resolve(button).gap)
            .align_y(Alignment::Center)
            .width(Length::Fill);
        (
            theme
                .button(button, content)
                .on_press(on_press)
                .width(Length::Fill)
                .into(),
            padded(theme, button, height),
        )
    }

    /// The facts about a connection, one line each.
    fn details<'a>(&'a self, ctx: &Context<'a>, node: &Node, lines: Vec<String>) -> Block<'a> {
        let theme = ctx.theme;
        let details = node.child("details");
        let line = details.child("line");
        let s = theme.resolve(&details);
        let each = padded(theme, &line, theme.line_height(&line));
        let n = lines.len();
        let rows: Vec<Element<'a, Message>> = lines
            .into_iter()
            .map(|l| {
                theme
                    .container(&line, theme.text(&line, l).wrapping(text::Wrapping::None))
                    .width(Length::Fill)
                    .into()
            })
            .collect();
        (
            theme
                .container(&details, column(rows).spacing(s.gap).width(Length::Fill))
                .width(Length::Fill)
                .into(),
            padded(
                theme,
                &details,
                each * n as f32 + s.gap * n.saturating_sub(1) as f32,
            ),
        )
    }

    /// A row of text buttons (Disconnect, Forget).
    fn actions<'a>(
        &'a self,
        ctx: &Context<'a>,
        node: &Node,
        actions: Vec<(&'static str, &'a str, Message)>,
    ) -> Block<'a> {
        let theme = ctx.theme;
        let container = node.child("actions");
        let s = theme.resolve(&container);
        let mut height: f32 = 0.0;
        let buttons: Vec<Element<'a, Message>> = actions
            .into_iter()
            .map(|(class, label, message)| {
                let b = container.child("button").class(class);
                height = height.max(self.text_button_height(ctx, &b));
                theme
                    .button(&b, theme.text(&b.child("text"), label))
                    .on_press(message)
                    .into()
            })
            .collect();
        (
            theme
                .container(&container, row(buttons).spacing(s.gap))
                .width(Length::Fill)
                .align_x(Alignment::End)
                .into(),
            padded(theme, &container, height),
        )
    }

    /// The password field with its eye and the Connect button.
    fn auth<'a>(&'a self, ctx: &Context<'a>, node: &Node, busy: bool) -> Block<'a> {
        let theme = ctx.theme;
        let locale = ctx.locale;
        let auth = node.child("auth").class_if("busy", busy);
        let input_node = auth.child("input");
        let mut input = theme
            .text_input(&input_node, locale.tr("network.password"), &self.password)
            .id(self.input.clone())
            .secure(!self.peek)
            .on_submit(Message::Submit);
        if !busy {
            input = input.on_input(Message::Password);
        }
        let peek = auth.child("peek").class_if("on", self.peek);
        let eye = theme
            .button(
                &peek,
                self.icon(
                    ctx,
                    &peek.child("icon"),
                    if self.peek { ICON_CONCEAL } else { ICON_PEEK },
                ),
            )
            .on_press_maybe((!busy).then_some(Message::Peek));
        let ready = !busy && !self.password.is_empty();
        let connect = auth
            .child("button")
            .class("connect")
            .class_if("disabled", !ready);
        let connect_button = theme
            .button(
                &connect,
                theme.text(&connect.child("text"), locale.tr("network.connect")),
            )
            .on_press_maybe(ready.then_some(Message::Submit));
        let s = theme.resolve(&auth);
        let input_style = theme.resolve(&input_node);
        let input_height = theme.line_height(&input_node)
            + input_style.padding.top
            + input_style.padding.bottom
            + 2.0 * input_style.border_width;
        let height = input_height
            .max(self.icon_button_height(ctx, &peek))
            .max(self.text_button_height(ctx, &connect));
        (
            theme
                .container(
                    &auth,
                    row![
                        theme.tag(&input_node, input.width(Length::Fill)),
                        eye,
                        connect_button
                    ]
                    .spacing(s.gap)
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
                )
                .width(Length::Fill)
                .into(),
            padded(theme, &auth, height),
        )
    }

    /// A line of text under a row: an error, a hint.
    fn message<'a>(&'a self, ctx: &Context<'a>, node: &Node, text: &'a str) -> Block<'a> {
        let theme = ctx.theme;
        let m = node.child("message");
        let width = self.list_width(ctx);
        let height = theme
            .measure_in(&m, text, width)
            .height
            .max(theme.line_height(&m));
        (
            theme
                .container(&m, theme.text(&m, text))
                .width(Length::Fill)
                .into(),
            padded(theme, &m, height),
        )
    }
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

fn px(l: Option<theme::Length>) -> Option<f32> {
    match l {
        Some(theme::Length::Px(px)) => Some(px),
        _ => None,
    }
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
