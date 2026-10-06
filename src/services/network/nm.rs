//! NetworkManager over the system bus: the event stream (its name
//! coming and going, every signal it emits under its path folded into
//! one debounced re-read of the [`Snapshot`]) and the calls behind the
//! [`Command`]s.
//!
//! Reads go through `org.freedesktop.DBus.Properties.GetAll`, one
//! round trip per object and interface, rather than a property proxy
//! each (the objects are many and short-lived); the calls through
//! `#[proxy]` traits with only the methods used.
//!
//! Reference: <https://networkmanager.dev/docs/api/latest/> (the
//! `NM*` enums are `nm-dbus-interface.h`).

use std::collections::HashMap;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::future::join_all;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream as iced_stream;
use zbus::fdo::{DBusProxy, PropertiesProxy};
use zbus::message::Type as MessageType;
use zbus::names::{BusName, InterfaceName};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream, proxy};

use super::{
    AccessPoint, ActiveConnection, ActiveState, Connectivity, Device, DeviceKind, DeviceState,
    Event, IpConfig, Profile, ProfileKind, RawAccessPoint, Security, Snapshot,
};

pub const NAME: &str = "org.freedesktop.NetworkManager";
const PATH: &str = "/org/freedesktop/NetworkManager";
const IFACE_MANAGER: &str = "org.freedesktop.NetworkManager";
const IFACE_DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const IFACE_WIRED: &str = "org.freedesktop.NetworkManager.Device.Wired";
const IFACE_WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const IFACE_AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
const IFACE_ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const IFACE_IP4: &str = "org.freedesktop.NetworkManager.IP4Config";
const IFACE_IP6: &str = "org.freedesktop.NetworkManager.IP6Config";

/// `NMDeviceType`.
const DEVICE_ETHERNET: u32 = 1;
const DEVICE_WIFI: u32 = 2;

/// How long after the last signal the snapshot is re-read.
const DEBOUNCE: Duration = Duration::from_millis(200);

#[proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
trait Manager {
    fn activate_connection(
        &self,
        connection: &ObjectPath<'_>,
        device: &ObjectPath<'_>,
        specific_object: &ObjectPath<'_>,
    ) -> zbus::Result<OwnedObjectPath>;

    fn add_and_activate_connection(
        &self,
        connection: HashMap<&str, HashMap<&str, Value<'_>>>,
        device: &ObjectPath<'_>,
        specific_object: &ObjectPath<'_>,
    ) -> zbus::Result<(OwnedObjectPath, OwnedObjectPath)>;

    fn deactivate_connection(&self, active_connection: &ObjectPath<'_>) -> zbus::Result<()>;

    #[zbus(property)]
    fn set_wireless_enabled(&self, enabled: bool) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Settings",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Settings"
)]
trait Settings {
    fn list_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Settings.Connection",
    default_service = "org.freedesktop.NetworkManager"
)]
trait SettingsConnection {
    fn get_settings(&self) -> zbus::Result<HashMap<String, HashMap<String, OwnedValue>>>;
    fn delete(&self) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Device",
    default_service = "org.freedesktop.NetworkManager"
)]
trait DeviceMethods {
    fn disconnect(&self) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Device.Wireless",
    default_service = "org.freedesktop.NetworkManager"
)]
trait Wireless {
    fn get_all_access_points(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
    fn request_scan(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;
}

/// The event stream: the bus, then NetworkManager's presence and a
/// snapshot after every burst of its signals.
pub fn events() -> impl Stream<Item = Event> {
    iced_stream::channel(64, async move |mut out: mpsc::Sender<Event>| {
        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("network: no system bus: {e}");
                return;
            }
        };
        let _ = out.send(Event::Connected(conn.clone())).await;
        if let Err(e) = follow(conn, out).await {
            log::error!("network: {e}");
        }
    })
}

/// A signal we care about, already sorted out.
enum Signal {
    /// NetworkManager's name changed hands (`Some` when owned).
    Owner(bool),
    /// Anything else under its path: a snapshot is due.
    Changed,
    DeviceFailed {
        device: String,
        reason: u32,
    },
    ActiveFailed {
        active: String,
        reason: u32,
    },
}

