//! The desktop background: one layer surface per output on the
//! `background` layer showing an image, sized by CSS `object-fit`
//! words. `[wallpaper]` for every output, `[wallpaper:<connector>]`
//! for one. [`Wallpapers`] opens a surface when an output comes and
//! closes it when it goes ([`Surfaces`], which the daemon carries out).
//! Images are decoded off-thread once per path and shared by the
//! outputs showing the same file.
//!
//! Still images only (what the `image` crate decodes: png, jpeg,
//! webp); the Python version also played gifs, videos and shadertoy
//! shaders.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use iced::widget::{Space, image};
use iced::window::Id;
use iced::{ContentFit, Element, Length, Task};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
use iced_wayland_subscriber::{OutputId, OutputInfo};

use crate::components::Surfaces;
use crate::config::{Config, RawSection, Section};
use crate::theme::{Node, Theme};

/// `[wallpaper]` / `[wallpaper:<connector>]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallpaperConfig {
    /// The image, resolved against the config file's directory (or
    /// `~`); `None` shows no wallpaper.
    pub source: Option<PathBuf>,
    pub fit: Fit,
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

    /// `source` comes back as written: [`WallpaperConfig::for_output`]
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
        Self {
            source: raw.get("source").map(PathBuf::from),
            fit,
        }
    }
}

impl WallpaperConfig {
    /// The wallpaper for `output`: `[wallpaper:<output>]` when it names
    /// a source, else `[wallpaper]` when it does, else none.
    pub fn for_output(config: &Config, output: &str) -> Option<Self> {
        let specific = format!("{}:{output}", Self::NAME);
        [specific.as_str(), Self::NAME]
            .into_iter()
            .map(|name| config.section::<Self>(Some(name)))
            .find(|c| c.source.is_some())
            .map(|c| Self {
                source: c.source.map(|s| config.resolve_path(&s.to_string_lossy())),
                fit: c.fit,
            })
    }
}

/// The background surfaces, and the decoded images they show.
#[derive(Default)]
pub struct Wallpapers {
    /// One surface per output with a wallpaper configured.
    surfaces: BTreeMap<Id, Wallpaper>,
    /// One image per file, for however many outputs show it. `None`:
    /// the file couldn't be read or decoded (logged), don't retry until
    /// it changes.
    images: HashMap<PathBuf, Option<image::Handle>>,
}

/// A wallpaper surface: its output and what it shows.
struct Wallpaper {
    output: OutputId,
    config: WallpaperConfig,
}

#[derive(Debug, Clone)]
pub enum Event {
    Loaded {
        path: PathBuf,
        handle: Option<image::Handle>,
    },
}

impl Wallpapers {
    /// The wallpaper `output` is configured for, unless it has one
    /// already: its surface, and its image to decode if no other output
    /// shows it.
    pub fn open(&mut self, config: &Config, output: &OutputInfo) -> (Surfaces, Task<Event>) {
        let output_id = OutputId::from(output);
        let none = (Surfaces::default(), Task::none());
        if self.surfaces.values().any(|w| w.output == output_id) {
            return none;
        }
        let name = output.name.clone().unwrap_or_default();
        let Some(config) = WallpaperConfig::for_output(config, &name) else {
            return none;
        };
        let Some(path) = config.source.clone() else {
            return none;
        };
        log::info!("wallpaper {} on output {name:?}", path.display());
        let load = if self.images.contains_key(&path) {
            Task::none()
        } else {
            self.load(path)
        };
        let id = Id::unique();
        let settings = NewLayerShellSettings {
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
        };
        self.surfaces.insert(
            id,
            Wallpaper {
                output: output_id,
                config,
            },
        );
        let surfaces = Surfaces {
            open: vec![(id, settings)],
            ..Surfaces::default()
        };
        (surfaces, load)
    }

