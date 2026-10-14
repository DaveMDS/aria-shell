//! Styling: the toolkit's base stylesheet, then Aria's
//! (`assets/aria.css`: its own elements, the palette), then the user
//! theme `[general] style` names, then the user's own touches over any
//! theme (`theme-override.css` beside the config file), compiled into
//! one `icssed::Sheet` for one [`Scheme`]. The views build their nodes from a surface's root
//! ([`Theme::root`]); the toolkit's widgets resolve their styles.
//!
//! Loaded once, and again on a scheme or theme switch (the `Themes`
//! gadget) or when the theme's file changes: the daemon watches the
//! files (`watch.rs`) and calls [`Theme::try_load`]. What a sheet can't
//! use is logged with its `file:line:col` and skipped; a syntax error
//! rejects the user theme, and a reload keeps the last good one.

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

use icssed::{Node, Sheet, Source};

use crate::config::{self, Config};

pub use icssed::Scheme;

/// Aria's own stylesheet, over the toolkit's base, under the user's.
const BASE: &str = include_str!("../../assets/aria.css");

/// The user's stylesheet over whatever theme is in use, beside the
/// config file.
const OVERRIDE: &str = "theme-override.css";

/// What a gadget asks the daemon to change about the theme.
#[derive(Debug, Clone)]
pub enum Command {
    ToggleScheme,
    SetScheme(Scheme),
    /// A theme name (as `[general] style`), or `None` for the base alone.
    SetStyle(Option<String>),
    /// Read the theme's own config over the user's, or stop.
    ToggleConfig,
}

pub struct Theme {
    sheet: Sheet,
    /// User files loaded, for the daemon to watch (see `watch.rs`).
    files: Vec<PathBuf>,
    /// The user theme loaded, as named in the config or picked by the
    /// user; `None` for the base alone.
    name: Option<String>,
    /// Its own config, `<name>.conf` beside its file, if it has one.
    config: Option<PathBuf>,
    /// Whether that config is read over the user's: the daemon's
    /// choice, kept here for the views.
    config_on: bool,
}

/// Why a user theme couldn't be loaded. `files` are still the ones to
/// watch, so fixing the file triggers a reload.
#[derive(Debug)]
pub struct LoadError {
    pub message: String,
    pub files: Vec<PathBuf>,
}

impl Theme {
    /// The base stylesheets plus the user theme `style` (a name looked
    /// up in the theme directories, or a path), if any, for `scheme`.
    /// Never fails: a missing or broken user theme is logged and the
    /// base alone is used (its file still watched).
    pub fn load(config: &Config, style: Option<&str>, scheme: Scheme) -> Self {
        Self::try_load(config, style, scheme).unwrap_or_else(|e| {
            log::error!("{}, using the base theme", e.message);
            let mut base = Self::base(config, scheme);
            base.files.extend(e.files);
            base
        })
    }

    /// Like [`Theme::load`], but a user theme that can't be read or has a
    /// syntax error is an error, so a reload can keep the last good one.
    pub fn try_load(
        config: &Config,
        style: Option<&str>,
        scheme: Scheme,
    ) -> Result<Self, LoadError> {
        let Some(style) = style else {
            return Ok(Self::base(config, scheme));
        };
        let Some(path) = locate(style, config) else {
            return Err(LoadError {
                message: format!("theme {style:?} not found"),
                files: Vec::new(),
            });
        };
        let over = Override::read(config);
        // A theme's folder is watched too: an image changed reloads it.
        let mut files = vec![path.clone()];
        files.extend(folder_of(&path));
        let text = fs::read_to_string(&path).map_err(|e| LoadError {
            message: format!("cannot read theme {}: {e}", path.display()),
            files: files.clone(),
        })?;
        log::info!("loading theme {} ({})", path.display(), scheme.name());
        let name = path.display().to_string();
        let user = Source {
            name: &name,
            text: &text,
            dir: path.parent(),
        };
        let sheet = over
            .compile(&[base_source(), user], scheme)
            .map_err(|message| LoadError {
                message,
                files: files.clone(),
            })?;
        files.extend(over.path);
        Ok(Self {
            sheet,
            files,
            name: Some(style.to_owned()),
            config: config_beside(&path),
            config_on: false,
        })
    }

