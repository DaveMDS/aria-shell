//! Media players over `org.mpris.MediaPlayer2` on the session bus: one
//! task per `org.mpris.MediaPlayer2.*` name reads the player's
//! properties and follows `PropertiesChanged`; names coming and going
//! are followed through `NameOwnerChanged`.
//!
//! Reference: <https://specifications.freedesktop.org/mpris-spec/latest/>

use std::collections::HashMap;

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream as iced_stream;
use tokio::task::AbortHandle;
use zbus::fdo::{DBusProxy, PropertiesProxy};
use zbus::names::{BusName, InterfaceName};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, proxy};

use super::{Event, PlaybackStatus, Player};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

#[proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2"
)]
pub trait MediaPlayer {
    fn play_pause(&self) -> zbus::Result<()>;
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;
}

pub async fn player_proxy(conn: &Connection, bus: &str) -> zbus::Result<MediaPlayerProxy<'static>> {
    MediaPlayerProxy::builder(conn)
        .destination(BusName::try_from(bus.to_owned())?)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
}

/// The event stream: the bus, then every player and its changes.
pub fn events() -> impl Stream<Item = Event> {
    iced_stream::channel(64, async move |mut out: mpsc::Sender<Event>| {
        let conn = match Connection::session().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("audio: no session bus for the players: {e}");
                return;
            }
        };
        let _ = out.send(Event::Bus(conn.clone())).await;
        if let Err(e) = watch_names(conn, out).await {
            log::error!("audio: players: {e}");
        }
    })
}

async fn watch_names(conn: Connection, out: mpsc::Sender<Event>) -> zbus::Result<()> {
    let dbus = DBusProxy::new(&conn).await?;
    // Subscribe before listing, so nothing appearing in between is lost.
    let mut owner_changed = dbus.receive_name_owner_changed().await?;
    let mut tasks: HashMap<String, AbortHandle> = HashMap::new();
    let start = |tasks: &mut HashMap<String, AbortHandle>, name: String| {
        if name.starts_with(PREFIX) && !tasks.contains_key(&name) {
            let task = tokio::spawn(watch_player(conn.clone(), name.clone(), out.clone()));
            tasks.insert(name, task.abort_handle());
        }
    };
    for name in dbus.list_names().await? {
        start(&mut tasks, name.to_string());
    }
    while let Some(signal) = owner_changed.next().await {
        let Ok(args) = signal.args() else { continue };
        let name = args.name().to_string();
        if !name.starts_with(PREFIX) {
            continue;
        }
        match args.new_owner().as_ref() {
            Some(_) => start(&mut tasks, name),
            None => {
                if let Some(task) = tasks.remove(&name) {
                    task.abort();
                }
                let _ = out.clone().send(Event::PlayerGone(name)).await;
            }
        }
    }
    Ok(())
}

async fn watch_player(conn: Connection, bus: String, mut out: mpsc::Sender<Event>) {
    if let Err(e) = follow_player(conn, bus.clone(), &mut out).await {
        log::debug!("audio: player {bus}: {e}");
    }
}

async fn follow_player(
    conn: Connection,
    bus: String,
    out: &mut mpsc::Sender<Event>,
) -> zbus::Result<()> {
    let properties = PropertiesProxy::builder(&conn)
        .destination(BusName::try_from(bus.clone())?)?
        .path(PATH)?
        .build()
        .await?;
    let root = InterfaceName::try_from(ROOT_IFACE)?;
    let player_iface = InterfaceName::try_from(PLAYER_IFACE)?;

    let mut player = Player::new(bus.clone());
    for (name, value) in properties.get_all(root.clone()).await? {
        player.set(&name, value);
    }
    for (name, value) in properties.get_all(player_iface.clone()).await? {
        player.set(&name, value);
    }
    log::info!("audio: player {bus} ({})", player.identity);
    let _ = out.send(Event::Player(player.clone())).await;

    let mut changed = properties.receive_properties_changed().await?;
    while let Some(change) = changed.next().await {
        let Ok(args) = change.args() else { continue };
        let iface = args.interface_name();
        if *iface != player_iface && *iface != root {
            continue;
        }
        for (name, value) in args.changed_properties() {
            if let Ok(v) = OwnedValue::try_from(value.clone()) {
                player.set(name, v);
            }
        }
        for name in args.invalidated_properties().iter() {
            if let Ok(v) = properties.get(iface.clone(), name).await {
                player.set(name, v);
            }
        }
        let _ = out.send(Event::Player(player.clone())).await;
    }
    Ok(())
}

