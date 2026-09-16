//! A status notifier item for the UI scenarios: registers with the
//! watcher (the shell's), serves a pixmap icon and a `dbusmenu`, and
//! reports what the host does to it on stdout, one line each:
//!
//!   activate <x> <y> | secondary <x> <y> | context <x> <y>
//!   scroll <delta> <orientation> | menu-event <id> <event>
//!   about-to-show <id>
//!
//! Stdin drives changes (one command per line): `status <Status>`,
//! `title <text>`, `icon <r> <g> <b>` (a new solid pixmap, `NewIcon`),
//! `menu-label <id> <text>` (`LayoutUpdated`). `ready` is printed once
//! registered.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, BufReader};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{interface, proxy};

const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";

/// `a(iiay)`: (width, height, ARGB32 pixels).
type Pixmaps = Vec<(i32, i32, Vec<u8>)>;
/// `(sa(iiay)ss)`: icon name, icon pixmaps, title, text.
type ToolTip = (String, Pixmaps, String, String);
/// A menu row: id, label, dbusmenu properties beyond `label`.
type Row = (i32, String, Vec<(&'static str, Value<'static>)>);

/// `a(iiay)`: a solid `size`x`size` ARGB32 square.
fn pixmap(size: i32, (r, g, b): (u8, u8, u8)) -> Pixmaps {
    let px = [0xff, r, g, b];
    vec![(size, size, px.repeat((size * size) as usize))]
}

#[derive(Clone)]
struct State {
    status: String,
    title: String,
    color: (u8, u8, u8),
    menu: Vec<Row>,
    revision: u32,
}

impl State {
    fn new() -> Self {
        Self {
            status: "Active".to_owned(),
            title: "Aria test item".to_owned(),
            color: (0xe0, 0x40, 0x40),
            menu: vec![
                (1, "Open".to_owned(), vec![]),
                (2, String::new(), vec![("type", "separator".into())]),
                (
                    3,
                    "Options".to_owned(),
                    vec![("children-display", "submenu".into())],
                ),
                (
                    4,
                    "Beep".to_owned(),
                    vec![
                        ("toggle-type", "checkmark".into()),
                        ("toggle-state", 1i32.into()),
                    ],
                ),
                (5, "Hidden".to_owned(), vec![("visible", false.into())]),
                (6, "Disabled".to_owned(), vec![("enabled", false.into())]),
                (7, "Quit".to_owned(), vec![]),
            ],
            revision: 1,
        }
    }

    /// Row `4` (Beep) is a child of `3` (Options), the rest are top-level.
    fn parent_of(id: i32) -> i32 {
        if id == 4 { 3 } else { 0 }
    }
}

struct Item {
    state: Arc<Mutex<State>>,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn activate(&self, x: i32, y: i32) {
        println!("activate {x} {y}");
    }

    fn secondary_activate(&self, x: i32, y: i32) {
        println!("secondary {x} {y}");
    }

    fn context_menu(&self, x: i32, y: i32) {
        println!("context {x} {y}");
    }

    fn scroll(&self, delta: i32, orientation: &str) {
        println!("scroll {delta} {orientation}");
    }

    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> &str {
        "aria-test"
    }

    #[zbus(property)]
    fn title(&self) -> String {
        self.state.lock().unwrap().title.clone()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        self.state.lock().unwrap().status.clone()
    }

    #[zbus(property)]
    fn icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Pixmaps {
        pixmap(22, self.state.lock().unwrap().color)
    }

    #[zbus(property)]
    fn attention_icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Pixmaps {
        pixmap(22, (0x40, 0x40, 0xe0))
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).unwrap()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> ToolTip {
        (
            String::new(),
            vec![],
            "Aria".to_owned(),
            "test item".to_owned(),
        )
    }

