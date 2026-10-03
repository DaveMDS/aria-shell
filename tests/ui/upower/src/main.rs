//! A UPower and a power-profiles-daemon for the UI scenarios: owns
//! `org.freedesktop.UPower` (and `org.freedesktop.UPower.PowerProfiles`
//! once asked) on the bus `DBUS_SYSTEM_BUS_ADDRESS` points at (the
//! scenario's session bus) and serves what the shell reads: the root
//! object (`OnBattery`, `EnumerateDevices`), the display device, the
//! laptop battery, peripherals, the profiles. Starts on AC with no
//! battery and no profiles. What the shell asks of it goes to stdout:
//!
//!   set-profile <name>
//!
//! Stdin drives changes (one command per line, answered `ok`, a
//! `PropertiesChanged` emitted under `/org/freedesktop/UPower`):
//!
//!   ac <on|off>                                       the charger
//!   battery <percent> <charging|discharging|full> [secs to empty/full]
//!   battery none
//!   warning <none|low|critical>
//!   device <model> <percent>                          a peripheral appears (or changes)
//!   profiles <on|off>                                 the profiles daemon comes / goes
//!   profile <name>                                    it switches by itself
//!   degraded <reason|none>
//!
//! `ready` is printed once the name is owned.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, BufReader};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, interface};

const UPOWER: &str = "org.freedesktop.UPower";
const ROOT: &str = "/org/freedesktop/UPower";
const DISPLAY: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const BAT0: &str = "/org/freedesktop/UPower/devices/battery_BAT0";
const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

#[derive(Clone)]
struct Battery {
    percent: f64,
    /// UPower's `State`: 1 charging, 2 discharging, 4 full.
    state: u32,
    secs: i64,
    /// UPower's `WarningLevel`: 1 none, 3 low, 4 critical.
    warning: u32,
}

#[derive(Default)]
struct State {
    on_battery: bool,
    battery: Option<Battery>,
    /// (model, percent)
    devices: Vec<(String, f64)>,
    profile: String,
    degraded: String,
}

type Shared = Arc<Mutex<State>>;

struct Root {
    state: Shared,
}

#[interface(name = "org.freedesktop.UPower")]
impl Root {
    #[zbus(property)]
    fn on_battery(&self) -> bool {
        self.state.lock().unwrap().on_battery
    }

    #[zbus(property)]
    fn daemon_version(&self) -> String {
        "1.90.0".to_owned()
    }

    fn enumerate_devices(&self) -> Vec<OwnedObjectPath> {
        let s = self.state.lock().unwrap();
        let mut paths = vec![OwnedObjectPath::try_from("/org/freedesktop/UPower/devices/line_power_AC").unwrap()];
        if s.battery.is_some() {
            paths.push(OwnedObjectPath::try_from(BAT0).unwrap());
        }
        for i in 0..s.devices.len() {
            paths.push(device_path(i));
        }
        paths
    }
}

fn device_path(i: usize) -> OwnedObjectPath {
    OwnedObjectPath::try_from(format!("/org/freedesktop/UPower/devices/mouse_{i}")).unwrap()
}

/// Which object a [`Device`] is.
#[derive(Clone, Copy)]
enum Which {
    Display,
    LinePower,
    Bat0,
    Peripheral(usize),
}

struct Device {
    state: Shared,
    which: Which,
}

/// UPower's icon for a level and a state.
fn battery_icon(b: &Battery) -> String {
    let level = ((b.percent / 10.0).round() * 10.0) as u32;
    match b.state {
        4 => "battery-level-100-charged-symbolic".to_owned(),
        1 => format!("battery-level-{level}-charging-symbolic"),
        _ => format!("battery-level-{level}-symbolic"),
    }
}

impl Device {
    fn battery(&self) -> Option<Battery> {
        self.state.lock().unwrap().battery.clone()
    }

    fn peripheral(&self) -> Option<(String, f64)> {
        match self.which {
            Which::Peripheral(i) => self.state.lock().unwrap().devices.get(i).cloned(),
            _ => None,
        }
    }
}

#[interface(name = "org.freedesktop.UPower.Device")]
impl Device {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> u32 {
        match self.which {
            Which::LinePower => 1,
            Which::Peripheral(_) => 5,
            // A display device with nothing to sum up is "unknown".
            Which::Display if self.battery().is_none() => 0,
            _ => 2,
        }
    }

    #[zbus(property)]
    fn is_present(&self) -> bool {
        match self.which {
            Which::Display | Which::Bat0 => self.battery().is_some(),
            _ => true,
        }
    }

    #[zbus(property)]
    fn power_supply(&self) -> bool {
        !matches!(self.which, Which::Peripheral(_))
    }

    #[zbus(property)]
    fn model(&self) -> String {
        match self.which {
            Which::Bat0 => "Fake Battery".to_owned(),
            _ => self.peripheral().map(|p| p.0).unwrap_or_default(),
        }
    }

    #[zbus(property)]
    fn percentage(&self) -> f64 {
        match self.which {
            Which::Peripheral(_) => self.peripheral().map_or(0.0, |p| p.1),
            _ => self.battery().map_or(0.0, |b| b.percent),
        }
    }

    #[zbus(property)]
    fn state(&self) -> u32 {
        match self.which {
            Which::Peripheral(_) => 2,
            _ => self.battery().map_or(0, |b| b.state),
        }
    }

    #[zbus(property)]
    fn time_to_empty(&self) -> i64 {
        self.battery().filter(|b| b.state == 2).map_or(0, |b| b.secs)
    }

    #[zbus(property)]
    fn time_to_full(&self) -> i64 {
        self.battery().filter(|b| b.state == 1).map_or(0, |b| b.secs)
    }

