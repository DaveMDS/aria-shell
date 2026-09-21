//! A NetworkManager for the UI scenarios: owns
//! `org.freedesktop.NetworkManager` on the bus `DBUS_SYSTEM_BUS_ADDRESS`
//! points at (the scenario's session bus) and serves the objects the
//! shell reads: the manager, the settings and their profiles, a wired
//! device (`eth0`) and a Wi‑Fi one (`wlan0`) with its access points,
//! the active connections, one IP4 config. What the shell asks of it
//! goes to stdout, one line each:
//!
//!   scan | wireless <true|false> | activate <id> | add-activate <ssid> [psk=<key>]
//!   | disconnect <iface> | deactivate <id> | delete <id>
//!
//! Stdin drives changes (one command per line, answered `ok`, the
//! matching signals emitted):
//!
//!   ap <ssid> <strength> <open|psk|eap>   a network appears (or changes)
//!   ap-remove <ssid>
//!   strength <ssid> <0..100>
//!   known <ssid>                          a saved Wi‑Fi profile
//!   vpn <id>                              a saved VPN profile
//!   wired <id>                            a saved wired profile
//!   carrier <on|off>                      the wired cable
//!   wired-up                              eth0 connected through a wired profile
//!   finish                                the pending activation succeeds
//!   fail <psk|nosecrets>                  the pending activation fails that way
//!
//! `ready` is printed once the name is owned.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, BufReader};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, interface};

const NAME: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";

const DEV_WIRED: usize = 0;
const DEV_WIFI: usize = 1;

// NMDeviceState / NMActiveConnectionState / NMDeviceStateReason
const STATE_DISCONNECTED: u32 = 30;
const STATE_CONFIG: u32 = 50;
const STATE_ACTIVATED: u32 = 100;
const STATE_FAILED: u32 = 120;
const ACTIVE_ACTIVATING: u32 = 1;
const ACTIVE_ACTIVATED: u32 = 2;
const ACTIVE_DEACTIVATED: u32 = 4;
const REASON_USER: u32 = 2;
const REASON_NO_SECRETS: u32 = 7;
const ACTIVE_REASON_DEVICE_DISCONNECTED: u32 = 3;
const ACTIVE_REASON_NO_SECRETS: u32 = 9;

#[derive(Clone)]
struct Ap {
    index: u32,
    ssid: String,
    strength: u8,
    security: String,
}

#[derive(Clone)]
struct Profile {
    index: u32,
    id: String,
    kind: String,
    ssid: Option<String>,
}

#[derive(Clone)]
struct Active {
    index: u32,
    profile: u32,
    device: Option<usize>,
    /// The access point, for a Wi‑Fi one.
    specific: Option<u32>,
    state: u32,
    vpn: bool,
}

#[derive(Default)]
struct State {
    wireless: bool,
    carrier: bool,
    device_state: [u32; 2],
    aps: Vec<Ap>,
    profiles: Vec<Profile>,
    active: Vec<Active>,
    next: u32,
    primary: Option<u32>,
    /// The activation waiting for `finish` / `fail`.
    pending: Option<u32>,
}

type Shared = Arc<Mutex<State>>;

fn path(kind: &str, index: u32) -> OwnedObjectPath {
    OwnedObjectPath::try_from(format!("{ROOT}/{kind}/{index}")).unwrap()
}

fn none() -> OwnedObjectPath {
    OwnedObjectPath::try_from("/").unwrap()
}

fn device_path(i: usize) -> OwnedObjectPath {
    path("Devices", i as u32)
}

fn report(line: String) {
    println!("{line}");
}

// --- the manager -----------------------------------------------------------

struct Manager {
    state: Shared,
    conn: Connection,
}

#[interface(name = "org.freedesktop.NetworkManager")]
impl Manager {
    #[zbus(property)]
    fn devices(&self) -> Vec<OwnedObjectPath> {
        vec![device_path(DEV_WIRED), device_path(DEV_WIFI)]
    }

    #[zbus(property)]
    fn active_connections(&self) -> Vec<OwnedObjectPath> {
        self.state
            .lock()
            .unwrap()
            .active
            .iter()
            .map(|a| path("ActiveConnection", a.index))
            .collect()
    }

