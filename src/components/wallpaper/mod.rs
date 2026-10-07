//! The desktop background: one layer surface per output on the
//! `background` layer showing an image, sized by CSS `object-fit`
//! words. `[wallpaper]` for every output, `[wallpaper:<connector>]`
//! for one. The source is a file, a folder (its images in turn, every
//! `interval`) or `auto` (the `backgrounds` folder of the XDG data
//! dirs: the user's, else the system's); each section is a [`Show`],
//! shared by the outputs it covers.
//!
//! [`Wallpapers`] knows the outputs and keeps a surface on each one
//! whose show has an image ([`Surfaces`], which the daemon carries
//! out). Images are decoded off-thread once per path and shared by the
//! outputs showing the same file; only those on screen or about to be
//! are kept. The folders are watched: an image coming or going is
//! picked up, a surface opens when the first one comes.
//!
//! Still images only (what the `image` crate decodes: png, jpeg,
//! webp); the Python version also played gifs, videos and shadertoy
//! shaders.

mod show;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use iced::advanced::widget::{self, Widget};
use iced::advanced::{self, Layout, layout, mouse, renderer};
use iced::futures::{Stream, stream};
use iced::widget::{Space, image};
use iced::window::Id;
use iced::{ContentFit, Element, Length, Rectangle, Size, Subscription, Task};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};

use crate::config::{Config, RawSection, Section};
use crate::services::idle::parse_duration;
use crate::ui::Surfaces;
use crate::ui::theme::{Node, Theme};
use show::Show;

/// `[wallpaper]` / `[wallpaper:<connector>]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallpaperConfig {
    pub source: Source,
    pub fit: Fit,
    /// Time each image of a folder stays; `None`: the first one stays.
    pub interval: Option<Duration>,
    pub order: Order,
}

/// What `source` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// `auto` (or empty): `$XDG_DATA_HOME/backgrounds`, else the first
    /// `$XDG_DATA_DIRS/backgrounds` with an image.
    Auto,
    /// `none`: no wallpaper.
    None,
    /// An image, or a folder of them; resolved against the config
    /// file's directory (or `~`) by [`WallpaperConfig::for_output`].
    Path(PathBuf),
}

/// The order of a folder's images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// By path.
    #[default]
    Name,
    /// Shuffled, each one shown once before any comes again.
    Random,
}

/// How the image fits the output: CSS `object-fit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// Fills the output, proportions kept, edges cropped.
    #[default]
    Cover,
    /// The whole image, proportions kept, bands around.
    Contain,
    /// Stretched to the output.
    Fill,
    /// 1:1, centred.
    None,
    /// `None`, shrunk when it doesn't fit.
    ScaleDown,
}

impl Fit {
    pub fn content_fit(self) -> ContentFit {
        match self {
            Self::Cover => ContentFit::Cover,
            Self::Contain => ContentFit::Contain,
            Self::Fill => ContentFit::Fill,
            Self::None => ContentFit::None,
            Self::ScaleDown => ContentFit::ScaleDown,
        }
    }
}

impl Section for WallpaperConfig {
    const NAME: &'static str = "wallpaper";

    /// A path `source` comes back as written: [`WallpaperConfig::for_output`]
    /// resolves it, it has the [`Config`].
    fn from_raw(raw: &RawSection) -> Self {
        let fit = match raw.get("fit") {
            None => Fit::default(),
            Some("cover") => Fit::Cover,
            Some("contain") => Fit::Contain,
            Some("fill") => Fit::Fill,
            Some("none") => Fit::None,
            Some("scale-down") => Fit::ScaleDown,
            Some(other) => {
                log::warn!(
                    "invalid wallpaper fit {other:?} (cover | contain | fill | none | scale-down), using cover"
                );
                Fit::default()
            }
        };
        let source = match raw.get("source") {
            None | Some("auto") => Source::Auto,
            Some("none") => Source::None,
            Some(path) => Source::Path(PathBuf::from(path)),
        };
        let interval = raw.get("interval").and_then(|v| {
            parse_duration(v).unwrap_or_else(|| {
                log::warn!("invalid wallpaper interval {v:?} (30s, 10m, 1h; 0 = never), never");
                None
            })
        });
        let order = match raw.get("order") {
            None | Some("name") => Order::Name,
            Some("random") => Order::Random,
            Some(other) => {
                log::warn!("invalid wallpaper order {other:?} (name | random), using name");
                Order::Name
            }
        };
        Self {
            source,
            fit,
            interval,
            order,
        }
    }
}

