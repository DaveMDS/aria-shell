//! Screenshots: the active window, one output, every output, saved as
//! PNG in `[Screenshot] directory`. `aria-shell screenshot window`.
//!
//! One [`Screenshot`] lives in the daemon. The pixels come from a
//! Wayland connection of its own (wayland.rs), one frame per output
//! the picture touches; the picture is a rectangle of the global
//! logical space cut out of them (pixels.rs), at the largest scale of
//! those outputs. A window's rectangle is asked of the compositor at
//! the time ([`Compositor::shown_windows`]).

mod pixels;
mod wayland;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use iced::{Subscription, Task};
use iced_wayland_subscriber::{OutputId, OutputInfo};

use crate::compositor::{Compositor, WindowGeometry};
use crate::config::{Config, RawSection, Section};
use pixels::{Frames, Rect, Shot};
use wayland::{Handle, Request};

/// `[Screenshot]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotConfig {
    /// Where the pictures go, as written (`~` and relative paths are
    /// resolved by [`Screenshot::new`], which has the [`Config`]).
    pub directory: String,
}

impl Section for ScreenshotConfig {
    const NAME: &'static str = "Screenshot";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            directory: raw.str_or("directory", "~/Pictures/Screenshots"),
        }
    }
}

/// What to capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// The window with the keyboard.
    Window,
    /// One output by connector name; the focused one when `None`.
    Output(Option<String>),
    /// Every output.
    All,
}

#[derive(Debug, Clone)]
pub enum Event {
    Wayland(wayland::Event),
    /// The windows on screen, for the job waiting on them.
    Windows(u64, Vec<WindowGeometry>),
    Saved(Result<Saved, String>),
}

#[derive(Debug, Clone)]
pub struct Saved {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

pub struct Screenshot {
    directory: PathBuf,
    handle: Option<Handle>,
    capable: bool,
    next_job: u64,
    /// Captures under way.
    jobs: HashMap<u64, Job>,
    last: Option<Saved>,
}

/// A capture under way.
struct Job {
    /// What to cut out; `None` while the windows are asked.
    rect: Option<Rect>,
    /// The outputs when it started: global name and place.
    outputs: Vec<(u32, Rect)>,
}

impl Screenshot {
    pub fn new(config: &Config) -> Self {
        Self {
            directory: Self::directory(config),
            handle: None,
            capable: false,
            next_job: 0,
            jobs: HashMap::new(),
            last: None,
        }
    }

    fn directory(config: &Config) -> PathBuf {
        let section: ScreenshotConfig = config.section(None);
        config.resolve_path(&section.directory)
    }

    pub fn set_config(&mut self, config: &Config) {
        self.directory = Self::directory(config);
    }

    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::run(wayland::events).map(Event::Wayland)
    }

    pub fn run(
        &mut self,
        command: Command,
        outputs: &BTreeMap<OutputId, OutputInfo>,
        compositor: &Compositor,
    ) -> Task<Event> {
        let placed: Vec<(u32, Rect)> = outputs.values().filter_map(place).collect();
        let rect = match &command {
            Command::Window => None,
            Command::All => {
                let all = placed.iter().map(|(_, r)| *r).reduce(|a, b| a.union(&b));
                if all.is_none() {
                    log::warn!("screenshot: no output to capture");
                    return Task::none();
                }
                all
            }
            Command::Output(name) => {
                let name = name.as_ref().or(compositor.focused_output.as_ref());
                let rect = outputs
                    .values()
                    .find(|o| o.name.is_some() && o.name.as_ref() == name)
                    .and_then(place)
                    .map(|(_, r)| r);
                if rect.is_none() {
                    log::warn!("screenshot: no output {name:?}");
                    return Task::none();
                }
                rect
            }
        };
        let job = self.next_job;
        self.next_job += 1;
        self.jobs.insert(
            job,
            Job {
                rect: None,
                outputs: placed,
            },
        );
        match rect {
            Some(rect) => {
                self.capture(job, rect);
                Task::none()
            }
            None => Task::perform(compositor.shown_windows(), move |windows| {
                Event::Windows(job, windows)
            }),
        }
    }

