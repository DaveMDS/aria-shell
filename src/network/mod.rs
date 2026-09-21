//! Network: the devices, the Wi‑Fi networks around, the saved profiles
//! and the active connections, as NetworkManager sees them.
//!
//! One [`Network`] lives in the daemon, shaped like `Audio`: the
//! [`Network::subscription`] is the system-bus connection (nm.rs), which
//! re-reads one whole [`Snapshot`] after any NetworkManager signal (a
//! few round trips, debounced; far simpler than patching object by
//! object, and what COSMIC's applet does too); gadgets read the result
//! from their view context and act with a [`Command`] the daemon runs
//! with [`Network::run`].
//!
//! Reference: <https://networkmanager.dev/docs/api/latest/>

mod nm;

use iced::{Subscription, Task};
use zbus::Connection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Wired,
    Wifi,
}

/// `NMDeviceState`, the ones we tell apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Unmanaged,
    Unavailable,
    Disconnected,
    /// Prepare, Config, IpConfig, IpCheck, Secondaries.
    Connecting,
    NeedAuth,
    Activated,
    Deactivating,
    Failed,
    Unknown,
}

impl DeviceState {
    pub fn from_nm(state: u32) -> Self {
        match state {
            10 => Self::Unmanaged,
            20 => Self::Unavailable,
            30 => Self::Disconnected,
            40 | 50 | 70 | 80 | 90 => Self::Connecting,
            60 => Self::NeedAuth,
            100 => Self::Activated,
            110 => Self::Deactivating,
            120 => Self::Failed,
            _ => Self::Unknown,
        }
    }

    pub fn is_connecting(self) -> bool {
        matches!(self, Self::Connecting | Self::NeedAuth)
    }
}

/// What a Wi‑Fi network asks to join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Open,
    Wep,
    /// WPA / WPA2 personal (a pre-shared key).
    Psk,
    /// WPA3 personal only.
    Sae,
    /// 802.1x: an identity and more, not done from the popup.
    Enterprise,
}

impl Security {
    /// From an access point's `Flags`, `WpaFlags` and `RsnFlags`.
    pub fn from_flags(flags: u32, wpa: u32, rsn: u32) -> Self {
        const PRIVACY: u32 = 0x1;
        const KEY_MGMT_PSK: u32 = 0x100;
        const KEY_MGMT_802_1X: u32 = 0x200;
        const KEY_MGMT_SAE: u32 = 0x400;
        const KEY_MGMT_EAP_SUITE_B_192: u32 = 0x2000;
        let both = wpa | rsn;
        if both & (KEY_MGMT_802_1X | KEY_MGMT_EAP_SUITE_B_192) != 0 {
            Self::Enterprise
        } else if both & KEY_MGMT_PSK != 0 {
            Self::Psk
        } else if both & KEY_MGMT_SAE != 0 {
            Self::Sae
        } else if flags & PRIVACY != 0 && both == 0 {
            Self::Wep
        } else {
            Self::Open
        }
    }

    pub fn secured(self) -> bool {
        self != Self::Open
    }

    /// The name shown in the details.
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Wep => "WEP",
            Self::Psk => "WPA2",
            Self::Sae => "WPA3",
            Self::Enterprise => "802.1X",
        }
    }
}

/// `NMConnectivityState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Connectivity {
    #[default]
    Unknown,
    None,
    Portal,
    Limited,
    Full,
}

impl Connectivity {
    pub fn from_nm(v: u32) -> Self {
        match v {
            1 => Self::None,
            2 => Self::Portal,
            3 => Self::Limited,
            4 => Self::Full,
            _ => Self::Unknown,
        }
    }
}

/// `NMActiveConnectionState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveState {
    Activating,
    Activated,
    Deactivating,
    Deactivated,
    Unknown,
}

impl ActiveState {
    pub fn from_nm(v: u32) -> Self {
        match v {
            1 => Self::Activating,
            2 => Self::Activated,
            3 => Self::Deactivating,
            4 => Self::Deactivated,
            _ => Self::Unknown,
        }
    }
}

/// A saved profile's `connection.type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileKind {
    Wifi,
    Wired,
    /// `vpn` or `wireguard`.
    Vpn,
    Other,
}

impl ProfileKind {
    pub fn from_nm(kind: &str) -> Self {
        match kind {
            "802-11-wireless" => Self::Wifi,
            "802-3-ethernet" => Self::Wired,
            "vpn" | "wireguard" => Self::Vpn,
            _ => Self::Other,
        }
    }
}

/// An IP configuration, as text for the details.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IpConfig {
    /// `address/prefix`.
    pub addresses: Vec<String>,
    pub gateway: String,
    pub dns: Vec<String>,
}

