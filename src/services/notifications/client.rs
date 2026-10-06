//! Sending notifications, as any app does: over the session bus, to
//! whichever daemon serves `org.freedesktop.Notifications` (ours, or
//! another one). For what the shell itself has to say: the battery
//! low, a device that won't mount.

use std::collections::HashMap;

use zbus::zvariant::Value;
use zbus::{Connection, proxy};

#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    fn close_notification(&self, id: u32) -> zbus::Result<()>;
}

/// Send (or replace, with `replaces`) a notification on the session
/// bus; its id.
pub async fn notify(
    replaces: u32,
    icon: String,
    summary: String,
    body: String,
    critical: bool,
) -> zbus::Result<u32> {
    let conn = Connection::session().await?;
    let mut hints = HashMap::new();
    hints.insert("urgency", Value::U8(if critical { 2 } else { 1 }));
    NotificationsProxy::new(&conn)
        .await?
        .notify(
            "Aria Shell",
            replaces,
            &icon,
            &summary,
            &body,
            &[],
            hints,
            -1,
        )
        .await
}

pub async fn close_notification(id: u32) -> zbus::Result<()> {
    let conn = Connection::session().await?;
    NotificationsProxy::new(&conn)
        .await?
        .close_notification(id)
        .await
}
