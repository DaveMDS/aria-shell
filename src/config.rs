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
}

impl Config {
    /// Load the first `aria-shell/aria.conf` found in `$XDG_CONFIG_HOME`
    /// then `$XDG_CONFIG_DIRS`, falling back to the sample `assets/aria.conf`
    /// in the source tree (dev convenience). A missing or unparsable file
    /// yields an empty config: every section then reports its defaults.
    pub fn load() -> Self {
        let path = lookup_config_file().or_else(dev_fallback_config_file);
        let mut ini = new_parser();
        match path {
            Some(p) => match ini.load(&p) {
                Ok(_) => log::info!("using config file {}", p.display()),
                Err(e) => log::error!("cannot parse {}: {e}", p.display()),
            },
            None => log::warn!("no configuration file found, using defaults"),
        }
        Self { ini }
    }

    /// Parse from an in-memory string (tests).
    #[cfg(test)]
    pub fn parse(text: &str) -> Self {
        let mut ini = new_parser();
        ini.read(text.to_owned()).expect("valid ini text");
        Self { ini }
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
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    pub fn str_or(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or(default).to_owned()
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

fn lookup_config_file() -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let xdg_config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let xdg_config_dirs = env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".to_owned());

    std::iter::once(xdg_config_home)
        .chain(xdg_config_dirs.split(':').map(PathBuf::from))
        .map(|dir| dir.join("aria-shell").join("aria.conf"))
        .find(|f| f.exists())
}

fn dev_fallback_config_file() -> Option<PathBuf> {
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/aria.conf");
    candidate.exists().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Demo {
        name: String,
        items: Vec<String>,
    }

    impl Section for Demo {
        const NAME: &'static str = "Demo";
        fn from_raw(raw: &RawSection) -> Self {
            Self {
                name: raw.str_or("name", "default"),
                items: raw.list_or("items", &["a"]),
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