/// A managed wired or Wi‑Fi device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub path: String,
    pub iface: String,
    pub kind: DeviceKind,
    pub state: DeviceState,
    /// A wired device with its cable plugged in (always true for Wi‑Fi).
    pub carrier: bool,
    /// Mb/s, when known.
    pub speed: u32,
    pub hw_address: String,
    /// The access point a Wi‑Fi device is on (or joining).
    pub active_ap: Option<String>,
    /// Its active connection's path.
    pub active: Option<String>,
    pub ip4: Option<IpConfig>,
    pub ip6: Option<IpConfig>,
}

/// One access point as NetworkManager lists it (a BSSID).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawAccessPoint {
    pub path: String,
    pub device: String,
    pub ssid: String,
    pub strength: u8,
    pub security: Security,
    /// MHz.
    pub frequency: u32,
    /// Kb/s.
    pub max_bitrate: u32,
    pub bssid: String,
}

/// A Wi‑Fi network: the strongest of its access points, plus what we
/// know about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessPoint {
    pub path: String,
    pub device: String,
    pub ssid: String,
    pub strength: u8,
    pub security: Security,
    pub frequency: u32,
    pub max_bitrate: u32,
    pub bssid: String,
    /// The saved profile's uuid, when there is one.
    pub known: Option<String>,
    pub active: bool,
    pub connecting: bool,
}

impl AccessPoint {
    /// The band, from the frequency.
    pub fn band(&self) -> &'static str {
        if self.frequency >= 5925 {
            "6 GHz"
        } else if self.frequency >= 4900 {
            "5 GHz"
        } else {
            "2.4 GHz"
        }
    }
}

/// A saved connection profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub path: String,
    pub uuid: String,
    pub id: String,
    pub kind: ProfileKind,
    /// A Wi‑Fi profile's network.
    pub ssid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveConnection {
    pub path: String,
    pub uuid: String,
    pub id: String,
    pub kind: ProfileKind,
    pub state: ActiveState,
    pub devices: Vec<String>,
    /// The access point of a Wi‑Fi connection.
    pub specific: String,
    /// Carries the default route.
    pub default: bool,
    pub vpn: bool,
    pub ip4: Option<IpConfig>,
}

/// Everything read from NetworkManager in one go.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub wireless_enabled: bool,
    pub wireless_hw_enabled: bool,
    pub connectivity: Connectivity,
    pub primary: Option<String>,
    pub devices: Vec<Device>,
    pub aps: Vec<RawAccessPoint>,
    pub profiles: Vec<Profile>,
    pub active: Vec<ActiveConnection>,
}

/// What a failure is about: a network (by SSID) or a profile (by uuid).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailKey {
    Ssid(String),
    Uuid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailReason {
    WrongPassword,
    /// NetworkManager needed a secret nobody had (a VPN's password).
    NoSecrets,
    /// The request itself was refused (polkit, a bad setting).
    Refused(String),
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub key: FailKey,
    pub reason: FailReason,
}

/// A connection the gadget asked for and hasn't settled yet.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Attempt {
    key: FailKey,
    /// The device it happens on (a wireless connect).
    device: Option<String>,
    /// The profile `AddAndActivateConnection` created, to delete on a
    /// failure (a wrong password leaves no broken profile behind).
    added: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// The system bus is up; commands can be sent.
    Connected(Connection),
    /// NetworkManager owns its name, or left the bus.
    Running(bool),
    Snapshot(Box<Snapshot>),
    /// A device reached the Failed state (`NMDeviceStateReason`).
    DeviceFailed {
        device: String,
        reason: u32,
    },
    /// An active connection deactivated with a reason
    /// (`NMActiveConnectionStateReason`).
    ActiveFailed {
        active: String,
        reason: u32,
    },
    /// Our `AddAndActivateConnection` returned its profile.
    Added {
        key: FailKey,
        path: String,
    },
    /// A command was refused by NetworkManager.
    Refused {
        key: FailKey,
        error: String,
    },
}

#[derive(Debug, Clone)]
pub enum Command {
    SetWireless(bool),
    ToggleWireless,
    /// Ask every Wi‑Fi device for a scan.
    Scan,
    /// Join a network: its saved profile, or a new one (open networks).
    Connect {
        ssid: String,
    },
    ConnectWithPassword {
        ssid: String,
        password: String,
    },
    /// Bring a device up with whatever profile suits it.
    ConnectDevice {
        device: String,
    },
    /// Drop a device's connection.
    Disconnect {
        device: String,
    },
    /// Delete a profile.
    Forget {
        uuid: String,
    },
    /// Activate a profile (a VPN, a wired one).
    Activate {
        uuid: String,
    },
    /// Deactivate the active connection of a profile.
    Deactivate {
        uuid: String,
    },
}