async fn follow(conn: Connection, mut out: mpsc::Sender<Event>) -> zbus::Result<()> {
    let dbus = DBusProxy::new(&conn).await?;
    // Subscribe before asking, so nothing in between is lost.
    let owner = dbus
        .receive_name_owner_changed_with_args(&[(0, NAME)])
        .await?
        .filter_map(|s| async move {
            s.args()
                .ok()
                .map(|a| Signal::Owner(a.new_owner().is_some()))
        });
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(NAME)?
        .path_namespace(PATH)?
        .build();
    let signals = MessageStream::for_match_rule(rule, &conn, None)
        .await?
        .filter_map(|m| async move { m.ok().and_then(|m| classify(&m)) });
    let mut signals = iced::futures::stream::select(owner.boxed(), signals.boxed());

    let mut running = dbus
        .name_has_owner(BusName::try_from(NAME)?)
        .await
        .unwrap_or(false);
    let _ = out.send(Event::Running(running)).await;
    let mut due = running;
    loop {
        let next = if due {
            // Debounce: a snapshot once the signals pause.
            match tokio::time::timeout(DEBOUNCE, signals.next()).await {
                Ok(next) => next,
                Err(_) => {
                    due = false;
                    match snapshot(&conn).await {
                        Ok(s) => {
                            let _ = out.send(Event::Snapshot(Box::new(s))).await;
                        }
                        Err(e) => log::warn!("network: snapshot: {e}"),
                    }
                    continue;
                }
            }
        } else {
            signals.next().await
        };
        let Some(signal) = next else { break };
        match signal {
            Signal::Owner(owned) => {
                if owned != running {
                    running = owned;
                    log::info!(
                        "network: NetworkManager {}",
                        if owned { "is up" } else { "left the bus" }
                    );
                    let _ = out.send(Event::Running(owned)).await;
                }
                due = owned;
            }
            Signal::Changed => due = running,
            Signal::DeviceFailed { device, reason } => {
                let _ = out.send(Event::DeviceFailed { device, reason }).await;
                due = running;
            }
            Signal::ActiveFailed { active, reason } => {
                let _ = out.send(Event::ActiveFailed { active, reason }).await;
                due = running;
            }
        }
    }
    Ok(())
}

/// What a signal from NetworkManager means to us.
fn classify(m: &zbus::Message) -> Option<Signal> {
    let header = m.header();
    let iface = header.interface()?.as_str();
    let member = header.member()?.as_str();
    let path = header.path()?.to_string();
    match (iface, member) {
        (IFACE_DEVICE, "StateChanged") => {
            // (new_state u, old_state u, reason u)
            let (new, _old, reason): (u32, u32, u32) = m.body().deserialize().ok()?;
            if DeviceState::from_nm(new) == DeviceState::Failed {
                Some(Signal::DeviceFailed {
                    device: path,
                    reason,
                })
            } else {
                Some(Signal::Changed)
            }
        }
        (IFACE_ACTIVE, "StateChanged") => {
            // (state u, reason u)
            let (state, reason): (u32, u32) = m.body().deserialize().ok()?;
            if ActiveState::from_nm(state) == ActiveState::Deactivated {
                Some(Signal::ActiveFailed {
                    active: path,
                    reason,
                })
            } else {
                Some(Signal::Changed)
            }
        }
        _ => Some(Signal::Changed),
    }
}

// --- reading ---------------------------------------------------------------

type Props = HashMap<String, OwnedValue>;

/// `GetAll` on one interface of one object; an empty map when the
/// object is gone meanwhile (the snapshot goes on without it).
async fn props(conn: &Connection, path: &str, iface: &str) -> Props {
    let proxy = match PropertiesProxy::builder(conn)
        .destination(NAME)
        .and_then(|b| b.path(path.to_owned()))
        .map(|b| b.cache_properties(CacheProperties::No))
    {
        Ok(b) => match b.build().await {
            Ok(p) => p,
            Err(_) => return Props::new(),
        },
        Err(_) => return Props::new(),
    };
    let Ok(iface) = InterfaceName::try_from(iface.to_owned()) else {
        return Props::new();
    };
    match proxy.get_all(iface).await {
        Ok(p) => p,
        Err(e) => {
            log::debug!("network: {path} {e}");
            Props::new()
        }
    }
}

