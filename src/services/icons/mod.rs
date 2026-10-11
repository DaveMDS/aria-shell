//! Application icons: from a window class (`firefox`, `org.kde.dolphin`)
//! to an iced image handle, through the `.desktop` database and the
//! icon theme.
//!
//! Daemon-owned, like `Compositor`: [`Icons::load`] builds the index off
//! the main thread (the bars come up right away, icons appear a few
//! tens of milliseconds later), [`Icons::apply`] installs it,
//! [`Icons::resolve`] fills a per-class cache the daemon keeps warm for
//! the windows that exist, and `view` only reads it with [`Icons::get`].
//! Named icons the same way ([`Icons::resolve_name`]); the toolkit asks
//! for them at draw ([`icssed::Icons`]), from that cache only, without
//! allocating. Handles are created once here and cloned into the view:
//! iced caches decoded images by handle id, a fresh handle every frame
//! would decode every frame.
//!
//! The index is rebuilt when a watched directory (`applications/`, the
//! theme dirs) changes, so an app installed while the shell runs gets
//! its icon.

pub mod desktop;
mod theme;

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use iced::Task;
use iced::widget::{image, svg};

use crate::config::{self, Config, GeneralConfig};
use desktop::DesktopDb;
use theme::IconIndex;

/// Icons are looked up at this nominal size; the theme's scalable
/// directory usually wins, and iced scales the result to the CSS size.
const LOOKUP_SIZE: u32 = 32;

/// Shown for windows whose class resolves to nothing.
const FALLBACK: &str = "application-x-executable";

/// A resolved icon, ready to draw: an svg (a `-symbolic` one takes the
/// text's colour) or a raster.
pub use icssed::IconFile as Icon;

/// The icon in file `path`: an svg, symbolic when its name ends in
/// `-symbolic`, else a raster.
pub fn from_path(path: PathBuf) -> Icon {
    if path.extension().is_some_and(|e| e == "svg") {
        let symbolic = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.ends_with("-symbolic"));
        Icon::Svg {
            handle: svg::Handle::from_path(path),
            symbolic,
        }
    } else {
        Icon::Image(image::Handle::from_path(path))
    }
}

/// The scanned data: theme chain plus desktop entries.
pub struct Index {
    icons: IconIndex,
    apps: DesktopDb,
    /// What `Icons::watch_dirs` hands out, listed once with the index
    /// (asked after every update, with the subscriptions: hundreds of
    /// `stat`s each time otherwise).
    watch: Vec<PathBuf>,
}

impl Index {
    pub fn apps(&self) -> &DesktopDb {
        &self.apps
    }