    /// The base stylesheets alone, the user's override over them.
    fn base(config: &Config, scheme: Scheme) -> Self {
        let over = Override::read(config);
        Self {
            sheet: over
                .compile(&[base_source()], scheme)
                .expect("aria.css is valid"),
            files: over.path.into_iter().collect(),
            ..Self::default_for(scheme)
        }
    }

    /// The base stylesheets alone, nothing of the user's.
    fn default_for(scheme: Scheme) -> Self {
        Self {
            sheet: compile(&[base_source()], scheme).expect("aria.css is valid"),
            files: Vec::new(),
            name: None,
            config: None,
            config_on: false,
        }
    }

    pub fn sheet(&self) -> &Sheet {
        &self.sheet
    }

    /// The root element of a surface: `kind` (`panel`, `popup`,
    /// `launcher`, ...), at `scale` device pixels per logical one.
    pub fn root(&self, kind: &str, scale: f32) -> Node<'_> {
        self.sheet.root(kind, scale)
    }

    pub fn scheme(&self) -> Scheme {
        self.sheet.scheme()
    }

    /// The user theme loaded, `None` for the base alone.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// The theme's own config, if it has one.
    pub fn config(&self) -> Option<&Path> {
        self.config.as_deref()
    }

    /// Whether the theme's config is read over the user's.
    pub fn config_on(&self) -> bool {
        self.config_on && self.config.is_some()
    }

    pub fn set_config_on(&mut self, on: bool) {
        self.config_on = on;
    }
}

/// The config of the theme `style` names (`<name>.conf` beside its
/// file), if it has one: read over the user's (`Config::with_theme`).
pub fn config_file(style: &str, config: &Config) -> Option<PathBuf> {
    locate(style, config).and_then(|path| config_beside(&path))
}

fn config_beside(file: &Path) -> Option<PathBuf> {
    Some(file.with_extension("conf")).filter(|f| f.is_file())
}

impl Default for Theme {
    /// The base stylesheets alone, light.
    fn default() -> Self {
        Self::default_for(Scheme::default())
    }
}

fn base_source() -> Source<'static> {
    Source {
        name: "aria.css",
        text: BASE,
        dir: None,
    }
}

/// The user's override: its path (watched even before it exists) and
/// its text, when there's a config file to be beside and it's there.
struct Override {
    path: Option<PathBuf>,
    text: Option<String>,
}

impl Override {
    fn read(config: &Config) -> Self {
        let path = config.dir().map(|dir| dir.join(OVERRIDE));
        let text = path.as_ref().and_then(|p| match fs::read_to_string(p) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                log::error!("cannot read {}: {e}, skipped", p.display());
                None
            }
        });
        Self { path, text }
    }

    /// One sheet from `sources` and the override over them. A syntax
    /// error in the override drops it alone (logged): the theme stays.
    fn compile(&self, sources: &[Source], scheme: Scheme) -> Result<Sheet, String> {
        let (Some(path), Some(text)) = (&self.path, &self.text) else {
            return compile(sources, scheme);
        };
        let name = path.display().to_string();
        let over = Source {
            name: &name,
            text,
            dir: path.parent(),
        };
        let all: Vec<Source> = sources.iter().cloned().chain([over]).collect();
        compile(&all, scheme).or_else(|e| {
            let sheet = compile(sources, scheme)?;
            log::error!("{e}, {OVERRIDE} skipped");
            Ok(sheet)
        })
    }
}

/// One sheet from `sources`, over the toolkit's base, its warnings
/// logged; a syntax error is the error.
fn compile(sources: &[Source], scheme: Scheme) -> Result<Sheet, String> {
    let (sheet, warnings) = Sheet::load(sources, scheme).map_err(|e| e.to_string())?;
    for warning in warnings {
        log::warn!("{warning}, skipped");
    }
    Ok(sheet)
}

