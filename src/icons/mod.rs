//! Application icons: from a window class (`firefox`, `org.kde.dolphin`)
//! to an iced image handle, through the `.desktop` database and the
//! icon theme.
//!
//! Daemon-owned, like `Compositor`: [`Icons::load`] builds the index off
//! the main thread (the bars come up right away, icons appear a few
//! tens of milliseconds later), [`Icons::apply`] installs it,
//! [`Icons::resolve`] fills a per-class cache the daemon keeps warm for
//! the windows that exist, and `view` only reads it with [`Icons::get`].
//! Handles are created once here and cloned into the view: iced caches
//! decoded images by handle id, a fresh handle every frame would decode
//! every frame.
//!
//! The index is rebuilt when a watched directory (`applications/`, the
//! theme dirs) changes, so an app installed while the shell runs gets
//! its icon.

pub mod desktop;
mod theme;

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use iced::widget::{image, svg};
use iced::{Color, Element, Length, Task};

use crate::config::{self, Config, GeneralConfig};
use desktop::DesktopDb;
use theme::IconIndex;

/// Icons are looked up at this nominal size; the theme's scalable
/// directory usually wins, and iced scales the result to the CSS size.
const LOOKUP_SIZE: u32 = 32;

/// Shown for windows whose class resolves to nothing.
const FALLBACK: &str = "application-x-executable";

/// A resolved icon, ready to draw.
#[derive(Debug, Clone)]
pub enum Icon {
    Svg {
        handle: svg::Handle,
        /// A `-symbolic` icon: a monochrome shape meant to take the
        /// text colour.
        symbolic: bool,
    },
    Raster(image::Handle),
}

impl Icon {
    fn from_path(path: PathBuf) -> Self {
        if path.extension().is_some_and(|e| e == "svg") {
            let symbolic = path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.ends_with("-symbolic"));
            Self::Svg {
                handle: svg::Handle::from_path(path),
                symbolic,
            }
        } else {
            Self::Raster(image::Handle::from_path(path))
        }
    }

    /// The icon as a `size`-pixel square; `color` tints symbolic icons.
    pub fn view<'a, M: 'a>(&self, size: f32, color: Option<Color>) -> Element<'a, M> {
        let size = Length::Fixed(size);
        match self {
            Self::Svg { handle, symbolic } => {
                let color = color.filter(|_| *symbolic);
                svg(handle.clone())
                    .width(size)
                    .height(size)
                    .style(move |_, _| svg::Style { color })
                    .into()
            }
            Self::Raster(handle) => image(handle.clone()).width(size).height(size).into(),
        }
    }
}

/// The scanned data: theme chain plus desktop entries.
pub struct Index {
    icons: IconIndex,
    apps: DesktopDb,
}

impl Index {
    pub fn apps(&self) -> &DesktopDb {
        &self.apps
    }
}

impl fmt::Debug for Index {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Index({} icons in {:?}, {} apps)",
            self.icons.icon_count(),
            self.icons.theme_names(),
            self.apps.len()
        )
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Loaded(Arc<Index>),
}

pub struct Icons {
    theme: String,
    /// `[apps_class_map]`: window class -> desktop id or icon name, for
    /// apps whose class matches nothing.
    overrides: HashMap<String, String>,
    index: Option<Arc<Index>>,
    /// Window class -> its icon (`None`: nothing found, don't retry).
    cache: HashMap<String, Option<Icon>>,
}

