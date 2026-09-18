//! Sway backend, over the i3 IPC socket in `$SWAYSOCK`: every message is
//! `i3-ipc` + u32 payload length + u32 type (native endian) + JSON payload.
//! Requests get one connection each (Sway answers and we drop it), the
//! event connection sends one `subscribe` and then reads forever.
//!
//! Workspaces are identified by name (unique in Sway, and what the
//! `workspace` command takes; con ids don't select workspaces), windows
//! by their con id. Full lists come from `get_workspaces` and a walk of
//! `get_tree`, re-fetched on the events that change them; focus and
//! urgency are patched directly from the events.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream};
use iced::stream;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use super::{Command, Event, Window, Workspace};

const MAGIC: &[u8; 6] = b"i3-ipc";

const RUN_COMMAND: u32 = 0;
const GET_WORKSPACES: u32 = 1;
const SUBSCRIBE: u32 = 2;
const GET_TREE: u32 = 4;

/// Events have the high bit set, the rest is the event's index.
const EVENT: u32 = 0x8000_0000;
const EVENT_WORKSPACE: u32 = EVENT;
const EVENT_OUTPUT: u32 = EVENT | 1;
const EVENT_WINDOW: u32 = EVENT | 3;

fn socket_path() -> Option<PathBuf> {
    let path = PathBuf::from(env::var_os("SWAYSOCK")?);
    path.exists().then_some(path)
}

pub fn available() -> bool {
    socket_path().is_some()
}

async fn send(sock: &mut UnixStream, kind: u32, payload: &[u8]) -> std::io::Result<()> {
    let mut message = Vec::with_capacity(14 + payload.len());
    message.extend_from_slice(MAGIC);
    message.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    message.extend_from_slice(&kind.to_ne_bytes());
    message.extend_from_slice(payload);
    sock.write_all(&message).await
}

/// One message: its type and payload.
async fn recv<R: AsyncRead + Unpin>(sock: &mut R) -> std::io::Result<(u32, Vec<u8>)> {
    let mut header = [0u8; 14];
    sock.read_exact(&mut header).await?;
    if &header[..6] != MAGIC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bad i3-ipc magic",
        ));
    }
    let len = u32::from_ne_bytes(header[6..10].try_into().unwrap()) as usize;
    let kind = u32::from_ne_bytes(header[10..14].try_into().unwrap());
    let mut payload = vec![0; len];
    sock.read_exact(&mut payload).await?;
    Ok((kind, payload))
}

/// One request on its own connection; the reply is the raw payload.
async fn request(kind: u32, payload: &[u8]) -> std::io::Result<Vec<u8>> {
    let path = socket_path().ok_or(std::io::ErrorKind::NotFound)?;
    let mut sock = UnixStream::connect(path).await?;
    send(&mut sock, kind, payload).await?;
    let (reply_kind, reply) = recv(&mut sock).await?;
    if reply_kind != kind {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("reply type {reply_kind} to request {kind}"),
        ));
    }
    Ok(reply)
}

async fn request_json<T: for<'de> Deserialize<'de>>(kind: u32, payload: &[u8]) -> Option<T> {
    match request(kind, payload).await {
        Ok(body) => match serde_json::from_slice(&body) {
            Ok(v) => Some(v),
            Err(e) => {
                log::error!("sway: bad reply to message {kind}: {e}");
                None
            }
        },
        Err(e) => {
            log::error!("sway: message {kind} failed: {e}");
            None
        }
    }
}

#[derive(Deserialize)]
struct SwayWorkspace {
    /// The leading number of the name, -1 for a purely named one.
    num: i64,
    name: String,
    output: String,
    focused: bool,
    /// The one shown on its output.
    visible: bool,
}

