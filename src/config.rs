//! `aria.conf` loading and typed sections.
//!
//! The file format is the user-facing contract inherited from the Python
//! implementation and kept compatible on purpose: INI, case-sensitive
//! section and key names, `[Name]` for the default instance of a section
//! and `[Name:id]` for additional instances, empty values meaning "use the
//! default".

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

use configparser::ini::Ini;

/// The loaded configuration file. Plain data owned by the application
/// state; pass it by reference to whoever needs a section.
pub struct Config {
    ini: Ini,
    /// The file it was loaded from, if any; relative paths in values
    /// (a theme file) are resolved against its directory.
    path: Option<PathBuf>,
}

impl Config {
    /// Load the first `aria-shell/aria.conf` found in `$XDG_CONFIG_HOME`
    /// then `$XDG_CONFIG_DIRS`, falling back to the sample `assets/aria.conf`
    /// in the source tree (dev convenience). A missing or unparsable file
    /// yields an empty config: every section then reports its defaults.
    pub fn load() -> Self {
        let path = lookup_config_file().or_else(dev_fallback_config_file);
        let mut ini = new_parser();
        match &path {
            Some(p) => match ini.load(p) {
                Ok(_) => log::info!("using config file {}", p.display()),
                Err(e) => log::error!("cannot parse {}: {e}", p.display()),
            },
            None => log::warn!("no configuration file found, using defaults"),
        }
        Self { ini, path }
    }

    /// Parse from an in-memory string (tests).
    #[cfg(test)]
    pub fn parse(text: &str) -> Self {
        let mut ini = new_parser();
        ini.read(text.to_owned()).expect("valid ini text");
        Self { ini, path: None }
    }

    /// The file it was loaded from, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Directory of the loaded file, if any.
    pub fn dir(&self) -> Option<&Path> {
        self.path().and_then(Path::parent)
    }

    /// Typed section `name` (default: `T::NAME`). Missing sections and
    /// keys fall back to the type's defaults.
    pub fn section<T: Section>(&self, name: Option<&str>) -> T {
        T::from_raw(&self.raw_section(name.unwrap_or(T::NAME)))
    }

    /// Raw key/value map of one section; bare keys (no `=`) become `""`.
    pub fn raw_section(&self, name: &str) -> RawSection {
        let map = self
            .ini
            .get_map_ref()
            .get(name)
            .map(|kv| {
                kv.iter()
                    .map(|(k, v)| (k.clone(), v.clone().unwrap_or_default()))
                    .collect()
            })
            .unwrap_or_default();
        RawSection(map)
    }

    /// Names of every instance of a section: `prefix` itself plus every
    /// `prefix:id`, in file order.
    pub fn instances(&self, prefix: &str) -> Vec<String> {
        self.ini
            .sections()
            .into_iter()
            .filter(|s| s == prefix || s.strip_prefix(prefix).is_some_and(|r| r.starts_with(':')))
            .collect()
    }
}

/// A typed view over one INI section.
pub trait Section: Sized {
    /// Default section name, e.g. `"Clock"`.
    const NAME: &'static str;

    /// Build from the raw map, applying defaults for missing/empty keys and
    /// ignoring unknown ones.
    fn from_raw(raw: &RawSection) -> Self;
}

/// The string map of one section, with typed accessors. Empty values are
/// treated as absent, so a user can leave `key =` to get the default.
#[derive(Debug, Default, Clone)]
pub struct RawSection(HashMap<String, String>);

impl RawSection {
    /// Every non-empty key/value pair.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    pub fn str_or(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or(default).to_owned()
    }

    /// `1 on yes true` / `0 off no false` (case-insensitive), as in the
    /// Python implementation. Anything else is logged and gives `default`.
    pub fn bool_or(&self, key: &str, default: bool) -> bool {
        match self.get(key).map(str::to_ascii_lowercase).as_deref() {
            None => default,
            Some("1" | "on" | "yes" | "true") => true,
            Some("0" | "off" | "no" | "false") => false,
            Some(other) => {
                log::warn!("invalid boolean {other:?} for {key}, using {default}");
                default
            }
        }
    }

    /// Whitespace-separated list; empty/missing gives `default`.
    pub fn list_or(&self, key: &str, default: &[&str]) -> Vec<String> {
        match self.get(key) {
            Some(v) => v.split_whitespace().map(str::to_owned).collect(),
            None => default.iter().map(|s| (*s).to_owned()).collect(),
        }
    }
}

fn new_parser() -> Ini {
    let mut ini = Ini::new_cs(); // case-sensitive sections and keys
    ini.set_comment_symbols(&['#']);
    // No inline comments: configparser strips from the first `#` anywhere
    // in a value (unlike Python, which requires leading whitespace), which
    // would eat colours like `#ff0000` and URL fragments.
    ini.set_inline_comment_symbols(Some(&[]));
    ini
}

