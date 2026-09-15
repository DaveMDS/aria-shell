use std::collections::HashMap;

/// Mirrors `aria_shell.config.AriaConfigModel`: a typed config section.
///
/// Python uses runtime introspection of class annotations to generically
/// parse any subclass. Rust has no such reflection, so each concrete
/// section (e.g. `ClockConfig`) hand-writes its own `from_section`
/// constructor: str -> typed conversion, defaults for missing/empty keys,
/// unknown keys silently ignored. No generic `validate_<key>` hook exists
/// here (Rust has no runtime `getattr(self, f'validate_{key}')`) -- a
/// section that needs validation just calls a function inside its own
/// `from_section` body.
pub trait ConfigSection: Sized {
    /// INI section name this struct is loaded from, e.g. "Clock"
    /// (mirrors `AriaConfigModel.__section__`).
    const SECTION: &'static str;

    /// Parse from the raw string map for one section. Must apply its own
    /// defaults for missing/empty keys (mirrors: empty values are ignored
    /// in Python).
    fn from_section(raw: &HashMap<String, String>) -> Self;
}

/// Mirrors the `str_val in ('1', 'on', 'yes', 'true')` / `('0', 'off',
/// 'no', 'false')` branch of `AriaConfigModel.__init__`. Only consumer
/// today is `GeneralConfig` (unused this spike, see its `dead_code` note).
#[allow(dead_code)]
pub fn parse_bool(raw: &str) -> Option<bool> {
    match raw {
        "1" | "on" | "yes" | "true" => Some(true),
        "0" | "off" | "no" | "false" => Some(false),
        _ => None,
    }
}

/// Mirrors the `list[str]` branch: `str_val.split()`. Only consumer today
/// is `GeneralConfig` (unused this spike, see its `dead_code` note).
#[allow(dead_code)]
pub fn parse_list(raw: &str) -> Vec<String> {
    raw.split_whitespace().map(str::to_owned).collect()
}

/// Helper used by every `from_section` impl: get a key, fall back to
/// `default` when missing or empty (mirrors "empty values are ignored").
pub fn get_or(raw: &HashMap<String, String>, key: &str, default: &str) -> String {
    match raw.get(key) {
        Some(v) if !v.is_empty() => v.clone(),
        _ => default.to_owned(),
    }
}