/// `[general] color_scheme`'s value.
pub fn parse_scheme(value: &str) -> Option<Scheme> {
    match value {
        "light" => Some(Scheme::Light),
        "dark" => Some(Scheme::Dark),
        _ => None,
    }
}

/// The fonts built into the binary (`assets/fonts/`, with their
/// licenses): `aria.css` names them, so the text looks the same on
/// every system, whatever is installed.
const FONTS: [&[u8]; 3] = [
    include_bytes!("../../assets/fonts/NotoSansDisplay-Regular.ttf"),
    include_bytes!("../../assets/fonts/NotoSansDisplay-Medium.ttf"),
    include_bytes!("../../assets/fonts/NotoSansDisplay-Bold.ttf"),
];

/// The built-in fonts into iced's font system, before anything is laid
/// out: a surface is measured before it exists, and a sheet keeps the
/// metrics of the font it first finds for a family (the runtime would
/// load `Settings::fonts` only with its first surface).
pub fn load_fonts() {
    let mut system = iced::advanced::graphics::text::font_system()
        .write()
        .unwrap_or_else(|e| e.into_inner());
    for font in FONTS {
        system.load_font(Cow::Borrowed(font));
    }
}

/// The directories a theme name is looked up in, in order.
fn theme_dirs() -> Vec<PathBuf> {
    config::config_dirs()
        .into_iter()
        .chain(config::data_dirs())
        .chain(std::iter::once(config::dev_assets_dir()))
        .collect()
}

/// Every theme a name resolves to: `(name, path)`, sorted by name; a
/// name found in an earlier directory hides the later ones, as
/// [`locate`] would pick it.
pub fn available() -> Vec<(String, PathBuf)> {
    available_in(&theme_dirs())
}

fn available_in(dirs: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for dir in dirs {
        let themes = dir.join("themes");
        let Ok(entries) = fs::read_dir(&themes) else {
            continue;
        };
        for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
            let name = if path.is_dir() {
                path.file_name()
            } else if path.extension().is_some_and(|e| e == "css") {
                path.file_stem()
            } else {
                None
            };
            let Some(name) = name.and_then(|s| s.to_str()) else {
                continue;
            };
            if found.iter().any(|(n, _)| n == name) {
                continue;
            }
            if let Some(file) = theme_in(&themes, name) {
                found.push((name.to_owned(), file));
            }
        }
    }
    found.sort();
    found
}

/// Where `[general] style = <value>` points: a path if it looks like
/// one (has a `/` or ends in `.css`, relative to the config file's
/// directory), else the theme of that name under the config dirs, the
/// data dirs, then the source tree's `assets/` (see [`theme_in`]).
fn locate(style: &str, config: &Config) -> Option<PathBuf> {
    if style.contains('/') || style.ends_with(".css") {
        let path = config.resolve_path(style);
        return path.is_file().then_some(path);
    }
    theme_dirs()
        .into_iter()
        .find_map(|dir| theme_in(&dir.join("themes"), style))
}

/// The theme `name` in a `themes/` directory: its folder
/// (`<name>/<name>.css`, its images beside it), else `<name>.css`.
fn theme_in(themes: &Path, name: &str) -> Option<PathBuf> {
    [
        themes.join(name).join(format!("{name}.css")),
        themes.join(format!("{name}.css")),
    ]
    .into_iter()
    .find(|f| f.is_file())
}