    #[zbus(property)]
    fn primary_connection(&self) -> OwnedObjectPath {
        self.state
            .lock()
            .unwrap()
            .primary
            .map_or_else(none, |i| path("ActiveConnection", i))
    }

    #[zbus(property)]
    fn wireless_enabled(&self) -> bool {
        self.state.lock().unwrap().wireless
    }

    #[zbus(property)]
    fn set_wireless_enabled(&mut self, enabled: bool) {
        self.state.lock().unwrap().wireless = enabled;
        report(format!("wireless {enabled}"));
        let conn = self.conn.clone();
        let state = self.state.clone();
        tokio::spawn(async move { sync(&conn, &state).await });
    }

    #[zbus(property)]
    fn wireless_hardware_enabled(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn connectivity(&self) -> u32 {
        if self.state.lock().unwrap().primary.is_some() {
            4
        } else {
            1
        }
    }

    #[zbus(property)]
    fn state(&self) -> u32 {
        if self.state.lock().unwrap().primary.is_some() {
            70
        } else {
            20
        }
    }

    async fn activate_connection(
        &self,
        connection: ObjectPath<'_>,
        device: ObjectPath<'_>,
        specific_object: ObjectPath<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let (profile, dev, ap) = {
            let s = self.state.lock().unwrap();
            let dev = device_index(&device);
            let profile = match index_of(&connection) {
                Some(i) => s.profiles.iter().find(|p| p.index == i).cloned(),
                // "/": whatever suits the device.
                None => s
                    .profiles
                    .iter()
                    .find(|p| match dev {
                        Some(DEV_WIRED) => p.kind == "802-3-ethernet",
                        Some(DEV_WIFI) => p.kind == "802-11-wireless",
                        _ => false,
                    })
                    .cloned(),
            };
            (profile, dev, index_of(&specific_object))
        };
        let Some(profile) = profile else {
            return Err(zbus::fdo::Error::Failed("no such profile".into()));
        };
        report(format!("activate {}", profile.id));
        let active = start_activation(&self.state, &profile, dev, ap);
        sync(&self.conn, &self.state).await;
        Ok(path("ActiveConnection", active))
    }

    async fn add_and_activate_connection(
        &self,
        connection: HashMap<String, HashMap<String, OwnedValue>>,
        device: ObjectPath<'_>,
        specific_object: ObjectPath<'_>,
    ) -> zbus::fdo::Result<(OwnedObjectPath, OwnedObjectPath)> {
        let c = connection.get("connection").cloned().unwrap_or_default();
        let id = text(&c, "id");
        let ssid = connection
            .get("802-11-wireless")
            .and_then(|w| w.get("ssid"))
            .map(bytes)
            .unwrap_or_default();
        let psk = connection
            .get("802-11-wireless-security")
            .map(|sec| text(sec, "psk"));
        match psk {
            Some(psk) => report(format!("add-activate {ssid} psk={psk}")),
            None => report(format!("add-activate {ssid}")),
        }
        let profile = {
            let mut s = self.state.lock().unwrap();
            let index = s.next;
            s.next += 1;
            let p = Profile {
                index,
                id: if id.is_empty() { ssid.clone() } else { id },
                kind: text(&c, "type"),
                ssid: Some(ssid),
            };
            s.profiles.push(p.clone());
            p
        };
        serve_profile(&self.conn, &self.state, profile.index).await;
        let active = start_activation(
            &self.state,
            &profile,
            device_index(&device),
            index_of(&specific_object),
        );
        sync(&self.conn, &self.state).await;
        Ok((
            path("Settings", profile.index),
            path("ActiveConnection", active),
        ))
    }

    async fn deactivate_connection(
        &self,
        active_connection: ObjectPath<'_>,
    ) -> zbus::fdo::Result<()> {
        let Some(index) = index_of(&active_connection) else {
            return Err(zbus::fdo::Error::Failed("no such connection".into()));
        };
        let id = {
            let s = self.state.lock().unwrap();
            s.active
                .iter()
                .find(|a| a.index == index)
                .and_then(|a| profile_id(&s, a.profile))
        };
        report(format!("deactivate {}", id.unwrap_or_default()));
        end_activation(&self.conn, &self.state, index, REASON_USER, false).await;
        Ok(())
    }
}

// --- the settings and the profiles -----------------------------------------

struct Settings {
    state: Shared,
}

#[interface(name = "org.freedesktop.NetworkManager.Settings")]
impl Settings {
    fn list_connections(&self) -> Vec<OwnedObjectPath> {
        self.state
            .lock()
            .unwrap()
            .profiles
            .iter()
            .map(|p| path("Settings", p.index))
            .collect()
    }

