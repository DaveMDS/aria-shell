//! Hyprland backend, over its two unix sockets: `.socket.sock` is
//! request/response (one connection per request, Hyprland closes it after
//! answering), `.socket2.sock` streams `event>>data` lines.
//!
//! Same strategy as the Python implementation: fetch the full lists once,
//! then re-fetch the affected list on the events that change it, and patch
//! the active/urgent state directly from the `*v2` events.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream};
use iced::stream;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use super::{Command, Event, Window, Workspace};

fn runtime_dir() -> Option<PathBuf> {
    let sig = env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let base = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    let dir = base.join("hypr").join(sig);
    dir.is_dir().then_some(dir)
}

pub fn available() -> bool {
    runtime_dir().is_some_and(|d| d.join(".socket.sock").exists())
}

/// One request on the command socket; the reply is the raw body.
async fn request(command: &str) -> std::io::Result<Vec<u8>> {
    let dir = runtime_dir().ok_or(std::io::ErrorKind::NotFound)?;
    let mut sock = UnixStream::connect(dir.join(".socket.sock")).await?;
    sock.write_all(command.as_bytes()).await?;
    let mut reply = Vec::new();
    sock.read_to_end(&mut reply).await?;
    Ok(reply)
}

async fn request_json<T: for<'de> Deserialize<'de>>(command: &str) -> Option<T> {
    match request(command).await {
        Ok(body) => match serde_json::from_slice(&body) {
            Ok(v) => Some(v),
            Err(e) => {
                log::error!("hyprland: bad reply to {command:?}: {e}");
                None
            }
        },
        Err(e) => {
            log::error!("hyprland: {command:?} failed: {e}");
            None
        }
    }
}

#[derive(Deserialize)]
struct HyprWorkspace {
    id: i64,
    name: String,
    monitor: String,
}

#[derive(Deserialize)]
struct HyprClient {
    address: String,
    mapped: bool,
    class: String,
    title: String,
    workspace: HyprWorkspaceRef,
}

#[derive(Deserialize)]
struct HyprWorkspaceRef {
    id: i64,
}

#[derive(Deserialize)]
struct HyprMonitor {
    #[serde(rename = "activeWorkspace")]
    active_workspace: HyprWorkspaceRef,
}

#[derive(Deserialize)]
struct HyprActiveWindow {
    #[serde(default)]
    address: String,
}

/// Hyprland reports workspaces in creation order and includes special
/// (scratchpad) ones, which have negative ids. Sort by id, drop specials.
async fn workspaces() -> Option<Event> {
    let mut list: Vec<HyprWorkspace> = request_json("j/workspaces").await?;
    list.retain(|w| w.id >= 0);
    list.sort_by_key(|w| w.id);
    Some(Event::Workspaces(
        list.into_iter()
            .map(|w| Workspace {
                id: w.id.to_string(),
                name: w.name,
                output: w.monitor,
                active: false,
                urgent: false,
            })
            .collect(),
    ))
}

async fn windows() -> Option<Event> {
    let list: Vec<HyprClient> = request_json("j/clients").await?;
    Some(Event::Windows(
        list.into_iter()
            .filter(|c| c.mapped)
            .map(|c| Window {
                id: window_id(&c.address).to_owned(),
                class: c.class,
                title: c.title,
                workspace_id: c.workspace.id.to_string(),
                active: false,
                urgent: false,
            })
            .collect(),
    ))
}

/// The workspace shown on each monitor.
async fn active_workspaces() -> Vec<Event> {
    let monitors: Vec<HyprMonitor> = request_json("j/monitors").await.unwrap_or_default();
    monitors
        .into_iter()
        .map(|m| Event::ActiveWorkspace(m.active_workspace.id.to_string()))
        .collect()
}

async fn active_window() -> Option<Event> {
    let win: HyprActiveWindow = request_json("j/activewindow").await?;
    Some(Event::ActiveWindow(
        (!win.address.is_empty()).then(|| window_id(&win.address).to_owned()),
    ))
}

