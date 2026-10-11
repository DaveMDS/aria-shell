//! Styling: the toolkit's base stylesheet, then Aria's
//! (`assets/base.css`: its own elements, the palette), then the user
//! theme `[general] style` names, compiled into one `icssed::Sheet` for
//! one [`Scheme`]. The views build their nodes from a surface's root
//! ([`Theme::root`]); the toolkit's widgets resolve their styles.
//!
//! Loaded once, and again on a scheme or theme switch (the `Themes`
//! gadget) or when the theme's file changes: the daemon watches the
//! files (`watch.rs`) and calls [`Theme::try_load`]. What a sheet can't
//! use is logged with its `file:line:col` and skipped; a syntax error
//! rejects the user theme, and a reload keeps the last good one.

use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;

use icssed::{Node, Sheet, Source};

use crate::config::{self, Config};

pub use icssed::Scheme;

/// Aria's own stylesheet, over the toolkit's base, under the user's.
const BASE: &str = include_str!("../../assets/base.css");

/// What a gadget asks the daemon to change about the theme.
#[derive(Debug, Clone)]
pub enum Command {
    ToggleScheme,
    SetScheme(Scheme),
    /// A theme name (as `[general] style`), or `None` for the base alone.
    SetStyle(Option<String>),
}

pub struct Theme {
    sheet: Sheet,
    /// User files loaded, for the daemon to watch (see `watch.rs`).
    files: Vec<PathBuf>,
    /// The user theme loaded, as named in the config or picked by the
    /// user; `None` for the base alone.
    name: Option<String>,
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
            Self {
                files: e.files,
                ..Self::base(scheme)
            }
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
            return Ok(Self::base(scheme));
        };
        let Some(path) = locate(style, config) else {
            return Err(LoadError {
                message: format!("theme {style:?} not found"),
                files: Vec::new(),
            });
        };
        let files = vec![path.clone()];
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
        let sheet = compile(&[base_source(), user], scheme).map_err(|message| LoadError {
            message,
            files: files.clone(),
        })?;
        Ok(Self {
            sheet,
            files,
            name: Some(style.to_owned()),
        })
    }

    /// The base stylesheets alone.
    fn base(scheme: Scheme) -> Self {
        Self {
            sheet: compile(&[base_source()], scheme).expect("base.css is valid"),
            files: Vec::new(),
            name: None,
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
}

impl Default for Theme {
    /// The base stylesheets alone, light.
    fn default() -> Self {
        Self::base(Scheme::default())
    }
}

fn base_source() -> Source<'static> {
    Source {
        name: "base.css",
        text: BASE,
        dir: None,
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
/// licenses): `base.css` names them, so the text looks the same on
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
        let Ok(entries) = fs::read_dir(dir.join("themes")) else {
            continue;
        };
        for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
            let Some(name) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|_| path.extension().is_some_and(|e| e == "css") && path.is_file())
            else {
                continue;
            };
            if !found.iter().any(|(n, _)| n == name) {
                found.push((name.to_owned(), path.clone()));
            }
        }
    }
    found.sort();
    found
}

/// Where `[general] style = <value>` points: a path if it looks like
/// one (has a `/` or ends in `.css`, relative to the config file's
/// directory), else `themes/<name>.css` under the config dirs, the data
/// dirs, then the source tree's `assets/`.
fn locate(style: &str, config: &Config) -> Option<PathBuf> {
    if style.contains('/') || style.ends_with(".css") {
        let path = config.resolve_path(style);
        return path.is_file().then_some(path);
    }
    theme_dirs()
        .into_iter()
        .map(|dir| dir.join("themes").join(format!("{style}.css")))
        .find(|f| f.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_stylesheet_loads_clean_in_both_schemes() {
        for scheme in [Scheme::Light, Scheme::Dark] {
            let (_, warnings) = Sheet::load(&[base_source()], scheme).expect("base.css parses");
            assert!(
                warnings.is_empty(),
                "base.css ({}): {}",
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
        let list = available_in(&[dir.join("a"), dir.join("b")]);
        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["alpha", "beta", "zeta"]);
        assert_eq!(list[0].1, a.join("alpha.css"), "the first directory wins");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn schemes_parse() {
        assert_eq!(parse_scheme("dark"), Some(Scheme::Dark));
        assert_eq!(parse_scheme("light"), Some(Scheme::Light));
        assert_eq!(parse_scheme("Dark"), None);
    }
}
