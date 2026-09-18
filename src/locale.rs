//! The UI language: every text the shell shows comes from a catalogue
//! keyed by a stable id (`locker.unlock`, `sysmon.tab.processes`), one
//! catalogue per language compiled in (`locale/en.rs`, `locale/it.rs`),
//! and the dates are formatted with chrono's locale data. Owned by the
//! daemon, read through `Shared` in `view`: `ctx.locale.tr("audio.output")`.
//!
//! The language is `[general] language`, or the environment's
//! (`LC_ALL`, `LC_MESSAGES`, `LANG`, in glibc's order). English is a
//! catalogue like the others and the fallback under every other one.
//! Adding a language: a file with the same keys, one line in
//! [`CATALOGUES`]; `cargo test` checks that every catalogue has exactly
//! the keys the code uses.

mod en;
mod it;

use std::collections::HashMap;
use std::fmt::Display;

use chrono::{DateTime, Local, NaiveDate};

/// Key -> text, as a language file declares it.
pub type Catalogue = &'static [(&'static str, &'static str)];

/// The languages: code, the region used for dates when the
/// environment doesn't give one, the texts.
const CATALOGUES: &[(&str, &str, Catalogue)] = &[
    ("en", "en_US", en::CATALOGUE),
    ("it", "it_IT", it::CATALOGUE),
];

const DEFAULT: &str = "en";

pub struct Locale {
    lang: &'static str,
    /// English under the chosen language's texts.
    texts: HashMap<&'static str, &'static str>,
    dates: chrono::Locale,
}

impl Locale {
    /// `language` is `[general] language`; empty means the environment.
    pub fn new(language: &str) -> Self {
        let (wanted, region) = if language.is_empty() {
            environment()
        } else {
            (language.to_owned(), None)
        };
        let (lang, default_region, catalogue) = CATALOGUES
            .iter()
            .copied()
            .find(|(code, ..)| *code == wanted)
            .unwrap_or_else(|| {
                let fallback = CATALOGUES
                    .iter()
                    .copied()
                    .find(|(code, ..)| *code == DEFAULT)
                    .expect("the default language has a catalogue");
                log::warn!("no translation for language {wanted:?}, using {DEFAULT}");
                fallback
            });
        let dates = region
            .as_deref()
            .and_then(|r| chrono::Locale::try_from(r).ok())
            .or_else(|| chrono::Locale::try_from(default_region).ok())
            .unwrap_or(chrono::Locale::POSIX);
        let mut texts: HashMap<&'static str, &'static str> =
            en::CATALOGUE.iter().copied().collect();
        texts.extend(catalogue.iter().copied());
        log::info!("language {lang}, dates as {dates:?}");
        Self { lang, texts, dates }
    }

    /// The text for `key`; the key itself when no catalogue has it
    /// (`cargo test` makes sure that doesn't happen).
    pub fn tr(&self, key: &'static str) -> &'static str {
        self.texts.get(key).copied().unwrap_or_else(|| {
            log::error!("no text for {key:?}");
            key
        })
    }

    /// The text for `key` with its `{name}` placeholders filled.
    pub fn fmt(&self, key: &'static str, args: &[(&str, &dyn Display)]) -> String {
        let mut text = self.tr(key).to_owned();
        for (name, value) in args {
            text = text.replace(&format!("{{{name}}}"), &value.to_string());
        }
        text
    }

    /// `dt` formatted with a strftime pattern, the names of days and
    /// months in the language.
    pub fn date(&self, dt: &DateTime<Local>, fmt: &str) -> String {
        dt.format_localized(fmt, self.dates).to_string()
    }

    /// The same for a date alone.
    pub fn naive_date(&self, date: &NaiveDate, fmt: &str) -> String {
        date.format_localized(fmt, self.dates).to_string()
    }

    /// `lang=<code> dates=<locale>`, for `debug locale`.
    pub fn describe(&self) -> String {
        format!("lang={} dates={:?}", self.lang, self.dates)
    }
}

/// The language and region the environment asks for: `it_IT.UTF-8`
/// gives `("it", Some("it_IT"))`; `C`/`POSIX`/unset give English.
fn environment() -> (String, Option<String>) {
    let value = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|v| !v.is_empty());
    parse_locale(value.as_deref().unwrap_or(""))
}