/// The bar's summary of the primary connection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    /// The device kind of the primary connection, or of the best
    /// candidate when nothing is connected.
    pub kind: Option<DeviceKind>,
    pub connected: bool,
    pub connecting: bool,
    /// Connected, but without internet.
    pub limited: bool,
    /// The SSID or the profile's name.
    pub label: String,
    /// Wi‑Fi strength, 0..=100.
    pub strength: u8,
    pub vpn: bool,
}

#[derive(Default)]
pub struct Network {
    bus: Option<Connection>,
    running: bool,
    snapshot: Snapshot,
    /// The networks around, merged by SSID and sorted (active, known
    /// by strength, the rest by strength).
    aps: Vec<AccessPoint>,
    failures: Vec<Failure>,
    attempt: Option<Attempt>,
}

impl Network {
    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::run(nm::events)
    }

    /// Apply an event; whether what gadgets see changed.
    pub fn apply(&mut self, event: Event) -> (bool, Task<Event>) {
        match event {
            Event::Connected(conn) => {
                self.bus = Some(conn);
                (true, Task::none())
            }
            Event::Running(running) => {
                let changed = self.running != running;
                self.running = running;
                if !running {
                    self.snapshot = Snapshot::default();
                    self.aps.clear();
                    self.attempt = None;
                    self.failures.clear();
                }
                (changed, Task::none())
            }
            Event::Snapshot(snapshot) => {
                if *snapshot == self.snapshot {
                    return (false, Task::none());
                }
                self.snapshot = *snapshot;
                self.rebuild();
                (true, Task::none())
            }
            Event::DeviceFailed { device, reason } => {
                let Some(attempt) = self.attempt.take() else {
                    return (false, Task::none());
                };
                if attempt.device.as_deref() != Some(device.as_str()) {
                    self.attempt = Some(attempt);
                    return (false, Task::none());
                }
                let reason = match reason {
                    // NO_SECRETS, and the supplicant's disconnect /
                    // config / failed / timeout: what a wrong key looks
                    // like from here.
                    7..=11 => FailReason::WrongPassword,
                    _ => FailReason::Other,
                };
                self.fail(attempt.key.clone(), reason);
                (true, self.delete_added(&attempt))
            }
            Event::ActiveFailed { active, reason } => {
                // Only for a profile activated as such (a VPN): a Wi‑Fi
                // attempt fails through its device, whose reason is the
                // telling one.
                let Some(attempt) = self.attempt.take() else {
                    return (false, Task::none());
                };
                let uuid = self
                    .snapshot
                    .active
                    .iter()
                    .find(|a| a.path == active)
                    .map(|a| a.uuid.clone());
                let ours = match (&attempt.key, uuid) {
                    (FailKey::Uuid(u), Some(active)) => *u == active && attempt.device.is_none(),
                    _ => false,
                };
                if !ours {
                    self.attempt = Some(attempt);
                    return (false, Task::none());
                }
                let reason = match reason {
                    9 => FailReason::NoSecrets,
                    10 => FailReason::WrongPassword,
                    // A user disconnect, a device gone: not a failure.
                    0..=4 | 11 | 14 => {
                        return (false, Task::none());
                    }
                    _ => FailReason::Other,
                };
                self.fail(attempt.key.clone(), reason);
                (true, self.delete_added(&attempt))
            }
            Event::Added { key, path } => {
                if let Some(a) = &mut self.attempt
                    && a.key == key
                {
                    a.added = Some(path);
                }
                (false, Task::none())
            }
            Event::Refused { key, error } => {
                log::warn!("network: {key:?}: {error}");
                if self.attempt.as_ref().is_some_and(|a| a.key == key) {
                    self.attempt = None;
                }
                self.fail(key, FailReason::Refused(error));
                (true, Task::none())
            }
        }
    }

    fn fail(&mut self, key: FailKey, reason: FailReason) {
        self.failures.retain(|f| f.key != key);
        self.failures.push(Failure { key, reason });
    }

    /// The task deleting the profile an attempt added, if any.
    fn delete_added(&self, attempt: &Attempt) -> Task<Event> {
        let (Some(conn), Some(path)) = (self.bus.clone(), attempt.added.clone()) else {
            return Task::none();
        };
        Task::future(async move {
            if let Err(e) = nm::delete_profile(&conn, &path).await {
                log::warn!("network: deleting {path}: {e}");
            }
        })
        .discard()
    }

    /// Derive the merged network list from the snapshot, and settle
    /// the attempt and the failures the snapshot answers.
    fn rebuild(&mut self) {
        let s = &self.snapshot;
        // One entry per SSID: the BSSID a device is on, else the
        // strongest (the bar's strength and the details are that one's).
        let on_air = |raw: &RawAccessPoint| {
            s.devices
                .iter()
                .any(|d| d.active_ap.as_deref() == Some(raw.path.as_str()))
        };
        let mut raws: Vec<&RawAccessPoint> = s.aps.iter().filter(|r| !r.ssid.is_empty()).collect();
        raws.sort_by(|a, b| on_air(b).cmp(&on_air(a)).then(b.strength.cmp(&a.strength)));
        let mut aps: Vec<AccessPoint> = Vec::new();
        for raw in raws {
            match aps.iter_mut().find(|a| a.ssid == raw.ssid) {
                Some(_) => {}
                None => aps.push(AccessPoint {
                    path: raw.path.clone(),
                    device: raw.device.clone(),
                    ssid: raw.ssid.clone(),
                    strength: raw.strength,
                    security: raw.security,
                    frequency: raw.frequency,
                    max_bitrate: raw.max_bitrate,
                    bssid: raw.bssid.clone(),
                    known: None,
                    active: false,
                    connecting: false,
                }),
            }
        }
        for ap in &mut aps {
            ap.known = s
                .profiles
                .iter()
                .find(|p| p.kind == ProfileKind::Wifi && p.ssid.as_deref() == Some(&ap.ssid))
                .map(|p| p.uuid.clone());
            // The device on this network (any of its BSSIDs), and how far along.
            let on_it = s.devices.iter().find(|d| {
                d.kind == DeviceKind::Wifi
                    && d.active_ap.as_ref().is_some_and(|p| {
                        s.aps
                            .iter()
                            .any(|raw| raw.path == *p && raw.ssid == ap.ssid)
                    })
            });
            if let Some(d) = on_it {
                ap.active = d.state == DeviceState::Activated;
                ap.connecting = d.state.is_connecting();
            }
        }
        aps.sort_by(|a, b| {
            b.active
                .cmp(&a.active)
                .then(b.known.is_some().cmp(&a.known.is_some()))
                .then(b.strength.cmp(&a.strength))
                .then(a.ssid.cmp(&b.ssid))
        });
        self.aps = aps;

        // An attempt that got there is over; so is its failure.
        let settled = match &self.attempt {
            Some(a) => match &a.key {
                FailKey::Ssid(ssid) => self.aps.iter().any(|ap| ap.active && ap.ssid == *ssid),
                FailKey::Uuid(uuid) => self
                    .snapshot
                    .active
                    .iter()
                    .any(|c| c.uuid == *uuid && c.state == ActiveState::Activated),
            },
            None => false,
        };
        if settled {
            let key = self.attempt.take().map(|a| a.key);
            self.failures.retain(|f| Some(&f.key) != key.as_ref());
        }
        let active_ssids: Vec<&str> = self
            .aps
            .iter()
            .filter(|ap| ap.active)
            .map(|ap| ap.ssid.as_str())
            .collect();
        self.failures.retain(|f| match &f.key {
            FailKey::Ssid(ssid) => !active_ssids.contains(&ssid.as_str()),
            FailKey::Uuid(uuid) => !self
                .snapshot
                .active
                .iter()
                .any(|c| c.uuid == *uuid && c.state == ActiveState::Activated),
        });
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn wireless_enabled(&self) -> bool {
        self.snapshot.wireless_enabled
    }

    pub fn devices(&self) -> &[Device] {
        &self.snapshot.devices
    }

    pub fn devices_of(&self, kind: DeviceKind) -> impl Iterator<Item = &Device> {
        self.snapshot.devices.iter().filter(move |d| d.kind == kind)
    }

    pub fn access_points(&self) -> &[AccessPoint] {
        &self.aps
    }

    pub fn access_point(&self, ssid: &str) -> Option<&AccessPoint> {
        self.aps.iter().find(|ap| ap.ssid == ssid)
    }

    pub fn profile(&self, uuid: &str) -> Option<&Profile> {
        self.snapshot.profiles.iter().find(|p| p.uuid == uuid)
    }

    /// The active connection of a device.
    pub fn active_of(&self, device: &Device) -> Option<&ActiveConnection> {
        let path = device.active.as_ref()?;
        self.snapshot.active.iter().find(|a| a.path == *path)
    }

    /// The active connection of a profile, if any.
    pub fn active_by_uuid(&self, uuid: &str) -> Option<&ActiveConnection> {
        self.snapshot.active.iter().find(|a| a.uuid == uuid)
    }

    /// The VPN profiles, in NetworkManager's order.
    pub fn vpns(&self) -> impl Iterator<Item = &Profile> {
        self.snapshot
            .profiles
            .iter()
            .filter(|p| p.kind == ProfileKind::Vpn)
    }

    pub fn failure(&self, key: &FailKey) -> Option<&Failure> {
        self.failures.iter().find(|f| f.key == *key)
    }

    /// Whether the gadget's attempt on `key` is still under way.
    pub fn attempting(&self, key: &FailKey) -> bool {
        self.attempt.as_ref().is_some_and(|a| a.key == *key)
    }

    /// The bar's view of things: the primary connection, else the
    /// device closest to one.
    pub fn summary(&self) -> Summary {
        let s = &self.snapshot;
        let vpn = s
            .active
            .iter()
            .any(|a| a.vpn && a.state == ActiveState::Activated);
        let primary = s
            .primary
            .as_ref()
            .and_then(|p| s.active.iter().find(|a| a.path == *p))
            .filter(|a| !a.vpn)
            .or_else(|| {
                s.active
                    .iter()
                    .filter(|a| !a.vpn && matches!(a.kind, ProfileKind::Wifi | ProfileKind::Wired))
                    .max_by_key(|a| (a.state == ActiveState::Activated, a.default))
            });
        if let Some(a) = primary {
            let device = a
                .devices
                .iter()
                .find_map(|p| s.devices.iter().find(|d| d.path == *p));
            let kind = device.map(|d| d.kind).or(match a.kind {
                ProfileKind::Wifi => Some(DeviceKind::Wifi),
                ProfileKind::Wired => Some(DeviceKind::Wired),
                _ => None,
            });
            // The network it's on: by the connection's access point,
            // else by that one's SSID (another BSSID of it may be the
            // one listed).
            let ap = self
                .aps
                .iter()
                .find(|ap| ap.path == a.specific)
                .or_else(|| {
                    let raw = s.aps.iter().find(|raw| raw.path == a.specific)?;
                    self.aps.iter().find(|ap| ap.ssid == raw.ssid)
                });
            let connected = a.state == ActiveState::Activated;
            return Summary {
                kind,
                connected,
                connecting: a.state == ActiveState::Activating,
                limited: connected
                    && matches!(
                        s.connectivity,
                        Connectivity::Limited | Connectivity::Portal | Connectivity::None
                    ),
                label: ap.map_or_else(|| a.id.clone(), |ap| ap.ssid.clone()),
                strength: ap.map_or(0, |ap| ap.strength),
                vpn,
            };
        }
        // Nothing connected: a Wi‑Fi device if there is one, else wired.
        let kind = if s.devices.iter().any(|d| d.kind == DeviceKind::Wifi) {
            Some(DeviceKind::Wifi)
        } else {
            s.devices.first().map(|d| d.kind)
        };
        Summary {
            kind,
            connecting: s.devices.iter().any(|d| d.state.is_connecting()),
            vpn,
            ..Summary::default()
        }
    }

    /// `debug network`: the devices, the networks, the profiles and the
    /// active connections, one line each.
    pub fn describe(&self) -> String {
        let s = &self.snapshot;
        let mut lines: Vec<String> = Vec::new();
        if !self.running {
            lines.push("not running".to_owned());
        }
        lines.push(format!(
            "wireless={} hw={} connectivity={:?}",
            s.wireless_enabled, s.wireless_hw_enabled, s.connectivity
        ));
        for d in &s.devices {
            lines.push(format!(
                "device {} {:?} {:?}{}{}",
                d.iface,
                d.kind,
                d.state,
                if d.carrier { "" } else { " unplugged" },
                d.ip4
                    .as_ref()
                    .map(|ip| format!(" ip={}", ip.addresses.join(",")))
                    .unwrap_or_default(),
            ));
        }
        for ap in &self.aps {
            lines.push(format!(
                "ap {:?} {}% {:?} {}MHz{}{}{}",
                ap.ssid,
                ap.strength,
                ap.security,
                ap.frequency,
                if ap.known.is_some() { " known" } else { "" },
                if ap.active { " active" } else { "" },
                if ap.connecting { " connecting" } else { "" },
            ));
        }
        for p in &s.profiles {
            lines.push(format!("profile {:?} {:?} {}", p.id, p.kind, p.uuid));
        }
        for a in &s.active {
            lines.push(format!(
                "active {:?} {:?} {:?}{}{}",
                a.id,
                a.kind,
                a.state,
                if a.default { " default" } else { "" },
                if a.vpn { " vpn" } else { "" },
            ));
        }
        for f in &self.failures {
            lines.push(format!("failed {:?} {:?}", f.key, f.reason));
        }
        lines.join("; ")
    }

    pub fn run(&mut self, command: Command) -> Task<Event> {
        let Some(conn) = self.bus.clone() else {
            log::warn!("network: no bus connection, dropping {command:?}");
            return Task::none();
        };
        if !self.running {
            log::warn!("network: NetworkManager isn't running, dropping {command:?}");
            return Task::none();
        }
        let s = &self.snapshot;
        match command {
            Command::SetWireless(on) => logged(async move { nm::set_wireless(&conn, on).await }),
            Command::ToggleWireless => {
                let on = !s.wireless_enabled;
                logged(async move { nm::set_wireless(&conn, on).await })
            }
            Command::Scan => {
                // A device that's off or unmanaged refuses; nothing to ask.
                if !s.wireless_enabled {
                    return Task::none();
                }
                let devices: Vec<String> = self
                    .devices_of(DeviceKind::Wifi)
                    .filter(|d| {
                        !matches!(d.state, DeviceState::Unmanaged | DeviceState::Unavailable)
                    })
                    .map(|d| d.path.clone())
                    .collect();
                if devices.is_empty() {
                    return Task::none();
                }
                logged(async move {
                    for d in devices {
                        nm::request_scan(&conn, &d).await?;
                    }
                    Ok(())
                })
            }
            Command::Connect { ssid } => {
                let Some(ap) = self.access_point(&ssid).cloned() else {
                    return Task::none();
                };
                let key = FailKey::Ssid(ssid.clone());
                let profile = ap.known.as_ref().and_then(|u| self.profile(u));
                match (profile, ap.security) {
                    (Some(p), _) => {
                        let (profile, device, specific) =
                            (p.path.clone(), ap.device.clone(), ap.path.clone());
                        self.start(key.clone(), Some(ap.device.clone()));
                        attempted(key, async move {
                            nm::activate(&conn, &profile, &device, &specific).await?;
                            Ok(None)
                        })
                    }
                    (None, Security::Open) => {
                        self.start(key.clone(), Some(ap.device.clone()));
                        attempted(key.clone(), async move {
                            let path = nm::add_and_activate(&conn, &ap, None).await?;
                            Ok(Some(Event::Added { key, path }))
                        })
                    }
                    (None, Security::Enterprise) => {
                        log::warn!("network: {ssid:?} needs 802.1x settings");
                        Task::none()
                    }
                    // The gadget asks the password first.
                    (None, _) => Task::none(),
                }
            }
            Command::ConnectWithPassword { ssid, password } => {
                let Some(ap) = self.access_point(&ssid).cloned() else {
                    return Task::none();
                };
                let key = FailKey::Ssid(ssid);
                self.start(key.clone(), Some(ap.device.clone()));
                // A known network whose key was refused: the old
                // profile goes, the new one takes its place.
                let old = ap
                    .known
                    .as_ref()
                    .and_then(|u| self.profile(u))
                    .map(|p| p.path.clone());
                attempted(key.clone(), async move {
                    if let Some(old) = old
                        && let Err(e) = nm::delete_profile(&conn, &old).await
                    {
                        log::warn!("network: replacing {old}: {e}");
                    }
                    let path = nm::add_and_activate(&conn, &ap, Some(&password)).await?;
                    Ok(Some(Event::Added { key, path }))
                })
            }
            Command::ConnectDevice { device } => {
                logged(async move { nm::activate(&conn, "/", &device, "/").await })
            }
            Command::Disconnect { device } => {
                logged(async move { nm::disconnect(&conn, &device).await })
            }
            Command::Forget { uuid } => {
                // Refused on a system-wide profile (polkit): logged, the
                // profile stays.
                let Some(path) = self.profile(&uuid).map(|p| p.path.clone()) else {
                    return Task::none();
                };
                logged(async move { nm::delete_profile(&conn, &path).await })
            }
            Command::Activate { uuid } => {
                let Some(profile) = self.profile(&uuid).map(|p| p.path.clone()) else {
                    return Task::none();
                };
                let key = FailKey::Uuid(uuid);
                self.start(key.clone(), None);
                attempted(key, async move {
                    nm::activate(&conn, &profile, "/", "/").await?;
                    Ok(None)
                })
            }
            Command::Deactivate { uuid } => {
                let Some(a) = s.active.iter().find(|a| a.uuid == uuid) else {
                    return Task::none();
                };
                let path = a.path.clone();
                logged(async move { nm::deactivate(&conn, &path).await })
            }
        }
    }

    /// Begin an attempt on `key`: its earlier failure is forgotten.
    fn start(&mut self, key: FailKey, device: Option<String>) {
        self.failures.retain(|f| f.key != key);
        self.attempt = Some(Attempt {
            key,
            device,
            added: None,
        });
    }
}

