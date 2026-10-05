//! The captures, over a Wayland connection of our own (the runtime's
//! isn't reachable from here): `ext-image-copy-capture-v1` on an
//! `ext-output-image-capture-source-v1` per output, one frame each,
//! copied into a shm buffer we read back. The outputs of a request are
//! captured together and answered together.
//!
//! The clipboard is `ext-data-control-v1` on the same connection: a
//! picture copied is offered as `image/png` until another selection
//! replaces it, its bytes kept here till then.
//!
//! The connection lives in the subscription's future, as idle's does
//! (see `idle/wayland.rs`): its fd polled by tokio next to the daemon's
//! [`Request`]s, which come through the [`Handle`] sent with
//! [`Event::Connected`].

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::FileExt;
use std::sync::Arc;

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream;
use tokio::io::unix::AsyncFd;
use wayland_client::backend::WaylandError;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_output::{Transform, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum, delegate_noop, event_created_child};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_device_v1::{
    self, ExtDataControlDeviceV1,
};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1;
use wayland_protocols::ext::data_control::v1::client::ext_data_control_offer_v1::ExtDataControlOfferV1;
use wayland_protocols::ext::data_control::v1::client::ext_data_control_source_v1::{
    self, ExtDataControlSourceV1,
};
use wayland_protocols::ext::image_capture_source::v1::client::ext_image_capture_source_v1::ExtImageCaptureSourceV1;
use wayland_protocols::ext::image_capture_source::v1::client::ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1;
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_frame_v1::{
    self, ExtImageCopyCaptureFrameV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_manager_v1::{
    ExtImageCopyCaptureManagerV1, Options,
};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_session_v1::{
    self, ExtImageCopyCaptureSessionV1,
};

use super::pixels::{Format, Frames, RawFrame};

/// What the daemon asks of the connection.
#[derive(Debug)]
pub enum Request {
    /// A frame of each output (global names), answered with
    /// [`Event::Captured`] under `job`.
    Capture { job: u64, outputs: Vec<u32> },
    /// Put a PNG on the clipboard.
    Copy(Arc<Vec<u8>>),
}

/// What the compositor lets us do.
#[derive(Debug, Clone, Copy)]
pub struct Support {
    pub capture: bool,
    pub clipboard: bool,
}

#[derive(Debug, Clone)]
pub enum Event {
    Connected(Handle, Support),
    Captured(u64, Result<Frames, String>),
}

/// The daemon's way to the connection; dead once it dropped.
#[derive(Clone)]
pub struct Handle(mpsc::UnboundedSender<Request>);

impl Handle {
    pub fn send(&self, request: Request) {
        if let Err(e) = self.0.unbounded_send(request) {
            log::warn!(
                "screenshot: dropped {:?}, the connection is gone",
                e.into_inner()
            );
        }
    }
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Handle")
    }
}

pub fn events() -> impl Stream<Item = Event> {
    stream::channel(16, async move |mut out: mpsc::Sender<Event>| {
        if let Err(e) = serve(&mut out).await {
            log::error!("screenshot: {e}");
        }
    })
}

#[derive(Default)]
struct State {
    shm: Option<WlShm>,
    sources: Option<ExtOutputImageCaptureSourceManagerV1>,
    copier: Option<ExtImageCopyCaptureManagerV1>,
    seat: Option<WlSeat>,
    clipboard: Option<ExtDataControlManagerV1>,
    device: Option<ExtDataControlDeviceV1>,
    /// What we have on the clipboard, while it's ours.
    copied: Option<(ExtDataControlSourceV1, Arc<Vec<u8>>)>,
    /// Every output, by its global name.
    outputs: Vec<(u32, WlOutput)>,
    /// One per output being captured, by a key of ours.
    captures: HashMap<u64, Capture>,
    next_capture: u64,
    jobs: HashMap<u64, Job>,
    /// Collected while dispatching, sent after.
    events: Vec<Event>,
}

/// A request's outputs: answered once none is left waiting.
#[derive(Default)]
struct Job {
    waiting: usize,
    frames: Vec<RawFrame>,
    error: Option<String>,
}

/// One output's capture: the session tells the buffer it wants, the
/// frame fills it.
struct Capture {
    job: u64,
    output: u32,
    source: ExtImageCaptureSourceV1,
    session: ExtImageCopyCaptureSessionV1,
    size: Option<(u32, u32)>,
    formats: Vec<Format>,
    frame: Option<Frame>,
}

struct Frame {
    frame: ExtImageCopyCaptureFrameV1,
    buffer: WlBuffer,
    file: File,
    size: (u32, u32),
    format: Format,
    transform: Transform,
}

