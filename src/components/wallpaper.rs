//! The desktop background: one layer surface per output on the
//! `background` layer showing an image, sized by CSS `object-fit`
//! words. `[wallpaper]` for every output, `[wallpaper:<connector>]`
//! for one; the daemon opens and closes the surfaces with the outputs
//! as it does the panels. Images are decoded off-thread once per path
//! and shared by the outputs showing the same file ([`Wallpapers`]).
//!
//! Still images only (what the `image` crate decodes: png, jpeg,
//! webp); the Python version also played gifs, videos and shadertoy
//! shaders.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use iced::ContentFit;
use iced::Task;
use iced::widget::image;

use crate::config::{Config, RawSection, Section};

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

/// The decoded images, one per file, for however many outputs show it.
#[derive(Default)]
pub struct Wallpapers {
    /// `None`: the file couldn't be read or decoded (logged), don't
    /// retry until it changes.
    images: HashMap<PathBuf, Option<image::Handle>>,
}

#[derive(Debug, Clone)]
pub enum Event {
    Loaded {
        path: PathBuf,
        handle: Option<image::Handle>,
    },
}

impl Wallpapers {
    /// Whether `path` was loaded (or failed) already.
    pub fn has(&self, path: &Path) -> bool {
        self.images.contains_key(path)
    }

    pub fn get(&self, path: &Path) -> Option<&image::Handle> {
        self.images.get(path).and_then(|h| h.as_ref())
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
