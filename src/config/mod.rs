mod general;
mod model;

pub use general::GeneralConfig;
pub use model::{ConfigSection, get_or};

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use configparser::ini::Ini;

static GLOBAL: OnceLock<AriaConfig> = OnceLock::new();

/// Mirrors `aria_shell.config.AriaConfig`: the singleton that loads
/// `aria.conf` and hands out typed sections. Rust singleton idiom: a
/// `static OnceLock`, not a metaclass (same pattern reused by `Service`).
pub struct AriaConfig {
    ini: Ini,
    // Read via `parsed_file()`; not yet called by anything in this spike
    // (no reload-on-change logic exists yet) -- kept for parity with
    // Python's `AriaConfig.parsed_file` property.
    #[allow(dead_code)]
    parsed_file: Option<PathBuf>,
}

impl AriaConfig {
    /// Global accessor -- loads the config on first access.
    pub fn global() -> &'static AriaConfig {
        GLOBAL.get_or_init(|| AriaConfig::load(None))
    }

    /// Mirrors `AriaConfig.load_conf()` / `utils.env.lookup_config_file`:
    /// search `$XDG_CONFIG_HOME/aria-shell/aria.conf`, then each
    /// `$XDG_CONFIG_DIRS/aria-shell/aria.conf`, then fall back to the
    /// `assets/aria.conf` sample file (repo-relative -- pure local-dev
    /// convenience for this spike, not a real install path).
    fn load(explicit: Option<&Path>) -> Self {
        let path = explicit
            .map(PathBuf::from)
            .or_else(lookup_config_file)
            .or_else(dev_fallback_config_file);

        let mut ini = Ini::new_cs(); // case-sensitive sections+keys, mirrors
        // Python's `self._parser.optionxform = str`
        ini.set_comment_symbols(&['#']);
        ini.set_inline_comment_symbols(Some(&['#']));

        let parsed_file = match &path {
            Some(p) => match ini.load(p) {
                Ok(_) => Some(p.clone()),
                Err(e) => {
                    eprintln!("Config file parsing error: {e}");
                    None
                }
            },
            None => {
                eprintln!("Cannot find a configuration file");
                None
            }
        };

        Self { ini, parsed_file }
    }

    /// Mirrors `AriaConfig.section()`: fetch section `name` (defaults to
    /// `T::SECTION` when `None`), parsed via `T::from_section`.
    pub fn section<T: ConfigSection>(&self, name: Option<&str>) -> T {
        let section_name = name.unwrap_or(T::SECTION);
        let raw = self.section_dict(section_name);
        T::from_section(&raw)
    }

    /// Mirrors `AriaConfig.section_dict()`: the raw section map, with
    /// `None` values (bare keys) normalized to `""` (mirrors Python:
    /// `if not str_val: continue`, i.e. empty/missing = "use the default").
    fn section_dict(&self, section_name: &str) -> HashMap<String, String> {
        self.ini
            .get_map_ref()
            .get(section_name)
            .map(|kv| {
                kv.iter()
                    .map(|(k, v)| (k.clone(), v.clone().unwrap_or_default()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Mirrors `AriaConfig.sections(prefix)`: section names equal to
    /// `prefix` or starting with `"prefix:"` (e.g. "Clock", "Clock:2").
    /// Not called yet in this spike (the Panel doesn't read `[panel]`
    /// sections here) -- built now since future modules will need it.
    #[allow(dead_code)]
    pub fn sections_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.ini
            .sections()
            .into_iter()
            .filter(|s| s == prefix || s.starts_with(&format!("{prefix}:")))
            .collect()
    }

    // Not called yet in this spike -- the Clock module is hard-registered
    // rather than read from `general.modules` (see `modules::request_gadget`).
    // Kept for parity, since every future module will hang off of it.
    #[allow(dead_code)]
    pub fn general(&self) -> GeneralConfig {
        self.section(None)
    }

    #[allow(dead_code)]
    pub fn parsed_file(&self) -> Option<&Path> {
        self.parsed_file.as_deref()
    }
}

fn lookup_config_file() -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let xdg_config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let xdg_config_dirs = env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".to_owned());

    let mut candidates = vec![xdg_config_home];
    candidates.extend(xdg_config_dirs.split(':').map(PathBuf::from));

    candidates
        .into_iter()
        .map(|dir| dir.join("aria-shell").join("aria.conf"))
        .find(|f| f.exists())
}

/// Not part of the Python behavior: pure convenience for running this
/// spike straight out of the repo without an installed config, using a
/// trimmed-down sample config under `assets/` (only the sections this
/// spike actually reads -- see `assets/aria.conf`).
fn dev_fallback_config_file() -> Option<PathBuf> {
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("aria.conf");
    candidate.exists().then_some(candidate)
}