async fn serve(out: &mut mpsc::Sender<Event>) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut state = State::default();
    queue.roundtrip(&mut state)?;
    let capture = state.shm.is_some() && state.sources.is_some() && state.copier.is_some();
    if !capture {
        log::warn!(
            "screenshot: the compositor has no ext-image-copy-capture-v1 for outputs, no screenshots"
        );
    }
    if let (Some(manager), Some(seat)) = (&state.clipboard, &state.seat) {
        state.device = Some(manager.get_data_device(seat, &qh, ()));
    } else {
        log::warn!("screenshot: the compositor has no ext-data-control-v1, nothing copied");
    }
    let support = Support {
        capture,
        clipboard: state.device.is_some(),
    };

    let (tx, mut requests) = mpsc::unbounded();
    out.send(Event::Connected(Handle(tx), support)).await?;
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
                // As in idle/wayland.rs: `WouldBlock` once drained is
                // what clears tokio's readiness.
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
            Request::Capture { job, outputs } => {
                let (Some(sources), Some(copier), true) =
                    (&self.sources, &self.copier, self.shm.is_some())
                else {
                    self.events.push(Event::Captured(
                        job,
                        Err("the compositor has no ext-image-copy-capture-v1".to_owned()),
                    ));
                    return;
                };
                let mut entry = Job::default();
                for name in outputs {
                    let Some((_, output)) = self.outputs.iter().find(|(n, _)| *n == name) else {
                        entry.error = Some(format!("output {name} is gone"));
                        continue;
                    };
                    let key = self.next_capture;
                    self.next_capture += 1;
                    let source = sources.create_source(output, qh, ());
                    let session = copier.create_session(&source, Options::empty(), qh, key);
                    self.captures.insert(
                        key,
                        Capture {
                            job,
                            output: name,
                            source,
                            session,
                            size: None,
                            formats: Vec::new(),
                            frame: None,
                        },
                    );
                    entry.waiting += 1;
                }
                if entry.waiting == 0 {
                    let error = entry.error.unwrap_or_else(|| "no output to capture".to_owned());
                    self.events.push(Event::Captured(job, Err(error)));
                } else {
                    self.jobs.insert(job, entry);
                }
            }
            Request::Copy(png) => {
                let (Some(manager), Some(device)) = (&self.clipboard, &self.device) else {
                    return;
                };
                let source = manager.create_data_source(qh, ());
                source.offer(PNG.to_owned());
                device.set_selection(Some(&source));
                if let Some((old, _)) = self.copied.replace((source, png)) {
                    old.destroy();
                }
            }
        }
    }

    /// The session's constraints are in: a buffer that fits them, and
    /// the frame that fills it.
    fn start_frame(&mut self, key: u64, qh: &QueueHandle<Self>) -> Result<(), String> {
        let shm = self.shm.as_ref().ok_or("no wl_shm")?;
        let capture = self.captures.get_mut(&key).ok_or("unknown capture")?;
        if capture.frame.is_some() {
            return Ok(());
        }
        let (width, height) = capture.size.ok_or("no buffer size")?;
        let format = *capture
            .formats
            .first()
            .ok_or("no 8-bit RGB shm format offered")?;
        let stride = width * 4;
        let len = stride as usize * height as usize;
        let file = memfd(len).map_err(|e| format!("shm buffer: {e}"))?;
        let pool = shm.create_pool(file.as_fd(), len as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            shm_format(format),
            qh,
            (),
        );
        pool.destroy();
        let frame = capture.session.create_frame(qh, key);
        frame.attach_buffer(&buffer);
        frame.damage_buffer(0, 0, width as i32, height as i32);
        frame.capture();
        capture.frame = Some(Frame {
            frame,
            buffer,
            file,
            size: (width, height),
            format,
            transform: Transform::Normal,
        });
        Ok(())
    }

    /// The capture is over, one way or the other: its objects go, its
    /// job hears of it.
    fn finish(&mut self, key: u64, result: Result<RawFrame, String>) {
        let Some(capture) = self.captures.remove(&key) else {
            return;
        };
        if let Some(frame) = capture.frame {
            frame.frame.destroy();
            frame.buffer.destroy();
        }
        capture.session.destroy();
        capture.source.destroy();
        let Some(job) = self.jobs.get_mut(&capture.job) else {
            return;
        };
        match result {
            Ok(frame) => job.frames.push(frame),
            Err(e) => {
                log::warn!("screenshot: output {}: {e}", capture.output);
                job.error.get_or_insert(e);
            }
        }
        job.waiting -= 1;
        if job.waiting == 0 {
            let job_id = capture.job;
            let job = self.jobs.remove(&job_id).expect("just seen");
            let result = match job.error {
                Some(e) => Err(e),
                None => Ok(Arc::new(job.frames)),
            };
            self.events.push(Event::Captured(job_id, result));
        }
    }

    /// The frame is ready: its buffer's bytes.
    fn read(&mut self, key: u64) -> Result<RawFrame, String> {
        let capture = self.captures.get(&key).ok_or("unknown capture")?;
        let frame = capture.frame.as_ref().ok_or("ready before a frame")?;
        let (width, height) = frame.size;
        let mut data = vec![0; width as usize * height as usize * 4];
        frame
            .file
            .read_exact_at(&mut data, 0)
            .map_err(|e| format!("reading the buffer: {e}"))?;
        Ok(RawFrame {
            output: capture.output,
            width,
            height,
            format: frame.format,
            transform: frame.transform,
            data,
        })
    }
}

