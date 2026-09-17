//! The bus side: the `org.freedesktop.Notifications` object we serve.
//!
//! The object is always served; the name is requested without queueing
//! and, if another daemon owns it (mako, the desktop's shell), asked
//! for again when that one goes away. Every `Notify` and
//! `CloseNotification` becomes an [`Event`] for the daemon; the
//! `NotificationClosed` / `ActionInvoked` signals are emitted from the
//! daemon ([`closed`], [`invoked`]) on the same connection.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream as iced_stream;
use iced::widget::image;
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, interface};

use super::{Event, IconSource, Notification, Reason, Timeout, Urgency};
use crate::icons::Icon;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const SPEC_VERSION: &str = "1.3";

/// Images larger than this on a side are dropped (a wallpaper as an
/// `image-data` hint would be uploaded to the GPU at full size).
const IMAGE_MAX: i32 = 1024;

struct Server {
    out: mpsc::Sender<Event>,
    next_id: AtomicU32,
    next_serial: AtomicU64,
}

#[interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<String> {
        // `body-markup`: we take the markup the spec allows and show
        // it as plain text; not advertising it wouldn't stop apps from
        // sending it.
        ["body", "actions", "body-markup", "icon-static"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    #[zbus(out_args("name", "vendor", "version", "spec_version"))]
    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "aria-shell".to_owned(),
            "gurumeditation.it".to_owned(),
            env!("CARGO_PKG_VERSION").to_owned(),
            SPEC_VERSION.to_owned(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id > 0 {
            replaces_id
        } else {
            self.next_id.fetch_add(1, Ordering::Relaxed)
        };
        let n = Notification {
            id,
            serial: self.next_serial.fetch_add(1, Ordering::Relaxed),
            app_name: app_name.to_owned(),
            summary: summary.to_owned(),
            body: plain_text(body),
            icon: hint_string(&hints, "image-path")
                .or_else(|| hint_string(&hints, "image_path"))
                .as_deref()
                .and_then(IconSource::parse)
                .or_else(|| IconSource::parse(app_icon)),
            image: hints
                .get("image-data")
                .or_else(|| hints.get("image_data"))
                .or_else(|| hints.get("icon_data"))
                .and_then(|v| {
                    let image = image_data(v);
                    if image.is_none() {
                        log::debug!(
                            "notification from {app_name:?}: unusable image-data hint ({})",
                            v.value_signature()
                        );
                    }
                    image
                }),
            urgency: match hints.get("urgency").and_then(|v| u8::try_from(v).ok()) {
                Some(0) => Urgency::Low,
                Some(2) => Urgency::Critical,
                _ => Urgency::Normal,
            },
            actions: actions
                .chunks(2)
                .map(|pair| (pair[0].clone(), pair.get(1).cloned().unwrap_or_default()))
                .collect(),
            timeout: Timeout::parse(expire_timeout),
        };
        let _ = self.out.clone().send(Event::Notify(Box::new(n))).await;
        id
    }

    async fn close_notification(&self, id: u32) {
        let _ = self.out.clone().send(Event::Close(id)).await;
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

fn hint_string(hints: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    hints
        .get(key)
        .and_then(|v| String::try_from(v.clone()).ok())
        .filter(|s| !s.is_empty())
}

/// The `image-data` hint, `(iiibiiay)`: width, height, row stride, has
/// alpha, bits per sample, channels, pixels; RGB or RGBA, 8 bits per
/// sample, rows padded to the stride.
fn image_data(value: &OwnedValue) -> Option<Icon> {
    let (width, height, stride, alpha, bps, channels, data) =
        <(i32, i32, i32, bool, i32, i32, Vec<u8>)>::try_from(value.clone()).ok()?;
    if width <= 0 || height <= 0 || width > IMAGE_MAX || height > IMAGE_MAX || bps != 8 {
        return None;
    }
    let channels = match (alpha, channels) {
        (false, 3) => 3,
        (true, 4) => 4,
        _ => return None,
    };
    let (w, h, stride) = (width as usize, height as usize, stride as usize);
    if stride < w * channels || data.len() < stride * (h - 1) + w * channels {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in data.chunks(stride).take(h) {
        for px in row[..w * channels].chunks(channels) {
            rgba.extend_from_slice(&px[..3]);
            rgba.push(if channels == 4 { px[3] } else { 255 });
        }
    }
    Some(Icon::Raster(image::Handle::from_rgba(
        width as u32,
        height as u32,
        rgba,
    )))
}

/// The body as plain text: the spec's markup (`<b>`, `<i>`, `<u>`,
/// `<a href>`, `<img>`) stripped, entities decoded. Apps that don't
/// escape a bare `<` or `&` in plain text keep it.
pub fn plain_text(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(i) = rest.find(['<', '&']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(after) = tail.strip_prefix('<') {
            match tail.find('>') {
                // A tag, if it's one: letters, `/`, attributes.
                Some(end)
                    if tail[1..end]
                        .trim_start_matches('/')
                        .starts_with(|c: char| c.is_ascii_alphabetic()) =>
                {
                    if tail[1..end].starts_with("br") {
                        out.push('\n');
                    }
                    rest = &tail[end + 1..];
                }
                _ => {
                    out.push('<');
                    rest = after;
                }
            }
        } else {
            let entity = tail.find(';').map(|end| &tail[1..end]);
            let decoded = match entity {
                Some("amp") => Some('&'),
                Some("lt") => Some('<'),
                Some("gt") => Some('>'),
                Some("quot") => Some('"'),
                Some("apos") => Some('\''),
                Some(num) if num.starts_with('#') => {
                    let code = num[1..]
                        .strip_prefix('x')
                        .map(|hex| u32::from_str_radix(hex, 16))
                        .unwrap_or_else(|| num[1..].parse());
                    code.ok().and_then(char::from_u32)
                }
                _ => None,
            };
            match (decoded, entity) {
                (Some(c), Some(e)) => {
                    out.push(c);
                    rest = &tail[e.len() + 2..];
                }
                _ => {
                    out.push('&');
                    rest = &tail[1..];
                }
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn events() -> impl Stream<Item = Event> {
    iced_stream::channel(64, async move |mut out: mpsc::Sender<Event>| {
        let conn = match Connection::session().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("notifications: no session bus: {e}");
                return;
            }
        };
        let _ = out.send(Event::Connected(conn.clone())).await;
        if let Err(e) = serve(conn, out).await {
            log::error!("notifications: {e}");
        }
    })
}

/// Ask for the name (without queueing): whether we got it.
async fn own_name(conn: &Connection) -> bool {
    match conn
        .request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into())
        .await
    {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
            log::info!("notifications: serving {NAME}");
            true
        }
        Ok(_) | Err(zbus::Error::NameTaken) => {
            log::warn!("notifications: {NAME} is owned by another daemon, waiting for it to go");
            false
        }
        Err(e) => {
            log::warn!("notifications: cannot request {NAME}: {e}");
            false
        }
    }
}

async fn serve(conn: Connection, out: mpsc::Sender<Event>) -> zbus::Result<()> {
    conn.object_server()
        .at(
            PATH,
            Server {
                out,
                next_id: AtomicU32::new(1),
                next_serial: AtomicU64::new(1),
            },
        )
        .await?;
    let dbus = DBusProxy::new(&conn).await?;
    let mut owner_changed = dbus
        .receive_name_owner_changed_with_args(&[(0, NAME)])
        .await?;
    let unique = conn
        .unique_name()
        .map(|n| n.to_string())
        .unwrap_or_default();
    own_name(&conn).await;
    while let Some(change) = owner_changed.next().await {
        let Ok(args) = change.args() else { continue };
        match args.new_owner().as_ref().map(|n| n.to_string()) {
            None => {
                own_name(&conn).await;
            }
            Some(owner) if owner != unique => {
                log::warn!("notifications: {NAME} taken by {owner}");
            }
            Some(_) => {}
        }
    }
    Ok(())
}

pub async fn closed(conn: Connection, id: u32, reason: Reason) {
    let result = async {
        let emitter = SignalEmitter::new(&conn, PATH)?;
        Server::notification_closed(&emitter, id, reason as u32).await
    }
    .await;
    if let Err(e) = result {
        log::warn!("notifications: NotificationClosed({id}): {e}");
    }
}

pub async fn invoked(conn: Connection, id: u32, key: String) {
    let result = async {
        let emitter = SignalEmitter::new(&conn, PATH)?;
        Server::action_invoked(&emitter, id, &key).await
    }
    .await;
    if let Err(e) = result {
        log::warn!("notifications: ActionInvoked({id}, {key}): {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    #[test]
    fn markup_is_stripped() {
        assert_eq!(plain_text("plain"), "plain");
        assert_eq!(
            plain_text(
                "<b>bold</b> and <i>it</i>, <a href=\"x\">link</a><img src=\"i\" alt=\"a\"/>"
            ),
            "bold and it, link"
        );
        assert_eq!(plain_text("a<br/>b<br>c"), "a\nb\nc");
        assert_eq!(plain_text("1 &lt; 2 &amp;&amp; 3 &gt; 2"), "1 < 2 && 3 > 2");
        assert_eq!(plain_text("&#65;&#x42;&quot;&apos;"), "AB\"'");
        // Not markup: kept.
        assert_eq!(plain_text("a < b & c > d"), "a < b & c > d");
        assert_eq!(plain_text("<3 you &nope; <"), "<3 you &nope; <");
    }

    #[test]
    fn image_data_rgb_and_rgba() {
        // 2x1 RGB with a padded stride.
        let rgb = vec![1u8, 2, 3, 4, 5, 6, 0, 0];
        let v =
            OwnedValue::try_from(Value::from((2i32, 1i32, 8i32, false, 8i32, 3i32, rgb))).unwrap();
        assert!(matches!(image_data(&v), Some(Icon::Raster(_))));
        let rgba = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let v =
            OwnedValue::try_from(Value::from((2i32, 1i32, 8i32, true, 8i32, 4i32, rgba))).unwrap();
        assert!(image_data(&v).is_some());
        // Short data, wrong depth: dropped.
        let v = OwnedValue::try_from(Value::from((
            2i32,
            2i32,
            8i32,
            true,
            8i32,
            4i32,
            vec![0u8; 8],
        )))
        .unwrap();
        assert!(image_data(&v).is_none());
        let v = OwnedValue::try_from(Value::from((
            1i32,
            1i32,
            8i32,
            true,
            16i32,
            4i32,
            vec![0u8; 8],
        )))
        .unwrap();
        assert!(image_data(&v).is_none());
    }
}
