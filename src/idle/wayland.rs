//! The idle timers and the outputs' power, over a Wayland connection of
//! our own (the runtime's isn't reachable from here):
//! `ext-idle-notify-v1` says when the seat has been idle for each
//! timeout and when it's active again (honouring the apps' idle
//! inhibitors: a fullscreen video holds every timer),
//! `wlr-output-power-management-v1` turns the outputs off and on.
//!
//! The connection lives in the subscription's future: its fd is polled
//! by tokio next to the daemon's [`Request`]s, which come through the
//! [`Handle`] sent with [`Event::Connected`].

use std::io;
use std::os::fd::AsRawFd;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream;
use tokio::io::unix::AsyncFd;
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use wayland_protocols_wlr::output_power_management::v1::client::zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1;
use wayland_protocols_wlr::output_power_management::v1::client::zwlr_output_power_v1::{
    Mode, ZwlrOutputPowerV1,
};

use super::{Event, Stage};

/// What the daemon asks of the connection.
#[derive(Debug)]
pub enum Request {
    /// Replace the timers: one per stage, from now (none: idle never
    /// fires).
    Timers(Vec<(Stage, Duration)>),
    /// Every output on or off.
    Screens(bool),
}

/// The daemon's way to the connection; dead once it dropped.
#[derive(Clone)]
pub struct Handle(mpsc::UnboundedSender<Request>);

impl Handle {
    pub fn send(&self, request: Request) {
        if let Err(e) = self.0.unbounded_send(request) {
            log::warn!("idle: dropped {:?}, the connection is gone", e.into_inner());
        }
    }
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Handle")
    }
}

/// The event stream: the connection's [`Handle`], then the stages
/// going idle and active again.
pub fn events() -> impl Stream<Item = Event> {
    stream::channel(16, async move |mut out: mpsc::Sender<Event>| {
        if let Err(e) = serve(&mut out).await {
            log::error!("idle: {e}");
        }
    })
}

#[derive(Default)]
struct State {
    seat: Option<WlSeat>,
    notifier: Option<ExtIdleNotifierV1>,
    power: Option<ZwlrOutputPowerManagerV1>,
    /// Every output, by its global name.
    outputs: Vec<(u32, WlOutput)>,
    timers: Vec<ExtIdleNotificationV1>,
    /// Collected while dispatching, sent after.
    events: Vec<Event>,
}

async fn serve(out: &mut mpsc::Sender<Event>) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut state = State::default();
    queue.roundtrip(&mut state)?;
    if state.notifier.is_none() {
        log::warn!("idle: the compositor has no ext-idle-notify-v1, nothing will go idle");
    }
    if state.seat.is_none() {
        log::warn!("idle: no seat, nothing will go idle");
    }
    if state.power.is_none() {
        log::warn!("idle: the compositor has no wlr-output-power-management-v1, screens stay on");
    }

    let (tx, mut requests) = mpsc::unbounded();
    out.send(Event::Connected(Handle(tx))).await?;
    let fd = AsyncFd::new(conn.backend().poll_fd().as_raw_fd())?;
    loop {
        queue.dispatch_pending(&mut state)?;
        for event in std::mem::take(&mut state.events) {
            out.send(event).await?;
        }
        queue.flush()?;
        tokio::select! {
            ready = fd.readable() => {
                let mut ready = ready?;
                // libwayland reads what's there and takes an empty
                // socket as success: say `WouldBlock` ourselves once
                // drained, which is what clears tokio's readiness.
                // Events already queued make `prepare_read` refuse; the
                // loop dispatches them.
                let read = ready.try_io(|fd| {
                    if let Some(guard) = queue.prepare_read() {
                        guard.read().map_err(|e| match e {
                            WaylandError::Io(e) => e,
                            e => io::Error::other(e),
                        })?;
                    }
                    if readable_now(*fd.get_ref()) {
                        Ok(())
                    } else {
                        Err(io::ErrorKind::WouldBlock.into())
                    }
                });
                if let Ok(Err(e)) = read {
                    return Err(e.into());
                }
            }
            request = requests.next() => match request {
                Some(request) => state.apply(request, &qh),
                None => return Ok(()),
            },
        }
    }
}

/// Whether `fd` has data to read right now.
fn readable_now(fd: std::os::fd::RawFd) -> bool {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd, no timeout.
    unsafe { libc::poll(&mut pfd, 1, 0) > 0 }
}

impl State {
    fn apply(&mut self, request: Request, qh: &QueueHandle<Self>) {
        match request {
            Request::Timers(timers) => {
                for timer in self.timers.drain(..) {
                    timer.destroy();
                }
                let (Some(notifier), Some(seat)) = (&self.notifier, &self.seat) else {
                    return;
                };
                for (stage, timeout) in timers {
                    let ms = timeout.as_millis().min(u32::MAX as u128) as u32;
                    self.timers
                        .push(notifier.get_idle_notification(ms, seat, qh, stage));
                }
            }
            Request::Screens(on) => {
                let Some(power) = &self.power else {
                    return;
                };
                let mode = if on { Mode::On } else { Mode::Off };
                // The mode stays once set: the object is only the way
                // to set it.
                for (_, output) in &self.outputs {
                    let control = power.get_output_power(output, qh, ());
                    control.set_mode(mode);
                    control.destroy();
                }
            }
        }
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name, interface, ..
            } => match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, 1, qh, ()));
                }
                "ext_idle_notifier_v1" => {
                    state.notifier = Some(registry.bind(name, 1, qh, ()));
                }
                "zwlr_output_power_manager_v1" => {
                    state.power = Some(registry.bind(name, 1, qh, ()));
                }
                "wl_output" => {
                    state.outputs.push((name, registry.bind(name, 1, qh, ())));
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                state.outputs.retain(|(n, _)| *n != name);
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtIdleNotificationV1, Stage> for State {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        stage: &Stage,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => state.events.push(Event::Idled(*stage)),
            ext_idle_notification_v1::Event::Resumed => state.events.push(Event::Resumed(*stage)),
            _ => {}
        }
    }
}

delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ignore WlOutput);
delegate_noop!(State: ExtIdleNotifierV1);
delegate_noop!(State: ZwlrOutputPowerManagerV1);
delegate_noop!(State: ignore ZwlrOutputPowerV1);
