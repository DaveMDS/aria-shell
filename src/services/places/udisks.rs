//! UDisks2 over the system bus: the event stream (the service coming
//! and going, every signal under `/org/freedesktop/UDisks2` folded into
//! one debounced `GetManagedObjects`) and the calls behind the
//! [`Command`](super::Command)s: mount, unmount, lock, eject, power off.
//!
//! The objects are read into plain [`Block`]s and [`Drive`]s here;
//! which of them the popup lists is [`super::devices`]'s business.
//!
//! Reference: <https://storaged.org/doc/udisks2-api/latest/>

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream;
use zbus::fdo::{DBusProxy, ManagedObjects, ObjectManagerProxy};
use zbus::message::Type as MessageType;
use zbus::names::BusName;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream, proxy};

use super::{Block, Drive, Event};

const UDISKS: &str = "org.freedesktop.UDisks2";
const UDISKS_PATH: &str = "/org/freedesktop/UDisks2";
const IFACE_BLOCK: &str = "org.freedesktop.UDisks2.Block";
const IFACE_FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
const IFACE_ENCRYPTED: &str = "org.freedesktop.UDisks2.Encrypted";
const IFACE_DRIVE: &str = "org.freedesktop.UDisks2.Drive";

/// How long after the last signal the objects are re-read.
const DEBOUNCE: Duration = Duration::from_millis(200);

