//! The system bus side of idle: logind (the session's `Lock` signal,
//! `loginctl lock-session`; `PrepareForSleep` and a delay inhibitor so
//! the screen is locked before the machine sleeps; `Suspend`) and
//! UPower (`OnBattery`). Either may be missing: what's there is used.
//!
//! References: <https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html>,
//! <https://upower.freedesktop.org/docs/UPower.html>

use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex};

use iced::futures::channel::mpsc;
use iced::futures::stream::{BoxStream, select_all};
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream;
use zbus::{Connection, proxy};

use super::Event;

#[proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Manager {
    fn get_session(&self, session_id: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    fn inhibit(
        &self,
        what: &str,
        who: &str,
        why: &str,
        mode: &str,
    ) -> zbus::Result<zbus::zvariant::OwnedFd>;

    fn suspend(&self, interactive: bool) -> zbus::Result<()>;

    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
trait Session {
    #[zbus(signal)]
    fn lock(&self) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.UPower",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower"
)]
trait UPower {
    #[zbus(property)]
    fn on_battery(&self) -> zbus::Result<bool>;
}

/// logind's delay inhibitor, held while the machine is about to sleep:
/// it waits until [`SleepLock::release`] (the screen locked) or
/// logind's `InhibitDelayMaxSec`.
#[derive(Debug, Clone)]
pub struct SleepLock(Arc<Mutex<Option<OwnedFd>>>);

impl SleepLock {
    pub fn release(&self) {
        if let Ok(mut fd) = self.0.lock()
            && fd.take().is_some()
        {
            log::debug!("idle: sleep inhibitor released");
        }
    }
}

enum Signal {
    Sleep(bool),
    Lock,
    OnBattery(bool),
}

/// The event stream: the bus, the power source now, then the session
/// lock requests, the sleeps and the power source changing. Holds a
/// sleep inhibitor between sleeps when `lock_before_sleep`.
pub fn events(lock_before_sleep: bool) -> impl Stream<Item = Event> {
    stream::channel(16, async move |mut out: mpsc::Sender<Event>| {
        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("idle: no system bus: {e}");
                return;
            }
        };
        let _ = out.send(Event::Bus(conn.clone())).await;
        follow(conn, lock_before_sleep, out).await;
    })
}

async fn follow(conn: Connection, lock_before_sleep: bool, mut out: mpsc::Sender<Event>) {
    let mut signals: Vec<BoxStream<'static, Signal>> = Vec::new();

    let manager = ManagerProxy::new(&conn).await;
    match &manager {
        Ok(manager) => match manager.receive_prepare_for_sleep().await {
            Ok(s) => signals.push(
                s.filter_map(|s| async move { s.args().ok().map(|a| Signal::Sleep(a.start)) })
                    .boxed(),
            ),
            Err(e) => log::warn!("idle: no PrepareForSleep from logind: {e}"),
        },
        Err(e) => log::warn!("idle: no logind: {e}"),
    }
    if let Ok(manager) = &manager {
        match session(&conn, manager).await {
            Ok(session) => match session.receive_lock().await {
                Ok(s) => signals.push(s.map(|_| Signal::Lock).boxed()),
                Err(e) => log::warn!("idle: no Lock from the logind session: {e}"),
            },
            Err(e) => log::warn!("idle: no logind session: {e}"),
        }
    }

    match UPowerProxy::new(&conn).await {
        Ok(upower) => {
            match upower.on_battery().await {
                Ok(on) => {
                    let _ = out.send(Event::OnBattery(on)).await;
                }
                Err(e) => log::info!("idle: no UPower ({e}), taken as on AC"),
            }
            signals.push(
                upower
                    .receive_on_battery_changed()
                    .await
                    .filter_map(|c| async move { c.get().await.ok().map(Signal::OnBattery) })
                    .boxed(),
            );
        }
        Err(e) => log::info!("idle: no UPower ({e}), taken as on AC"),
    }

    let inhibit = async || match &manager {
        Ok(manager) if lock_before_sleep => manager
            .inhibit(
                "sleep",
                "Aria Shell",
                "Lock the screen before sleeping",
                "delay",
            )
            .await
            .inspect_err(|e| log::warn!("idle: no sleep inhibitor: {e}"))
            .ok()
            .map(OwnedFd::from),
        _ => None,
    };
    let mut inhibitor = inhibit().await;

    let mut signals = select_all(signals);
    while let Some(signal) = signals.next().await {
        let event = match signal {
            Signal::Sleep(true) => {
                log::info!("idle: the machine is going to sleep");
                Event::Sleeping(SleepLock(Arc::new(Mutex::new(inhibitor.take()))))
            }
            Signal::Sleep(false) => {
                log::info!("idle: the machine woke up");
                inhibitor = inhibit().await;
                continue;
            }
            Signal::Lock => Event::LockRequested,
            Signal::OnBattery(on) => Event::OnBattery(on),
        };
        if out.send(event).await.is_err() {
            return;
        }
    }
}

/// Our logind session: `auto` is the caller's, or the user's display
/// session when the caller has none (a shell started by a user unit).
async fn session(conn: &Connection, manager: &ManagerProxy<'_>) -> zbus::Result<SessionProxy<'static>> {
    let path = manager.get_session("auto").await?;
    SessionProxy::builder(conn).path(path)?.build().await
}

/// Ask logind to suspend the machine.
pub async fn suspend(conn: Connection) -> zbus::Result<()> {
    ManagerProxy::new(&conn).await?.suspend(false).await
}
