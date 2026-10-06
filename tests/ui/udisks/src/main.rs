//! A UDisks2 for the UI scenarios: owns `org.freedesktop.UDisks2` on
//! the bus `DBUS_SYSTEM_BUS_ADDRESS` points at (the scenario's session
//! bus) and serves what the shell reads: an object manager at
//! `/org/freedesktop/UDisks2`, one drive and one partition (its block,
//! filesystem or LUKS container) per disk. Starts with no disk. What the
//! shell asks of it goes to stdout:
//!
//!   mount <name>          (mounted under $ARIA_UI_OUT/media/<label>)
//!   unmount <name>
//!   power-off <name>      (the disk is gone, as a real one goes)
//!
//! Stdin drives changes (one command per line, answered `ok`):
//!
//!   internal <name> <label> <mount point|->   a system disk, mounted or not
//!   stick <name> <label>                      a USB stick, not mounted
//!   luks <name>                               a USB disk with a locked LUKS volume
//!   remove <name>                             unplugged
//!   busy <name> <on|off>                      unmounting it fails (in use)
//!   deny <name> <on|off>                      mounting it fails (polkit says no)
//!
//! `ready` is printed once the name is owned.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, BufReader};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{Connection, DBusError, interface};

const UDISKS: &str = "org.freedesktop.UDisks2";
const ROOT: &str = "/org/freedesktop/UDisks2";
const MANAGER: &str = "/org/freedesktop/UDisks2/Manager";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Internal,
    Stick,
    Luks,
}

struct Disk {
    kind: Kind,
    label: String,
    mount: Option<PathBuf>,
    busy: bool,
    deny: bool,
}

type Shared = Arc<Mutex<BTreeMap<String, Disk>>>;

#[derive(Debug, DBusError)]
#[zbus(prefix = "org.freedesktop.UDisks2.Error")]
enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    DeviceBusy(String),
    NotAuthorizedCanObtain(String),
    Failed(String),
}

fn drive_path(name: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(format!("{ROOT}/drives/{name}")).unwrap()
}

fn block_path(name: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(format!("{ROOT}/block_devices/{name}1")).unwrap()
}

/// What changes everything for the shell: any signal under the root.
struct Manager {
    serial: u32,
}

#[interface(name = "org.freedesktop.UDisks2.Manager")]
impl Manager {
    #[zbus(property)]
    fn version(&self) -> String {
        format!("2.10.{}", self.serial)
    }
}

struct Drive {
    conn: Connection,
    state: Shared,
    name: String,
}

impl Drive {
    fn kind(&self) -> Option<Kind> {
        self.state.lock().unwrap().get(&self.name).map(|d| d.kind)
    }

    fn external(&self) -> bool {
        matches!(self.kind(), Some(Kind::Stick | Kind::Luks))
    }
}

#[interface(name = "org.freedesktop.UDisks2.Drive")]
impl Drive {
    #[zbus(property)]
    fn removable(&self) -> bool {
        self.external()
    }

    #[zbus(property)]
    fn media_removable(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn ejectable(&self) -> bool {
        self.kind() == Some(Kind::Stick)
    }

    #[zbus(property)]
    fn can_power_off(&self) -> bool {
        self.external()
    }

    #[zbus(property)]
    fn optical(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn connection_bus(&self) -> String {
        if self.external() { "usb" } else { "" }.to_owned()
    }

    #[zbus(property)]
    fn media(&self) -> String {
        if self.kind() == Some(Kind::Stick) {
            "thumb"
        } else {
            ""
        }
        .to_owned()
    }

    fn power_off(&self, _options: HashMap<String, OwnedValue>) -> Result<(), Error> {
        println!("power-off {}", self.name);
        // Gone, as a real one goes: off the bus once the call has
        // returned (removing it from inside the call would wait on it).
        let (conn, state, name) = (self.conn.clone(), self.state.clone(), self.name.clone());
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            remove(&conn, &state, &name).await;
            changed(&conn).await;
        });
        Ok(())
    }

