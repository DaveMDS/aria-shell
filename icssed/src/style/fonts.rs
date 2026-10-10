//! Fonts: a `font-family` list down to the family to use, as iced's
//! `Font`, and the metrics of the first available font (what CSS's
//! `normal` line height and `text-box` edges are measured on).

use std::sync::{Arc, Mutex};

use iced::Font;
use iced::advanced::graphics::text::cosmic_text::fontdb;
use iced::advanced::graphics::text::font_system;
use iced::font::{Family, Style, Weight};
use rustc_hash::FxHashMap;

use super::value::{FamilyName, FontStyle};

/// A font's vertical metrics, in ems.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Metrics {
    pub ascent: f32,
    /// Below the baseline, positive.
    pub descent: f32,
    pub leading: f32,
    pub cap_height: f32,
    pub x_height: f32,
}

impl Metrics {
    /// What a font without them is taken to have.
    const FALLBACK: Self = Self {
        ascent: 0.9,
        descent: 0.25,
        leading: 0.0,
        cap_height: 0.7,
        x_height: 0.5,
    };
}

/// The first family of `names` that's generic or installed (else the
/// first one: the font system falls back on its own), with `weight` and
/// `style`. Only asks the font database when there's a choice to make.
pub(crate) fn font(names: &Arc<[FamilyName]>, weight: u16, style: FontStyle) -> Font {
    let picked = match &names[..] {
        [one] => one,
        _ => names
            .iter()
            .find(|n| !matches!(n, FamilyName::Named(name) if !installed(name)))
            .unwrap_or_else(|| {
                log::warn!("none of the fonts {names:?} is installed");
                &names[0]
            }),
    };
    Font {
        family: match picked {
            FamilyName::Named(name) => Family::Name(intern(name)),
            FamilyName::Serif => Family::Serif,
            FamilyName::SansSerif => Family::SansSerif,
            FamilyName::Monospace => Family::Monospace,
            FamilyName::Cursive => Family::Cursive,
            FamilyName::Fantasy => Family::Fantasy,
        },
        weight: match weight {
            ..150 => Weight::Thin,
            150..250 => Weight::ExtraLight,
            250..350 => Weight::Light,
            350..450 => Weight::Normal,
            450..550 => Weight::Medium,
            550..650 => Weight::Semibold,
            650..750 => Weight::Bold,
            750..850 => Weight::ExtraBold,
            _ => Weight::Black,
        },
        style: match style {
            FontStyle::Normal => Style::Normal,
            FontStyle::Italic => Style::Italic,
            FontStyle::Oblique => Style::Oblique,
        },
        ..Font::DEFAULT
    }
}

/// The metrics of the face the font database gives for `font`.
pub(crate) fn metrics(font: Font) -> Metrics {
    static CACHE: Mutex<Option<FxHashMap<Font, Metrics>>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(FxHashMap::default);
    if let Some(m) = cache.get(&font) {
        return *m;
    }
    let m = lookup(font).unwrap_or(Metrics::FALLBACK);
    cache.insert(font, m);
    m
}

fn lookup(font: Font) -> Option<Metrics> {
    let mut system = font_system().write().unwrap_or_else(|e| e.into_inner());
    let system = system.raw();
    let family = match font.family {
        Family::Name(name) => fontdb::Family::Name(name),
        Family::Serif => fontdb::Family::Serif,
        Family::SansSerif => fontdb::Family::SansSerif,
        Family::Monospace => fontdb::Family::Monospace,
        Family::Cursive => fontdb::Family::Cursive,
        Family::Fantasy => fontdb::Family::Fantasy,
    };
    let weight = fontdb::Weight(font_weight(font.weight));
    let id = system.db().query(&fontdb::Query {
        families: &[family],
        weight,
        stretch: fontdb::Stretch::Normal,
        style: match font.style {
            Style::Normal => fontdb::Style::Normal,
            Style::Italic => fontdb::Style::Italic,
            Style::Oblique => fontdb::Style::Oblique,
        },
    })?;
    let face = system.get_font(id, weight)?;
    let m = face.metrics();
    let em = f32::from(m.units_per_em.max(1));
    Some(Metrics {
        ascent: m.ascent / em,
        descent: -m.descent / em,
        leading: m.leading / em,
        cap_height: m
            .cap_height
            .map_or(Metrics::FALLBACK.cap_height, |c| c / em),
        x_height: m.x_height.map_or(Metrics::FALLBACK.x_height, |x| x / em),
    })
}

fn font_weight(weight: Weight) -> u16 {
    match weight {
        Weight::Thin => 100,
        Weight::ExtraLight => 200,
        Weight::Light => 300,
        Weight::Normal => 400,
        Weight::Medium => 500,
        Weight::Semibold => 600,
        Weight::Bold => 700,
        Weight::ExtraBold => 800,
        Weight::Black => 900,
    }
}

/// Is a font family with this name installed?
fn installed(name: &str) -> bool {
    let mut system = font_system().write().unwrap_or_else(|e| e.into_inner());
    system
        .raw()
        .db()
        .faces()
        .any(|face| face.families.iter().any(|(family, _)| family == name))
}

/// iced's `Family::Name` wants a `&'static str`: a name is leaked once,
/// and reused by every sheet after.
fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(n) = names.iter().find(|n| **n == name) {
        return n;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.push(leaked);
    leaked
}
