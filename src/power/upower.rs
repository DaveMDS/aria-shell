//! UPower and the power profiles over the system bus: the event stream
//! (either service coming and going, every signal under
//! `/org/freedesktop/UPower` folded into one debounced re-read of both)
//! and the calls behind the [`Command`](super::Command)s; the low
//! battery notification over the session bus, as any app sends one.
//!
//! The profiles are `org.freedesktop.UPower.PowerProfiles`, served by
//! power-profiles-daemon and tuned-ppd alike, at a path under UPower's.
//!
//! References: <https://upower.freedesktop.org/docs/>,
//! <https://upower.pages.freedesktop.org/power-profiles-daemon/>

use std::collections::HashMap;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream;
use zbus::fdo::{DBusProxy, PropertiesProxy};
use zbus::message::Type as MessageType;
use zbus::names::{BusName, InterfaceName};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream, proxy};

use super::{Battery, Device, Event, Profiles, Snapshot, State, Warning};

const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const IFACE_DEVICE: &str = "org.freedesktop.UPower.Device";
const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

/// `Type` of a UPower device.
const TYPE_LINE_POWER: u32 = 1;
const TYPE_BATTERY: u32 = 2;

/// How long after the last signal both are re-read.
const DEBOUNCE: Duration = Duration::from_millis(200);

#[proxy(
    interface = "org.freedesktop.UPower",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower"
)]
trait UPower {
    fn enumerate_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[proxy(
    interface = "org.freedesktop.UPower.PowerProfiles",
    default_service = "org.freedesktop.UPower.PowerProfiles",
    default_path = "/org/freedesktop/UPower/PowerProfiles"
)]
trait PowerProfiles {
    #[zbus(property)]
    fn set_active_profile(&self, profile: &str) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    fn close_notification(&self, id: u32) -> zbus::Result<()>;
}

/// The event stream: the bus, then both services' state after every
/// burst of their signals.
pub fn events() -> impl Stream<Item = Event> {
    stream::channel(16, async move |mut out: mpsc::Sender<Event>| {
        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("power: no system bus: {e}");
                return;
            }
        };
        let _ = out.send(Event::Bus(conn.clone())).await;
        if let Err(e) = follow(conn, out).await {
            log::error!("power: {e}");
        }
    })
}

async fn follow(conn: Connection, mut out: mpsc::Sender<Event>) -> zbus::Result<()> {
    let dbus = DBusProxy::new(&conn).await?;
    // Subscribe before reading, so nothing in between is lost.
    let owners = dbus
        .receive_name_owner_changed()
        .await?
        .filter_map(|s| async move {
            let a = s.args().ok()?;
            [UPOWER, PROFILES].contains(&a.name().as_str()).then_some(())
        });
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .path_namespace(UPOWER_PATH)?
        .build();
    let signals = MessageStream::for_match_rule(rule, &conn, None)
        .await?
        .map(|_| ());
    let mut signals = iced::futures::stream::select(owners.boxed(), signals.boxed());

    let mut due = true;
    loop {
        if due {
            // Debounce: a re-read once the signals pause.
            match tokio::time::timeout(DEBOUNCE, signals.next()).await {
                Ok(Some(())) => continue,
                Ok(None) => break,
                Err(_) => {
                    due = false;
                    let upower = snapshot(&conn, &dbus).await.map(Box::new);
                    let profiles = profiles(&conn, &dbus).await;
                    if out.send(Event::UPower(upower)).await.is_err()
                        || out.send(Event::Profiles(profiles)).await.is_err()
                    {
                        break;
                    }
                }
            }
        } else {
            match signals.next().await {
                Some(()) => due = true,
                None => break,
            }
        }
    }
    Ok(())
}

async fn running(dbus: &DBusProxy<'_>, name: &'static str) -> bool {
    match BusName::try_from(name) {
        Ok(n) => dbus.name_has_owner(n).await.unwrap_or(false),
        Err(_) => false,
    }
}