fn string(p: &Props, key: &str) -> String {
    p.get(key)
        .and_then(|v| match &**v {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn u32_of(p: &Props, key: &str) -> u32 {
    p.get(key)
        .and_then(|v| match &**v {
            Value::U32(n) => Some(*n),
            Value::U8(n) => Some(u32::from(*n)),
            Value::I32(n) => u32::try_from(*n).ok(),
            _ => None,
        })
        .unwrap_or_default()
}

fn bool_of(p: &Props, key: &str) -> bool {
    p.get(key)
        .and_then(|v| bool::try_from(&**v).ok())
        .unwrap_or(false)
}

/// An object path, `None` for `/` (NetworkManager's "none").
fn path_of(p: &Props, key: &str) -> Option<String> {
    p.get(key)
        .and_then(|v| match &**v {
            Value::ObjectPath(o) => Some(o.to_string()),
            _ => None,
        })
        .filter(|s| s != "/")
}

fn paths_of(p: &Props, key: &str) -> Vec<String> {
    p.get(key)
        .and_then(|v| match &**v {
            Value::Array(a) => Some(
                a.iter()
                    .filter_map(|v| match v {
                        Value::ObjectPath(o) => Some(o.to_string()),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

/// An `ay` as text (an SSID is bytes; UTF‑8 in practice).
fn bytes_string(v: &Value<'_>) -> String {
    match v {
        Value::Array(a) => {
            let bytes: Vec<u8> = a
                .iter()
                .filter_map(|b| match b {
                    Value::U8(b) => Some(*b),
                    _ => None,
                })
                .collect();
            String::from_utf8_lossy(&bytes).into_owned()
        }
        Value::Str(s) => s.to_string(),
        _ => String::new(),
    }
}

/// The `address` entries of an `aa{sv}` (`AddressData`, `NameserverData`),
/// with `/prefix` when asked.
fn address_data(p: &Props, key: &str, with_prefix: bool) -> Vec<String> {
    let Some(v) = p.get(key) else {
        return Vec::new();
    };
    let Value::Array(entries) = &**v else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|e| {
            let Value::Dict(d) = e else { return None };
            let mut address = None;
            let mut prefix = None;
            for (k, v) in d.iter() {
                let Value::Str(k) = k else { continue };
                let mut v = v;
                while let Value::Value(inner) = v {
                    v = inner;
                }
                match (k.as_str(), v) {
                    ("address", Value::Str(s)) => address = Some(s.to_string()),
                    ("prefix", Value::U32(n)) => prefix = Some(*n),
                    _ => {}
                }
            }
            let address = address?;
            Some(match (with_prefix, prefix) {
                (true, Some(n)) => format!("{address}/{n}"),
                _ => address,
            })
        })
        .collect()
}

async fn ip_config(conn: &Connection, path: Option<String>, iface: &str) -> Option<IpConfig> {
    let path = path?;
    let p = props(conn, &path, iface).await;
    if p.is_empty() {
        return None;
    }
    Some(IpConfig {
        addresses: address_data(&p, "AddressData", true),
        gateway: string(&p, "Gateway"),
        dns: address_data(&p, "NameserverData", false),
    })
}

async fn device(conn: &Connection, path: String) -> (Option<Device>, Vec<RawAccessPoint>) {
    let p = props(conn, &path, IFACE_DEVICE).await;
    let kind = match u32_of(&p, "DeviceType") {
        DEVICE_ETHERNET => DeviceKind::Wired,
        DEVICE_WIFI => DeviceKind::Wifi,
        _ => return (None, Vec::new()),
    };
    if !bool_of(&p, "Managed") {
        return (None, Vec::new());
    }
    let (ip4, ip6) = iced::futures::join!(
        ip_config(conn, path_of(&p, "Ip4Config"), IFACE_IP4),
        ip_config(conn, path_of(&p, "Ip6Config"), IFACE_IP6),
    );
    let mut d = Device {
        path: path.clone(),
        iface: string(&p, "Interface"),
        kind,
        state: DeviceState::from_nm(u32_of(&p, "State")),
        carrier: true,
        speed: 0,
        hw_address: string(&p, "HwAddress"),
        active_ap: None,
        active: path_of(&p, "ActiveConnection"),
        ip4,
        ip6,
    };
    let mut aps = Vec::new();
    match kind {
        DeviceKind::Wired => {
            let w = props(conn, &path, IFACE_WIRED).await;
            d.carrier = bool_of(&w, "Carrier");
            d.speed = u32_of(&w, "Speed");
        }
        DeviceKind::Wifi => {
            let w = props(conn, &path, IFACE_WIRELESS).await;
            d.active_ap = path_of(&w, "ActiveAccessPoint");
            let listed = match WirelessProxy::builder(conn)
                .path(path.clone())
                .map(|b| b.cache_properties(CacheProperties::No))
            {
                Ok(b) => match b.build().await {
                    Ok(w) => w.get_all_access_points().await.unwrap_or_default(),
                    Err(_) => Vec::new(),
                },
                Err(_) => Vec::new(),
            };
            let reads = listed.into_iter().map(|ap| {
                let ap = ap.to_string();
                let device = path.clone();
                async move {
                    let a = props(conn, &ap, IFACE_AP).await;
                    if a.is_empty() {
                        return None;
                    }
                    Some(RawAccessPoint {
                        path: ap,
                        device,
                        ssid: a.get("Ssid").map(|v| bytes_string(v)).unwrap_or_default(),
                        strength: u32_of(&a, "Strength").min(100) as u8,
                        security: Security::from_flags(
                            u32_of(&a, "Flags"),
                            u32_of(&a, "WpaFlags"),
                            u32_of(&a, "RsnFlags"),
                        ),
                        frequency: u32_of(&a, "Frequency"),
                        max_bitrate: u32_of(&a, "MaxBitrate"),
                        bssid: string(&a, "HwAddress"),
                    })
                }
            });
            aps = join_all(reads).await.into_iter().flatten().collect();
        }
    }
    (Some(d), aps)
}

async fn profile(conn: &Connection, path: String) -> Option<Profile> {
    let proxy = SettingsConnectionProxy::builder(conn)
        .path(path.clone())
        .ok()?
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .ok()?;
    let settings = proxy.get_settings().await.ok()?;
    let connection = settings.get("connection")?;
    let kind = string(connection, "type");
    let ssid = settings
        .get("802-11-wireless")
        .and_then(|w| w.get("ssid"))
        .map(|v| bytes_string(v))
        .filter(|s| !s.is_empty());
    Some(Profile {
        path,
        uuid: string(connection, "uuid"),
        id: string(connection, "id"),
        kind: ProfileKind::from_nm(&kind),
        ssid,
    })
}

async fn active_connection(conn: &Connection, path: String) -> Option<ActiveConnection> {
    let a = props(conn, &path, IFACE_ACTIVE).await;
    if a.is_empty() {
        return None;
    }
    let ip4 = ip_config(conn, path_of(&a, "Ip4Config"), IFACE_IP4).await;
    Some(ActiveConnection {
        path,
        uuid: string(&a, "Uuid"),
        id: string(&a, "Id"),
        kind: ProfileKind::from_nm(&string(&a, "Type")),
        state: ActiveState::from_nm(u32_of(&a, "State")),
        devices: paths_of(&a, "Devices"),
        specific: path_of(&a, "SpecificObject").unwrap_or_default(),
        default: bool_of(&a, "Default") || bool_of(&a, "Default6"),
        vpn: bool_of(&a, "Vpn"),
        ip4,
    })
}

/// Everything at once: the manager, then its devices (with their
/// access points), the profiles and the active connections, each list
/// read concurrently.
pub async fn snapshot(conn: &Connection) -> zbus::Result<Snapshot> {
    let m = props(conn, PATH, IFACE_MANAGER).await;
    if m.is_empty() {
        return Err(zbus::Error::Failure("no answer from NetworkManager".into()));
    }
    let settings = SettingsProxy::builder(conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let listed = settings.list_connections().await.unwrap_or_default();
    let (devices, profiles, active) = iced::futures::join!(
        join_all(paths_of(&m, "Devices").into_iter().map(|p| device(conn, p))),
        join_all(listed.into_iter().map(|p| profile(conn, p.to_string()))),
        join_all(
            paths_of(&m, "ActiveConnections")
                .into_iter()
                .map(|p| active_connection(conn, p))
        ),
    );
    let mut aps = Vec::new();
    let devices = devices
        .into_iter()
        .filter_map(|(d, mut found)| {
            aps.append(&mut found);
            d
        })
        .collect();
    Ok(Snapshot {
        wireless_enabled: bool_of(&m, "WirelessEnabled"),
        wireless_hw_enabled: bool_of(&m, "WirelessHardwareEnabled"),
        connectivity: Connectivity::from_nm(u32_of(&m, "Connectivity")),
        primary: path_of(&m, "PrimaryConnection"),
        devices,
        aps,
        profiles: profiles.into_iter().flatten().collect(),
        active: active.into_iter().flatten().collect(),
    })
}

// --- calls -----------------------------------------------------------------

async fn manager(conn: &Connection) -> zbus::Result<ManagerProxy<'_>> {
    ManagerProxy::builder(conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await
}

pub async fn set_wireless(conn: &Connection, enabled: bool) -> zbus::Result<()> {
    manager(conn).await?.set_wireless_enabled(enabled).await
}

pub async fn request_scan(conn: &Connection, device: &str) -> zbus::Result<()> {
    let w = WirelessProxy::builder(conn)
        .path(device.to_owned())?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    match w.request_scan(HashMap::new()).await {
        // "Scanning not allowed immediately following previous scan":
        // NetworkManager rate-limits, nothing to report.
        Err(zbus::Error::MethodError(name, _, _)) if name.contains("NotAllowed") => Ok(()),
        r => r,
    }
}

pub async fn activate(
    conn: &Connection,
    profile: &str,
    device: &str,
    specific: &str,
) -> zbus::Result<()> {
    manager(conn)
        .await?
        .activate_connection(
            &ObjectPath::try_from(profile)?,
            &ObjectPath::try_from(device)?,
            &ObjectPath::try_from(specific)?,
        )
        .await
        .map(|_| ())
}

/// A new profile for `ap`, activated at once; the profile's path.
/// Owned by this user (`connection.permissions`), so
/// `settings.modify.own` is enough (`.system` asks the admin's password
/// on most polkit setups); the key stored by NetworkManager itself.
pub async fn add_and_activate(
    conn: &Connection,
    ap: &AccessPoint,
    password: Option<&str>,
) -> zbus::Result<String> {
    let login = crate::components::locker::login();
    let settings = connection_settings(ap, password, &login);
    let (path, _active) = manager(conn)
        .await?
        .add_and_activate_connection(
            settings,
            &ObjectPath::try_from(ap.device.as_str())?,
            &ObjectPath::try_from(ap.path.as_str())?,
        )
        .await?;
    Ok(path.to_string())
}

/// The `a{sa{sv}}` of a Wi‑Fi profile for `ap`.
fn connection_settings<'a>(
    ap: &'a AccessPoint,
    password: Option<&'a str>,
    login: &'a str,
) -> HashMap<&'a str, HashMap<&'a str, Value<'a>>> {
    let mut connection: HashMap<&str, Value> = HashMap::new();
    connection.insert("id", Value::from(ap.ssid.as_str()));
    connection.insert("type", Value::from("802-11-wireless"));
    if !login.is_empty() {
        connection.insert("permissions", Value::from(vec![format!("user:{login}:")]));
    }
    let mut wireless: HashMap<&str, Value> = HashMap::new();
    wireless.insert("ssid", Value::from(ap.ssid.as_bytes().to_vec()));
    wireless.insert("mode", Value::from("infrastructure"));
    let mut settings = HashMap::new();
    settings.insert("connection", connection);
    settings.insert("802-11-wireless", wireless);
    if let Some(password) = password {
        let mut security: HashMap<&str, Value> = HashMap::new();
        match ap.security {
            Security::Wep => {
                security.insert("key-mgmt", Value::from("none"));
                security.insert("wep-key0", Value::from(password));
                security.insert("wep-key-type", Value::from(1u32));
            }
            Security::Sae => {
                security.insert("key-mgmt", Value::from("sae"));
                security.insert("psk", Value::from(password));
            }
            _ => {
                security.insert("key-mgmt", Value::from("wpa-psk"));
                security.insert("psk", Value::from(password));
            }
        }
        settings.insert("802-11-wireless-security", security);
    }
    settings
}

pub async fn deactivate(conn: &Connection, active: &str) -> zbus::Result<()> {
    manager(conn)
        .await?
        .deactivate_connection(&ObjectPath::try_from(active)?)
        .await
}

pub async fn disconnect(conn: &Connection, device: &str) -> zbus::Result<()> {
    DeviceMethodsProxy::builder(conn)
        .path(device.to_owned())?
        .cache_properties(CacheProperties::No)
        .build()
        .await?
        .disconnect()
        .await
}

pub async fn delete_profile(conn: &Connection, path: &str) -> zbus::Result<()> {
    SettingsConnectionProxy::builder(conn)
        .path(path.to_owned())?
        .cache_properties(CacheProperties::No)
        .build()
        .await?
        .delete()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{Array, Dict, Signature};

    fn owned(v: Value<'_>) -> OwnedValue {
        OwnedValue::try_from(v).unwrap()
    }

    #[test]
    fn property_readers() {
        let mut p = Props::new();
        p.insert("Interface".into(), owned(Value::from("wlan0")));
        p.insert("State".into(), owned(Value::from(100u32)));
        p.insert("Strength".into(), owned(Value::from(73u8)));
        p.insert("Managed".into(), owned(Value::from(true)));
        p.insert(
            "ActiveConnection".into(),
            owned(Value::from(ObjectPath::try_from("/").unwrap())),
        );
        p.insert(
            "Ip4Config".into(),
            owned(Value::from(
                ObjectPath::try_from("/org/freedesktop/NetworkManager/IP4Config/3").unwrap(),
            )),
        );
        let mut devices = Array::new(&Signature::ObjectPath);
        devices
            .append(Value::from(ObjectPath::try_from("/d/1").unwrap()))
            .unwrap();
        p.insert("Devices".into(), owned(Value::from(devices)));
        p.insert("Ssid".into(), owned(Value::from(b"Casa".to_vec())));
        assert_eq!(string(&p, "Interface"), "wlan0");
        assert_eq!(u32_of(&p, "State"), 100);
        assert_eq!(u32_of(&p, "Strength"), 73);
        assert!(bool_of(&p, "Managed"));
        assert_eq!(path_of(&p, "ActiveConnection"), None);
        assert_eq!(
            path_of(&p, "Ip4Config").as_deref(),
            Some("/org/freedesktop/NetworkManager/IP4Config/3")
        );
        assert_eq!(paths_of(&p, "Devices"), ["/d/1"]);
        assert_eq!(bytes_string(&p["Ssid"]), "Casa");
        assert_eq!(string(&p, "Missing"), "");
    }

    #[test]
    fn address_data_entries() {
        let mut entry = Dict::new(&Signature::Str, &Signature::Variant);
        entry
            .add("address", Value::new(Value::from("10.0.0.5")))
            .unwrap();
        entry.add("prefix", Value::new(Value::from(24u32))).unwrap();
        let mut list = Array::new(&Signature::try_from("a{sv}").unwrap());
        list.append(Value::from(entry)).unwrap();
        let mut p = Props::new();
        p.insert("AddressData".into(), owned(Value::from(list)));
        assert_eq!(address_data(&p, "AddressData", true), ["10.0.0.5/24"]);
        assert_eq!(address_data(&p, "AddressData", false), ["10.0.0.5"]);
        assert!(address_data(&p, "Nope", true).is_empty());
    }

    #[test]
    fn wifi_profile_settings() {
        let ap = AccessPoint {
            path: "/ap/1".into(),
            device: "/dev/1".into(),
            ssid: "Casa".into(),
            strength: 50,
            security: Security::Psk,
            frequency: 2412,
            max_bitrate: 54000,
            bssid: String::new(),
            known: None,
            active: false,
            connecting: false,
        };
        let s = connection_settings(&ap, Some("secret"), "dave");
        assert_eq!(s["connection"]["id"], Value::from("Casa"));
        assert_eq!(
            s["connection"]["permissions"],
            Value::from(vec!["user:dave:".to_owned()])
        );
        assert_eq!(s["802-11-wireless"]["ssid"], Value::from(b"Casa".to_vec()));
        assert_eq!(
            s["802-11-wireless-security"]["key-mgmt"],
            Value::from("wpa-psk")
        );
        assert_eq!(s["802-11-wireless-security"]["psk"], Value::from("secret"));
        let open = connection_settings(&ap, None, "");
        assert!(!open.contains_key("802-11-wireless-security"));
        assert!(!open["connection"].contains_key("permissions"));
        let sae = AccessPoint {
            security: Security::Sae,
            ..ap
        };
        assert_eq!(
            connection_settings(&sae, Some("x"), "")["802-11-wireless-security"]["key-mgmt"],
            Value::from("sae")
        );
    }
}
