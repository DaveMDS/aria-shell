//! The bus side of the tray: the `org.kde.StatusNotifierWatcher` we
//! serve (or defer to), the host that follows its item list, and one
//! task per item reading its properties and following its `New*`
//! signals.
//!
//! Only one watcher can own the name. We always serve the object and
//! ask for the name; if another bar has it, we use that one as any host
//! would (same proxy, same signals), and try again for the name when it
//! goes away. Either way the items reach the daemon through the
//! well-known name, so the two cases share the code below.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use iced::futures::channel::mpsc;
use iced::futures::stream::{self, BoxStream, StreamExt};
use iced::futures::{SinkExt, Stream};
use iced::stream as iced_stream;
use tokio::task::AbortHandle;
use zbus::fdo::{DBusProxy, PropertiesProxy, RequestNameFlags, RequestNameReply};
use zbus::message::Header;
use zbus::names::{BusName, InterfaceName};
use zbus::object_server::SignalEmitter;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};
use zbus::{Connection, interface, proxy};

use super::{Event, Orientation, Pixmap, Props, Status, menu, split_key};

const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";

/// Pixmaps larger than this are skipped when a smaller one is offered:
/// the bar draws them at ~16-24px.
const PIXMAP_MAX: u32 = 64;

// --- the watcher we serve -------------------------------------------------