impl Icons {
    pub fn new(config: &Config) -> Self {
        let general: GeneralConfig = config.section(None);
        let overrides = config
            .raw_section("apps_class_map")
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.to_owned()))
            .collect();
        Self {
            theme: general.icon_theme.unwrap_or_else(detect_theme),
            overrides,
            index: None,
            cache: HashMap::new(),
        }
    }

    /// Scan the theme chain and the desktop entries on a blocking
    /// thread; the result comes back as [`Event::Loaded`].
    pub fn load(&self) -> Task<Event> {
        let theme = self.theme.clone();
        Task::perform(
            async move {
                let started = Instant::now();
                let index = tokio::task::spawn_blocking(move || build(&theme))
                    .await
                    .expect("icon index build doesn't panic");
                log::info!("icons: {index:?} in {:?}", started.elapsed());
                Arc::new(index)
            },
            Event::Loaded,
        )
    }

    /// A new index: forget every resolution, the daemon re-resolves the
    /// classes it shows.
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Loaded(index) => {
                self.index = Some(index);
                self.cache.clear();
            }
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.index.is_some()
    }

    /// The current index, shared: what the launcher searches.
    pub fn index(&self) -> Option<Arc<Index>> {
        self.index.clone()
    }

    /// Keep serving `previous`'s index until [`Icons::load`] delivers a
    /// new one (a config reload shouldn't blank the icons).
    pub fn keep_index_of(&mut self, previous: &Icons) {
        self.index = previous.index.clone();
    }

    /// Make sure `class` has a cache entry. A no-op until the index is
    /// loaded, and for classes already resolved.
    pub fn resolve(&mut self, class: &str) {
        let Some(index) = &self.index else {
            return;
        };
        if self.cache.contains_key(class) {
            return;
        }
        let icon = lookup(index, &self.overrides, class).map(Icon::from_path);
        if icon.is_none() {
            log::debug!("no icon for window class {class:?}");
        }
        self.cache.insert(class.to_owned(), icon);
    }

    /// The icon for `class`, once resolved and found.
    pub fn get(&self, class: &str) -> Option<&Icon> {
        self.cache.get(class).and_then(Option::as_ref)
    }

    /// Directories whose changes should rebuild the index: where the
    /// desktop entries and the theme icons live.
    pub fn watch_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = application_dirs()
            .into_iter()
            .filter(|d| d.is_dir())
            .collect();
        if let Some(index) = &self.index {
            dirs.extend(index.icons.dirs());
        }
        dirs
    }
}

fn build(theme: &str) -> Index {
    let data = config::xdg_data_dirs();
    let icons: Vec<PathBuf> = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".icons"))
        .into_iter()
        .chain(data.iter().map(|d| d.join("icons")))
        .collect();
    let pixmaps: Vec<PathBuf> = data.iter().map(|d| d.join("pixmaps")).collect();
    let t = Instant::now();
    let icons = IconIndex::load(theme, &icons, &pixmaps);
    log::debug!("icons: theme index in {:?}", t.elapsed());
    let t = Instant::now();
    let apps = DesktopDb::load(&application_dirs());
    log::debug!("icons: desktop entries in {:?}", t.elapsed());
    Index { icons, apps }
}

fn application_dirs() -> Vec<PathBuf> {
    config::xdg_data_dirs()
        .into_iter()
        .map(|d| d.join("applications"))
        .collect()
}

/// Class -> file. The desktop entry's `Icon` first (by id,
/// `StartupWMClass`, executable), then the `[apps_class_map]` override
/// (a desktop id or an icon name), then the class as an icon name,
/// then the generic fallback.
fn lookup(index: &Index, overrides: &HashMap<String, String>, class: &str) -> Option<PathBuf> {
    let by_entry = |c: &str| {
        index
            .apps
            .for_class(c)
            .and_then(|e| e.icon.as_deref())
            .and_then(|name| index.icons.lookup(name, LOOKUP_SIZE))
    };
    by_entry(class)
        .or_else(|| {
            let o = overrides.get(&class.to_ascii_lowercase())?;
            by_entry(o).or_else(|| index.icons.lookup(o, LOOKUP_SIZE))
        })
        .or_else(|| index.icons.lookup(class, LOOKUP_SIZE))
        .or_else(|| index.icons.lookup(&class.to_ascii_lowercase(), LOOKUP_SIZE))
        .or_else(|| index.icons.lookup(FALLBACK, LOOKUP_SIZE))
}

/// The icon theme GTK apps use on this desktop: `gtk-icon-theme-name`
/// from the GTK 4 then GTK 3 `settings.ini`, else `Adwaita` (a
/// `gsettings` query would need dconf; the config key is the override).
fn detect_theme() -> String {
    let found = config::config_dirs()
        .into_iter()
        .filter_map(|d| d.parent().map(PathBuf::from))
        .flat_map(|d| {
            [
                d.join("gtk-4.0/settings.ini"),
                d.join("gtk-3.0/settings.ini"),
            ]
        })
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .find_map(|text| {
            text.lines().find_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim() == "gtk-icon-theme-name").then(|| v.trim().to_owned())
            })
        });
    let theme = found.unwrap_or_else(|| "Adwaita".to_owned());
    log::info!("icon theme: {theme}");
    theme
}