/// Window ids are the client address without the `0x`, which is how the
/// event socket reports them.
fn window_id(address: &str) -> &str {
    address.strip_prefix("0x").unwrap_or(address)
}

/// The event stream: initial state, then one [`Event`] per relevant
/// Hyprland event. Reconnects if the event socket goes away.
pub fn events() -> impl Stream<Item = Event> {
    stream::channel(16, async move |mut tx| {
        loop {
            match listen(&mut tx).await {
                Ok(()) => log::warn!("hyprland: event socket closed"),
                Err(e) => log::warn!("hyprland: event socket error: {e}"),
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
}

async fn listen(tx: &mut mpsc::Sender<Event>) -> std::io::Result<()> {
    // Connect before the initial fetch so no event is missed in between.
    let dir = runtime_dir().ok_or(std::io::ErrorKind::NotFound)?;
    let sock = UnixStream::connect(dir.join(".socket2.sock")).await?;
    let mut lines = BufReader::new(sock).lines();

    let initial = [workspaces().await, windows().await]
        .into_iter()
        .flatten()
        .chain(active_workspaces().await)
        .chain(active_window().await);
    for event in initial {
        let _ = tx.send(event).await;
    }

    while let Some(line) = lines.next_line().await? {
        let Some((name, data)) = line.split_once(">>") else {
            log::warn!("hyprland: malformed event {line:?}");
            continue;
        };
        log::trace!("hyprland: {name} {data:?}");
        let events: Vec<Option<Event>> = match name {
            // `workspacev2>>ID,NAME`: switched on the focused monitor.
            "workspacev2" => {
                let id = data.split(',').next().unwrap_or(data);
                vec![Some(Event::ActiveWorkspace(id.to_owned()))]
            }
            // `focusedmonv2>>MONITOR,WSID`: that monitor is already
            // showing WSID, but say so in case we missed the switch.
            "focusedmonv2" => {
                let id = data.split_once(',').map(|(_, id)| id).unwrap_or(data);
                vec![Some(Event::ActiveWorkspace(id.to_owned()))]
            }
            "activewindowv2" => vec![Some(Event::ActiveWindow(
                (!data.is_empty()).then(|| data.to_owned()),
            ))],
            "urgent" => vec![Some(Event::UrgentWindow(data.to_owned()))],
            "openwindow" | "closewindow" | "movewindowv2" | "windowtitlev2" => {
                vec![windows().await]
            }
            "createworkspacev2" | "destroyworkspacev2" | "renameworkspace" => {
                vec![workspaces().await, windows().await]
            }
            "moveworkspacev2" | "monitoradded" | "monitorremoved" => {
                let mut events = vec![workspaces().await, windows().await];
                events.extend(active_workspaces().await.into_iter().map(Some));
                events
            }
            _ => vec![],
        };
        for event in events.into_iter().flatten() {
            let _ = tx.send(event).await;
        }
    }
    Ok(())
}

/// Dispatchers are Lua expressions since Hyprland 0.56 (`hl.dsp.*`); the
/// old `dispatch workspace 3` form is rejected by the same socket.
pub async fn run(command: Command) {
    let dispatch = match command {
        Command::ActivateWorkspace(id) => format!("dispatch hl.dsp.focus({{ workspace = {id} }})"),
        Command::ActivateWindow(id) => {
            format!("dispatch hl.dsp.focus({{ window = \"address:0x{id}\" }})")
        }
    };
    match request(&dispatch).await {
        Ok(reply) if reply == b"ok" => {}
        Ok(reply) => log::warn!(
            "hyprland: {dispatch:?} replied {:?}",
            String::from_utf8_lossy(&reply)
        ),
        Err(e) => log::error!("hyprland: {dispatch:?} failed: {e}"),
    }
}