#[derive(Default)]
struct Watcher {
    items: Arc<Mutex<Vec<String>>>,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    /// `service` is either the object path of the item on the caller's
    /// connection (`/org/ayatana/NotificationItem/nm_applet`) or a bus
    /// name (`:1.12`, then the path is `/StatusNotifierItem`); the
    /// registered key is always `<bus name><path>`.
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let (bus, path) = if service.starts_with('/') {
            (sender.clone(), service.to_owned())
        } else if service.starts_with(':') {
            (service.to_owned(), "/StatusNotifierItem".to_owned())
        } else {
            (sender.clone(), "/StatusNotifierItem".to_owned())
        };
        let key = format!("{bus}{path}");
        {
            let mut items = self.items.lock().expect("watcher list");
            if items.contains(&key) {
                return;
            }
            items.push(key.clone());
        }
        log::debug!("watcher: item {key} registered by {sender}");
        let _ = Self::status_notifier_item_registered(&emitter, &key).await;
        // Unregister it when its connection leaves the bus.
        tokio::spawn(forget_when_gone(conn.clone(), self.items.clone(), bus, key));
    }

    async fn register_status_notifier_host(
        &self,
        service: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        log::debug!("watcher: host {service} registered");
        let _ = Self::status_notifier_host_registered(&emitter).await;
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().expect("watcher list").clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_unregistered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Wait for `bus` to drop off the bus, then unregister `key`.
async fn forget_when_gone(
    conn: Connection,
    items: Arc<Mutex<Vec<String>>>,
    bus: String,
    key: String,
) {
    let Ok(dbus) = DBusProxy::new(&conn).await else {
        return;
    };
    let Ok(mut changes) = dbus
        .receive_name_owner_changed_with_args(&[(0, bus.as_str())])
        .await
    else {
        return;
    };
    // It may have left between the registration and the match rule.
    let present = BusName::try_from(bus.as_str())
        .map(|name| async move { dbus.name_has_owner(name).await.unwrap_or(true) });
    let present = match present {
        Ok(f) => f.await,
        Err(_) => true,
    };
    if present {
        while let Some(change) = changes.next().await {
            if change.args().is_ok_and(|a| a.new_owner().is_none()) {
                break;
            }
        }
    }
    items.lock().expect("watcher list").retain(|k| *k != key);
    log::debug!("watcher: {key} left the bus");
    if let Ok(emitter) = SignalEmitter::new(&conn, WATCHER_PATH) {
        let _ = Watcher::status_notifier_item_unregistered(&emitter, &key).await;
    }
}

// --- proxies --------------------------------------------------------------

#[proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait StatusNotifierWatcher {
    fn register_status_notifier_host(&self, service: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> zbus::Result<Vec<String>>;

    #[zbus(signal)]
    fn status_notifier_item_registered(&self, service: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn status_notifier_item_unregistered(&self, service: String) -> zbus::Result<()>;
}

#[proxy(interface = "org.kde.StatusNotifierItem")]
trait StatusNotifierItem {
    fn activate(&self, x: i32, y: i32) -> zbus::Result<()>;
    fn secondary_activate(&self, x: i32, y: i32) -> zbus::Result<()>;
    fn context_menu(&self, x: i32, y: i32) -> zbus::Result<()>;
    fn scroll(&self, delta: i32, orientation: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn new_title(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn new_icon(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn new_attention_icon(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn new_tool_tip(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn new_menu(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn new_status(&self, status: String) -> zbus::Result<()>;
}

// --- the host: the event stream -----------------------------------------

/// What the host loop reacts to.
enum Signal {
    Registered(String),
    Unregistered(String),
    /// The watcher name changed hands (`None`: nobody owns it).
    WatcherOwner(Option<String>),
}

pub fn events() -> impl Stream<Item = Event> {
    iced_stream::channel(64, async move |mut out: mpsc::Sender<Event>| {
        let conn = match Connection::session().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("tray: no session bus: {e}");
                return;
            }
        };
        let _ = out.send(Event::Connected(conn.clone())).await;
        if let Err(e) = host(conn, out).await {
            log::error!("tray: {e}");
        }
    })
}

/// Ask for the watcher name (without queueing): whether we got it.
async fn own_watcher(conn: &Connection) -> bool {
    match conn
        .request_name_with_flags(WATCHER_NAME, RequestNameFlags::DoNotQueue.into())
        .await
    {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
            log::info!("tray: serving {WATCHER_NAME}");
            true
        }
        Ok(_) | Err(zbus::Error::NameTaken) => {
            log::info!("tray: {WATCHER_NAME} is owned by another bar, using it");
            false
        }
        Err(e) => {
            log::warn!("tray: cannot request {WATCHER_NAME}: {e}");
            false
        }
    }
}

async fn host(conn: Connection, mut out: mpsc::Sender<Event>) -> zbus::Result<()> {
    conn.object_server()
        .at(WATCHER_PATH, Watcher::default())
        .await?;
    own_watcher(&conn).await;

    let watcher = StatusNotifierWatcherProxy::new(&conn).await?;
    let dbus = DBusProxy::new(&conn).await?;
    // Subscribe before listing, so nothing registered in between is lost.
    let registered = watcher
        .receive_status_notifier_item_registered()
        .await?
        .filter_map(|s| async move { s.args().ok().map(|a| Signal::Registered(a.service)) });
    let unregistered = watcher
        .receive_status_notifier_item_unregistered()
        .await?
        .filter_map(|s| async move { s.args().ok().map(|a| Signal::Unregistered(a.service)) });
    let owner_changed = dbus
        .receive_name_owner_changed_with_args(&[(0, WATCHER_NAME)])
        .await?
        .filter_map(|s| async move {
            s.args()
                .ok()
                .map(|a| Signal::WatcherOwner(a.new_owner().as_ref().map(|n| n.to_string())))
        });
    let mut signals: stream::SelectAll<BoxStream<'static, Signal>> = stream::select_all([
        registered.boxed(),
        unregistered.boxed(),
        owner_changed.boxed(),
    ]);

    let unique = conn
        .unique_name()
        .map(|n| n.to_string())
        .unwrap_or_default();
    let mut tasks: HashMap<String, AbortHandle> = HashMap::new();
    let register =
        |tasks: &mut HashMap<String, AbortHandle>, key: String, out: &mpsc::Sender<Event>| {
            if tasks.contains_key(&key) {
                return;
            }
            let task = tokio::spawn(watch_item(conn.clone(), key.clone(), out.clone()));
            tasks.insert(key, task.abort_handle());
        };
    let _ = watcher.register_status_notifier_host(&unique).await;
    for key in watcher
        .registered_status_notifier_items()
        .await
        .unwrap_or_default()
    {
        register(&mut tasks, key, &out);
    }

    while let Some(signal) = signals.next().await {
        match signal {
            Signal::Registered(key) => register(&mut tasks, key, &out),
            Signal::Unregistered(key) => {
                if let Some(task) = tasks.remove(&key) {
                    task.abort();
                    let _ = out.send(Event::Removed(key)).await;
                }
            }
            Signal::WatcherOwner(new_owner) => {
                let ours = new_owner.as_deref() == Some(unique.as_str());
                if ours {
                    continue;
                }
                // The other bar left and the name is ours now: the
                // items re-register with us (KDE and libayatana do on
                // NameOwnerChanged), so start from an empty list.
                let owner = new_owner.is_none() && own_watcher(&conn).await;
                if !owner {
                    log::info!("tray: {WATCHER_NAME} changed owner, re-reading its items");
                }
                for (key, task) in tasks.drain() {
                    task.abort();
                    let _ = out.send(Event::Removed(key)).await;
                }
                if !owner {
                    let _ = watcher.register_status_notifier_host(&unique).await;
                    for key in watcher
                        .registered_status_notifier_items()
                        .await
                        .unwrap_or_default()
                    {
                        register(&mut tasks, key, &out);
                    }
                }
            }
        }
    }
    Ok(())
}

// --- one item -------------------------------------------------------------

/// Read the item's properties, report them, then follow its signals
/// (and its menu's) until aborted.
async fn watch_item(conn: Connection, key: String, mut out: mpsc::Sender<Event>) {
    if let Err(e) = follow_item(conn, key.clone(), &mut out).await {
        log::warn!("tray: item {key}: {e}");
    }
}

async fn follow_item(
    conn: Connection,
    key: String,
    out: &mut mpsc::Sender<Event>,
) -> zbus::Result<()> {
    let (bus, path) = split_key(&key);
    let bus = BusName::try_from(bus.to_owned())?;
    let path = ObjectPath::try_from(path.to_owned())?;
    let iface = InterfaceName::try_from(ITEM_IFACE)?;
    let properties = PropertiesProxy::builder(&conn)
        .destination(bus.clone())?
        .path(path.clone())?
        .build()
        .await?;
    let item = StatusNotifierItemProxy::builder(&conn)
        .destination(bus.clone())?
        .path(path.clone())?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;

    let mut props = Props::default();
    for (name, value) in properties.get_all(iface.clone()).await? {
        props.set(&name, value);
    }
    let _ = out.send(Event::Added(key.clone(), props.clone())).await;

    // Each signal names the properties to read again; `NewStatus`
    // carries the value but is re-read too, to keep one path.
    type Names = &'static [&'static str];
    fn refetch<S: Stream + Send + 'static>(s: S, names: Names) -> BoxStream<'static, Names> {
        s.map(move |_| names).boxed()
    }
    let mut signals: stream::SelectAll<BoxStream<'static, Names>> = stream::select_all([
        refetch(item.receive_new_title().await?, &["Title"]),
        refetch(item.receive_new_icon().await?, &["IconName", "IconPixmap"]),
        refetch(
            item.receive_new_attention_icon().await?,
            &["AttentionIconName", "AttentionIconPixmap"],
        ),
        refetch(item.receive_new_tool_tip().await?, &["ToolTip"]),
        refetch(item.receive_new_menu().await?, &["Menu"]),
        refetch(item.receive_new_status().await?, &["Status"]),
    ]);
    // Well-behaved apps also emit PropertiesChanged; take the values
    // it carries, re-read the invalidated ones.
    let mut changed = properties.receive_properties_changed().await?;
    let mut menu_changed = match &props.menu {
        Some(menu_path) => Some(menu::changes(&conn, bus.clone(), menu_path).await?),
        None => None,
    };

    loop {
        let names: Vec<String> = tokio::select! {
            Some(names) = signals.next() => names.iter().map(|s| (*s).to_owned()).collect(),
            Some(change) = changed.next() => {
                let Ok(args) = change.args() else { continue };
                if *args.interface_name() != iface {
                    continue;
                }
                for (name, value) in args.changed_properties() {
                    if let Ok(v) = OwnedValue::try_from(value.clone()) {
                        props.set(name, v);
                    }
                }
                args.invalidated_properties().iter().map(|s| (*s).to_owned()).collect()
            }
            Some(()) = async {
                match &mut menu_changed {
                    Some(s) => s.next().await,
                    None => std::future::pending().await,
                }
            } => {
                let _ = out.send(Event::MenuChanged(key.clone())).await;
                continue;
            }
            else => break,
        };
        for name in &names {
            match properties.get(iface.clone(), name).await {
                Ok(value) => props.set(name, value),
                // Apps advertise NewIcon without having every property.
                Err(e) => log::debug!("tray: {key}: reading {name}: {e}"),
            }
        }
        let _ = out.send(Event::Updated(key.clone(), props.clone())).await;
    }
    Ok(())
}