    #[zbus(signal)]
    async fn new_connection(
        emitter: &SignalEmitter<'_>,
        connection: OwnedObjectPath,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn connection_removed(
        emitter: &SignalEmitter<'_>,
        connection: OwnedObjectPath,
    ) -> zbus::Result<()>;
}

struct SettingsConnection {
    state: Shared,
    conn: Connection,
    index: u32,
}

#[interface(name = "org.freedesktop.NetworkManager.Settings.Connection")]
impl SettingsConnection {
    fn get_settings(&self) -> HashMap<String, HashMap<String, OwnedValue>> {
        let s = self.state.lock().unwrap();
        let mut out = HashMap::new();
        let Some(p) = s.profiles.iter().find(|p| p.index == self.index) else {
            return out;
        };
        let mut connection = HashMap::new();
        connection.insert("id".to_owned(), owned(Value::from(p.id.as_str())));
        connection.insert(
            "uuid".to_owned(),
            owned(Value::from(format!("uuid-{}", p.index))),
        );
        connection.insert("type".to_owned(), owned(Value::from(p.kind.as_str())));
        out.insert("connection".to_owned(), connection);
        if let Some(ssid) = &p.ssid {
            let mut wireless = HashMap::new();
            wireless.insert(
                "ssid".to_owned(),
                owned(Value::from(ssid.as_bytes().to_vec())),
            );
            out.insert("802-11-wireless".to_owned(), wireless);
        }
        out
    }

    async fn delete(&self) -> zbus::fdo::Result<()> {
        let id = {
            let mut s = self.state.lock().unwrap();
            let id = profile_id(&s, self.index).unwrap_or_default();
            s.profiles.retain(|p| p.index != self.index);
            id
        };
        report(format!("delete {id}"));
        let conn = self.conn.clone();
        let state = self.state.clone();
        let index = self.index;
        tokio::spawn(async move {
            let _ = conn
                .object_server()
                .remove::<SettingsConnection, _>(path("Settings", index))
                .await;
            let emitter = SignalEmitter::new(&conn, format!("{ROOT}/Settings")).unwrap();
            let _ = Settings::connection_removed(&emitter, path("Settings", index)).await;
            sync(&conn, &state).await;
        });
        Ok(())
    }
}

// --- the devices -----------------------------------------------------------

struct Device {
    state: Shared,
    conn: Connection,
    index: usize,
}

#[interface(name = "org.freedesktop.NetworkManager.Device")]
impl Device {
    #[zbus(property)]
    fn interface(&self) -> String {
        if self.index == DEV_WIRED {
            "eth0"
        } else {
            "wlan0"
        }
        .to_owned()
    }

    #[zbus(property)]
    fn device_type(&self) -> u32 {
        if self.index == DEV_WIRED { 1 } else { 2 }
    }

    #[zbus(property)]
    fn state(&self) -> u32 {
        self.state.lock().unwrap().device_state[self.index]
    }