impl WallpaperConfig {
    /// The section for `output` and what it says: `[wallpaper:<output>]`
    /// when it names a source, else `[wallpaper]`.
    pub fn for_output(config: &Config, output: &str) -> (String, Self) {
        let specific = format!("{}:{output}", Self::NAME);
        let name = if config.raw_section(&specific).get("source").is_some() {
            specific
        } else {
            Self::NAME.to_owned()
        };
        let mut section: Self = config.section(Some(&name));
        if let Source::Path(path) = &section.source {
            section.source = Source::Path(config.resolve_path(&path.to_string_lossy()));
        }
        (name, section)
    }
}

/// The outputs, their shows, their surfaces and the decoded images.
#[derive(Default)]
pub struct Wallpapers {
    /// Every output known, with the section it shows (`None`: `source =
    /// none`, or the compositor closed its surface).
    outputs: BTreeMap<OutputId, Output>,
    /// One per section some output shows.
    shows: BTreeMap<String, Show>,
    /// One surface per output whose show has an image.
    surfaces: BTreeMap<Id, OutputId>,
    /// One per file on screen or about to be, for however many outputs
    /// show it.
    images: HashMap<PathBuf, Picture>,
}

struct Output {
    info: OutputInfo,
    show: Option<String>,
}

/// A file's decoding.
enum Picture {
    Loading,
    /// When it was last modified, to see it change.
    Ready(image::Handle, Option<SystemTime>),
    /// Logged; tried again when the file changes.
    Failed(Option<SystemTime>),
}

#[derive(Debug, Clone)]
pub enum Event {
    Loaded {
        path: PathBuf,
        handle: Option<image::Handle>,
        modified: Option<SystemTime>,
    },
    /// Section `.0`'s interval is up.
    Tick(String),
}

impl Wallpapers {
    /// A new output: its wallpaper, if its section has an image.
    pub fn add_output(&mut self, config: &Config, output: &OutputInfo) -> (Surfaces, Task<Event>) {
        let id = OutputId::from(output);
        if self.outputs.contains_key(&id) {
            return (Surfaces::default(), Task::none());
        }
        self.outputs.insert(
            id,
            Output {
                info: output.clone(),
                show: None,
            },
        );
        self.set_config(config)
    }

    /// Output `output` went away: its surface goes.
    pub fn output_removed(&mut self, output: OutputId) -> Surfaces {
        self.outputs.remove(&output);
        let mut surfaces = Surfaces::default();
        self.surfaces.retain(|&id, o| {
            let keep = *o != output;
            if !keep {
                surfaces.close.push(id);
            }
            keep
        });
        self.shows
            .retain(|name, _| self.outputs.values().any(|o| o.show.as_ref() == Some(name)));
        self.evict();
        surfaces
    }

    /// Apply the config: each output's section again; a show whose
    /// section didn't change goes on where it was.
    pub fn set_config(&mut self, config: &Config) -> (Surfaces, Task<Event>) {
        let mut shows = BTreeMap::new();
        for output in self.outputs.values_mut() {
            let name = output.info.name.clone().unwrap_or_default();
            let (section, wanted) = WallpaperConfig::for_output(config, &name);
            if wanted.source == Source::None {
                output.show = None;
                continue;
            }
            output.show = Some(section.clone());
            if shows.contains_key(&section) {
                continue;
            }
            let show = match self.shows.remove(&section) {
                Some(show) if show.config == wanted => show,
                _ => {
                    let show = Show::new(wanted);
                    match &show.root {
                        Some(root) => log::info!(
                            "wallpaper [{section}]: {} image(s) from {}",
                            show.files.len(),
                            root.display()
                        ),
                        None => log::info!("wallpaper [{section}]: no image"),
                    }
                    show
                }
            };
            shows.insert(section, show);
        }
        self.shows = shows;
        self.sync()
    }

    /// Something under `paths` changed: the shows watching it look at
    /// their source again, the images whose file changed are decoded
    /// again. `None` when none of it is ours.
    pub fn changed(&mut self, paths: &[PathBuf]) -> Option<(Surfaces, Task<Event>)> {
        let mut ours = false;
        for (name, show) in &mut self.shows {
            if show.watch.iter().any(|w| paths.contains(w)) {
                ours = true;
                let before = show.files.len();
                show.rescan();
                if show.files.len() != before {
                    log::info!("wallpaper [{name}]: {} image(s) now", show.files.len());
                }
            }
        }
        let stale: Vec<PathBuf> = self
            .images
            .iter()
            .filter(|(path, picture)| {
                let then = match picture {
                    Picture::Loading => return false,
                    Picture::Ready(_, t) | Picture::Failed(t) => *t,
                };
                modified(path).is_some_and(|now| Some(now) != then)
            })
            .map(|(path, _)| path.clone())
            .collect();
        if !ours && stale.is_empty() {
            return None;
        }
        let reload = stale.into_iter().map(|path| {
            log::info!("wallpaper {} changed, reloading", path.display());
            self.reload(path)
        });
        let reload: Vec<Task<Event>> = reload.collect();
        let (surfaces, task) = self.sync();
        Some((surfaces, Task::batch(reload.into_iter().chain([task]))))
    }

