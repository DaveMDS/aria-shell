//! Screenshots: the active window, one output, every output, or what
//! the picker selects (picker.rs: a window, an output, an area), saved
//! as PNG in `[Screenshot] directory` (with `--edit`, then opened in
//! `[Screenshot] editor`) or, with `--clipboard`, only copied to the
//! clipboard. `aria-shell screenshot [window | output | all]`.
//!
//! One [`Screenshot`] lives in the daemon. The pixels come from a
//! Wayland connection of its own (wayland.rs, the clipboard too), one
//! frame per output the picture touches; the picture is a rectangle of
//! the global logical space cut out of them (pixels.rs), at the largest
//! scale of those outputs. The windows' rectangles are asked of the
//! compositor at the time ([`Compositor::shown_windows`]). The picker
//! shows every output frozen: captured first, its surfaces opened by
//! the daemon after ([`Surfaces`]), the picture cut from those frames.

mod picker;
mod pixels;
mod wayland;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::window::Id;
use iced::{Element, Subscription, Task};
use iced_wayland_subscriber::{OutputId, OutputInfo};

use crate::components::Surfaces;
use crate::config::{Config, RawSection, Section};
use crate::locale::Locale;
use crate::process;
use crate::services::compositor::{Compositor, WindowGeometry};
use crate::theme::Theme;
use picker::Picker;
use pixels::{Frames, RawFrame, Rect, Shot};
use wayland::{Handle, Request, Support};

/// `[Screenshot]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotConfig {
    /// Where the pictures go, as written (`~` and relative paths are
    /// resolved by [`Screenshot::new`], which has the [`Config`]).
    pub directory: String,
    /// The command line that edits a picture, its path in place of
    /// `%f` (last when there's none); `auto`: the first of [`EDITORS`]
    /// on the PATH; `none`/`off`: no editing.
    pub editor: Option<String>,
    /// The gadget's.
    pub icon: String,
}

impl Section for ScreenshotConfig {
    const NAME: &'static str = "Screenshot";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            directory: raw.str_or("directory", "~/Pictures/Screenshots"),
            editor: process::chosen(raw.get("editor"), auto_editor),
            icon: raw.str_or("icon", "applets-screenshooter-symbolic"),
        }
    }
}

/// The editors `editor = auto` tries, in order; each saves over the
/// picture it opened.
pub const EDITORS: &[&str] = &[
    "satty --filename %f --output-filename %f",
    "swappy -f %f -o %f",
    "ksnip -e %f",
    "spectacle --edit-existing %f",
];

/// The first of [`EDITORS`] whose program is on the PATH.
fn auto_editor() -> Option<String> {
    let editor = process::first_installed(EDITORS, process::on_path);
    if editor.is_none() {
        log::info!("screenshot: none of the editors on the PATH, no editing");
    }
    editor.map(str::to_owned)
}

/// What to capture, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub target: Target,
    pub destination: Destination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// What the picker selects.
    Pick,
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
    /// The picker's outputs, upright, ready to show.
    Frozen {
        shots: Shots,
        windows: Vec<WindowGeometry>,
        destination: Destination,
    },
    Picker(picker::Message),
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

/// The outputs' pictures, for the picker.
#[derive(Clone)]
pub struct Shots(Arc<Vec<Shot>>);

impl std::fmt::Debug for Shots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Shots({})", self.0.len())
    }
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
    picker: Option<Picker>,
    last: Option<Picture>,
}

/// A capture under way.
struct Job {
    /// What to cut out; `None` while the windows are asked.
    rect: Option<Rect>,
    /// The outputs when it started: global name and place.
    outputs: Vec<(u32, Rect)>,
    destination: Destination,
    /// For the picker: the frames and the windows, as they come.
    pick: Option<Pick>,
}

