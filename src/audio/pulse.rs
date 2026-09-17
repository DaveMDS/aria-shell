//! The mixer over the PulseAudio client API (`libpulse`), which
//! PipeWire serves through `pipewire-pulse`: sinks, sources and the
//! streams playing to a sink, their volume and mute, and the default
//! devices.
//!
//! libpulse is callback-driven and single-threaded behind a lock. A
//! thread of ours owns the threaded mainloop and the context; the
//! callbacks libpulse runs never touch the context themselves, they
//! post a [`Request`] on a channel that thread serves under the
//! mainloop lock (a `RefCell` shared with the callbacks would be
//! borrowed twice the moment `connect` reports its first state). What
//! the introspection answers goes out as [`Event`]s. The daemon sends
//! its commands over the same channel through the [`Handle`] it gets
//! with [`Event::Connected`].

use std::sync::mpsc;
use std::time::Duration;

use iced::futures::channel::mpsc as fmpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream;
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::introspect::{ServerInfo, SinkInfo, SinkInputInfo, SourceInfo};
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet, Operation};
use libpulse_binding::context::{Context, FlagSet, State};
use libpulse_binding::mainloop::threaded::Mainloop;
use libpulse_binding::proplist::{Proplist, properties};
use libpulse_binding::volume::{ChannelVolumes, Volume};

use super::{Channel, Event, Kind};

/// What the serving thread does under the lock.
#[derive(Debug)]
pub enum Request {
    /// The context changed state (from its state callback).
    State,
    /// An object came, changed or went (from the subscribe callback).
    Changed(Option<Facility>, Option<Operation>, u32),
    SetVolume {
        kind: Kind,
        index: u32,
        channels: u8,
        volume: f32,
    },
    SetMute {
        kind: Kind,
        index: u32,
        mute: bool,
    },
    SetDefault {
        kind: Kind,
        name: String,
    },
}

/// The daemon's way to the serving thread; dead once the connection
/// dropped (the next [`Event::Connected`] brings a new one).
#[derive(Clone)]
pub struct Handle(mpsc::Sender<Request>);

impl Handle {
    pub fn send(&self, request: Request) {
        if let Err(e) = self.0.send(request) {
            log::warn!("audio: dropped {:?}, the mixer is gone", e.0);
        }
    }
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Handle")
    }
}

/// The event stream: the connection, the channels, then every change.
/// Reconnects if the server goes away.
pub fn events() -> impl Stream<Item = Event> {
    stream::channel(64, async move |mut out| {
        let (tx, mut rx) = fmpsc::unbounded();
        std::thread::Builder::new()
            .name("aria-audio".into())
            .spawn(move || {
                loop {
                    match session(&tx) {
                        Ok(()) => log::warn!("audio: the mixer connection ended"),
                        Err(e) => log::warn!("audio: mixer: {e}"),
                    }
                    let _ = tx.unbounded_send(Event::Disconnected);
                    std::thread::sleep(Duration::from_secs(3));
                }
            })
            .expect("spawn the audio thread");
        while let Some(event) = rx.next().await {
            let _ = out.send(event).await;
        }
    })
}

/// One connection, served until it fails.
fn session(tx: &fmpsc::UnboundedSender<Event>) -> Result<(), String> {
    let mut mainloop = Mainloop::new().ok_or("cannot create the mainloop")?;
    let mut proplist = Proplist::new().ok_or("cannot create a proplist")?;
    let _ = proplist.set_str(properties::APPLICATION_NAME, "aria-shell");
    let mut ctx = Context::new_with_proplist(&mainloop, "aria-shell", &proplist)
        .ok_or("cannot create the context")?;
    let (req_tx, req_rx) = mpsc::channel::<Request>();

    let req = req_tx.clone();
    ctx.set_state_callback(Some(Box::new(move || {
        let _ = req.send(Request::State);
    })));
    let req = req_tx.clone();
    ctx.set_subscribe_callback(Some(Box::new(move |facility, operation, index| {
        let _ = req.send(Request::Changed(facility, operation, index));
    })));

    mainloop.lock();
    let connected = ctx.connect(None, FlagSet::NOFLAGS, None);
    let started = connected.and_then(|()| mainloop.start());
    mainloop.unlock();
    started.map_err(|e| format!("cannot connect: {e}"))?;

    let mut handle = Some(Handle(req_tx));
    let result = loop {
        let Ok(request) = req_rx.recv() else {
            break Ok(());
        };
        mainloop.lock();
        let outcome = serve(&mut ctx, request, tx, &mut handle);
        mainloop.unlock();
        if let Err(e) = outcome {
            break Err(e);
        }
    };
    mainloop.lock();
    ctx.disconnect();
    mainloop.unlock();
    mainloop.stop();
    result
}