    /// The next image, on every output (`aria-shell wallpaper next`);
    /// the intervals start over.
    pub fn next(&mut self) -> (Surfaces, Task<Event>) {
        for show in self.shows.values_mut() {
            show.next();
            show.generation += 1;
        }
        self.sync()
    }

    pub fn apply(&mut self, event: Event) -> (Surfaces, Task<Event>) {
        match event {
            Event::Loaded {
                path,
                handle,
                modified,
            } => {
                // Not wanted any more (an interval, a rescan, while it
                // was decoding).
                if !self.images.contains_key(&path) {
                    return (Surfaces::default(), Task::none());
                }
                let picture = match handle {
                    Some(handle) => Picture::Ready(handle, modified),
                    None => Picture::Failed(modified),
                };
                self.images.insert(path, picture);
            }
            Event::Tick(name) => {
                if let Some(show) = self.shows.get_mut(&name) {
                    show.next();
                }
            }
        }
        self.sync()
    }

    /// Bring everything in line with the shows: past the images that
    /// failed, the wanted ones decoded (and shown once they are), a
    /// surface on each output with an image and none elsewhere, only
    /// the images in use kept.
    fn sync(&mut self) -> (Surfaces, Task<Event>) {
        let mut loads = Vec::new();
        for show in self.shows.values_mut() {
            show.skip(|p| matches!(self.images.get(p), Some(Picture::Failed(_))));
            let Some(wanted) = show.wanted().map(Path::to_path_buf) else {
                show.shown = None;
                continue;
            };
            match self.images.get(&wanted) {
                Some(Picture::Ready(..)) => show.shown = Some(wanted),
                Some(_) => {}
                None => {
                    self.images.insert(wanted.clone(), Picture::Loading);
                    loads.push(wanted);
                }
            }
        }
        let tasks: Vec<Task<Event>> = loads.into_iter().map(load).collect();

        let mut surfaces = Surfaces::default();
        let has_image = |output: &Output| {
            output
                .show
                .as_ref()
                .and_then(|name| self.shows.get(name))
                .is_some_and(|show| !show.files.is_empty())
        };
        self.surfaces.retain(|&id, output| {
            let keep = self.outputs.get(output).is_some_and(has_image);
            if !keep {
                surfaces.close.push(id);
            }
            keep
        });
        for (&id, output) in &self.outputs {
            if has_image(output) && !self.surfaces.values().any(|o| *o == id) {
                let window = Id::unique();
                surfaces.open.push((window, layer_settings(&output.info)));
                self.surfaces.insert(window, id);
            }
        }
        self.evict();
        (surfaces, Task::batch(tasks))
    }

    /// Drop the images no show wants or shows.
    fn evict(&mut self) {
        let used: HashSet<&Path> = self
            .shows
            .values()
            .flat_map(|s| [s.wanted(), s.shown.as_deref()])
            .flatten()
            .collect();
        self.images.retain(|path, _| used.contains(path.as_path()));
    }

    /// Decode `path` again (it changed), keeping what's on screen until
    /// it's done.
    fn reload(&mut self, path: PathBuf) -> Task<Event> {
        self.images.entry(path.clone()).or_insert(Picture::Loading);
        load(path)
    }

    /// Surface `window` was closed: whether it was one of ours. Not
    /// opened again on that output until the config is.
    pub fn closed(&mut self, window: Id) -> bool {
        let Some(output) = self.surfaces.remove(&window) else {
            return false;
        };
        if let Some(o) = self.outputs.get_mut(&output) {
            o.show = None;
        }
        true
    }