/// An anonymous file of `len` bytes, to share with the compositor.
fn memfd(len: usize) -> io::Result<File> {
    // SAFETY: a NUL-terminated name and valid flags; the fd is ours.
    let fd = unsafe { libc::memfd_create(c"aria-screenshot".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: just created, owned by nobody else.
    let file = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
    file.set_len(len as u64)?;
    Ok(file)
}

const PNG: &str = "image/png";

fn from_shm(format: wl_shm::Format) -> Option<Format> {
    match format {
        wl_shm::Format::Xrgb8888 => Some(Format::Xrgb),
        wl_shm::Format::Argb8888 => Some(Format::Argb),
        wl_shm::Format::Xbgr8888 => Some(Format::Xbgr),
        wl_shm::Format::Abgr8888 => Some(Format::Abgr),
        _ => None,
    }
}

fn shm_format(format: Format) -> wl_shm::Format {
    match format {
        Format::Xrgb => wl_shm::Format::Xrgb8888,
        Format::Argb => wl_shm::Format::Argb8888,
        Format::Xbgr => wl_shm::Format::Xbgr8888,
        Format::Abgr => wl_shm::Format::Abgr8888,
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
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, 1, qh, ()));
                }
                "ext_data_control_manager_v1" => {
                    state.clipboard = Some(registry.bind(name, 1, qh, ()));
                }
                "ext_output_image_capture_source_manager_v1" => {
                    state.sources = Some(registry.bind(name, 1, qh, ()));
                }
                "ext_image_copy_capture_manager_v1" => {
                    state.copier = Some(registry.bind(name, 1, qh, ()));
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

impl Dispatch<ExtImageCopyCaptureSessionV1, u64> for State {
    fn event(
        state: &mut Self,
        _: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        key: &u64,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_session_v1::Event as E;
        let Some(capture) = state.captures.get_mut(key) else {
            return;
        };
        match event {
            E::BufferSize { width, height } => capture.size = Some((width, height)),
            E::ShmFormat {
                format: WEnum::Value(format),
            } => capture.formats.extend(from_shm(format)),
            E::Done => {
                if let Err(e) = state.start_frame(*key, qh) {
                    state.finish(*key, Err(e));
                }
            }
            E::Stopped => state.finish(*key, Err("the capture session stopped".to_owned())),
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, u64> for State {
    fn event(
        state: &mut Self,
        _: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        key: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_frame_v1::Event as E;
        match event {
            E::Transform {
                transform: WEnum::Value(transform),
            } => {
                if let Some(frame) = state
                    .captures
                    .get_mut(key)
                    .and_then(|c| c.frame.as_mut())
                {
                    frame.transform = transform;
                }
            }
            E::Ready => {
                let result = state.read(*key);
                state.finish(*key, result);
            }
            E::Failed { reason } => {
                state.finish(*key, Err(format!("the capture failed: {reason:?}")));
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, ()> for State {
    fn event(
        state: &mut Self,
        source: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let current = match &state.copied {
            Some((s, png)) if s == source => Some(png.clone()),
            _ => None,
        };
        match event {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => {
                if let (Some(png), PNG) = (current, mime_type.as_str()) {
                    // A pipe: the reader takes its time, and a big
                    // picture doesn't fit in its buffer.
                    std::thread::spawn(move || {
                        if let Err(e) = File::from(fd).write_all(&png) {
                            log::warn!("screenshot: pasting: {e}");
                        }
                    });
                }
            }
            ext_data_control_source_v1::Event::Cancelled => {
                source.destroy();
                if current.is_some() {
                    state.copied = None;
                }
            }
            _ => {}
        }
    }
}

/// Other clients' selections: not ours to read, their offers go at once.
impl Dispatch<ExtDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_data_control_device_v1::Event as E;
        match event {
            E::Selection { id: Some(offer) } | E::PrimarySelection { id: Some(offer) } => {
                offer.destroy();
            }
            E::Finished => {
                log::warn!("screenshot: the clipboard device is gone, nothing copied");
                if let Some(device) = state.device.take() {
                    device.destroy();
                }
            }
            _ => {}
        }
    }

    event_created_child!(State, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

delegate_noop!(State: ignore WlShm);
delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ExtDataControlManagerV1);
delegate_noop!(State: ignore ExtDataControlOfferV1);
delegate_noop!(State: WlShmPool);
delegate_noop!(State: ignore WlBuffer);
delegate_noop!(State: ignore WlOutput);
delegate_noop!(State: ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(State: ExtImageCaptureSourceV1);
delegate_noop!(State: ExtImageCopyCaptureManagerV1);