/// UPower's state; `None` when it isn't running.
async fn snapshot(conn: &Connection, dbus: &DBusProxy<'_>) -> Option<Snapshot> {
    if !running(dbus, UPOWER).await {
        return None;
    }
    let root = props(conn, UPOWER, UPOWER_PATH, UPOWER).await;
    let display = props(conn, UPOWER, DISPLAY_DEVICE, IFACE_DEVICE).await;
    let battery = (u32_of(&display, "Type") == TYPE_BATTERY && bool_of(&display, "IsPresent"))
        .then(|| battery(&display));
    let mut devices = Vec::new();
    let mut capacity = None;
    let paths = match UPowerProxy::new(conn).await {
        Ok(p) => p.enumerate_devices().await.unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    for path in paths {
        let p = props(conn, UPOWER, path.as_str(), IFACE_DEVICE).await;
        let kind = u32_of(&p, "Type");
        if kind == TYPE_LINE_POWER || !bool_of(&p, "IsPresent") {
            continue;
        }
        if bool_of(&p, "PowerSupply") {
            // The laptop's own battery: its health goes with the
            // display device's figures (the first one found).
            if kind == TYPE_BATTERY && capacity.is_none() {
                capacity = Some(f64_of(&p, "Capacity")).filter(|c| *c > 0.0);
            }
            continue;
        }
        devices.push(Device {
            path: path.to_string(),
            kind,
            model: string(&p, "Model"),
            percentage: f64_of(&p, "Percentage"),
            state: State::from_upower(u32_of(&p, "State")),
            icon: string(&p, "IconName"),
        });
    }
    devices.sort_by(|a, b| a.model.cmp(&b.model).then(a.path.cmp(&b.path)));
    Some(Snapshot {
        on_battery: bool_of(&root, "OnBattery"),
        battery: battery.map(|b| Battery { capacity, ..b }),
        devices,
    })
}

fn battery(p: &Props) -> Battery {
    let secs = |key| u64::try_from(i64_of(p, key)).unwrap_or(0);
    Battery {
        percentage: f64_of(p, "Percentage"),
        state: State::from_upower(u32_of(p, "State")),
        time_to_empty: secs("TimeToEmpty"),
        time_to_full: secs("TimeToFull"),
        energy_rate: f64_of(p, "EnergyRate"),
        warning: Warning::from_upower(u32_of(p, "WarningLevel")),
        icon: string(p, "IconName"),
        capacity: None,
    }
}

/// The profiles' state; `None` when no daemon serves them.
async fn profiles(conn: &Connection, dbus: &DBusProxy<'_>) -> Option<Profiles> {
    if !running(dbus, PROFILES).await {
        return None;
    }
    let p = props(conn, PROFILES, PROFILES_PATH, PROFILES).await;
    let available = match p.get("Profiles").map(|v| &**v) {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|d| match d {
                Value::Dict(d) => d
                    .get::<&str, String>(&"Profile")
                    .ok()
                    .flatten(),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    Some(Profiles {
        active: string(&p, "ActiveProfile"),
        available,
        degraded: string(&p, "PerformanceDegraded"),
    })
}

pub async fn set_profile(conn: Connection, profile: String) -> zbus::Result<()> {
    PowerProfilesProxy::new(&conn)
        .await?
        .set_active_profile(&profile)
        .await
}

/// Send (or replace, with `replaces`) a notification on the session
/// bus; its id.
pub async fn notify(
    replaces: u32,
    icon: String,
    summary: String,
    body: String,
    critical: bool,
) -> zbus::Result<u32> {
    let conn = Connection::session().await?;
    let mut hints = HashMap::new();
    hints.insert("urgency", Value::U8(if critical { 2 } else { 1 }));
    NotificationsProxy::new(&conn)
        .await?
        .notify("Aria Shell", replaces, &icon, &summary, &body, &[], hints, -1)
        .await
}

pub async fn close_notification(id: u32) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    NotificationsProxy::new(&conn)
        .await?
        .close_notification(id)
        .await
}

// --- reading ---------------------------------------------------------------

type Props = HashMap<String, OwnedValue>;

/// `GetAll` on one interface of one object; empty when it fails.
async fn props(conn: &Connection, service: &str, path: &str, iface: &str) -> Props {
    let proxy = match PropertiesProxy::builder(conn)
        .destination(service.to_owned())
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
            log::debug!("power: {path} {e}");
            Props::new()
        }
    }
}

fn string(p: &Props, key: &str) -> String {
    match p.get(key).map(|v| &**v) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}

fn bool_of(p: &Props, key: &str) -> bool {
    matches!(p.get(key).map(|v| &**v), Some(Value::Bool(true)))
}

fn u32_of(p: &Props, key: &str) -> u32 {
    match p.get(key).map(|v| &**v) {
        Some(Value::U32(n)) => *n,
        Some(Value::U8(n)) => u32::from(*n),
        _ => 0,
    }
}

fn i64_of(p: &Props, key: &str) -> i64 {
    match p.get(key).map(|v| &**v) {
        Some(Value::I64(n)) => *n,
        Some(Value::I32(n)) => i64::from(*n),
        _ => 0,
    }
}

fn f64_of(p: &Props, key: &str) -> f64 {
    match p.get(key).map(|v| &**v) {
        Some(Value::F64(n)) => *n,
        _ => 0.0,
    }
}