    /// The open surfaces, with their output.
    pub fn windows(&self) -> impl Iterator<Item = (Id, OutputId)> + '_ {
        self.surfaces.iter().map(|(&id, &o)| (id, o))
    }

    pub fn output_of(&self, window: Id) -> Option<OutputId> {
        self.surfaces.get(&window).copied()
    }

    fn show_of(&self, output: OutputId) -> Option<&Show> {
        let name = self.outputs.get(&output)?.show.as_ref()?;
        self.shows.get(name)
    }

    /// Surface `window`'s picture, on output `output` (its name).
    pub fn view<'a, M: 'a>(&'a self, window: Id, theme: &'a Theme, output: &str) -> Element<'a, M> {
        let Some(show) = self.output_of(window).and_then(|o| self.show_of(o)) else {
            return Space::new().into();
        };
        let root = Node::root("wallpaper").attr("output", output.to_owned());
        let image = show
            .shown
            .as_deref()
            .and_then(|p| self.images.get(p))
            .and_then(|picture| match picture {
                Picture::Ready(handle, _) => Some(handle),
                _ => None,
            });
        let picture: Element<'a, M> = match image {
            Some(handle) => Element::new(Uploaded {
                handle: handle.clone(),
                image: image::Image::new(handle.clone())
                    .content_fit(show.config.fit.content_fit())
                    .width(Length::Fill)
                    .height(Length::Fill),
            }),
            None => Space::new().into(),
        };
        theme
            .container(&root, picture)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// What to watch: the folders and files of the sources.
    pub fn watched(&self) -> impl Iterator<Item = &PathBuf> {
        self.shows.values().flat_map(|s| s.watch.iter())
    }

    /// A timer per show with images to rotate.
    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::batch(self.shows.iter().filter(|(_, s)| s.rotates()).filter_map(
            |(name, show)| {
                let every = show.config.interval?;
                Some(Subscription::run_with(
                    (name.clone(), every, show.generation),
                    |(name, every, _)| ticks(name.clone(), *every),
                ))
            },
        ))
    }

    /// `aria-shell debug wallpaper`: each show, and where it is.
    pub fn describe(&self) -> String {
        if self.shows.is_empty() {
            return "none".to_owned();
        }
        self.shows
            .iter()
            .map(|(name, show)| {
                let outputs: Vec<&str> = self
                    .outputs
                    .values()
                    .filter(|o| o.show.as_ref() == Some(name))
                    .filter_map(|o| o.info.name.as_deref())
                    .collect();
                let source = match &show.config.source {
                    Source::Auto => "auto".to_owned(),
                    Source::None => "none".to_owned(),
                    Source::Path(p) => p.display().to_string(),
                };
                let position = show
                    .wanted()
                    .and_then(|w| show.files.iter().position(|f| f == w))
                    .map_or("-".to_owned(), |i| format!("{}", i + 1));
                let order = match show.config.order {
                    Order::Name => "name",
                    Order::Random => "random",
                };
                format!(
                    "[{name}] outputs={} source={source} root={} images={} wanted={} {} shown={} interval={} order={order}",
                    outputs.join(","),
                    show.root.as_ref().map_or("-".to_owned(), |r| r.display().to_string()),
                    show.files.len(),
                    position,
                    show.wanted().map_or("-".to_owned(), |p| p.display().to_string()),
                    show.shown.as_ref().map_or("-".to_owned(), |p| p.display().to_string()),
                    show.config.interval.map_or("-".to_owned(), |d| format!("{}s", d.as_secs())),
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// An image put on the GPU before it's drawn. iced_wgpu uploads a
/// raster of 2 MiB or more (any real wallpaper) on a worker thread,
/// draws nothing meanwhile and asks no frame when it's done: the
/// wallpaper stayed blank until something else redrew its surface.
/// `load_image` is the renderer's synchronous upload, once per surface
/// (its cache keeps the image while it's drawn every frame).
struct Uploaded {
    handle: image::Handle,
    image: image::Image<image::Handle>,
}

impl<M, T, R> Widget<M, T, R> for Uploaded
where
    R: advanced::image::Renderer<Handle = image::Handle>,
{
    fn size(&self) -> Size<Length> {
        Widget::<M, T, R>::size(&self.image)
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &R,
        limits: &layout::Limits,
    ) -> layout::Node {
        Widget::<M, T, R>::layout(&mut self.image, tree, renderer, limits)
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut R,
        theme: &T,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Err(e) = renderer.load_image(&self.handle) {
            log::error!("wallpaper: cannot upload the image: {e}");
        }
        Widget::<M, T, R>::draw(
            &self.image,
            tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
}

fn layer_settings(output: &OutputInfo) -> NewLayerShellSettings {
    NewLayerShellSettings {
        anchor: Anchor::all(),
        size: LayerSize::FILL,
        layer: Layer::Background,
        exclusive_zone: Some(-1),
        margin: None,
        keyboard_interactivity: KeyboardInteractivity::None,
        output_option: OutputOption::GlobalName(output.id),
        // Nothing to point at: the pointer over the desktop would
        // only be messages, each a rebuild of every surface and a
        // frame of this one (`Message::redraw_scope`).
        events_transparent: true,
        namespace: Some("aria-wallpaper".to_owned()),
        ..Default::default()
    }
}

/// Section `name`'s ticks, the first one `every` from now.
fn ticks(name: String, every: Duration) -> impl Stream<Item = Event> {
    stream::unfold(name, move |name| async move {
        tokio::time::sleep(every).await;
        Some((Event::Tick(name.clone()), name))
    })
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Decode `path` on a blocking thread; the pixels come back as
/// [`Event::Loaded`].
fn load(path: PathBuf) -> Task<Event> {
    Task::perform(
        async move {
            let modified = modified(&path);
            let decoded = tokio::task::spawn_blocking({
                let path = path.clone();
                move || decode(&path)
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            let handle = match decoded {
                Ok(handle) => Some(handle),
                Err(e) => {
                    log::error!("wallpaper {}: {e}", path.display());
                    None
                }
            };
            Event::Loaded {
                path,
                handle,
                modified,
            }
        },
        |e| e,
    )
}

/// Read and decode by content (the extension isn't trusted: a file
/// without one is fine), RGBA8 for iced.
fn decode(path: &Path) -> Result<image::Handle, String> {
    let started = Instant::now();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let decoded = ::image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let (w, h) = decoded.dimensions();
    log::info!(
        "wallpaper {} loaded ({w}x{h}) in {:?}",
        path.display(),
        started.elapsed()
    );
    Ok(image::Handle::from_rgba(w, h, decoded.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_and_defaults() {
        let cfg: WallpaperConfig = Config::parse("").section(None);
        assert_eq!(
            cfg,
            WallpaperConfig {
                source: Source::Auto,
                fit: Fit::Cover,
                interval: None,
                order: Order::Name,
            }
        );
        for (word, fit) in [
            ("cover", Fit::Cover),
            ("contain", Fit::Contain),
            ("fill", Fit::Fill),
            ("none", Fit::None),
            ("scale-down", Fit::ScaleDown),
            ("stretch", Fit::Cover),
        ] {
            let cfg: WallpaperConfig =
                Config::parse(&format!("[wallpaper]\nfit = {word}\n")).section(None);
            assert_eq!(cfg.fit, fit, "{word}");
        }
        let cfg: WallpaperConfig =
            Config::parse("[wallpaper]\nsource = none\ninterval = 10m\norder = random\n")
                .section(None);
        assert_eq!(cfg.source, Source::None);
        assert_eq!(cfg.interval, Some(Duration::from_secs(600)));
        assert_eq!(cfg.order, Order::Random);
        for (text, interval) in [("0", None), ("10", None), ("30s", Some(30))] {
            let cfg: WallpaperConfig =
                Config::parse(&format!("[wallpaper]\ninterval = {text}\n")).section(None);
            assert_eq!(cfg.interval, interval.map(Duration::from_secs), "{text}");
        }
        let cfg: WallpaperConfig =
            Config::parse("[wallpaper]\nsource = auto\norder = shuffle\n").section(None);
        assert_eq!((cfg.source, cfg.order), (Source::Auto, Order::Name));
    }

    #[test]
    fn per_output_section_wins_when_it_has_a_source() {
        let cfg = Config::parse(
            "[wallpaper]\nsource = /a.png\nfit = contain\n\
             [wallpaper:DP-1]\nsource = /b.png\n\
             [wallpaper:DP-2]\nfit = fill\n\
             [wallpaper:DP-3]\nsource = none\n",
        );
        let (name, dp1) = WallpaperConfig::for_output(&cfg, "DP-1");
        assert_eq!(name, "wallpaper:DP-1");
        assert_eq!(dp1.source, Source::Path("/b.png".into()));
        assert_eq!(dp1.fit, Fit::Cover);
        // No source of its own: the generic one, wholly.
        let (name, dp2) = WallpaperConfig::for_output(&cfg, "DP-2");
        assert_eq!(name, "wallpaper");
        assert_eq!(dp2.source, Source::Path("/a.png".into()));
        assert_eq!(dp2.fit, Fit::Contain);
        assert_eq!(
            WallpaperConfig::for_output(&cfg, "DP-3").1.source,
            Source::None
        );
        let (name, any) = WallpaperConfig::for_output(&Config::parse(""), "DP-1");
        assert_eq!((name.as_str(), any.source), ("wallpaper", Source::Auto));
        // Relative to the config's directory (none here: the cwd).
        let cfg = Config::parse("[wallpaper]\nsource = walls\n");
        let (_, w) = WallpaperConfig::for_output(&cfg, "DP-1");
        assert_eq!(w.source, Source::Path("./walls".into()));
    }
}