#[derive(Default)]
struct Pick {
    frames: Option<Frames>,
    windows: Option<Vec<WindowGeometry>>,
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
            picker: None,
            last: None,
        }
    }

    pub fn set_config(&mut self, config: &Config) {
        self.config = config.section(None);
        self.directory = config.resolve_path(&self.config.directory);
    }

    pub fn subscription(&self) -> Subscription<Event> {
        let wayland = Subscription::run(wayland::events).map(Event::Wayland);
        match self.picker {
            Some(_) => Subscription::batch([wayland, Picker::subscription().map(Event::Picker)]),
            None => wayland,
        }
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
        let picking = self.picker.is_some() || self.jobs.values().any(|j| j.pick.is_some());
        let rect = match &command.target {
            Target::Pick if picking => {
                log::debug!("screenshot: the picker is open already");
                return Task::none();
            }
            Target::Window => None,
            Target::All | Target::Pick => {
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
                pick: (command.target == Target::Pick).then(Pick::default),
            },
        );
        let windows = Task::perform(compositor.shown_windows(), move |windows| {
            Event::Windows(job, windows)
        });
        match (rect, &command.target) {
            (Some(rect), Target::Pick) => {
                self.capture(job, rect);
                windows
            }
            (Some(rect), _) => {
                self.capture(job, rect);
                Task::none()
            }
            (None, _) => windows,
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

    pub fn apply(
        &mut self,
        event: Event,
        outputs: &BTreeMap<OutputId, OutputInfo>,
        compositor: &Compositor,
    ) -> (Task<Event>, Surfaces) {
        let mut surfaces = Surfaces::default();
        match event {
            Event::Wayland(wayland::Event::Connected(handle, support)) => {
                self.handle = Some(handle);
                self.support = support;
            }
            Event::Windows(job, windows) => {
                let Some(entry) = self.jobs.get_mut(&job) else {
                    return (Task::none(), surfaces);
                };
                if let Some(pick) = &mut entry.pick {
                    pick.windows = Some(windows);
                    return (self.freeze(job), surfaces);
                }
                match windows.iter().find(|w| w.active) {
                    Some(w) => self.capture(job, Rect::new(w.x, w.y, w.width, w.height)),
                    None => {
                        log::warn!("screenshot: no active window");
                        self.jobs.remove(&job);
                    }
                }
            }
            Event::Wayland(wayland::Event::Captured(job, result)) => {
                let frames = match result {
                    Ok(frames) => frames,
                    Err(e) => {
                        log::warn!("screenshot: {e}");
                        self.jobs.remove(&job);
                        return (Task::none(), surfaces);
                    }
                };
                if let Some(pick) = self.jobs.get_mut(&job).and_then(|j| j.pick.as_mut()) {
                    pick.frames = Some(frames);
                    return (self.freeze(job), surfaces);
                }
                let Some(Job {
                    rect: Some(rect),
                    outputs,
                    destination,
                    ..
                }) = self.jobs.remove(&job)
                else {
                    return (Task::none(), surfaces);
                };
                let directory = self.directory_for(destination);
                let task = Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            picture(&shots_of(&frames, &outputs), rect, directory)
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    move |result| Event::Taken {
                        destination,
                        result,
                    },
                );
                return (task, surfaces);
            }
            Event::Frozen {
                shots,
                windows,
                destination,
            } => {
                let outputs: Vec<picker::Output> = outputs
                    .values()
                    .map(|info| picker::Output {
                        id: OutputId::from(info),
                        global: info.id,
                        name: info.name.clone().unwrap_or_default(),
                        focused: info.name.is_some() && info.name == compositor.focused_output,
                    })
                    .collect();
                let setup = picker::Setup {
                    shots: shots.0,
                    windows: windows
                        .iter()
                        .map(|w| Rect::new(w.x, w.y, w.width, w.height))
                        .collect(),
                    destination,
                    can_copy: self.support.clipboard,
                    can_edit: self.config.editor.is_some(),
                };
                let (picker, open) = Picker::open(setup, &outputs);
                log::info!("screenshot: picking on {} output(s)", open.len());
                surfaces.open = open;
                self.picker = Some(picker);
            }
            Event::Picker(message) => {
                let Some(picker) = &mut self.picker else {
                    return (Task::none(), surfaces);
                };
                match picker.update(message) {
                    picker::Action::Redraw(ids) => surfaces.redraw = ids,
                    picker::Action::Cancel => {
                        log::info!("screenshot: picking cancelled");
                        surfaces = self.close_picker();
                    }
                    picker::Action::Take { rect, destination } => {
                        let shots = picker.shots.clone();
                        surfaces = self.close_picker();
                        let directory = self.directory_for(destination);
                        let task = Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    picture(&shots, rect, directory)
                                })
                                .await
                                .map_err(|e| e.to_string())?
                            },
                            move |result| Event::Taken {
                                destination,
                                result,
                            },
                        );
                        return (task, surfaces);
                    }
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
        (Task::none(), surfaces)
    }

    fn directory_for(&self, destination: Destination) -> Option<PathBuf> {
        match destination {
            Destination::File { .. } => Some(self.directory.clone()),
            Destination::Clipboard => None,
        }
    }

    /// A pick with its frames and its windows in: turned upright off
    /// the runtime's threads, then shown ([`Event::Frozen`]).
    fn freeze(&mut self, job: u64) -> Task<Event> {
        let ready = self
            .jobs
            .get(&job)
            .and_then(|j| j.pick.as_ref())
            .is_some_and(|p| p.frames.is_some() && p.windows.is_some());
        if !ready {
            return Task::none();
        }
        let Some(Job {
            outputs,
            destination,
            pick:
                Some(Pick {
                    frames: Some(frames),
                    windows: Some(windows),
                }),
            ..
        }) = self.jobs.remove(&job)
        else {
            return Task::none();
        };
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || shots_of(&frames, &outputs))
                    .await
                    .unwrap_or_default()
            },
            move |shots| Event::Frozen {
                shots: Shots(Arc::new(shots)),
                windows,
                destination,
            },
        )
    }

    fn close_picker(&mut self) -> Surfaces {
        Surfaces {
            close: self
                .picker
                .take()
                .map(|p| p.surfaces().iter().map(|s| s.window).collect())
                .unwrap_or_default(),
            ..Surfaces::default()
        }
    }

    /// One of our surfaces went away (its output did): the picker
    /// closes, it was showing every output.
    pub fn surface_closed(&mut self, window: Id) -> Surfaces {
        match &self.picker {
            Some(p) if p.has_window(window) => {
                log::info!("screenshot: a picker surface closed, picking cancelled");
                self.close_picker()
            }
            _ => Surfaces::default(),
        }
    }

    /// The outputs changed under the picker: it closes.
    pub fn outputs_changed(&mut self) -> Surfaces {
        if self.picker.is_some() {
            log::info!("screenshot: the outputs changed, picking cancelled");
        }
        self.close_picker()
    }

    /// The picker's surfaces: window and output.
    pub fn picker_surfaces(&self) -> impl Iterator<Item = (Id, OutputId)> + '_ {
        self.picker
            .iter()
            .flat_map(|p| p.surfaces().iter().map(|s| (s.window, s.output.clone())))
    }

    pub fn view<'a>(
        &'a self,
        window: Id,
        theme: &'a Theme,
        locale: &'a Locale,
    ) -> Option<Element<'a, Event>> {
        let view = self.picker.as_ref()?.view(window, theme, locale)?;
        Some(view.map(Event::Picker))
    }

    /// Open the picture in `[Screenshot] editor`.
    fn edit(&self, path: &Path) {
        let Some(editor) = &self.config.editor else {
            log::warn!(
                "screenshot: no [Screenshot] editor to edit {}",
                path.display()
            );
            return;
        };
        process::run_argv(&process::on_file(editor, path));
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
        let picker = match &self.picker {
            Some(p) => p.describe(),
            None => "picker=closed".to_owned(),
        };
        let editor = self.config.editor.as_deref().unwrap_or("none");
        format!("capture={capture}; clipboard={clipboard}; editor={editor}; {picker}; last={last}")
    }
}