impl Props {
    /// Apply one property from the bus; unknown names and values of the
    /// wrong type are ignored.
    fn set(&mut self, name: &str, value: OwnedValue) {
        fn string(v: OwnedValue) -> Option<String> {
            String::try_from(v).ok()
        }
        fn pixmap(v: OwnedValue) -> Option<Pixmap> {
            Vec::<(i32, i32, Vec<u8>)>::try_from(v)
                .ok()
                .and_then(|list| pick_pixmap(&list))
        }
        match name {
            "Id" => self.id = string(value).unwrap_or_default(),
            "Title" => self.title = string(value).unwrap_or_default(),
            "Status" => {
                self.status = match string(value).as_deref() {
                    Some("Passive") => Status::Passive,
                    Some("NeedsAttention") => Status::NeedsAttention,
                    _ => Status::Active,
                }
            }
            "IconName" => self.icon_name = string(value).unwrap_or_default(),
            "IconPixmap" => self.icon_pixmap = pixmap(value),
            "AttentionIconName" => self.attention_icon_name = string(value).unwrap_or_default(),
            "AttentionIconPixmap" => self.attention_pixmap = pixmap(value),
            "IconThemePath" => self.icon_theme_path = string(value).unwrap_or_default(),
            "ItemIsMenu" => self.item_is_menu = bool::try_from(value).unwrap_or(false),
            "Menu" => {
                self.menu = OwnedObjectPath::try_from(value)
                    .ok()
                    .map(|p| p.to_string())
                    .filter(|p| p != "/");
            }
            "ToolTip" => {
                // (icon name, icon pixmaps, title, text)
                self.tooltip =
                    <(String, Vec<(i32, i32, Vec<u8>)>, String, String)>::try_from(value)
                        .map(
                            |(_, _, title, text)| match (title.is_empty(), text.is_empty()) {
                                (false, false) => format!("{title}\n{text}"),
                                (false, true) => title,
                                _ => text,
                            },
                        )
                        .unwrap_or_default();
            }
            _ => {}
        }
    }
}