    fn eject(&self, _options: HashMap<String, OwnedValue>) -> Result<(), Error> {
        println!("eject {}", self.name);
        Ok(())
    }
}

struct Block {
    state: Shared,
    name: String,
}

#[interface(name = "org.freedesktop.UDisks2.Block")]
impl Block {
    #[zbus(property)]
    fn device(&self) -> Vec<u8> {
        format!("/dev/fake-{}1\0", self.name).into_bytes()
    }

    #[zbus(property)]
    fn preferred_device(&self) -> Vec<u8> {
        self.device()
    }

    #[zbus(property)]
    fn size(&self) -> u64 {
        32_000_000_000
    }

    #[zbus(property)]
    fn id_usage(&self) -> String {
        match self.state.lock().unwrap().get(&self.name).map(|d| d.kind) {
            Some(Kind::Luks) => "crypto",
            _ => "filesystem",
        }
        .to_owned()
    }

    #[zbus(property)]
    fn id_type(&self) -> String {
        match self.state.lock().unwrap().get(&self.name).map(|d| d.kind) {
            Some(Kind::Luks) => "crypto_LUKS",
            _ => "ext4",
        }
        .to_owned()
    }

    #[zbus(property)]
    fn id_label(&self) -> String {
        let state = self.state.lock().unwrap();
        state
            .get(&self.name)
            .filter(|d| d.kind != Kind::Luks)
            .map(|d| d.label.clone())
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn hint_ignore(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn hint_system(&self) -> bool {
        self.state
            .lock()
            .unwrap()
            .get(&self.name)
            .is_some_and(|d| d.kind == Kind::Internal)
    }

    #[zbus(property)]
    fn drive(&self) -> OwnedObjectPath {
        drive_path(&self.name)
    }

    #[zbus(property)]
    fn crypto_backing_device(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from("/").unwrap()
    }

    #[zbus(property)]
    fn configuration(&self) -> Vec<(String, HashMap<String, OwnedValue>)> {
        Vec::new()
    }
}

struct Filesystem {
    state: Shared,
    name: String,
}

#[interface(name = "org.freedesktop.UDisks2.Filesystem")]
impl Filesystem {
    #[zbus(property)]
    fn mount_points(&self) -> Vec<Vec<u8>> {
        let state = self.state.lock().unwrap();
        state
            .get(&self.name)
            .and_then(|d| d.mount.as_ref())
            .map(|m| {
                let mut bytes = m.to_string_lossy().into_owned().into_bytes();
                bytes.push(0);
                vec![bytes]
            })
            .unwrap_or_default()
    }

    async fn mount(
        &self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<String, Error> {
        let path = {
            let mut state = self.state.lock().unwrap();
            let Some(disk) = state.get_mut(&self.name) else {
                return Err(Error::Failed("no such disk".to_owned()));
            };
            if disk.deny {
                return Err(Error::NotAuthorizedCanObtain(
                    "Not authorized to perform operation".to_owned(),
                ));
            }
            let out = std::env::var_os("ARIA_UI_OUT")
                .map(PathBuf::from)
                .unwrap_or_default();
            let path = out.join("media").join(&disk.label);
            let _ = std::fs::create_dir_all(&path);
            disk.mount = Some(path.clone());
            path
        };
        println!("mount {}", self.name);
        let _ = self.mount_points_changed(&emitter).await;
        Ok(path.to_string_lossy().into_owned())
    }

    async fn unmount(
        &self,
        _options: HashMap<String, OwnedValue>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        {
            let mut state = self.state.lock().unwrap();
            let Some(disk) = state.get_mut(&self.name) else {
                return Err(Error::Failed("no such disk".to_owned()));
            };
            if disk.busy {
                println!("unmount-busy {}", self.name);
                return Err(Error::DeviceBusy(format!(
                    "Error unmounting /dev/fake-{}1: target is busy",
                    self.name
                )));
            }
            disk.mount = None;
        }
        println!("unmount {}", self.name);
        let _ = self.mount_points_changed(&emitter).await;
        Ok(())
    }
}

struct Encrypted;

#[interface(name = "org.freedesktop.UDisks2.Encrypted")]
impl Encrypted {
    #[zbus(property)]
    fn cleartext_device(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from("/").unwrap()
    }
}

async fn add(conn: &Connection, state: &Shared, name: &str) -> zbus::Result<()> {
    let kind = state.lock().unwrap().get(name).map(|d| d.kind);
    let server = conn.object_server();
    let (drive, block) = (drive_path(name), block_path(name));
    server
        .at(
            &drive,
            Drive {
                conn: conn.clone(),
                state: state.clone(),
                name: name.to_owned(),
            },
        )
        .await?;
    server
        .at(
            &block,
            Block {
                state: state.clone(),
                name: name.to_owned(),
            },
        )
        .await?;
    if kind == Some(Kind::Luks) {
        server.at(&block, Encrypted).await?;
    } else {
        server
            .at(
                &block,
                Filesystem {
                    state: state.clone(),
                    name: name.to_owned(),
                },
            )
            .await?;
    }
    Ok(())
}

async fn remove(conn: &Connection, state: &Shared, name: &str) {
    state.lock().unwrap().remove(name);
    let server = conn.object_server();
    let (drive, block) = (drive_path(name), block_path(name));
    let _ = server.remove::<Drive, _>(&drive).await;
    let _ = server.remove::<Filesystem, _>(&block).await;
    let _ = server.remove::<Encrypted, _>(&block).await;
    let _ = server.remove::<Block, _>(&block).await;
}

/// A signal under the root, for the shell to read everything again.
async fn changed(conn: &Connection) {
    if let Ok(m) = conn.object_server().interface::<_, Manager>(MANAGER).await {
        m.get_mut().await.serial += 1;
        let _ = m.get().await.version_changed(m.signal_emitter()).await;
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> zbus::Result<()> {
    let state: Shared = Arc::new(Mutex::new(BTreeMap::new()));
    // The scenario points DBUS_SYSTEM_BUS_ADDRESS at its session bus;
    // this side just joins the same bus.
    let conn = Connection::session().await?;
    let server = conn.object_server();
    server.at(ROOT, zbus::fdo::ObjectManager).await?;
    server.at(MANAGER, Manager { serial: 0 }).await?;
    conn.request_name(UDISKS).await?;
    println!("ready");

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let words: Vec<&str> = line.split_whitespace().collect();
        let new = |kind, label: &str, mount: Option<PathBuf>| Disk {
            kind,
            label: label.to_owned(),
            mount,
            busy: false,
            deny: false,
        };
        match words.as_slice() {
            ["internal", name, label, mount] => {
                let mount = (*mount != "-").then(|| PathBuf::from(mount));
                state
                    .lock()
                    .unwrap()
                    .insert((*name).to_owned(), new(Kind::Internal, label, mount));
                add(&conn, &state, name).await?;
            }
            ["stick", name, label] => {
                state
                    .lock()
                    .unwrap()
                    .insert((*name).to_owned(), new(Kind::Stick, label, None));
                add(&conn, &state, name).await?;
            }
            ["luks", name] => {
                state
                    .lock()
                    .unwrap()
                    .insert((*name).to_owned(), new(Kind::Luks, "", None));
                add(&conn, &state, name).await?;
            }
            ["remove", name] => remove(&conn, &state, name).await,
            ["busy", name, on] => {
                if let Some(d) = state.lock().unwrap().get_mut(*name) {
                    d.busy = *on == "on";
                }
            }
            ["deny", name, on] => {
                if let Some(d) = state.lock().unwrap().get_mut(*name) {
                    d.deny = *on == "on";
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
