//! Screenshots: the active window, one output, every output, saved as
//! PNG in `[Screenshot] directory` (with `--edit`, then opened in
//! `[Screenshot] editor`) or, with `--clipboard`, only copied to the
//! clipboard. `aria-shell screenshot window`.
//!
//! One [`Screenshot`] lives in the daemon. The pixels come from a
//! Wayland connection of its own (wayland.rs, the clipboard too), one
//! frame per output the picture touches; the picture is a rectangle of
//! the global logical space cut out of them (pixels.rs), at the largest
//! scale of those outputs. A window's rectangle is asked of the
//! compositor at the time ([`Compositor::shown_windows`]).

mod pixels;
mod wayland;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::{Subscription, Task};
use iced_wayland_subscriber::{OutputId, OutputInfo};

use crate::compositor::{Compositor, WindowGeometry};
use crate::config::{Config, RawSection, Section};
use crate::process;
use pixels::{Frames, Rect, Shot};
use wayland::{Handle, Request, Support};

/// `[Screenshot]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotConfig {
    /// Where the pictures go, as written (`~` and relative paths are
    /// resolved by [`Screenshot::new`], which has the [`Config`]).
    pub directory: String,
    /// The command line that edits a picture, given its path last.
    pub editor: Option<String>,
}

impl Section for ScreenshotConfig {
    const NAME: &'static str = "Screenshot";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            directory: raw.str_or("directory", "~/Pictures/Screenshots"),
            editor: raw
                .get("editor")
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .map(str::to_owned),
        }
    }
}

/// What to capture, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub target: Target,
    pub destination: Destination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The window with the keyboard.
    Window,
    /// One output by connector name; the focused one when `None`.
    Output(Option<String>),
    /// Every output.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// A new file, opened in the editor after when `edit`.
    File { edit: bool },
    /// The clipboard only, no file.
    Clipboard,
}

#[derive(Debug, Clone)]
pub enum Event {
    Wayland(wayland::Event),
    /// The windows on screen, for the job waiting on them.
    Windows(u64, Vec<WindowGeometry>),
    Taken {
        destination: Destination,
        result: Result<(Picture, Png), String>,
    },
}

#[derive(Debug, Clone)]
pub struct Picture {
    /// The file; `None` for the clipboard.
    pub path: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
}

/// A picture encoded, for the clipboard.
#[derive(Clone)]
pub struct Png(Arc<Vec<u8>>);

impl std::fmt::Debug for Png {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Png({} bytes)", self.0.len())
    }
}

pub struct Screenshot {
    config: ScreenshotConfig,
    directory: PathBuf,
    handle: Option<Handle>,
    support: Support,
    next_job: u64,
    /// Captures under way.
    jobs: HashMap<u64, Job>,
    last: Option<Picture>,
}

/// A capture under way.
struct Job {
    /// What to cut out; `None` while the windows are asked.
    rect: Option<Rect>,
    /// The outputs when it started: global name and place.
    outputs: Vec<(u32, Rect)>,
    destination: Destination,
}

impl Screenshot {
    pub fn new(config: &Config) -> Self {
        let section: ScreenshotConfig = config.section(None);
        Self {
            directory: config.resolve_path(&section.directory),
            config: section,
            handle: None,
            support: Support {
                capture: false,
                clipboard: false,
            },
            next_job: 0,
            jobs: HashMap::new(),
            last: None,
        }
    }