/// A fire-and-forget request: an error is logged.
fn logged(f: impl Future<Output = zbus::Result<()>> + Send + 'static) -> Task<Event> {
    Task::future(async move {
        if let Err(e) = f.await {
            log::warn!("network: {e}");
        }
    })
    .discard()
}

/// A request on behalf of `key`: a refusal becomes a failure the
/// gadget shows.
fn attempted(
    key: FailKey,
    f: impl Future<Output = zbus::Result<Option<Event>>> + Send + 'static,
) -> Task<Event> {
    Task::future(async move {
        match f.await {
            Ok(event) => event,
            Err(e) => Some(Event::Refused {
                key,
                error: e.to_string(),
            }),
        }
    })
    .and_then(Task::done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wifi_device(state: DeviceState, active_ap: Option<&str>) -> Device {
        Device {
            path: "/dev/1".into(),
            iface: "wlan0".into(),
            kind: DeviceKind::Wifi,
            state,
            carrier: true,
            speed: 0,
            hw_address: "aa:bb".into(),
            active_ap: active_ap.map(str::to_owned),
            active: Some("/active/1".into()),
            ip4: None,
            ip6: None,
        }
    }

    fn ap(path: &str, ssid: &str, strength: u8) -> RawAccessPoint {
        RawAccessPoint {
            path: path.into(),
            device: "/dev/1".into(),
            ssid: ssid.into(),
            strength,
            security: Security::Psk,
            frequency: 5180,
            max_bitrate: 866_000,
            bssid: path.into(),
        }
    }

    fn profile(id: &str, kind: ProfileKind, ssid: Option<&str>) -> Profile {
        Profile {
            path: format!("/settings/{id}"),
            uuid: format!("uuid-{id}"),
            id: id.into(),
            kind,
            ssid: ssid.map(str::to_owned),
        }
    }

    fn active(id: &str, kind: ProfileKind, state: ActiveState, specific: &str) -> ActiveConnection {
        ActiveConnection {
            path: "/active/1".into(),
            uuid: format!("uuid-{id}"),
            id: id.into(),
            kind,
            state,
            devices: vec!["/dev/1".into()],
            specific: specific.into(),
            default: true,
            vpn: kind == ProfileKind::Vpn,
            ip4: None,
        }
    }

    fn apply(n: &mut Network, s: Snapshot) -> bool {
        n.apply(Event::Snapshot(Box::new(s))).0
    }

    fn running() -> Network {
        Network {
            running: true,
            ..Network::default()
        }
    }

    #[test]
    fn security_from_flags() {
        assert_eq!(Security::from_flags(0, 0, 0), Security::Open);
        assert_eq!(Security::from_flags(1, 0, 0), Security::Wep);
        assert_eq!(Security::from_flags(1, 0, 0x100 | 0x8), Security::Psk);
        assert_eq!(Security::from_flags(1, 0, 0x400 | 0x8), Security::Sae);
        // Transition mode: PSK wins (the key works).
        assert_eq!(Security::from_flags(1, 0, 0x500), Security::Psk);
        assert_eq!(Security::from_flags(1, 0x200, 0), Security::Enterprise);
        assert_eq!(Security::from_flags(1, 0, 0x2000), Security::Enterprise);
    }

    #[test]
    fn networks_merged_and_sorted() {
        let mut n = running();
        let snapshot = Snapshot {
            wireless_enabled: true,
            devices: vec![wifi_device(DeviceState::Activated, Some("/ap/2b"))],
            aps: vec![
                ap("/ap/1", "Guest", 30),
                ap("/ap/2a", "Casa", 40),
                ap("/ap/2b", "Casa", 70),
                ap("/ap/3", "Office", 90),
                ap("/ap/4", "", 100),
                ap("/ap/5", "Known", 20),
            ],
            profiles: vec![
                profile("Known", ProfileKind::Wifi, Some("Known")),
                profile("Casa", ProfileKind::Wifi, Some("Casa")),
            ],
            active: vec![active(
                "Casa",
                ProfileKind::Wifi,
                ActiveState::Activated,
                "/ap/2b",
            )],
            ..Snapshot::default()
        };
        assert!(apply(&mut n, snapshot.clone()));
        assert!(!apply(&mut n, snapshot), "the same snapshot: no change");
        let order: Vec<(&str, u8, bool, bool)> = n
            .access_points()
            .iter()
            .map(|ap| (ap.ssid.as_str(), ap.strength, ap.known.is_some(), ap.active))
            .collect();
        assert_eq!(
            order,
            [
                ("Casa", 70, true, true),
                ("Known", 20, true, false),
                ("Office", 90, false, false),
                ("Guest", 30, false, false),
            ]
        );
        let s = n.summary();
        assert_eq!(s.kind, Some(DeviceKind::Wifi));
        assert!(s.connected);
        assert_eq!(s.label, "Casa");
        assert_eq!(s.strength, 70);
        assert!(!s.vpn);
    }

    #[test]
    fn the_bssid_in_use_wins_over_the_strongest() {
        let mut n = running();
        apply(
            &mut n,
            Snapshot {
                devices: vec![wifi_device(DeviceState::Activated, Some("/ap/2a"))],
                aps: vec![ap("/ap/2a", "Casa", 40), ap("/ap/2b", "Casa", 70)],
                active: vec![active(
                    "Casa",
                    ProfileKind::Wifi,
                    ActiveState::Activated,
                    "/ap/2a",
                )],
                ..Snapshot::default()
            },
        );
        let listed = &n.access_points()[0];
        assert_eq!(
            (listed.path.as_str(), listed.strength, listed.active),
            ("/ap/2a", 40, true)
        );
        assert_eq!(n.summary().strength, 40);
        // The connection on a BSSID that isn't the one listed: found by SSID.
        apply(
            &mut n,
            Snapshot {
                devices: vec![wifi_device(DeviceState::Activated, Some("/ap/2b"))],
                aps: vec![ap("/ap/2a", "Casa", 40), ap("/ap/2b", "Casa", 70)],
                active: vec![active(
                    "Casa",
                    ProfileKind::Wifi,
                    ActiveState::Activated,
                    "/ap/2a",
                )],
                ..Snapshot::default()
            },
        );
        assert_eq!(n.summary().strength, 70);
        assert_eq!(n.summary().label, "Casa");
    }

    #[test]
    fn summary_without_a_connection() {
        let mut n = Network::default();
        let s = Snapshot {
            devices: vec![wifi_device(DeviceState::Disconnected, None)],
            ..Snapshot::default()
        };
        apply(&mut n, s);
        let s = n.summary();
        assert_eq!(s.kind, Some(DeviceKind::Wifi));
        assert!(!s.connected && !s.connecting);
        assert_eq!(n.summary().label, "");
    }

    #[test]
    fn a_wrong_password_fails_the_attempt_and_settles_on_success() {
        let mut n = running();
        apply(
            &mut n,
            Snapshot {
                devices: vec![wifi_device(DeviceState::Disconnected, None)],
                aps: vec![ap("/ap/1", "Guest", 30)],
                ..Snapshot::default()
            },
        );
        let key = FailKey::Ssid("Guest".into());
        n.start(key.clone(), Some("/dev/1".into()));
        assert!(n.attempting(&key));
        let _ = n.apply(Event::Added {
            key: key.clone(),
            path: "/settings/9".into(),
        });
        // Another device failing isn't ours.
        assert!(
            !n.apply(Event::DeviceFailed {
                device: "/dev/2".into(),
                reason: 7
            })
            .0
        );
        assert!(n.attempting(&key));
        assert!(
            n.apply(Event::DeviceFailed {
                device: "/dev/1".into(),
                reason: 7
            })
            .0
        );
        assert!(!n.attempting(&key));
        assert_eq!(
            n.failure(&key).map(|f| &f.reason),
            Some(&FailReason::WrongPassword)
        );
        // A new attempt forgets the failure; reaching the network settles it.
        n.start(key.clone(), Some("/dev/1".into()));
        assert!(n.failure(&key).is_none());
        apply(
            &mut n,
            Snapshot {
                devices: vec![wifi_device(DeviceState::Activated, Some("/ap/1"))],
                aps: vec![ap("/ap/1", "Guest", 30)],
                profiles: vec![profile("Guest", ProfileKind::Wifi, Some("Guest"))],
                active: vec![active(
                    "Guest",
                    ProfileKind::Wifi,
                    ActiveState::Activated,
                    "/ap/1",
                )],
                ..Snapshot::default()
            },
        );
        assert!(!n.attempting(&key));
        assert!(n.access_points()[0].active);
    }

    #[test]
    fn vpn_needing_secrets() {
        let mut n = running();
        apply(
            &mut n,
            Snapshot {
                profiles: vec![profile("Office", ProfileKind::Vpn, None)],
                active: vec![active(
                    "Office",
                    ProfileKind::Vpn,
                    ActiveState::Activating,
                    "/",
                )],
                ..Snapshot::default()
            },
        );
        assert_eq!(n.vpns().count(), 1);
        let key = FailKey::Uuid("uuid-Office".into());
        n.start(key.clone(), None);
        // A user disconnect is no failure.
        assert!(
            !n.apply(Event::ActiveFailed {
                active: "/active/1".into(),
                reason: 2
            })
            .0
        );
        n.start(key.clone(), None);
        assert!(
            n.apply(Event::ActiveFailed {
                active: "/active/1".into(),
                reason: 9
            })
            .0
        );
        assert_eq!(
            n.failure(&key).map(|f| &f.reason),
            Some(&FailReason::NoSecrets)
        );
        assert!(!n.summary().vpn);
    }

    #[test]
    fn nm_leaving_clears_everything() {
        let mut n = running();
        apply(
            &mut n,
            Snapshot {
                devices: vec![wifi_device(DeviceState::Activated, None)],
                ..Snapshot::default()
            },
        );
        assert_eq!(n.devices().len(), 1);
        assert!(n.apply(Event::Running(false)).0);
        assert!(n.devices().is_empty());
        assert!(!n.running());
    }
}