    /// Output `output` went away: its surface goes.
    pub fn output_removed(&mut self, output: OutputId) -> Surfaces {
        let mut surfaces = Surfaces::default();
        self.surfaces.retain(|&id, w| {
            let keep = w.output != output;
            if !keep {
                surfaces.close.push(id);
            }
            keep
        });
        surfaces
    }

    /// Close every surface (the config changed: they open again); the
    /// images stay.
    pub fn close(&mut self) -> Surfaces {
        Surfaces {
            close: std::mem::take(&mut self.surfaces).into_keys().collect(),
            ..Surfaces::default()
        }
    }

    /// Surface `window` was closed: whether it was one of ours.
    pub fn closed(&mut self, window: Id) -> bool {
        self.surfaces.remove(&window).is_some()
    }

    /// The open surfaces, with their output.
    pub fn windows(&self) -> impl Iterator<Item = (Id, OutputId)> + '_ {
        self.surfaces.iter().map(|(&id, w)| (id, w.output))
    }

    pub fn output_of(&self, window: Id) -> Option<OutputId> {
        self.surfaces.get(&window).map(|w| w.output)
    }

    /// Surface `window`'s picture, on output `output` (its name).
    pub fn view<'a, M: 'a>(&'a self, window: Id, theme: &'a Theme, output: &str) -> Element<'a, M> {
        let Some(w) = self.surfaces.get(&window) else {
            return Space::new().into();
        };
        let root = Node::root("wallpaper").attr("output", output.to_owned());
        let image = w
            .config
            .source
            .as_deref()
            .and_then(|p| self.images.get(p))
            .and_then(Option::as_ref);
        let picture: Element<'a, M> = match image {
            Some(handle) => image::Image::new(handle.clone())
                .content_fit(w.config.fit.content_fit())
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            None => Space::new().into(),
        };
        theme
            .container(&root, picture)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The files loaded, for the watcher.
    pub fn files(&self) -> impl Iterator<Item = &PathBuf> {
        self.images.keys()
    }

    /// Decode `path` on a blocking thread; the pixels come back as
    /// [`Event::Loaded`]. Marks the path as pending so a second output
    /// with the same file doesn't decode it again.
    pub fn load(&mut self, path: PathBuf) -> Task<Event> {
        self.images.entry(path.clone()).or_insert(None);
        Task::perform(
            async move {
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
                Event::Loaded { path, handle }
            },
            |e| e,
        )
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Loaded { path, handle } => {
                self.images.insert(path, handle);
            }
        }
    }
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
    fn fit_words_and_default() {
        let cfg: WallpaperConfig = Config::parse("").section(None);
        assert_eq!(
            cfg,
            WallpaperConfig {
                source: None,
                fit: Fit::Cover
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
    }

    #[test]
    fn per_output_section_wins_when_it_has_a_source() {
        let cfg = Config::parse(
            "[wallpaper]\nsource = /a.png\nfit = contain\n\
             [wallpaper:DP-1]\nsource = /b.png\n\
             [wallpaper:DP-2]\nfit = fill\n",
        );
        let dp1 = WallpaperConfig::for_output(&cfg, "DP-1").unwrap();
        assert_eq!(dp1.source.as_deref(), Some(Path::new("/b.png")));
        assert_eq!(dp1.fit, Fit::Cover);
        // No source of its own: the generic one, wholly.
        let dp2 = WallpaperConfig::for_output(&cfg, "DP-2").unwrap();
        assert_eq!(dp2.source.as_deref(), Some(Path::new("/a.png")));
        assert_eq!(dp2.fit, Fit::Contain);
        assert!(WallpaperConfig::for_output(&Config::parse(""), "DP-1").is_none());
        // Relative to the config's directory (none here: the cwd).
        let cfg = Config::parse("[wallpaper]\nsource = walls/x.png\n");
        let w = WallpaperConfig::for_output(&cfg, "DP-1").unwrap();
        assert_eq!(w.source.as_deref(), Some(Path::new("./walls/x.png")));
    }
}