/// One request, under the lock. `handle` is given away on the first
/// `Ready`.
fn serve(
    ctx: &mut Context,
    request: Request,
    tx: &fmpsc::UnboundedSender<Event>,
    handle: &mut Option<Handle>,
) -> Result<(), String> {
    match request {
        Request::State => match ctx.get_state() {
            State::Ready => {
                if let Some(h) = handle.take() {
                    log::info!("audio: mixer connected");
                    let _ = tx.unbounded_send(Event::Connected(h));
                    ctx.subscribe(
                        InterestMaskSet::SINK
                            | InterestMaskSet::SOURCE
                            | InterestMaskSet::SINK_INPUT
                            | InterestMaskSet::SERVER,
                        |ok| {
                            if !ok {
                                log::warn!("audio: cannot subscribe to the mixer's events");
                            }
                        },
                    );
                    fetch_all(ctx, tx);
                }
            }
            State::Failed => return Err("the connection failed".into()),
            State::Terminated => return Err("the server closed the connection".into()),
            _ => {}
        },
        Request::Changed(facility, operation, index) => {
            let kind = match facility {
                Some(Facility::Sink) => Some(Kind::Output),
                Some(Facility::Source) => Some(Kind::Input),
                Some(Facility::SinkInput) => Some(Kind::Stream),
                Some(Facility::Server) => {
                    fetch_defaults(ctx, tx);
                    None
                }
                _ => None,
            };
            if let Some(kind) = kind {
                match operation {
                    Some(Operation::Removed) => {
                        let _ = tx.unbounded_send(Event::ChannelGone(kind, index));
                    }
                    _ => fetch_one(ctx, tx, kind, index),
                }
            }
        }
        Request::SetVolume {
            kind,
            index,
            channels,
            volume,
        } => {
            let mut volumes = ChannelVolumes::default();
            volumes.set(channels.max(1), to_pulse(volume));
            let mut introspect = ctx.introspect();
            match kind {
                Kind::Output => introspect.set_sink_volume_by_index(index, &volumes, None),
                Kind::Input => introspect.set_source_volume_by_index(index, &volumes, None),
                Kind::Stream => introspect.set_sink_input_volume(index, &volumes, None),
            };
        }
        Request::SetMute { kind, index, mute } => {
            let mut introspect = ctx.introspect();
            match kind {
                Kind::Output => introspect.set_sink_mute_by_index(index, mute, None),
                Kind::Input => introspect.set_source_mute_by_index(index, mute, None),
                Kind::Stream => introspect.set_sink_input_mute(index, mute, None),
            };
        }
        Request::SetDefault { kind, name } => {
            match kind {
                Kind::Output => ctx.set_default_sink(&name, |_| {}),
                Kind::Input => ctx.set_default_source(&name, |_| {}),
                Kind::Stream => return Ok(()),
            };
        }
    }
    Ok(())
}

fn fetch_all(ctx: &Context, tx: &fmpsc::UnboundedSender<Event>) {
    let introspect = ctx.introspect();
    let out = tx.clone();
    introspect.get_sink_info_list(move |r| {
        if let ListResult::Item(info) = r {
            let _ = out.unbounded_send(Event::Channel(from_sink(info)));
        }
    });
    let out = tx.clone();
    introspect.get_source_info_list(move |r| {
        if let ListResult::Item(info) = r
            && let Some(channel) = from_source(info)
        {
            let _ = out.unbounded_send(Event::Channel(channel));
        }
    });
    let out = tx.clone();
    introspect.get_sink_input_info_list(move |r| {
        if let ListResult::Item(info) = r {
            let _ = out.unbounded_send(Event::Channel(from_stream(info)));
        }
    });
    fetch_defaults(ctx, tx);
}