    #[zbus(property)]
    fn energy_rate(&self) -> f64 {
        match self.which {
            Which::Display | Which::Bat0 => self.battery().map_or(0.0, |_| 8.25),
            _ => 0.0,
        }
    }

    #[zbus(property)]
    fn capacity(&self) -> f64 {
        match self.which {
            Which::Bat0 => 91.4,
            _ => 0.0,
        }
    }

    #[zbus(property)]
    fn warning_level(&self) -> u32 {
        match self.which {
            Which::Display | Which::Bat0 => self.battery().map_or(1, |b| b.warning),
            _ => 1,
        }
    }

    #[zbus(property)]
    fn icon_name(&self) -> String {
        match self.which {
            Which::Peripheral(_) => "input-mouse-symbolic".to_owned(),
            Which::LinePower => String::new(),
            _ => self.battery().map(|b| battery_icon(&b)).unwrap_or_default(),
        }
    }
}

struct Profiles {
    state: Shared,
}

#[interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl Profiles {
    #[zbus(property)]
    fn active_profile(&self) -> String {
        self.state.lock().unwrap().profile.clone()
    }

    #[zbus(property)]
    fn set_active_profile(&mut self, profile: String) {
        println!("set-profile {profile}");
        self.state.lock().unwrap().profile = profile;
    }

    #[zbus(property)]
    fn profiles(&self) -> Vec<HashMap<String, OwnedValue>> {
        ["power-saver", "balanced", "performance"]
            .iter()
            .map(|p| {
                let mut d = HashMap::new();
                d.insert("Profile".to_owned(), OwnedValue::try_from(Value::from(*p)).unwrap());
                d.insert("Driver".to_owned(), OwnedValue::try_from(Value::from("fake")).unwrap());
                d
            })
            .collect()
    }

    #[zbus(property)]
    fn performance_degraded(&self) -> String {
        self.state.lock().unwrap().degraded.clone()
    }
}

/// Anything under `/org/freedesktop/UPower` makes the shell re-read it
/// all: one `PropertiesChanged` on the root is enough.
async fn changed(conn: &Connection) {
    if let Ok(r) = conn.object_server().interface::<_, Root>(ROOT).await {
        let _ = r.get().await.on_battery_changed(r.signal_emitter()).await;
    }
}

async fn serve_devices(conn: &Connection, state: &Shared) {
    let n = state.lock().unwrap().devices.len();
    for i in 0..n {
        let _ = conn
            .object_server()
            .at(
                device_path(i),
                Device {
                    state: state.clone(),
                    which: Which::Peripheral(i),
                },
            )
            .await;
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> zbus::Result<()> {
    let state: Shared = Arc::new(Mutex::new(State {
        profile: "balanced".to_owned(),
        ..State::default()
    }));
    // The scenario points DBUS_SYSTEM_BUS_ADDRESS at its session bus;
    // this side just joins the same bus.
    let conn = Connection::session().await?;
    let server = conn.object_server();
    server.at(ROOT, Root { state: state.clone() }).await?;
    for (path, which) in [
        (DISPLAY, Which::Display),
        ("/org/freedesktop/UPower/devices/line_power_AC", Which::LinePower),
        (BAT0, Which::Bat0),
    ] {
        server
            .at(
                path,
                Device {
                    state: state.clone(),
                    which,
                },
            )
            .await?;
    }
    server
        .at(PROFILES_PATH, Profiles { state: state.clone() })
        .await?;
    conn.request_name(UPOWER).await?;
    println!("ready");

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["ac", on] => state.lock().unwrap().on_battery = *on != "on",
            ["battery", "none"] => state.lock().unwrap().battery = None,
            ["battery", percent, how, rest @ ..] => {
                let mut s = state.lock().unwrap();
                let warning = s.battery.as_ref().map_or(1, |b| b.warning);
                s.battery = Some(Battery {
                    percent: percent.parse().unwrap_or(50.0),
                    state: match *how {
                        "charging" => 1,
                        "full" => 4,
                        _ => 2,
                    },
                    secs: rest.first().and_then(|s| s.parse().ok()).unwrap_or(0),
                    warning,
                });
            }
            ["warning", level] => {
                if let Some(b) = state.lock().unwrap().battery.as_mut() {
                    b.warning = match *level {
                        "low" => 3,
                        "critical" => 4,
                        _ => 1,
                    };
                }
            }
            ["device", model, percent] => {
                {
                    let mut s = state.lock().unwrap();
                    let percent = percent.parse().unwrap_or(50.0);
                    match s.devices.iter_mut().find(|d| d.0 == *model) {
                        Some(d) => d.1 = percent,
                        None => s.devices.push(((*model).to_owned(), percent)),
                    }
                }
                serve_devices(&conn, &state).await;
            }
            ["profiles", "on"] => {
                conn.request_name(PROFILES).await?;
            }
            ["profiles", "off"] => {
                conn.release_name(PROFILES).await?;
            }
            ["profile", name] => {
                state.lock().unwrap().profile = (*name).to_owned();
                if let Ok(p) = server.interface::<_, Profiles>(PROFILES_PATH).await {
                    let _ = p.get().await.active_profile_changed(p.signal_emitter()).await;
                }
            }
            ["degraded", reason] => {
                state.lock().unwrap().degraded =
                    if *reason == "none" { String::new() } else { (*reason).to_owned() };
                if let Ok(p) = server.interface::<_, Profiles>(PROFILES_PATH).await {
                    let _ = p
                        .get()
                        .await
                        .performance_degraded_changed(p.signal_emitter())
                        .await;
                }
            }
            ["quit"] => break,
            _ => eprintln!("unknown command: {line}"),
        }
        changed(&conn).await;
        println!("ok");
    }
    Ok(())
}