    /// Desktop entries with no icon theme, for the launcher's tests.
    #[cfg(test)]
    pub fn from_apps(apps: DesktopDb) -> Self {
        Self {
            icons: IconIndex::load("", &[], &[]),
            apps,
            watch: Vec::new(),
        }
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
    /// `[lang]` suffixes for the desktop entries' texts, best first.
    languages: Vec<String>,
    /// `[apps_class_map]`: window class -> desktop id or icon name, for
    /// apps whose class matches nothing.
    overrides: HashMap<String, String>,
    index: Option<Arc<Index>>,
    /// Window class -> its icon (`None`: nothing found, don't retry).
    cache: HashMap<String, Option<Icon>>,
    /// Plain icon names, from the theme; always resolves to something.
    named: HashMap<String, Icon>,
    /// Names looked up in an app's own directory first (tray items).
    named_in: HashMap<(String, PathBuf), Icon>,
}

impl Icons {
    /// `languages` are the `[lang]` suffixes the desktop entries are
    /// read in, best first (`Locale::languages`).
    pub fn new(config: &Config, languages: Vec<String>) -> Self {
        let general: GeneralConfig = config.section(None);
        let overrides = config
            .raw_section("apps_class_map")
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.to_owned()))
            .collect();
        Self {
            theme: general.icon_theme.unwrap_or_else(detect_theme),
            languages,
            overrides,
            index: None,
            cache: HashMap::new(),
            named: HashMap::new(),
            named_in: HashMap::new(),
        }
    }

    /// Scan the theme chain and the desktop entries on a blocking
    /// thread; the result comes back as [`Event::Loaded`].
    pub fn load(&self) -> Task<Event> {
        let theme = self.theme.clone();
        let languages = self.languages.clone();
        Task::perform(
            async move {
                let started = Instant::now();
                let index = tokio::task::spawn_blocking(move || build(&theme, &languages))
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
                self.named.clear();
                self.named_in.clear();
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
        let icon = lookup(index, &self.overrides, class).map(from_path);
        if icon.is_none() {
            log::debug!("no icon for window class {class:?}");
        }
        self.cache.insert(class.to_owned(), icon);
    }

    /// The icon for `class`, once resolved and found.
    pub fn get(&self, class: &str) -> Option<&Icon> {
        self.cache.get(class).and_then(Option::as_ref)
    }

    /// Make sure the icon called `name` has an entry: from `dir` (an
    /// app's own icon directory, searched as-is) if given, else the
    /// theme, else the generic fallback. A no-op until the index is
    /// loaded.
    pub fn resolve_name(&mut self, name: &str, dir: Option<&str>) {
        let Some(index) = &self.index else {
            return;
        };
        let known = match dir {
            Some(dir) => self
                .named_in
                .contains_key(&(name.to_owned(), PathBuf::from(dir))),
            None => self.named.contains_key(name),
        };
        if known {
            return;
        }
        let found = dir
            .and_then(|d| find_in_dir(Path::new(d), name, 0))
            .or_else(|| index.icons.lookup(name, LOOKUP_SIZE))
            .or_else(|| index.icons.lookup(FALLBACK, LOOKUP_SIZE));
        let Some(path) = found else {
            log::debug!("no icon named {name:?}, and no fallback");
            return;
        };
        let icon = from_path(path);
        match dir {
            Some(dir) => {
                self.named_in
                    .insert((name.to_owned(), PathBuf::from(dir)), icon);
            }
            None => {
                self.named.insert(name.to_owned(), icon);
            }
        }
    }

    /// Whether the theme has an icon called `name` (a hash probe; the
    /// generic fallback doesn't count), for a caller with alternatives.
    pub fn has_name(&self, name: &str) -> bool {
        self.index
            .as_ref()
            .is_some_and(|i| i.icons.lookup(name, LOOKUP_SIZE).is_some())
    }

    /// The icon called `name`, once resolved; from an app's own
    /// directory `dir` first (that lookup allocates its key: not while
    /// drawing).
    pub fn get_name(&self, name: &str, dir: Option<&str>) -> Option<&Icon> {
        match dir {
            Some(dir) => self.named_in.get(&(name.to_owned(), PathBuf::from(dir))),
            None => self.named.get(name),
        }
    }

    /// For a caller with alternatives, best first (every one of them
    /// among its icon names, for the daemon to resolve): the first the
    /// theme has, else the first (resolved to the generic fallback).
    pub fn first_of<S: AsRef<str>>(&self, names: &[S]) -> Option<&Icon> {
        let name = names
            .iter()
            .map(AsRef::as_ref)
            .find(|n| self.has_name(n))
            .or_else(|| names.first().map(AsRef::as_ref))?;
        self.get_name(name, None)
    }

    /// Directories whose changes should rebuild the index: where the
    /// desktop entries and the theme icons live (none until the index
    /// is built, which reads them anyway).
    pub fn watch_dirs(&self) -> &[PathBuf] {
        self.index.as_ref().map_or(&[], |index| &index.watch)
    }
}

/// The toolkit's named icons are the theme's, as resolved: a hash probe,
/// at draw. Their size doesn't pick the file yet (scale-1 directories,
/// svg first: iced scales them).
impl icssed::Icons for Icons {
    fn lookup(&self, name: &str, _size: f32, _scale: f32) -> Option<Icon> {
        self.named.get(name).cloned()
    }
}

fn build(theme: &str, languages: &[String]) -> Index {
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
    let apps = DesktopDb::load(&application_dirs(), languages);
    log::debug!("icons: desktop entries in {:?}", t.elapsed());
    let mut watch: Vec<PathBuf> = application_dirs()
        .into_iter()
        .filter(|d| d.is_dir())
        .collect();
    watch.extend(icons.dirs());
    Index { icons, apps, watch }
}

fn application_dirs() -> Vec<PathBuf> {
    config::xdg_data_dirs()
        .into_iter()
        .map(|d| d.join("applications"))
        .collect()
}

/// `name.svg` or `name.png` under `dir` (a few levels deep: apps ship
/// `hicolor/<size>/apps/` trees), svg preferred.
fn find_in_dir(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let svg = dir.join(format!("{name}.svg"));
    if svg.is_file() {
        return Some(svg);
    }
    let png = dir.join(format!("{name}.png"));
    if png.is_file() {
        return Some(png);
    }
    if depth >= 4 {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();
    subdirs.iter().find_map(|d| find_in_dir(d, name, depth + 1))
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