fn fetch_one(ctx: &Context, tx: &fmpsc::UnboundedSender<Event>, kind: Kind, index: u32) {
    let introspect = ctx.introspect();
    let out = tx.clone();
    match kind {
        Kind::Output => {
            introspect.get_sink_info_by_index(index, move |r| {
                if let ListResult::Item(info) = r {
                    let _ = out.unbounded_send(Event::Channel(from_sink(info)));
                }
            });
        }
        Kind::Input => {
            introspect.get_source_info_by_index(index, move |r| {
                if let ListResult::Item(info) = r
                    && let Some(channel) = from_source(info)
                {
                    let _ = out.unbounded_send(Event::Channel(channel));
                }
            });
        }
        Kind::Stream => {
            introspect.get_sink_input_info(index, move |r| {
                if let ListResult::Item(info) = r {
                    let _ = out.unbounded_send(Event::Channel(from_stream(info)));
                }
            });
        }
    }
}

fn fetch_defaults(ctx: &Context, tx: &fmpsc::UnboundedSender<Event>) {
    let out = tx.clone();
    ctx.introspect().get_server_info(move |info: &ServerInfo| {
        let _ = out.unbounded_send(Event::Defaults {
            sink: info.default_sink_name.as_deref().unwrap_or("").to_owned(),
            source: info.default_source_name.as_deref().unwrap_or("").to_owned(),
        });
    });
}

/// libpulse volumes are cubic-mapped integers, `NORMAL` being 100%.
fn from_pulse(v: Volume) -> f32 {
    v.0 as f32 / Volume::NORMAL.0 as f32
}

fn to_pulse(v: f32) -> Volume {
    let raw = (v.max(0.0) * Volume::NORMAL.0 as f32).round() as u32;
    Volume(raw.min(Volume::MAX.0))
}

fn from_sink(info: &SinkInfo) -> Channel {
    Channel {
        kind: Kind::Output,
        index: info.index,
        name: info.name.as_deref().unwrap_or("").to_owned(),
        label: info
            .description
            .as_deref()
            .or(info.name.as_deref())
            .unwrap_or("")
            .to_owned(),
        icon: info.proplist.get_str(properties::DEVICE_ICON_NAME),
        app: None,
        volume: from_pulse(info.volume.avg()),
        muted: info.mute,
        channels: info.volume.len(),
        has_volume: true,
        default: false,
    }
}

/// A sink's monitor is a source too, not one to show.
fn from_source(info: &SourceInfo) -> Option<Channel> {
    if info.monitor_of_sink.is_some() {
        return None;
    }
    Some(Channel {
        kind: Kind::Input,
        index: info.index,
        name: info.name.as_deref().unwrap_or("").to_owned(),
        label: info
            .description
            .as_deref()
            .or(info.name.as_deref())
            .unwrap_or("")
            .to_owned(),
        icon: info.proplist.get_str(properties::DEVICE_ICON_NAME),
        app: None,
        volume: from_pulse(info.volume.avg()),
        muted: info.mute,
        channels: info.volume.len(),
        has_volume: true,
        default: false,
    })
}

/// A stream is labelled by its application, captioned by what it plays
/// (`media.name`, the sink input's name).
fn from_stream(info: &SinkInputInfo) -> Channel {
    let app = info.proplist.get_str(properties::APPLICATION_NAME);
    let media = info.name.as_deref().unwrap_or("").to_owned();
    Channel {
        kind: Kind::Stream,
        index: info.index,
        name: media.clone(),
        label: app.clone().unwrap_or(media),
        icon: info.proplist.get_str(properties::APPLICATION_ICON_NAME),
        app,
        volume: from_pulse(info.volume.avg()),
        muted: info.mute,
        channels: info.volume.len(),
        has_volume: info.has_volume && info.volume_writable,
        default: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_roundtrip_and_clamp() {
        assert_eq!(from_pulse(Volume::NORMAL), 1.0);
        assert_eq!(to_pulse(1.0), Volume::NORMAL);
        assert_eq!(to_pulse(0.0), Volume::MUTED);
        assert_eq!(to_pulse(-1.0), Volume::MUTED);
        assert!((from_pulse(to_pulse(0.5)) - 0.5).abs() < 0.001);
        assert_eq!(to_pulse(1e9), Volume::MAX);
    }
}