/// The folder a theme's file is in, if it's the theme's own
/// (`<name>/<name>.css`), not a directory of themes.
fn folder_of(file: &Path) -> Option<PathBuf> {
    let dir = file.parent()?;
    (dir.file_name()? == file.file_stem()?).then(|| dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_stylesheet_loads_clean_in_both_schemes() {
        for scheme in [Scheme::Light, Scheme::Dark] {
            let (_, warnings) = Sheet::load(&[base_source()], scheme).expect("aria.css parses");
            assert!(
                warnings.is_empty(),
                "aria.css ({}): {}",
                scheme.name(),
                warnings
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    #[test]
    fn available_themes_dedup_and_sort() {
        let dir = std::env::temp_dir().join(format!("aria-themes-{}", std::process::id()));
        let (a, b) = (dir.join("a/themes"), dir.join("b/themes"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("zeta.css"), "").unwrap();
        fs::write(a.join("alpha.css"), "").unwrap();
        fs::write(b.join("alpha.css"), "").unwrap();
        fs::write(b.join("beta.css"), "").unwrap();
        fs::write(b.join("notes.txt"), "").unwrap();
        fs::create_dir_all(b.join("gamma")).unwrap();
        fs::write(b.join("gamma/gamma.css"), "").unwrap();
        fs::create_dir_all(b.join("images")).unwrap();
        let list = available_in(&[dir.join("a"), dir.join("b")]);
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["alpha", "beta", "gamma", "zeta"]);
        assert_eq!(list[0].1, a.join("alpha.css"), "the first directory wins");
        assert_eq!(list[2].1, b.join("gamma/gamma.css"), "a theme as a folder");
        assert_eq!(folder_of(&list[2].1), Some(b.join("gamma")));
        assert_eq!(
            folder_of(&list[0].1),
            None,
            "themes/ isn't a theme's folder"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_users_override_goes_over_any_theme() {
        let dir = std::env::temp_dir().join(format!("aria-override-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let config = Config::parse("").at(&dir.join("aria.conf"));
        let over = dir.join(OVERRIDE);
        let theme = dir.join("mine.css");
        fs::write(&theme, "surface.panel { font-size: 40px; }").unwrap();
        let size = |t: &Theme| t.root("panel", 1.0).base().font_size;
        // Watched before it exists: creating it reloads.
        let t = Theme::load(&config, None, Scheme::Light);
        assert_eq!(t.files(), std::slice::from_ref(&over));
        fs::write(&over, "surface.panel { font-size: 50px; }").unwrap();
        let t = Theme::load(&config, None, Scheme::Light);
        assert_eq!(size(&t), 50.0, "over the base");
        let t = Theme::load(&config, Some("mine.css"), Scheme::Light);
        assert_eq!(size(&t), 50.0, "over a theme");
        assert!(t.files().contains(&over));
        // Broken, it's left out alone: the theme stays.
        fs::write(&over, "surface.panel { font-size: 50px;").unwrap();
        let t = Theme::try_load(&config, Some("mine.css"), Scheme::Light).expect("the theme");
        assert_eq!(size(&t), 40.0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_override_example_holds_uncommented() {
        // Its rules are each in a comment of their own, `/*` and `*/`
        // alone on their lines; its explanations aren't.
        let text = include_str!("../../assets/theme-override.css");
        let lines: Vec<&str> = text.lines().collect();
        let open = lines
            .iter()
            .enumerate()
            .filter(|(i, line)| match **line {
                "*/" => false,
                "/*" => lines.get(i + 1).is_some_and(|next| next.starts_with(" *")),
                _ => true,
            })
            .map(|(_, line)| *line)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(open.contains("--accent: #d6336c"), "uncommented");
        for scheme in [Scheme::Light, Scheme::Dark] {
            let over = Source {
                name: OVERRIDE,
                text: &open,
                dir: None,
            };
            let (_, warnings) = Sheet::load(&[base_source(), over], scheme).expect("it parses");
            assert!(warnings.is_empty(), "{OVERRIDE}: {warnings:?}");
        }
    }

    #[test]
    fn schemes_parse() {
        assert_eq!(parse_scheme("dark"), Some(Scheme::Dark));
        assert_eq!(parse_scheme("light"), Some(Scheme::Light));
        assert_eq!(parse_scheme("Dark"), None);
    }
}
