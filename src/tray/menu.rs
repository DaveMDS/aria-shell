//! `com.canonical.dbusmenu`: the menu a tray item exports, fetched as
//! one layout tree and shown by the tray gadget in its popup.
//!
//! Reference: `libdbusmenu/libdbusmenu-glib/dbus-menu.xml`. Not
//! handled: icon data, shortcuts, `disposition`, `TextDirection`,
//! `IconThemePath`.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use iced::futures::{Stream, StreamExt};
use zbus::names::BusName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};
use zbus::{Connection, proxy};

/// `(id, properties, children)`, where each child is the same struct in
/// a variant.
type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

#[proxy(interface = "com.canonical.dbusmenu")]
trait DbusMenu {
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: &[&str],
    ) -> zbus::Result<(u32, Layout)>;

    fn event(&self, id: i32, event_id: &str, data: &Value<'_>, timestamp: u32) -> zbus::Result<()>;

    fn about_to_show(&self, id: i32) -> zbus::Result<bool>;

    #[zbus(signal)]
    fn layout_updated(&self, revision: u32, parent: i32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn items_properties_updated(
        &self,
        updated: Vec<(i32, HashMap<String, OwnedValue>)>,
        removed: Vec<(i32, Vec<String>)>,
    ) -> zbus::Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Check,
    Radio,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuItem {
    pub id: i32,
    /// Without the `_` mnemonic markers.
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub toggle: Option<Toggle>,
    /// `toggle-state`: 0 off, 1 on, anything else "indeterminate".
    pub checked: Option<bool>,
    pub icon_name: String,
    /// Has (or may have, once `AboutToShow`n) children.
    pub submenu: bool,
    /// Visible children only.
    pub children: Vec<MenuItem>,
}

/// A fetched layout: the root's children are the top-level entries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Menu {
    pub revision: u32,
    pub root: MenuItem,
}

impl Menu {
    pub fn items(&self) -> &[MenuItem] {
        &self.root.children
    }

    #[cfg(test)]
    pub fn find(&self, id: i32) -> Option<&MenuItem> {
        fn walk(item: &MenuItem, id: i32) -> Option<&MenuItem> {
            if item.id == id {
                return Some(item);
            }
            item.children.iter().find_map(|c| walk(c, id))
        }
        walk(&self.root, id)
    }
}

fn parse(layout: Layout) -> MenuItem {
    let (id, props, children) = layout;
    let str_prop = |name: &str| {
        props
            .get(name)
            .and_then(|v| <&str>::try_from(&**v).ok())
            .unwrap_or_default()
    };
    let bool_prop = |name: &str, default: bool| {
        props
            .get(name)
            .and_then(|v| bool::try_from(&**v).ok())
            .unwrap_or(default)
    };
    let visible = bool_prop("visible", true);
    let children: Vec<MenuItem> = children
        .into_iter()
        .filter_map(|v| Layout::try_from(v).ok())
        .map(parse)
        .filter(|c| c.id >= 0)
        .collect();
    // An invisible item is dropped by its parent (id -1 marks it).
    MenuItem {
        id: if visible { id } else { -1 },
        label: strip_mnemonics(str_prop("label")),
        enabled: bool_prop("enabled", true),
        separator: str_prop("type") == "separator",
        toggle: match str_prop("toggle-type") {
            "checkmark" => Some(Toggle::Check),
            "radio" => Some(Toggle::Radio),
            _ => None,
        },
        checked: props
            .get("toggle-state")
            .and_then(|v| i32::try_from(&**v).ok())
            .and_then(|s| match s {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            }),
        icon_name: str_prop("icon-name").to_owned(),
        submenu: str_prop("children-display") == "submenu",
        children,
    }
}

/// `_File` -> `File`, `Save__As` -> `Save_As`.
fn strip_mnemonics(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                chars.next();
                out.push('_');
            }
        } else {
            out.push(c);
        }
    }
    out
}

async fn menu_proxy(
    conn: &Connection,
    key: &str,
    path: &str,
) -> zbus::Result<DbusMenuProxy<'static>> {
    let (bus, _) = super::split_key(key);
    DbusMenuProxy::builder(conn)
        .destination(BusName::try_from(bus.to_owned())?)?
        .path(ObjectPath::try_from(path.to_owned())?)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
}