    #[zbus(property)]
    fn managed(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn hw_address(&self) -> String {
        if self.index == DEV_WIRED {
            "00:11:22:33:44:55"
        } else {
            "66:77:88:99:AA:BB"
        }
        .to_owned()
    }

    #[zbus(property)]
    fn active_connection(&self) -> OwnedObjectPath {
        self.state
            .lock()
            .unwrap()
            .active
            .iter()
            .find(|a| a.device == Some(self.index))
            .map_or_else(none, |a| path("ActiveConnection", a.index))
    }

    #[zbus(property)]
    fn ip4_config(&self) -> OwnedObjectPath {
        if self.state.lock().unwrap().device_state[self.index] == STATE_ACTIVATED {
            path("IP4Config", self.index as u32 + 1)
        } else {
            none()
        }
    }

    #[zbus(property)]
    fn ip6_config(&self) -> OwnedObjectPath {
        none()
    }

    async fn disconnect(&self) -> zbus::fdo::Result<()> {
        report(format!("disconnect {}", self.interface()));
        let active = self
            .state
            .lock()
            .unwrap()
            .active
            .iter()
            .find(|a| a.device == Some(self.index))
            .map(|a| a.index);
        if let Some(index) = active {
            end_activation(&self.conn, &self.state, index, REASON_USER, false).await;
        }
        Ok(())
    }

    #[zbus(signal, name = "StateChanged")]
    async fn device_state_changed(
        emitter: &SignalEmitter<'_>,
        new_state: u32,
        old_state: u32,
        reason: u32,
    ) -> zbus::Result<()>;
}

struct Wired {
    state: Shared,
}

#[interface(name = "org.freedesktop.NetworkManager.Device.Wired")]
impl Wired {
    #[zbus(property)]
    fn carrier(&self) -> bool {
        self.state.lock().unwrap().carrier
    }

    #[zbus(property)]
    fn speed(&self) -> u32 {
        1000
    }
}

struct Wireless {
    state: Shared,
}

#[interface(name = "org.freedesktop.NetworkManager.Device.Wireless")]
impl Wireless {
    #[zbus(property)]
    fn active_access_point(&self) -> OwnedObjectPath {
        self.state
            .lock()
            .unwrap()
            .active
            .iter()
            .find(|a| a.device == Some(DEV_WIFI))
            .and_then(|a| a.specific)
            .map_or_else(none, |i| path("AccessPoint", i))
    }

    fn get_all_access_points(&self) -> Vec<OwnedObjectPath> {
        self.state
            .lock()
            .unwrap()
            .aps
            .iter()
            .map(|ap| path("AccessPoint", ap.index))
            .collect()
    }

    fn request_scan(&self, _options: HashMap<String, OwnedValue>) {
        report("scan".to_owned());
    }

    #[zbus(signal)]
    async fn access_point_added(
        emitter: &SignalEmitter<'_>,
        access_point: OwnedObjectPath,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn access_point_removed(
        emitter: &SignalEmitter<'_>,
        access_point: OwnedObjectPath,
    ) -> zbus::Result<()>;
}

struct AccessPoint {
    state: Shared,
    index: u32,
}

impl AccessPoint {
    fn get(&self) -> Option<Ap> {
        self.state
            .lock()
            .unwrap()
            .aps
            .iter()
            .find(|ap| ap.index == self.index)
            .cloned()
    }
}

#[interface(name = "org.freedesktop.NetworkManager.AccessPoint")]
impl AccessPoint {
    #[zbus(property)]
    fn ssid(&self) -> Vec<u8> {
        self.get()
            .map(|ap| ap.ssid.into_bytes())
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn strength(&self) -> u8 {
        self.get().map_or(0, |ap| ap.strength)
    }

    #[zbus(property)]
    fn flags(&self) -> u32 {
        // PRIVACY on anything but an open network.
        u32::from(self.get().is_some_and(|ap| ap.security != "open"))
    }

    #[zbus(property)]
    fn wpa_flags(&self) -> u32 {
        0
    }

    #[zbus(property)]
    fn rsn_flags(&self) -> u32 {
        match self.get().map(|ap| ap.security).as_deref() {
            Some("psk") => 0x100 | 0x8,
            Some("eap") => 0x200 | 0x8,
            _ => 0,
        }
    }

    #[zbus(property)]
    fn frequency(&self) -> u32 {
        5180
    }

    #[zbus(property)]
    fn max_bitrate(&self) -> u32 {
        866_000
    }

    #[zbus(property)]
    fn hw_address(&self) -> String {
        format!("AP:00:00:00:00:{:02X}", self.index)
    }
}

// --- the active connections and the IP config ------------------------------

struct ActiveConnection {
    state: Shared,
    index: u32,
}

impl ActiveConnection {
    fn get(&self) -> Option<(Active, Profile)> {
        let s = self.state.lock().unwrap();
        let a = s.active.iter().find(|a| a.index == self.index)?.clone();
        let p = s.profiles.iter().find(|p| p.index == a.profile)?.clone();
        Some((a, p))
    }
}

#[interface(name = "org.freedesktop.NetworkManager.Connection.Active")]
impl ActiveConnection {
    #[zbus(property)]
    fn id(&self) -> String {
        self.get().map(|(_, p)| p.id).unwrap_or_default()
    }