/// `[general]` section: shell-wide settings.
#[derive(Debug, Clone)]
pub struct GeneralConfig {
    /// Theme to load on top of the built-in base: a name looked up in
    /// the theme directories, or a path. `None` for the base alone.
    pub style: Option<String>,
    /// Reload the theme when its file changes.
    pub reload_style: bool,
    /// Rebuild the panels when the config file changes.
    pub reload_config: bool,
    /// Icon theme name; `None` to detect it from the GTK settings.
    pub icon_theme: Option<String>,
}

impl Section for GeneralConfig {
    const NAME: &'static str = "general";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            style: raw.get("style").map(str::to_owned),
            reload_style: raw.bool_or("reload_style", true),
            reload_config: raw.bool_or("reload_config", true),
            icon_theme: raw.get("icon_theme").map(str::to_owned),
        }
    }
}

/// `$XDG_CONFIG_HOME/aria-shell` then each `$XDG_CONFIG_DIRS/aria-shell`.
pub fn config_dirs() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let xdg_config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".config")));
    let xdg_config_dirs = env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".to_owned());
    xdg_config_home
        .into_iter()
        .chain(xdg_config_dirs.split(':').map(PathBuf::from))
        .map(|dir| dir.join("aria-shell"))
        .collect()
}

/// `$XDG_DATA_HOME` then each `$XDG_DATA_DIRS`, in precedence order.
pub fn xdg_data_dirs() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let xdg_data_home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local/share")));
    let xdg_data_dirs =
        env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
    xdg_data_home
        .into_iter()
        .chain(xdg_data_dirs.split(':').map(PathBuf::from))
        .collect()
}

/// `$XDG_DATA_HOME/aria-shell` then each `$XDG_DATA_DIRS/aria-shell`.
pub fn data_dirs() -> Vec<PathBuf> {
    xdg_data_dirs()
        .into_iter()
        .map(|dir| dir.join("aria-shell"))
        .collect()
}

/// The source tree's `assets/`, for running out of a checkout.
pub fn dev_assets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
}

fn lookup_config_file() -> Option<PathBuf> {
    config_dirs()
        .into_iter()
        .map(|dir| dir.join("aria.conf"))
        .find(|f| f.exists())
}

fn dev_fallback_config_file() -> Option<PathBuf> {
    let candidate = dev_assets_dir().join("aria.conf");
    candidate.exists().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Demo {
        name: String,
        items: Vec<String>,
        flag: bool,
    }

    impl Section for Demo {
        const NAME: &'static str = "Demo";
        fn from_raw(raw: &RawSection) -> Self {
            Self {
                name: raw.str_or("name", "default"),
                items: raw.list_or("items", &["a"]),
                flag: raw.bool_or("flag", true),
            }
        }
    }

    #[test]
    fn defaults_for_missing_section_and_empty_values() {
        let cfg = Config::parse("[Demo]\nname =\nitems =\n");
        let d: Demo = cfg.section(None);
        assert_eq!(d.name, "default");
        assert_eq!(d.items, ["a"]);

        let d: Demo = cfg.section(Some("Nope"));
        assert_eq!(d.name, "default");
    }

    #[test]
    fn typed_values() {
        let cfg = Config::parse("[Demo]\nname = hi\nitems = x  y\tz\n");
        let d: Demo = cfg.section(None);
        assert_eq!(d.name, "hi");
        assert_eq!(d.items, ["x", "y", "z"]);
    }

    #[test]
    fn booleans() {
        let cfg = Config::parse("[Demo]\nflag = No\n");
        assert!(!cfg.section::<Demo>(None).flag);
        let cfg = Config::parse("[Demo]\nflag = 1\n");
        assert!(cfg.section::<Demo>(None).flag);
        let cfg = Config::parse("[Demo]\nflag = maybe\n");
        assert!(cfg.section::<Demo>(None).flag);
        let cfg = Config::parse("[Demo]\nflag =\n");
        assert!(cfg.section::<Demo>(None).flag);
    }

    #[test]
    fn case_sensitive_and_instances() {
        let cfg = Config::parse("[Demo]\nname=a\n[Demo:2]\nname=b\n[demo]\nname=c\n[Demos]\n");
        assert_eq!(cfg.instances("Demo"), ["Demo", "Demo:2"]);
        let d: Demo = cfg.section(Some("Demo:2"));
        assert_eq!(d.name, "b");
        let d: Demo = cfg.section(Some("demo"));
        assert_eq!(d.name, "c");
    }

    #[test]
    fn hash_inside_values_is_kept() {
        let cfg = Config::parse("# comment\n[Demo]\nname = #ff0000\n");
        let d: Demo = cfg.section(None);
        assert_eq!(d.name, "#ff0000");
    }
}