/// `AboutToShow(node)` (apps build their menu on it; the reply doesn't
/// matter, the layout is fetched anyway), then the whole tree.
pub async fn load(conn: Connection, key: String, path: String, node: i32) -> Option<Menu> {
    let result = async {
        let menu = menu_proxy(&conn, &key, &path).await?;
        if let Err(e) = menu.about_to_show(node).await {
            log::debug!("tray: {key}: AboutToShow({node}): {e}");
        }
        let (revision, layout) = menu.get_layout(0, -1, &[]).await?;
        zbus::Result::Ok(Menu {
            revision,
            root: parse(layout),
        })
    }
    .await;
    match result {
        Ok(menu) => Some(menu),
        Err(e) => {
            log::warn!("tray: {key}: cannot read the menu at {path}: {e}");
            None
        }
    }
}

pub async fn click(conn: Connection, key: String, path: String, id: i32) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    let result = async {
        menu_proxy(&conn, &key, &path)
            .await?
            .event(id, "clicked", &Value::from(""), timestamp)
            .await
    }
    .await;
    if let Err(e) = result {
        log::warn!("tray: {key}: menu click on {id}: {e}");
    }
}

/// A stream yielding once per layout or item-property change.
pub async fn changes(
    conn: &Connection,
    bus: BusName<'static>,
    path: &str,
) -> zbus::Result<impl Stream<Item = ()> + Send + 'static> {
    let menu = DbusMenuProxy::builder(conn)
        .destination(bus)?
        .path(ObjectPath::try_from(path.to_owned())?)?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let layout = menu.receive_layout_updated().await?.map(|_| ());
    let props = menu.receive_items_properties_updated().await?.map(|_| ());
    Ok(iced::futures::stream::select(layout, props))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(v: impl Into<Value<'static>>) -> OwnedValue {
        OwnedValue::try_from(v.into()).unwrap()
    }

    fn node(id: i32, props: &[(&str, Value<'static>)], children: Vec<OwnedValue>) -> OwnedValue {
        let props: HashMap<String, OwnedValue> = props
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OwnedValue::try_from(v.clone()).unwrap()))
            .collect();
        value(zbus::zvariant::Structure::from((id, props, children)))
    }

    #[test]
    fn mnemonics() {
        assert_eq!(strip_mnemonics("_Quit"), "Quit");
        assert_eq!(strip_mnemonics("Save__As"), "Save_As");
        assert_eq!(strip_mnemonics("plain"), "plain");
    }

    #[test]
    fn layout_tree() {
        let root: Layout = (
            0,
            HashMap::from([("children-display".to_owned(), value("submenu"))]),
            vec![
                node(1, &[("label", "_Open".into())], vec![]),
                node(2, &[("type", "separator".into())], vec![]),
                node(
                    3,
                    &[
                        ("label", "Options".into()),
                        ("children-display", "submenu".into()),
                    ],
                    vec![
                        node(
                            4,
                            &[
                                ("label", "Beep".into()),
                                ("toggle-type", "checkmark".into()),
                                ("toggle-state", 1i32.into()),
                            ],
                            vec![],
                        ),
                        node(
                            5,
                            &[("label", "Hidden".into()), ("visible", false.into())],
                            vec![],
                        ),
                    ],
                ),
                node(
                    6,
                    &[("label", "Quit".into()), ("enabled", false.into())],
                    vec![],
                ),
            ],
        );
        let menu = Menu {
            revision: 1,
            root: parse(root),
        };
        let labels: Vec<&str> = menu.items().iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Open", "", "Options", "Quit"]);
        assert!(menu.items()[1].separator);
        assert!(menu.items()[2].submenu);
        assert_eq!(menu.items()[2].children.len(), 1, "hidden child dropped");
        let beep = menu.find(4).unwrap();
        assert_eq!(beep.toggle, Some(Toggle::Check));
        assert_eq!(beep.checked, Some(true));
        assert!(!menu.find(6).unwrap().enabled);
        assert!(menu.find(5).is_none());
    }
}