/// The pixmap to draw from the offered sizes: the largest that isn't
/// oversized, else the smallest. Converted from ARGB32 (network byte
/// order) to straight RGBA.
fn pick_pixmap(list: &[(i32, i32, Vec<u8>)]) -> Option<Pixmap> {
    let valid = |(w, h, data): &&(i32, i32, Vec<u8>)| {
        *w > 0 && *h > 0 && data.len() == (*w as usize) * (*h as usize) * 4
    };
    let chosen = list
        .iter()
        .filter(valid)
        .filter(|(w, h, _)| (*w as u32).max(*h as u32) <= PIXMAP_MAX)
        .max_by_key(|(w, h, _)| w * h)
        .or_else(|| list.iter().filter(valid).min_by_key(|(w, h, _)| w * h))?;
    let (w, h, argb) = chosen;
    let rgba = argb
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|[a, r, g, b]| [*r, *g, *b, *a])
        .collect();
    Some(Pixmap {
        width: *w as u32,
        height: *h as u32,
        rgba,
    })
}

// --- commands -------------------------------------------------------------

async fn item_proxy(
    conn: &Connection,
    key: &str,
) -> zbus::Result<StatusNotifierItemProxy<'static>> {
    let (bus, path) = split_key(key);
    StatusNotifierItemProxy::builder(conn)
        .destination(BusName::try_from(bus.to_owned())?)?
        .path(ObjectPath::try_from(path.to_owned())?)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
}