    pub fn set_config(&mut self, config: &Config) {
        self.config = config.section(None);
        self.directory = config.resolve_path(&self.config.directory);
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
        if command.destination == Destination::Clipboard && !self.support.clipboard {
            log::warn!("screenshot: no clipboard to copy to");
            return Task::none();
        }
        let placed: Vec<(u32, Rect)> = outputs.values().filter_map(place).collect();
        let rect = match &command.target {
            Target::Window => None,
            Target::All => {
                let all = placed.iter().map(|(_, r)| *r).reduce(|a, b| a.union(&b));
                if all.is_none() {
                    log::warn!("screenshot: no output to capture");
                    return Task::none();
                }
                all
            }
            Target::Output(name) => {
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
                destination: command.destination,
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
        let (Some(handle), true) = (&self.handle, self.support.capture) else {
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
            Event::Wayland(wayland::Event::Connected(handle, support)) => {
                self.handle = Some(handle);
                self.support = support;
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
                    destination,
                }) = self.jobs.remove(&job)
                else {
                    return Task::none();
                };
                match result {
                    Ok(frames) => {
                        let directory = match destination {
                            Destination::File { .. } => Some(self.directory.clone()),
                            Destination::Clipboard => None,
                        };
                        return Task::perform(
                            develop(frames, outputs, rect, directory),
                            move |result| Event::Taken {
                                destination,
                                result,
                            },
                        );
                    }
                    Err(e) => log::warn!("screenshot: {e}"),
                }
            }
            Event::Taken {
                destination,
                result: Ok((picture, png)),
            } => {
                match (destination, &picture.path) {
                    (Destination::File { edit }, Some(path)) => {
                        log::info!(
                            "screenshot: saved {} ({}x{})",
                            path.display(),
                            picture.width,
                            picture.height
                        );
                        if edit {
                            self.edit(path);
                        }
                    }
                    _ => {
                        log::info!(
                            "screenshot: copied {}x{} to the clipboard",
                            picture.width,
                            picture.height
                        );
                        if let Some(handle) = &self.handle {
                            handle.send(Request::Copy(png.0));
                        }
                    }
                }
                self.last = Some(picture);
            }
            Event::Taken { result: Err(e), .. } => log::warn!("screenshot: {e}"),
        }
        Task::none()
    }

    /// Open the picture in `[Screenshot] editor`.
    fn edit(&self, path: &Path) {
        let Some(editor) = &self.config.editor else {
            log::warn!("screenshot: no [Screenshot] editor to edit {}", path.display());
            return;
        };
        let mut argv = process::split_words(editor);
        argv.push(path.to_string_lossy().into_owned());
        process::run_argv(&argv);
    }

    /// For `aria-shell debug screenshot`.
    pub fn describe(&self) -> String {
        let (capture, clipboard) = match (&self.handle, self.support) {
            (None, _) => ("connecting", "connecting"),
            (Some(_), s) => (
                if s.capture {
                    "ext-image-copy-capture-v1"
                } else {
                    "none"
                },
                if s.clipboard {
                    "ext-data-control-v1"
                } else {
                    "none"
                },
            ),
        };
        let last = match &self.last {
            Some(p) => {
                let place = match &p.path {
                    Some(path) => path.display().to_string(),
                    None => "clipboard".to_owned(),
                };
                format!("{place} {}x{}", p.width, p.height)
            }
            None => "none".to_owned(),
        };
        format!("capture={capture}; clipboard={clipboard}; last={last}")
    }
}

/// An output's global name and place, once xdg-output told it.
fn place(info: &OutputInfo) -> Option<(u32, Rect)> {
    let (x, y) = info.logical_position?;
    let (w, h) = info.logical_size?;
    Some((info.id, Rect::new(x, y, w, h)))
}

/// The picture out of the frames as a PNG, and a new file in
/// `directory` when there is one; off the runtime's threads, it's
/// seconds of work on big screens.
async fn develop(
    frames: Frames,
    outputs: Vec<(u32, Rect)>,
    rect: Rect,
    directory: Option<PathBuf>,
) -> Result<(Picture, Png), String> {
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
        let path = match directory {
            Some(directory) => {
                std::fs::create_dir_all(&directory)
                    .map_err(|e| format!("cannot create {}: {e}", directory.display()))?;
                let path = free_name(&directory, &chrono::Local::now());
                std::fs::write(&path, &png)
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                Some(path)
            }
            None => None,
        };
        let picture = Picture {
            path,
            width: picture.width(),
            height: picture.height(),
        };
        Ok((picture, Png(Arc::new(png))))
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