impl Player {
    fn new(bus: String) -> Self {
        Self {
            bus,
            identity: String::new(),
            desktop_entry: None,
            status: PlaybackStatus::Stopped,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            art: None,
            can_go_next: false,
            can_go_previous: false,
            can_play: false,
            can_pause: false,
        }
    }

    /// One property of either interface.
    fn set(&mut self, name: &str, value: OwnedValue) {
        let value = Value::from(value);
        let string = |v: &Value| String::try_from(v.clone()).unwrap_or_default();
        let boolean = |v: &Value| bool::try_from(v).unwrap_or(false);
        match name {
            "Identity" => self.identity = string(&value),
            "DesktopEntry" => {
                let id = string(&value);
                self.desktop_entry = (!id.is_empty()).then_some(id);
            }
            "PlaybackStatus" => {
                self.status = match string(&value).as_str() {
                    "Playing" => PlaybackStatus::Playing,
                    "Paused" => PlaybackStatus::Paused,
                    _ => PlaybackStatus::Stopped,
                }
            }
            "Metadata" => {
                let Value::Dict(dict) = &value else { return };
                // A variant holds each value; look through it.
                let get = |key: &str| -> Option<Value<'static>> {
                    let (_, mut v) = dict.iter().find(|(k, _)| string(k) == key)?;
                    while let Value::Value(boxed) = v {
                        v = boxed.as_ref();
                    }
                    v.try_to_owned().map(Value::from).ok()
                };
                self.title = get("xesam:title").map(|v| string(&v)).unwrap_or_default();
                self.album = get("xesam:album").map(|v| string(&v)).unwrap_or_default();
                self.artist = match get("xesam:artist") {
                    Some(Value::Array(list)) => list
                        .iter()
                        .map(string)
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(", "),
                    Some(v) => string(&v),
                    None => String::new(),
                };
                let art = get("mpris:artUrl").map(|v| string(&v)).unwrap_or_default();
                self.art = (!art.is_empty()).then_some(art);
            }
            "CanGoNext" => self.can_go_next = boolean(&value),
            "CanGoPrevious" => self.can_go_previous = boolean(&value),
            "CanPlay" => self.can_play = boolean(&value),
            "CanPause" => self.can_pause = boolean(&value),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{Array, Dict, Signature};

    #[test]
    fn metadata_and_status() {
        let mut p = Player::new("org.mpris.MediaPlayer2.test".into());
        p.set("Identity", OwnedValue::try_from(Value::from("Test")).unwrap());
        p.set(
            "PlaybackStatus",
            OwnedValue::try_from(Value::from("Playing")).unwrap(),
        );
        let mut dict = Dict::new(&Signature::Str, &Signature::Variant);
        dict.add("xesam:title", Value::new(Value::from("Song")))
            .unwrap();
        let mut artists = Array::new(&Signature::Str);
        artists.append(Value::from("A")).unwrap();
        artists.append(Value::from("B")).unwrap();
        dict.add("xesam:artist", Value::new(Value::from(artists)))
            .unwrap();
        dict.add(
            "mpris:artUrl",
            Value::new(Value::from("file:///tmp/cover.png")),
        )
        .unwrap();
        p.set("Metadata", OwnedValue::try_from(Value::from(dict)).unwrap());
        p.set("CanGoNext", OwnedValue::try_from(Value::from(true)).unwrap());
        assert_eq!(p.identity, "Test");
        assert_eq!(p.status, PlaybackStatus::Playing);
        assert_eq!(p.title, "Song");
        assert_eq!(p.artist, "A, B");
        assert_eq!(p.art.as_deref(), Some("file:///tmp/cover.png"));
        assert!(p.can_go_next);
    }
}
