//! The brightness worker: finds the screens (again when outputs come
//! or go), reads and writes their levels, one task per screen so the
//! writes to one are in order and only the latest of a burst is made
//! (a monitor takes ~100 ms per write; a slider sends dozens).

use std::collections::HashMap;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream;
use zbus::Connection;

use super::{Display, Event, Handle, Kind, backlight, ddc};

/// Outputs come in bursts (a dock, the start): look once they settle,
/// and give a monitor just plugged in a moment to answer.
const DETECT_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub enum Request {
    /// Look for the screens again.
    Detect,
    /// Read the monitors again.
    Read,
    /// A raw level for a screen.
    Write(String, u32),
    /// The kernel says the backlight changed.
    Changed(String),
}

/// What a screen's task does.
enum Op {
    Read,
    Write(u32),
}

pub fn events(backlight: bool) -> impl Stream<Item = Event> {
    stream::channel(16, async move |mut out: mpsc::Sender<Event>| {
        let ddc = ddc::available();
        if !ddc {
            log::warn!("brightness: no ddcutil on the PATH, external monitors can't be set");
        }
        let (tx, mut rx) = mpsc::unbounded();
        let _ = out.send(Event::Ready(Handle(tx.clone()))).await;
        let mut worker = Worker {
            backlight,
            ddc,
            out,
            tx,
            screens: HashMap::new(),
            watched: false,
        };
        worker.detect().await;
        while let Some(request) = rx.next().await {
            match request {
                Request::Detect => {
                    tokio::time::sleep(DETECT_DELAY).await;
                    // The rest of the burst; anything else meanwhile
                    // still goes.
                    let mut later = Vec::new();
                    while let Ok(r) = rx.try_recv() {
                        if !matches!(r, Request::Detect) {
                            later.push(r);
                        }
                    }
                    worker.detect().await;
                    for r in later {
                        worker.handle(r).await;
                    }
                }
                other => worker.handle(other).await,
            }
        }
    })
}

struct Worker {
    backlight: bool,
    /// `ddcutil` is there.
    ddc: bool,
    out: mpsc::Sender<Event>,
    tx: mpsc::UnboundedSender<Request>,
    /// One task per screen, by id.
    screens: HashMap<String, (Kind, mpsc::UnboundedSender<Op>)>,
    watched: bool,
}

impl Worker {
    async fn detect(&mut self) {
        let mut displays = Vec::new();
        if self.backlight
            && let Ok(Some(d)) = tokio::task::spawn_blocking(backlight::find).await
        {
            displays.push(d);
        }
        if self.ddc {
            displays.extend(ddc::detect().await);
        }
        log::debug!("brightness: {} screen(s)", displays.len());
        self.screens
            .retain(|id, _| displays.iter().any(|d| d.id == *id));
        for d in &displays {
            if !self.screens.contains_key(&d.id) {
                let ops = self.spawn(d);
                self.screens.insert(d.id.clone(), (d.kind, ops));
            }
        }
        if !self.watched
            && let Some(name) = displays
                .iter()
                .find(|d| d.kind == Kind::Backlight)
                .and_then(|d| backlight::name(&d.id))
        {
            self.watched = true;
            let (name, id, tx) = (name.to_owned(), backlight::id(name), self.tx.clone());
            std::thread::spawn(move || {
                backlight::watch(&name, || {
                    tx.unbounded_send(Request::Changed(id.clone())).is_ok()
                })
            });
        }
        // The list first, then the levels into it.
        let _ = self.out.send(Event::Displays(displays)).await;
        self.read_monitors();
    }

    async fn handle(&mut self, request: Request) {
        match request {
            Request::Detect => {}
            Request::Read => self.read_monitors(),
            Request::Write(id, value) => match self.screens.get(&id) {
                Some((_, ops)) => {
                    let _ = ops.unbounded_send(Op::Write(value));
                }
                None => log::debug!("brightness: {id} is gone"),
            },
            Request::Changed(id) => {
                let level = backlight::name(&id).and_then(backlight::read);
                if let Some(level) = level {
                    let _ = self.out.send(Event::Level(id, level)).await;
                }
            }
        }
    }

    fn read_monitors(&self) {
        for (kind, ops) in self.screens.values() {
            if *kind == Kind::Ddc {
                let _ = ops.unbounded_send(Op::Read);
            }
        }
    }

    /// The task of one screen, until the worker forgets it.
    fn spawn(&self, display: &Display) -> mpsc::UnboundedSender<Op> {
        let (tx, mut rx) = mpsc::unbounded::<Op>();
        let id = display.id.clone();
        let kind = display.kind;
        let mut out = self.out.clone();
        tokio::spawn(async move {
            let mut bus: Option<Connection> = None;
            while let Some(op) = rx.next().await {
                // What piled up meanwhile: the last write, any read.
                let (mut write, mut read) = (None, false);
                let mut take = |op: Op| match op {
                    Op::Read => read = true,
                    Op::Write(v) => write = Some(v),
                };
                take(op);
                while let Ok(op) = rx.try_recv() {
                    take(op);
                }
                if let Some(value) = write {
                    let result = match kind {
                        Kind::Ddc => match ddc::bus(&id) {
                            Some(b) => ddc::set(b, value).await,
                            None => Err("no bus".to_owned()),
                        },
                        Kind::Backlight => set_backlight(&mut bus, &id, value).await,
                    };
                    if let Err(e) = result {
                        log::warn!("brightness: can't set {id}: {e}");
                        let level = read_level(kind, &id).await;
                        let _ = out.send(Event::Failed(id.clone(), level)).await;
                        continue;
                    }
                    let _ = out.send(Event::Written(id.clone(), value)).await;
                }
                if read && let Some(level) = read_level(kind, &id).await {
                    let _ = out.send(Event::Level(id.clone(), level)).await;
                }
            }
        });
        tx
    }
}

async fn read_level(kind: Kind, id: &str) -> Option<super::Level> {
    match kind {
        Kind::Ddc => ddc::get(ddc::bus(id)?).await,
        Kind::Backlight => backlight::read(backlight::name(id)?),
    }
}

/// Through logind, on the system bus (opened at the first write).
async fn set_backlight(bus: &mut Option<Connection>, id: &str, value: u32) -> Result<(), String> {
    let name = backlight::name(id).ok_or("not a backlight")?;
    if bus.is_none() {
        *bus = Some(Connection::system().await.map_err(|e| e.to_string())?);
    }
    let conn = bus.as_ref().ok_or("no system bus")?;
    backlight::set(conn, name, value)
        .await
        .map_err(|e| e.to_string())
}