#[proxy(
    interface = "org.freedesktop.UDisks2.Filesystem",
    default_service = "org.freedesktop.UDisks2"
)]
trait Filesystem {
    fn mount(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<String>;
    fn unmount(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.UDisks2.Encrypted",
    default_service = "org.freedesktop.UDisks2"
)]
trait Encrypted {
    fn lock(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.UDisks2.Drive",
    default_service = "org.freedesktop.UDisks2"
)]
trait DriveCalls {
    fn eject(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;
    fn power_off(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;
}

/// The event stream: the bus, then the objects after every burst of
/// signals (`None` while UDisks2 isn't running).
pub fn events() -> impl Stream<Item = Event> {
    stream::channel(16, async move |mut out: mpsc::Sender<Event>| {
        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("places: no system bus: {e}");
                return;
            }
        };
        let _ = out.send(Event::Bus(conn.clone())).await;
        if let Err(e) = follow(conn, out).await {
            log::error!("places: udisks: {e}");
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
            (a.name().as_str() == UDISKS).then_some(())
        });
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .path_namespace(UDISKS_PATH)?
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
                    let objects = snapshot(&conn, &dbus).await;
                    if out.send(Event::Objects(objects)).await.is_err() {
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

/// The blocks and the drives; `None` when UDisks2 isn't running.
async fn snapshot(
    conn: &Connection,
    dbus: &DBusProxy<'_>,
) -> Option<(Vec<Block>, HashMap<String, Drive>)> {
    let running = match BusName::try_from(UDISKS) {
        Ok(n) => dbus.name_has_owner(n).await.unwrap_or(false),
        Err(_) => false,
    };
    if !running {
        return None;
    }
    let manager = ObjectManagerProxy::builder(conn)
        .destination(UDISKS)
        .ok()?
        .path(UDISKS_PATH)
        .ok()?
        .build()
        .await
        .ok()?;
    match manager.get_managed_objects().await {
        Ok(objects) => Some(read(&objects)),
        Err(e) => {
            log::warn!("places: udisks objects: {e}");
            None
        }
    }
}

type Props = HashMap<String, OwnedValue>;

fn read(objects: &ManagedObjects) -> (Vec<Block>, HashMap<String, Drive>) {
    let mut blocks = Vec::new();
    let mut drives = HashMap::new();
    for (path, ifaces) in objects {
        let iface = |name: &str| {
            ifaces
                .iter()
                .find(|(n, _)| n.as_str() == name)
                .map(|(_, p)| p)
        };
        if let Some(d) = iface(IFACE_DRIVE) {
            drives.insert(
                path.to_string(),
                Drive {
                    removable: bool_of(d, "Removable") || bool_of(d, "MediaRemovable"),
                    ejectable: bool_of(d, "Ejectable"),
                    can_power_off: bool_of(d, "CanPowerOff"),
                    optical: bool_of(d, "Optical"),
                    connection_bus: string(d, "ConnectionBus"),
                    media: string(d, "Media"),
                },
            );
        }
        let Some(b) = iface(IFACE_BLOCK) else {
            continue;
        };
        blocks.push(Block {
            path: path.to_string(),
            device: bytes_path(b.get("PreferredDevice"))
                .filter(|p| !p.as_os_str().is_empty())
                .or_else(|| bytes_path(b.get("Device")))
                .unwrap_or_default(),
            size: u64_of(b, "Size"),
            id_usage: string(b, "IdUsage"),
            id_type: string(b, "IdType"),
            id_label: string(b, "IdLabel"),
            hint_ignore: bool_of(b, "HintIgnore"),
            hint_system: bool_of(b, "HintSystem"),
            drive: object_path(b, "Drive"),
            crypto_backing: object_path(b, "CryptoBackingDevice"),
            fstab: fstab(b),
            mount_points: iface(IFACE_FILESYSTEM).map(|f| mount_points(f.get("MountPoints"))),
            cleartext: iface(IFACE_ENCRYPTED).map(|e| object_path(e, "CleartextDevice")),
        });
    }
    (blocks, drives)
}

// --- calls -----------------------------------------------------------------

/// Mount the filesystem of `block`; where it went.
pub async fn mount(conn: Connection, block: String) -> zbus::Result<PathBuf> {
    let fs = FilesystemProxy::builder(&conn).path(block)?.build().await?;
    fs.mount(HashMap::new()).await.map(PathBuf::from)
}

/// What an eject does, in order; each step only when set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Eject {
    /// The block to unmount.
    pub unmount: Option<String>,
    /// The LUKS container to lock after.
    pub lock: Option<String>,
    /// The drive to eject (optical media) or to power off (anything
    /// else that can be): the last step.
    pub drive: Option<(String, super::DriveAction)>,
}

pub async fn eject(conn: Connection, plan: Eject) -> zbus::Result<()> {
    if let Some(block) = plan.unmount {
        let fs = FilesystemProxy::builder(&conn).path(block)?.build().await?;
        fs.unmount(HashMap::new()).await?;
    }
    if let Some(block) = plan.lock {
        let luks = EncryptedProxy::builder(&conn).path(block)?.build().await?;
        luks.lock(HashMap::new()).await?;
    }
    if let Some((drive, action)) = plan.drive {
        let drive = DriveCallsProxy::builder(&conn).path(drive)?.build().await?;
        match action {
            super::DriveAction::Eject => drive.eject(HashMap::new()).await?,
            super::DriveAction::PowerOff => drive.power_off(HashMap::new()).await?,
        }
    }
    Ok(())
}

/// What a failed call says, and whether it's polkit refusing it (no
/// agent to ask for the password, or a wrong one).
pub fn failure(e: &zbus::Error) -> (String, bool) {
    match e {
        zbus::Error::MethodError(name, message, _) => (
            message.clone().unwrap_or_else(|| name.to_string()),
            name.as_str().contains("NotAuthorized"),
        ),
        other => (other.to_string(), false),
    }
}

// --- reading values --------------------------------------------------------

/// A value, through the variant a dict of `a{sv}` wraps it in.
fn plain<'a>(v: &'a Value<'a>) -> &'a Value<'a> {
    match v {
        Value::Value(inner) => plain(inner),
        v => v,
    }
}

fn string(p: &Props, key: &str) -> String {
    match p.get(key).map(|v| plain(v)) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}

fn bool_of(p: &Props, key: &str) -> bool {
    matches!(p.get(key).map(|v| plain(v)), Some(Value::Bool(true)))
}

fn u64_of(p: &Props, key: &str) -> u64 {
    match p.get(key).map(|v| plain(v)) {
        Some(Value::U64(n)) => *n,
        _ => 0,
    }
}

/// An object path; empty for none (UDisks' `/`).
fn object_path(p: &Props, key: &str) -> String {
    match p.get(key).map(|v| plain(v)) {
        Some(Value::ObjectPath(o)) if o.as_str() != "/" => o.to_string(),
        _ => String::new(),
    }
}

/// A NUL-terminated byte string (`ay`).
fn bytes(v: &Value<'_>) -> Option<Vec<u8>> {
    match plain(v) {
        Value::Array(a) => {
            let mut bytes: Vec<u8> = a
                .inner()
                .iter()
                .filter_map(|b| match b {
                    Value::U8(b) => Some(*b),
                    _ => None,
                })
                .collect();
            while bytes.last() == Some(&0) {
                bytes.pop();
            }
            Some(bytes)
        }
        _ => None,
    }
}

fn bytes_path(v: Option<&OwnedValue>) -> Option<PathBuf> {
    bytes(v?).map(|b| PathBuf::from(OsString::from_vec(b)))
}

/// `MountPoints` (`aay`).
fn mount_points(v: Option<&OwnedValue>) -> Vec<PathBuf> {
    match v.map(|v| plain(v)) {
        Some(Value::Array(a)) => a
            .inner()
            .iter()
            .filter_map(bytes)
            .filter(|b| !b.is_empty())
            .map(|b| PathBuf::from(OsString::from_vec(b)))
            .collect(),
        _ => Vec::new(),
    }
}

/// The fstab entries of `Configuration` (`a(sa{sv})`): `dir` and `opts`.
fn fstab(p: &Props) -> Vec<(PathBuf, String)> {
    let Some(Value::Array(items)) = p.get("Configuration").map(|v| plain(v)) else {
        return Vec::new();
    };
    items
        .inner()
        .iter()
        .filter_map(|item| {
            let Value::Structure(s) = plain(item) else {
                return None;
            };
            let [Value::Str(kind), Value::Dict(details)] = s.fields() else {
                return None;
            };
            if kind.as_str() != "fstab" {
                return None;
            }
            let field = |name: &str| {
                details
                    .iter()
                    .find(|(k, _)| matches!(plain(k), Value::Str(s) if s.as_str() == name))
                    .and_then(|(_, v)| bytes(v))
                    .unwrap_or_default()
            };
            Some((
                PathBuf::from(OsString::from_vec(field("dir"))),
                String::from_utf8_lossy(&field("opts")).into_owned(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::names::OwnedInterfaceName;
    use zbus::zvariant::OwnedObjectPath;

    fn v<'a>(value: impl Into<Value<'a>>) -> OwnedValue {
        OwnedValue::try_from(value.into()).unwrap()
    }

    fn object(ifaces: Vec<(&str, Vec<(&str, OwnedValue)>)>) -> HashMap<OwnedInterfaceName, Props> {
        ifaces
            .into_iter()
            .map(|(name, props)| {
                (
                    OwnedInterfaceName::try_from(name).unwrap(),
                    props.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn reads_blocks_and_drives() {
        let drive = "/org/freedesktop/UDisks2/drives/Stick";
        let fstab: HashMap<String, Value> = HashMap::from([
            ("dir".to_owned(), Value::from(b"/mnt/data\0".to_vec())),
            (
                "opts".to_owned(),
                Value::from(b"noauto,x-gvfs-show\0".to_vec()),
            ),
        ]);
        let objects: ManagedObjects = HashMap::from([
            (
                OwnedObjectPath::try_from("/org/freedesktop/UDisks2/block_devices/sdb1").unwrap(),
                object(vec![
                    (
                        IFACE_BLOCK,
                        vec![
                            ("Device", v(b"/dev/sdb1\0".to_vec())),
                            ("PreferredDevice", v(b"\0".to_vec())),
                            ("Size", v(32_000u64)),
                            ("IdUsage", v("filesystem")),
                            ("IdType", v("vfat")),
                            ("IdLabel", v("STICK")),
                            ("HintSystem", v(false)),
                            ("Drive", v(OwnedObjectPath::try_from(drive).unwrap())),
                            (
                                "CryptoBackingDevice",
                                v(OwnedObjectPath::try_from("/").unwrap()),
                            ),
                            ("Configuration", v(vec![("fstab".to_owned(), fstab)])),
                        ],
                    ),
                    (
                        IFACE_FILESYSTEM,
                        vec![("MountPoints", v(vec![b"/run/media/u/STICK\0".to_vec()]))],
                    ),
                ]),
            ),
            (
                OwnedObjectPath::try_from("/org/freedesktop/UDisks2/block_devices/sdc1").unwrap(),
                object(vec![
                    (
                        IFACE_BLOCK,
                        vec![
                            ("Device", v(b"/dev/sdc1\0".to_vec())),
                            ("IdUsage", v("crypto")),
                        ],
                    ),
                    (
                        IFACE_ENCRYPTED,
                        vec![(
                            "CleartextDevice",
                            v(OwnedObjectPath::try_from("/").unwrap()),
                        )],
                    ),
                ]),
            ),
            (
                OwnedObjectPath::try_from(drive).unwrap(),
                object(vec![(
                    IFACE_DRIVE,
                    vec![
                        ("Removable", v(true)),
                        ("CanPowerOff", v(true)),
                        ("ConnectionBus", v("usb")),
                        ("Media", v("thumb")),
                    ],
                )]),
            ),
        ]);
        let (mut blocks, drives) = read(&objects);
        blocks.sort_by(|a, b| a.path.cmp(&b.path));
        let stick = &blocks[0];
        assert_eq!(
            stick.device,
            PathBuf::from("/dev/sdb1"),
            "Device, PreferredDevice empty"
        );
        assert_eq!(stick.size, 32_000);
        assert_eq!(stick.id_label, "STICK");
        assert_eq!(stick.drive, drive);
        assert_eq!(stick.crypto_backing, "", "`/` is none");
        assert_eq!(
            stick.fstab,
            vec![(PathBuf::from("/mnt/data"), "noauto,x-gvfs-show".to_owned())]
        );
        assert_eq!(
            stick.mount_points,
            Some(vec![PathBuf::from("/run/media/u/STICK")])
        );
        assert_eq!(stick.cleartext, None);
        let luks = &blocks[1];
        assert_eq!(luks.mount_points, None, "no filesystem");
        assert_eq!(luks.cleartext.as_deref(), Some(""), "locked");
        let d = &drives[drive];
        assert!(d.removable && d.can_power_off && !d.ejectable);
        assert_eq!(
            (d.connection_bus.as_str(), d.media.as_str()),
            ("usb", "thumb")
        );
    }
}