/// A node of the tree, and of the `workspace`/`window` events.
#[derive(Deserialize)]
struct Node {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    name: Option<String>,
    /// Only on workspaces.
    #[serde(default)]
    output: Option<String>,
    /// Only on views (windows), unlike split containers.
    #[serde(default)]
    pid: Option<i64>,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    window_properties: Option<WindowProperties>,
    #[serde(default)]
    focused: bool,
    #[serde(default)]
    urgent: bool,
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    floating_nodes: Vec<Node>,
}

/// The X11 side of an Xwayland view.
#[derive(Deserialize)]
struct WindowProperties {
    #[serde(default)]
    class: Option<String>,
}

impl Node {
    fn is_view(&self) -> bool {
        self.pid.is_some() && matches!(self.kind.as_str(), "con" | "floating_con")
    }

    fn class(&self) -> String {
        self.app_id
            .clone()
            .or_else(|| self.window_properties.as_ref()?.class.clone())
            .unwrap_or_default()
    }

    /// Children in layout order, tiled then floating.
    fn children(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter().chain(&self.floating_nodes)
    }

    /// The focused view under this node, if any.
    fn focused_view(&self) -> Option<&Node> {
        if self.is_view() && self.focused {
            return Some(self);
        }
        self.children().find_map(Node::focused_view)
    }
}

/// The windows of the tree, in layout order, with the workspace holding
/// each. Sway's own `__i3` output holds the scratchpad: what's there is
/// hidden, so it is skipped along with anything else Sway keeps private.
fn walk<'a>(node: &'a Node, workspace: Option<&'a str>, out: &mut Vec<(&'a Node, &'a str)>) {
    let workspace = match node.kind.as_str() {
        "output" if node.name.as_deref().is_some_and(|n| n.starts_with("__")) => return,
        "workspace" => node.name.as_deref(),
        _ => workspace,
    };
    if node.is_view()
        && let Some(ws) = workspace
    {
        out.push((node, ws));
    }
    for child in node.children() {
        walk(child, workspace, out);
    }
}

/// Numbered workspaces in numeric order, then the named ones in Sway's
/// (per output) order.
async fn workspaces() -> Option<Event> {
    let mut list: Vec<SwayWorkspace> = request_json(GET_WORKSPACES, b"").await?;
    list.sort_by_key(|w| (w.num < 0, w.num));
    Some(Event::Workspaces(
        list.into_iter()
            .map(|w| Workspace {
                id: w.name.clone(),
                name: w.name,
                output: w.output,
                active: false,
                urgent: false,
            })
            .collect(),
    ))
}

/// The workspace shown on each output, and which output is focused (the
/// focused workspace's).
async fn active_workspaces() -> Vec<Event> {
    let list: Vec<SwayWorkspace> = request_json(GET_WORKSPACES, b"").await.unwrap_or_default();
    let focused = list
        .iter()
        .find(|w| w.focused)
        .map(|w| Event::FocusedOutput(w.output.clone()));
    list.into_iter()
        .filter(|w| w.visible)
        .map(|w| Event::ActiveWorkspace(w.name))
        .chain(focused)
        .collect()
}

/// The window list, then the focus and urgency the list itself doesn't
/// carry (a full list keeps the flags of known windows, new ones start
/// clear).
async fn windows() -> Vec<Event> {
    let Some(tree): Option<Node> = request_json(GET_TREE, b"").await else {
        return vec![];
    };
    let mut views = Vec::new();
    walk(&tree, None, &mut views);
    let list = views
        .iter()
        .map(|(n, ws)| Window {
            id: n.id.to_string(),
            class: n.class(),
            title: n.name.clone().unwrap_or_default(),
            workspace_id: (*ws).to_owned(),
            active: false,
            urgent: false,
        })
        .collect();
    let focused = views.iter().find(|(n, _)| n.focused).map(|(n, _)| n.id);
    let urgent: Vec<Event> = views
        .iter()
        .filter(|(n, _)| n.urgent)
        .map(|(n, _)| Event::UrgentWindow(n.id.to_string()))
        .collect();
    [
        Event::Windows(list),
        Event::ActiveWindow(focused.map(|id| id.to_string())),
    ]
    .into_iter()
    .chain(urgent)
    .collect()
}