    #[zbus(property)]
    fn uuid(&self) -> String {
        self.get()
            .map(|(_, p)| format!("uuid-{}", p.index))
            .unwrap_or_default()
    }

    #[zbus(property, name = "Type")]
    fn kind(&self) -> String {
        self.get().map(|(_, p)| p.kind).unwrap_or_default()
    }

    #[zbus(property)]
    fn state(&self) -> u32 {
        self.get().map_or(0, |(a, _)| a.state)
    }

    #[zbus(property)]
    fn devices(&self) -> Vec<OwnedObjectPath> {
        self.get()
            .and_then(|(a, _)| a.device)
            .map(device_path)
            .into_iter()
            .collect()
    }

    #[zbus(property)]
    fn specific_object(&self) -> OwnedObjectPath {
        self.get()
            .and_then(|(a, _)| a.specific)
            .map_or_else(none, |i| path("AccessPoint", i))
    }

    #[zbus(property)]
    fn default(&self) -> bool {
        self.state.lock().unwrap().primary == Some(self.index)
    }

    #[zbus(property)]
    fn vpn(&self) -> bool {
        self.get().is_some_and(|(a, _)| a.vpn)
    }

    #[zbus(property)]
    fn ip4_config(&self) -> OwnedObjectPath {
        match self.get() {
            Some((a, _)) if a.state == ACTIVE_ACTIVATED => match a.device {
                Some(d) => path("IP4Config", d as u32 + 1),
                None => none(),
            },
            _ => none(),
        }
    }

    #[zbus(signal, name = "StateChanged")]
    async fn active_state_changed(
        emitter: &SignalEmitter<'_>,
        state: u32,
        reason: u32,
    ) -> zbus::Result<()>;
}

struct Ip4Config {
    index: u32,
}

#[interface(name = "org.freedesktop.NetworkManager.IP4Config")]
impl Ip4Config {
    #[zbus(property)]
    fn address_data(&self) -> Vec<HashMap<String, OwnedValue>> {
        let mut entry = HashMap::new();
        entry.insert(
            "address".to_owned(),
            owned(Value::from(format!("10.0.{}.5", self.index))),
        );
        entry.insert("prefix".to_owned(), owned(Value::from(24u32)));
        vec![entry]
    }

    #[zbus(property)]
    fn gateway(&self) -> String {
        format!("10.0.{}.1", self.index)
    }