fn parse_locale(value: &str) -> (String, Option<String>) {
    // `it_IT.UTF-8@euro` -> `it_IT`
    let region = value.split(['.', '@']).next().unwrap_or("");
    if region.is_empty() || region == "C" || region == "POSIX" {
        return (DEFAULT.to_owned(), None);
    }
    let lang = region.split('_').next().unwrap_or(region).to_lowercase();
    (lang, Some(region.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    #[test]
    fn environment_values() {
        assert_eq!(
            parse_locale("it_IT.UTF-8"),
            ("it".into(), Some("it_IT".into()))
        );
        assert_eq!(
            parse_locale("de_DE@euro"),
            ("de".into(), Some("de_DE".into()))
        );
        assert_eq!(parse_locale("C"), ("en".into(), None));
        assert_eq!(parse_locale("POSIX.UTF-8"), ("en".into(), None));
        assert_eq!(parse_locale(""), ("en".into(), None));
    }

    #[test]
    fn lookup_and_fallback() {
        let it = Locale::new("it");
        assert_eq!(it.tr("locker.unlock"), "Sblocca");
        assert_eq!(it.fmt("notifications.age.minutes", &[("n", &5)]), "5 min");
        assert_eq!(it.describe(), "lang=it dates=it_IT");
        let en = Locale::new("en");
        assert_eq!(en.tr("locker.unlock"), "Unlock");
        // An unknown language is English.
        assert_eq!(Locale::new("xx").tr("locker.unlock"), "Unlock");
        assert_eq!(en.tr("no.such.key"), "no.such.key");
    }

    #[test]
    fn dates_follow_the_language() {
        let day = Local::now()
            .with_timezone(&Local)
            .date_naive()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap();
        let en = Locale::new("en").date(&day, "%A");
        let it = Locale::new("it").date(&day, "%A");
        assert_ne!(en, it, "{en} vs {it}");
    }

    /// Every `.tr("key")` / `.fmt("key"` in the sources has an English
    /// text, every English key is quoted somewhere in the sources (a
    /// `match` arm returning it counts), and every other language has
    /// exactly English's keys. The sources are everything under `src/`
    /// but this module.
    #[test]
    fn catalogues_are_complete() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let sources: Vec<String> = sources(root)
            .into_iter()
            .filter(|p| !p.to_string_lossy().contains("/locale"))
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect();
        let mut used = BTreeSet::new();
        for text in &sources {
            for marker in [".tr(\"", ".fmt(\""] {
                for (i, _) in text.match_indices(marker) {
                    let rest = &text[i + marker.len()..];
                    let key = &rest[..rest.find('"').unwrap()];
                    used.insert(key.to_owned());
                }
            }
        }
        let keys = |c: Catalogue| {
            c.iter()
                .map(|(k, _)| k.to_string())
                .collect::<BTreeSet<_>>()
        };
        let english = keys(en::CATALOGUE);
        let missing: Vec<_> = used.difference(&english).collect();
        assert!(
            missing.is_empty(),
            "used in the code, not in en: {missing:?}"
        );
        let unused: Vec<_> = english
            .iter()
            .filter(|k| !sources.iter().any(|t| t.contains(&format!("\"{k}\""))))
            .collect();
        assert!(unused.is_empty(), "in en, used nowhere: {unused:?}");
        for (lang, _, catalogue) in CATALOGUES {
            let set = keys(catalogue);
            assert_eq!(set.len(), catalogue.len(), "{lang}: duplicate keys");
            assert!(
                catalogue.iter().all(|(_, v)| !v.is_empty()),
                "{lang}: empty texts"
            );
            let missing: Vec<_> = english.difference(&set).collect();
            let extra: Vec<_> = set.difference(&english).collect();
            assert!(missing.is_empty(), "{lang} lacks: {missing:?}");
            assert!(extra.is_empty(), "{lang} has keys en doesn't: {extra:?}");
        }
    }

    fn sources(dir: std::path::PathBuf) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                out.extend(sources(path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
        out
    }
}