/// `Activate` / `SecondaryActivate` / `ContextMenu` at the pointer.
pub async fn activate(conn: Connection, key: String, method: &'static str, (x, y): (i32, i32)) {
    let result = async {
        let item = item_proxy(&conn, &key).await?;
        match method {
            "Activate" => item.activate(x, y).await,
            "SecondaryActivate" => item.secondary_activate(x, y).await,
            _ => item.context_menu(x, y).await,
        }
    }
    .await;
    if let Err(e) = result {
        log::warn!("tray: {method} on {key}: {e}");
    }
}

pub async fn scroll(conn: Connection, key: String, delta: i32, orientation: Orientation) {
    let orientation = match orientation {
        Orientation::Horizontal => "horizontal",
        Orientation::Vertical => "vertical",
    };
    let result = async {
        item_proxy(&conn, &key)
            .await?
            .scroll(delta, orientation)
            .await
    }
    .await;
    if let Err(e) = result {
        log::warn!("tray: Scroll on {key}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixmap_choice_and_conversion() {
        let px = |w: i32, h: i32| (w, h, vec![0u8; (w * h * 4) as usize]);
        let list = vec![px(16, 16), px(128, 128), px(32, 32)];
        let p = pick_pixmap(&list).unwrap();
        assert_eq!((p.width, p.height), (32, 32));

        let only_big = vec![px(256, 256), px(128, 128)];
        assert_eq!(pick_pixmap(&only_big).unwrap().width, 128);

        // ARGB -> RGBA
        let one = vec![(1, 1, vec![0x80, 0x11, 0x22, 0x33])];
        assert_eq!(pick_pixmap(&one).unwrap().rgba, [0x11, 0x22, 0x33, 0x80]);

        let bad = vec![(2, 2, vec![0; 3])];
        assert!(pick_pixmap(&bad).is_none());
    }

    #[test]
    fn tooltip_and_menu_props() {
        let mut p = Props::default();
        p.set(
            "Menu",
            OwnedValue::try_from(zbus::zvariant::Value::from(
                ObjectPath::try_from("/MenuBar").unwrap(),
            ))
            .unwrap(),
        );
        assert_eq!(p.menu.as_deref(), Some("/MenuBar"));
        p.set(
            "Menu",
            OwnedValue::try_from(zbus::zvariant::Value::from(
                ObjectPath::try_from("/").unwrap(),
            ))
            .unwrap(),
        );
        assert_eq!(p.menu, None);
        p.set(
            "Status",
            OwnedValue::try_from(zbus::zvariant::Value::from("NeedsAttention")).unwrap(),
        );
        assert_eq!(p.status, Status::NeedsAttention);
        p.set(
            "ItemIsMenu",
            OwnedValue::try_from(zbus::zvariant::Value::from(true)).unwrap(),
        );
        assert!(p.item_is_menu);
    }
}