/// An output's global name and place, once xdg-output told it.
fn place(info: &OutputInfo) -> Option<(u32, Rect)> {
    let (x, y) = info.logical_position?;
    let (w, h) = info.logical_size?;
    Some((info.id, Rect::new(x, y, w, h)))
}

/// Each frame upright, placed where its output is.
fn shots_of(frames: &[RawFrame], outputs: &[(u32, Rect)]) -> Vec<Shot> {
    frames
        .iter()
        .filter_map(|f| {
            let (_, place) = outputs.iter().find(|(name, _)| *name == f.output)?;
            Some(Shot {
                output: f.output,
                rect: *place,
                image: pixels::upright(f),
            })
        })
        .collect()
}

/// `rect` out of the shots as a PNG, and a new file in `directory`
/// when there is one. Seconds of work on big screens: run it off the
/// runtime's threads.
fn picture(
    shots: &[Shot],
    rect: Rect,
    directory: Option<PathBuf>,
) -> Result<(Picture, Png), String> {
    let picture = pixels::compose(shots, rect).ok_or("the picture is on no output")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(conf: &str) -> Option<String> {
        Config::parse(conf).section::<ScreenshotConfig>(None).editor
    }

    #[test]
    fn editor_none_off_or_a_command_line() {
        assert_eq!(editor("[Screenshot]\neditor = none\n"), None);
        assert_eq!(editor("[Screenshot]\neditor = off\n"), None);
        assert_eq!(
            editor("[Screenshot]\neditor =  gimp --new  \n").as_deref(),
            Some("gimp --new")
        );
    }

    #[test]
    fn editor_auto_by_default() {
        let auto = auto_editor();
        assert_eq!(editor(""), auto);
        assert_eq!(editor("[Screenshot]\neditor =\n"), auto);
        assert_eq!(editor("[Screenshot]\neditor = auto\n"), auto);
    }

    #[test]
    fn editors_take_the_picture() {
        assert!(EDITORS.iter().all(|line| line.contains("%f")));
    }
}