/// Everything: after (re)connecting and when outputs change.
async fn everything() -> Vec<Event> {
    let mut events: Vec<Event> = workspaces().await.into_iter().collect();
    events.extend(windows().await);
    events.extend(active_workspaces().await);
    events
}

/// The event stream: initial state, then [`Event`]s from Sway's
/// `workspace`, `window` and `output` events. Reconnects if the socket
/// goes away.
pub fn events() -> impl Stream<Item = Event> {
    stream::channel(16, async move |mut tx| {
        loop {
            match listen(&mut tx).await {
                Ok(()) => log::warn!("sway: event socket closed"),
                Err(e) => log::warn!("sway: event socket error: {e}"),
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
}

#[derive(Deserialize)]
struct WorkspaceEvent {
    change: String,
    /// The workspace concerned; absent on `reload`.
    current: Option<Node>,
}

#[derive(Deserialize)]
struct WindowEvent {
    change: String,
    container: Node,
}

#[derive(Deserialize)]
struct Subscribed {
    success: bool,
}

async fn listen(tx: &mut mpsc::Sender<Event>) -> std::io::Result<()> {
    // Subscribe before the initial fetch so no event is missed in between.
    let path = socket_path().ok_or(std::io::ErrorKind::NotFound)?;
    let mut sock = UnixStream::connect(path).await?;
    send(
        &mut sock,
        SUBSCRIBE,
        br#"["workspace", "window", "output"]"#,
    )
    .await?;
    let mut sock = BufReader::new(sock);
    let (kind, reply) = recv(&mut sock).await?;
    let ok =
        kind == SUBSCRIBE && serde_json::from_slice::<Subscribed>(&reply).is_ok_and(|r| r.success);
    if !ok {
        return Err(std::io::Error::other("subscribe refused"));
    }

    for event in everything().await {
        let _ = tx.send(event).await;
    }

    loop {
        let (kind, payload) = recv(&mut sock).await?;
        let events = match kind {
            EVENT_WORKSPACE => match serde_json::from_slice::<WorkspaceEvent>(&payload) {
                Ok(ev) => workspace_event(ev).await,
                Err(e) => {
                    log::warn!("sway: malformed workspace event: {e}");
                    vec![]
                }
            },
            EVENT_WINDOW => match serde_json::from_slice::<WindowEvent>(&payload) {
                Ok(ev) => window_event(ev).await,
                Err(e) => {
                    log::warn!("sway: malformed window event: {e}");
                    vec![]
                }
            },
            EVENT_OUTPUT => {
                log::trace!("sway: output event");
                everything().await
            }
            _ => vec![],
        };
        for event in events {
            let _ = tx.send(event).await;
        }
    }
}

async fn workspace_event(ev: WorkspaceEvent) -> Vec<Event> {
    log::trace!("sway: workspace {}", ev.change);
    match ev.change.as_str() {
        // The focused workspace, on the now focused output; the focus
        // went to a window in it or, on an empty one, to the workspace
        // itself, in which case no `window` event will say so.
        "focus" => {
            let Some(ws) = ev.current else { return vec![] };
            let Some(name) = ws.name.clone() else {
                return vec![];
            };
            let focused = ws.focused_view().map(|v| v.id.to_string());
            ws.output
                .clone()
                .map(Event::FocusedOutput)
                .into_iter()
                .chain([Event::ActiveWorkspace(name), Event::ActiveWindow(focused)])
                .collect()
        }
        // Windows refer to their workspace by name.
        "init" | "empty" | "rename" | "move" | "reload" => {
            let mut events: Vec<Event> = workspaces().await.into_iter().collect();
            if ev.change != "init" && ev.change != "empty" {
                events.extend(windows().await);
            }
            events.extend(active_workspaces().await);
            events
        }
        // A workspace is urgent while a window in it is: the window's
        // own event covers it.
        _ => vec![],
    }
}

async fn window_event(ev: WindowEvent) -> Vec<Event> {
    log::trace!("sway: window {} {}", ev.change, ev.container.id);
    match ev.change.as_str() {
        "focus" => vec![Event::ActiveWindow(Some(ev.container.id.to_string()))],
        // Sway also reports the flag clearing, which focus already does.
        "urgent" if ev.container.urgent => {
            vec![Event::UrgentWindow(ev.container.id.to_string())]
        }
        // The event carries the container but not its workspace.
        "new" | "close" | "move" | "title" => windows().await,
        _ => vec![],
    }
}

#[derive(Deserialize)]
struct CommandResult {
    success: bool,
    #[serde(default)]
    error: String,
}

/// A click on the current workspace means "go there", not (with
/// `workspace_auto_back_and_forth`) "go back".
pub async fn run(command: Command) {
    let line = match command {
        Command::ActivateWorkspace(name) => {
            let name = name.replace('\\', "\\\\").replace('"', "\\\"");
            format!("workspace --no-auto-back-and-forth \"{name}\"")
        }
        Command::ActivateWindow(id) => format!("[con_id={id}] focus"),
        Command::Exit => "exit".to_owned(),
    };
    let Some(results): Option<Vec<CommandResult>> =
        request_json(RUN_COMMAND, line.as_bytes()).await
    else {
        return;
    };
    for r in results.iter().filter(|r| !r.success) {
        log::warn!("sway: {line:?} failed: {}", r.error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_finds_views_under_their_workspace_and_skips_the_scratchpad() {
        let tree: Node = serde_json::from_value(serde_json::json!({
            "id": 1, "type": "root", "nodes": [
                {"id": 2147483647, "type": "output", "name": "__i3", "nodes": [
                    {"id": 2147483646, "type": "workspace", "name": "__i3_scratch",
                     "floating_nodes": [
                        {"id": 99, "type": "floating_con", "pid": 9, "app_id": "hidden"}
                     ]}
                ]},
                {"id": 3, "type": "output", "name": "DP-1", "nodes": [
                    {"id": 5, "type": "workspace", "name": "1", "output": "DP-1", "nodes": [
                        {"id": 6, "type": "con", "nodes": [
                            {"id": 7, "type": "con", "pid": 1, "app_id": "kitty",
                             "name": "sh", "focused": true},
                            {"id": 8, "type": "con", "pid": 2, "app_id": null,
                             "window_properties": {"class": "Firefox"}, "urgent": true}
                        ]}
                    ], "floating_nodes": [
                        {"id": 9, "type": "floating_con", "pid": 3, "app_id": "pavucontrol"}
                    ]},
                    {"id": 10, "type": "workspace", "name": "web", "output": "DP-1", "nodes": []}
                ]}
            ]
        }))
        .unwrap();
        let mut views = Vec::new();
        walk(&tree, None, &mut views);
        let got: Vec<_> = views.iter().map(|(n, ws)| (n.id, n.class(), *ws)).collect();
        assert_eq!(
            got,
            [
                (7, "kitty".to_owned(), "1"),
                (8, "Firefox".to_owned(), "1"),
                (9, "pavucontrol".to_owned(), "1"),
            ]
        );
        assert_eq!(tree.focused_view().map(|n| n.id), Some(7));
        assert!(views.iter().any(|(n, _)| n.id == 8 && n.urgent));
    }

    #[test]
    fn framing_roundtrip() {
        let mut message = Vec::new();
        message.extend_from_slice(MAGIC);
        message.extend_from_slice(&2u32.to_ne_bytes());
        message.extend_from_slice(&EVENT_WINDOW.to_ne_bytes());
        message.extend_from_slice(b"{}");
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (kind, payload) = rt
            .block_on(recv(&mut std::io::Cursor::new(message)))
            .unwrap();
        assert_eq!(kind, EVENT_WINDOW);
        assert_eq!(payload, b"{}");
    }
}