    #[zbus(property)]
    fn nameserver_data(&self) -> Vec<HashMap<String, OwnedValue>> {
        ["1.1.1.1", "8.8.8.8"]
            .iter()
            .map(|dns| {
                let mut entry = HashMap::new();
                entry.insert("address".to_owned(), owned(Value::from(*dns)));
                entry
            })
            .collect()
    }
}

// --- helpers ---------------------------------------------------------------

fn owned(v: Value<'_>) -> OwnedValue {
    OwnedValue::try_from(v).unwrap()
}

fn text(map: &HashMap<String, OwnedValue>, key: &str) -> String {
    map.get(key)
        .and_then(|v| match &**v {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn bytes(v: &OwnedValue) -> String {
    match &**v {
        Value::Array(a) => String::from_utf8_lossy(
            &a.iter()
                .filter_map(|b| match b {
                    Value::U8(b) => Some(*b),
                    _ => None,
                })
                .collect::<Vec<_>>(),
        )
        .into_owned(),
        _ => String::new(),
    }
}

/// The trailing number of an object path, `None` for `/`.
fn index_of(p: &ObjectPath<'_>) -> Option<u32> {
    p.as_str().rsplit('/').next()?.parse().ok()
}

fn device_index(p: &ObjectPath<'_>) -> Option<usize> {
    index_of(p).map(|i| i as usize)
}

fn profile_id(s: &State, index: u32) -> Option<String> {
    s.profiles
        .iter()
        .find(|p| p.index == index)
        .map(|p| p.id.clone())
}

/// A new active connection in the Activating state, its device
/// connecting; waits for `finish` / `fail`.
fn start_activation(
    state: &Shared,
    profile: &Profile,
    device: Option<usize>,
    specific: Option<u32>,
) -> u32 {
    let mut s = state.lock().unwrap();
    let vpn = profile.kind == "vpn" || profile.kind == "wireguard";
    let device = if vpn {
        None
    } else {
        device.or(Some(if profile.kind == "802-3-ethernet" {
            DEV_WIRED
        } else {
            DEV_WIFI
        }))
    };
    // A Wi‑Fi activation by profile: the AP with its SSID.
    let specific = specific.or_else(|| {
        let ssid = profile.ssid.as_ref()?;
        s.aps.iter().find(|ap| ap.ssid == *ssid).map(|ap| ap.index)
    });
    // One connection per device.
    if let Some(d) = device {
        s.active.retain(|a| a.device != Some(d));
    }
    let index = s.next;
    s.next += 1;
    s.active.push(Active {
        index,
        profile: profile.index,
        device,
        specific,
        state: ACTIVE_ACTIVATING,
        vpn,
    });
    if let Some(d) = device {
        s.device_state[d] = STATE_CONFIG;
    }
    s.pending = Some(index);
    index
}

/// The pending activation reaches Activated: the device too, the
/// connection becomes primary (unless a VPN).
async fn finish_activation(conn: &Connection, state: &Shared) {
    let device = {
        let mut s = state.lock().unwrap();
        let Some(index) = s.pending.take() else {
            return;
        };
        let Some(a) = s.active.iter_mut().find(|a| a.index == index) else {
            return;
        };
        a.state = ACTIVE_ACTIVATED;
        let (device, vpn) = (a.device, a.vpn);
        if let Some(d) = device {
            s.device_state[d] = STATE_ACTIVATED;
        }
        if !vpn {
            s.primary = Some(index);
        }
        device
    };
    if let Some(d) = device {
        let emitter = SignalEmitter::new(conn, device_path(d)).unwrap();
        let _ = Device::device_state_changed(&emitter, STATE_ACTIVATED, STATE_CONFIG, 0).await;
    }
    sync(conn, state).await;
}

/// An active connection ends: deactivated with `reason` (the device's
/// state Failed with `device_reason` when it's a failure, else
/// Disconnected).
async fn end_activation(conn: &Connection, state: &Shared, index: u32, reason: u32, failed: bool) {
    let (device, reason, device_reason) = {
        let mut s = state.lock().unwrap();
        let Some(pos) = s.active.iter().position(|a| a.index == index) else {
            return;
        };
        let a = s.active.remove(pos);
        if s.primary == Some(index) {
            s.primary = None;
        }
        if s.pending == Some(index) {
            s.pending = None;
        }
        // A Wi‑Fi key refused: the device fails for NO_SECRETS and the
        // connection reports the device gone, as NetworkManager does;
        // a VPN wanting a secret says NO_SECRETS on the connection.
        let (reason, device_reason) = match (a.device, reason) {
            (Some(_), ACTIVE_REASON_NO_SECRETS | REASON_NO_SECRETS) => {
                (ACTIVE_REASON_DEVICE_DISCONNECTED, REASON_NO_SECRETS)
            }
            (_, r) => (r, r),
        };
        if let Some(d) = a.device {
            s.device_state[d] = if failed {
                STATE_FAILED
            } else {
                STATE_DISCONNECTED
            };
        }
        (a.device, reason, device_reason)
    };
    let emitter = SignalEmitter::new(conn, path("ActiveConnection", index)).unwrap();
    let _ = ActiveConnection::active_state_changed(&emitter, ACTIVE_DEACTIVATED, reason).await;
    if let Some(d) = device {
        let emitter = SignalEmitter::new(conn, device_path(d)).unwrap();
        let new = if failed {
            STATE_FAILED
        } else {
            STATE_DISCONNECTED
        };
        let _ = Device::device_state_changed(&emitter, new, STATE_CONFIG, device_reason).await;
        // NetworkManager settles a failed device back to Disconnected.
        if failed {
            state.lock().unwrap().device_state[d] = STATE_DISCONNECTED;
        }
    }
    let _ = conn
        .object_server()
        .remove::<ActiveConnection, _>(path("ActiveConnection", index))
        .await;
    sync(conn, state).await;
}

async fn serve_profile(conn: &Connection, state: &Shared, index: u32) {
    let _ = conn
        .object_server()
        .at(
            path("Settings", index),
            SettingsConnection {
                state: state.clone(),
                conn: conn.clone(),
                index,
            },
        )
        .await;
    let emitter = SignalEmitter::new(conn, format!("{ROOT}/Settings")).unwrap();
    let _ = Settings::new_connection(&emitter, path("Settings", index)).await;
}

/// Serve every active connection and access point the state has but
/// the bus doesn't yet, and announce every property as changed: the
/// shell re-reads the whole picture on any signal.
async fn sync(conn: &Connection, state: &Shared) {
    let (actives, aps) = {
        let s = state.lock().unwrap();
        (
            s.active.iter().map(|a| a.index).collect::<Vec<_>>(),
            s.aps.iter().map(|ap| ap.index).collect::<Vec<_>>(),
        )
    };
    let server = conn.object_server();
    for index in &actives {
        let _ = server
            .at(
                path("ActiveConnection", *index),
                ActiveConnection {
                    state: state.clone(),
                    index: *index,
                },
            )
            .await;
    }
    for index in &aps {
        let _ = server
            .at(
                path("AccessPoint", *index),
                AccessPoint {
                    state: state.clone(),
                    index: *index,
                },
            )
            .await;
    }
    if let Ok(m) = server.interface::<_, Manager>(ROOT).await {
        let e = m.signal_emitter();
        let m = m.get().await;
        let _ = m.active_connections_changed(e).await;
        let _ = m.primary_connection_changed(e).await;
        let _ = m.wireless_enabled_changed(e).await;
        let _ = m.connectivity_changed(e).await;
    }
    for d in [DEV_WIRED, DEV_WIFI] {
        if let Ok(dev) = server.interface::<_, Device>(device_path(d)).await {
            let e = dev.signal_emitter();
            let dev = dev.get().await;
            let _ = dev.state_changed(e).await;
            let _ = dev.active_connection_changed(e).await;
            let _ = dev.ip4_config_changed(e).await;
        }
    }
    if let Ok(w) = server.interface::<_, Wired>(device_path(DEV_WIRED)).await {
        let _ = w.get().await.carrier_changed(w.signal_emitter()).await;
    }
    if let Ok(w) = server.interface::<_, Wireless>(device_path(DEV_WIFI)).await {
        let _ = w
            .get()
            .await
            .active_access_point_changed(w.signal_emitter())
            .await;
    }
    for index in aps {
        if let Ok(ap) = server
            .interface::<_, AccessPoint>(path("AccessPoint", index))
            .await
        {
            let _ = ap.get().await.strength_changed(ap.signal_emitter()).await;
        }
    }
    for index in actives {
        if let Ok(a) = server
            .interface::<_, ActiveConnection>(path("ActiveConnection", index))
            .await
        {
            let _ = a.get().await.state_changed(a.signal_emitter()).await;
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> zbus::Result<()> {
    let state: Shared = Arc::new(Mutex::new(State {
        wireless: true,
        carrier: false,
        device_state: [20, STATE_DISCONNECTED],
        next: 1,
        ..State::default()
    }));
    // The scenario points DBUS_SYSTEM_BUS_ADDRESS at its session bus;
    // this side just joins the same bus.
    let conn = Connection::session().await?;
    let server = conn.object_server();
    server
        .at(
            ROOT,
            Manager {
                state: state.clone(),
                conn: conn.clone(),
            },
        )
        .await?;
    server
        .at(
            format!("{ROOT}/Settings"),
            Settings {
                state: state.clone(),
            },
        )
        .await?;
    for d in [DEV_WIRED, DEV_WIFI] {
        server
            .at(
                device_path(d),
                Device {
                    state: state.clone(),
                    conn: conn.clone(),
                    index: d,
                },
            )
            .await?;
    }
    server
        .at(
            device_path(DEV_WIRED),
            Wired {
                state: state.clone(),
            },
        )
        .await?;
    server
        .at(
            device_path(DEV_WIFI),
            Wireless {
                state: state.clone(),
            },
        )
        .await?;
    server
        .at(path("IP4Config", 1), Ip4Config { index: 1 })
        .await?;
    server
        .at(path("IP4Config", 2), Ip4Config { index: 2 })
        .await?;
    conn.request_name(NAME).await?;
    println!("ready");

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["ap", ssid, strength, security] => {
                let strength: u8 = strength.parse().unwrap_or(50);
                let (index, new) = {
                    let mut s = state.lock().unwrap();
                    match s.aps.iter_mut().find(|ap| ap.ssid == *ssid) {
                        Some(ap) => {
                            ap.strength = strength;
                            ap.security = (*security).to_owned();
                            (ap.index, false)
                        }
                        None => {
                            let index = s.next;
                            s.next += 1;
                            s.aps.push(Ap {
                                index,
                                ssid: (*ssid).to_owned(),
                                strength,
                                security: (*security).to_owned(),
                            });
                            (index, true)
                        }
                    }
                };
                sync(&conn, &state).await;
                if new {
                    let emitter = SignalEmitter::new(&conn, device_path(DEV_WIFI))?;
                    Wireless::access_point_added(&emitter, path("AccessPoint", index)).await?;
                }
            }
            ["ap-remove", ssid] => {
                let index = {
                    let mut s = state.lock().unwrap();
                    let index = s.aps.iter().find(|ap| ap.ssid == *ssid).map(|ap| ap.index);
                    s.aps.retain(|ap| ap.ssid != *ssid);
                    index
                };
                if let Some(index) = index {
                    let _ = conn
                        .object_server()
                        .remove::<AccessPoint, _>(path("AccessPoint", index))
                        .await;
                    let emitter = SignalEmitter::new(&conn, device_path(DEV_WIFI))?;
                    Wireless::access_point_removed(&emitter, path("AccessPoint", index)).await?;
                }
            }
            ["strength", ssid, strength] => {
                {
                    let mut s = state.lock().unwrap();
                    if let Some(ap) = s.aps.iter_mut().find(|ap| ap.ssid == *ssid) {
                        ap.strength = strength.parse().unwrap_or(0);
                    }
                }
                sync(&conn, &state).await;
            }
            ["known", ssid] | ["vpn", ssid] | ["wired", ssid] => {
                let index = {
                    let mut s = state.lock().unwrap();
                    let index = s.next;
                    s.next += 1;
                    let (kind, net) = match words[0] {
                        "known" => ("802-11-wireless", Some((*ssid).to_owned())),
                        "vpn" => ("vpn", None),
                        _ => ("802-3-ethernet", None),
                    };
                    s.profiles.push(Profile {
                        index,
                        id: (*ssid).to_owned(),
                        kind: kind.to_owned(),
                        ssid: net,
                    });
                    index
                };
                serve_profile(&conn, &state, index).await;
            }
            ["carrier", on] => {
                {
                    let mut s = state.lock().unwrap();
                    s.carrier = *on == "on";
                    if s.device_state[DEV_WIRED] != STATE_ACTIVATED {
                        s.device_state[DEV_WIRED] = if s.carrier { STATE_DISCONNECTED } else { 20 };
                    }
                }
                sync(&conn, &state).await;
            }
            ["wired-up"] => {
                let profile = state
                    .lock()
                    .unwrap()
                    .profiles
                    .iter()
                    .find(|p| p.kind == "802-3-ethernet")
                    .cloned();
                if let Some(p) = profile {
                    state.lock().unwrap().carrier = true;
                    start_activation(&state, &p, Some(DEV_WIRED), None);
                    finish_activation(&conn, &state).await;
                }
            }
            ["finish"] => finish_activation(&conn, &state).await,
            ["fail", how] => {
                let pending = state.lock().unwrap().pending;
                if let Some(index) = pending {
                    let reason = match *how {
                        "nosecrets" => ACTIVE_REASON_NO_SECRETS,
                        _ => REASON_NO_SECRETS,
                    };
                    end_activation(&conn, &state, index, reason, true).await;
                }
            }
            ["quit"] => break,
            _ => eprintln!("unknown command: {line}"),
        }
        println!("ok");
    }
    Ok(())
}