    #[zbus(signal)]
    async fn new_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn new_attention_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn new_title(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn new_status(emitter: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

struct Menu {
    state: Arc<Mutex<State>>,
}

type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

fn owned(v: Value<'static>) -> OwnedValue {
    OwnedValue::try_from(v).unwrap()
}

impl Menu {
    fn node(state: &State, id: i32) -> Layout {
        let mut props: HashMap<String, OwnedValue> = HashMap::new();
        if id == 0 {
            props.insert("children-display".to_owned(), owned("submenu".into()));
        } else if let Some((_, label, extras)) = state.menu.iter().find(|(i, ..)| *i == id) {
            props.insert("label".to_owned(), owned(label.clone().into()));
            for (k, v) in extras {
                props.insert((*k).to_owned(), owned(v.clone()));
            }
        }
        let children = state
            .menu
            .iter()
            .filter(|(i, ..)| State::parent_of(*i) == id)
            .map(|(i, ..)| {
                let (id, props, children) = Self::node(state, *i);
                owned(Value::from(zbus::zvariant::Structure::from((
                    id, props, children,
                ))))
            })
            .collect();
        (id, props, children)
    }
}

#[interface(name = "com.canonical.dbusmenu")]
impl Menu {
    fn get_layout(
        &self,
        parent_id: i32,
        _recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> (u32, Layout) {
        let state = self.state.lock().unwrap();
        (state.revision, Self::node(&state, parent_id))
    }

    fn event(&self, id: i32, event_id: &str, _data: Value<'_>, _timestamp: u32) {
        println!("menu-event {id} {event_id}");
    }

    fn about_to_show(&self, id: i32) -> bool {
        println!("about-to-show {id}");
        false
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }

    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }

    #[zbus(signal)]
    async fn layout_updated(
        emitter: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait StatusNotifierWatcher {
    fn register_status_notifier_item(&self, service: &str) -> zbus::Result<()>;
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> zbus::Result<()> {
    let state = Arc::new(Mutex::new(State::new()));
    let conn = zbus::connection::Builder::session()?
        .serve_at(
            ITEM_PATH,
            Item {
                state: state.clone(),
            },
        )?
        .serve_at(
            MENU_PATH,
            Menu {
                state: state.clone(),
            },
        )?
        .build()
        .await?;
    let watcher = StatusNotifierWatcherProxy::new(&conn).await?;
    // Wait for the shell's watcher (the scenario starts us alongside).
    let mut tries = 0;
    loop {
        match watcher.register_status_notifier_item(ITEM_PATH).await {
            Ok(()) => break,
            Err(e) if tries < 50 => {
                tries += 1;
                let _ = e;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Err(e) => {
                eprintln!("no watcher: {e}");
                std::process::exit(1);
            }
        }
    }
    println!("ready");

    let item = SignalEmitter::new(&conn, ITEM_PATH)?;
    let menu = SignalEmitter::new(&conn, MENU_PATH)?;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["status", s] => {
                state.lock().unwrap().status = (*s).to_owned();
                Item::new_status(&item, s).await?;
            }
            ["title", rest @ ..] => {
                state.lock().unwrap().title = rest.join(" ");
                Item::new_title(&item).await?;
            }
            ["icon", r, g, b] => {
                state.lock().unwrap().color = (
                    r.parse().unwrap_or(0),
                    g.parse().unwrap_or(0),
                    b.parse().unwrap_or(0),
                );
                Item::new_icon(&item).await?;
            }
            ["menu-label", id, rest @ ..] => {
                let revision = {
                    let mut s = state.lock().unwrap();
                    let id: i32 = id.parse().unwrap_or(0);
                    if let Some(row) = s.menu.iter_mut().find(|(i, ..)| *i == id) {
                        row.1 = rest.join(" ");
                    }
                    s.revision += 1;
                    s.revision
                };
                Menu::layout_updated(&menu, revision, 0).await?;
            }
            ["quit"] => break,
            _ => eprintln!("unknown command: {line}"),
        }
        println!("ok");
    }
    Ok(())
}