    /// Ask for the frames of the outputs `rect` touches.
    fn capture(&mut self, job: u64, rect: Rect) {
        let (Some(handle), true) = (&self.handle, self.capable) else {
            log::warn!("screenshot: no way to capture the screen");
            self.jobs.remove(&job);
            return;
        };
        let Some(entry) = self.jobs.get_mut(&job) else {
            return;
        };
        entry.rect = Some(rect);
        let outputs = entry
            .outputs
            .iter()
            .filter(|(_, r)| r.intersection(&rect).is_some())
            .map(|(name, _)| *name)
            .collect();
        log::debug!("screenshot: capturing {rect} from outputs {outputs:?}");
        handle.send(Request::Capture { job, outputs });
    }

    pub fn apply(&mut self, event: Event) -> Task<Event> {
        match event {
            Event::Wayland(wayland::Event::Connected(handle, capable)) => {
                self.handle = Some(handle);
                self.capable = capable;
            }
            Event::Windows(job, windows) => match windows.iter().find(|w| w.active) {
                Some(w) => self.capture(job, Rect::new(w.x, w.y, w.width, w.height)),
                None => {
                    log::warn!("screenshot: no active window");
                    self.jobs.remove(&job);
                }
            },
            Event::Wayland(wayland::Event::Captured(job, result)) => {
                let Some(Job {
                    rect: Some(rect),
                    outputs,
                }) = self.jobs.remove(&job)
                else {
                    return Task::none();
                };
                match result {
                    Ok(frames) => {
                        let directory = self.directory.clone();
                        return Task::perform(save(frames, outputs, rect, directory), Event::Saved);
                    }
                    Err(e) => log::warn!("screenshot: {e}"),
                }
            }
            Event::Saved(Ok(saved)) => {
                log::info!(
                    "screenshot: saved {} ({}x{})",
                    saved.path.display(),
                    saved.width,
                    saved.height
                );
                self.last = Some(saved);
            }
            Event::Saved(Err(e)) => log::warn!("screenshot: {e}"),
        }
        Task::none()
    }

    /// For `aria-shell debug screenshot`.
    pub fn describe(&self) -> String {
        let capture = match (&self.handle, self.capable) {
            (None, _) => "connecting",
            (Some(_), true) => "ext-image-copy-capture-v1",
            (Some(_), false) => "none",
        };
        let last = match &self.last {
            Some(s) => format!("{} {}x{}", s.path.display(), s.width, s.height),
            None => "none".to_owned(),
        };
        format!("capture={capture}; last={last}")
    }
}

/// An output's global name and place, once xdg-output told it.
fn place(info: &OutputInfo) -> Option<(u32, Rect)> {
    let (x, y) = info.logical_position?;
    let (w, h) = info.logical_size?;
    Some((info.id, Rect::new(x, y, w, h)))
}

/// The picture out of the frames, as a new PNG in `directory`; off the
/// runtime's threads, it's seconds of work on big screens.
async fn save(
    frames: Frames,
    outputs: Vec<(u32, Rect)>,
    rect: Rect,
    directory: PathBuf,
) -> Result<Saved, String> {
    tokio::task::spawn_blocking(move || {
        let shots: Vec<Shot> = frames
            .iter()
            .filter_map(|f| {
                let (_, place) = outputs.iter().find(|(name, _)| *name == f.output)?;
                Some(Shot {
                    rect: *place,
                    image: pixels::upright(f),
                })
            })
            .collect();
        let picture = pixels::compose(&shots, rect).ok_or("the picture is on no output")?;
        let png = pixels::encode_png(&picture)?;
        std::fs::create_dir_all(&directory)
            .map_err(|e| format!("cannot create {}: {e}", directory.display()))?;
        let path = free_name(&directory, &chrono::Local::now());
        std::fs::write(&path, png).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(Saved {
            path,
            width: picture.width(),
            height: picture.height(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `Screenshot_2026-10-05_14-03-22.png`, or `..._2.png` and on when
/// taken in the same second.
fn free_name(directory: &Path, now: &chrono::DateTime<chrono::Local>) -> PathBuf {
    let stem = now.format("Screenshot_%Y-%m-%d_%H-%M-%S").to_string();
    let mut path = directory.join(format!("{stem}.png"));
    let mut n = 2;
    while path.exists() {
        path = directory.join(format!("{stem}_{n}.png"));
        n += 1;
    }
    path
}
