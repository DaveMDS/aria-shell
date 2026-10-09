//! Font families: a theme's `font-family` list down to the one to use
//! (the first generic or installed one), as iced's `Family`; and the
//! fonts built into the shell.

use std::borrow::Cow;
use std::sync::Mutex;

use iced::font::Family;

/// The fonts built into the binary, loaded at start next to the
/// system's: `base.css` names them, so the text looks the same on every
/// system, whatever is installed (`assets/fonts/`, with their licenses).
pub fn embedded() -> Vec<Cow<'static, [u8]>> {
    [
        include_bytes!("../../../assets/fonts/NotoSansDisplay-Regular.ttf").as_slice(),
        include_bytes!("../../../assets/fonts/NotoSansDisplay-Medium.ttf").as_slice(),
        include_bytes!("../../../assets/fonts/NotoSansDisplay-Bold.ttf").as_slice(),
    ]
    .into_iter()
    .map(Cow::Borrowed)
    .collect()
}

const GENERIC_FAMILIES: [&str; 5] = ["serif", "sans-serif", "monospace", "cursive", "fantasy"];

/// The first of `names` that is a generic family or an installed font,
/// else the first one (and the font system falls back on its own).
/// Only asks the font database when there's a choice to make.
pub(super) fn pick_family(names: &[String]) -> &str {
    if names.len() == 1 {
        return &names[0];
    }
    let picked = names
        .iter()
        .find(|n| GENERIC_FAMILIES.contains(&n.as_str()) || font_installed(n));
    match picked {
        Some(n) => n,
        None => {
            log::warn!("none of the fonts {names:?} is installed");
            &names[0]
        }
    }
}

/// Is a font family with this name installed? Uses the renderer's font
/// database (it loads the system fonts on first use, which iced would
/// do at the first text draw anyway).
fn font_installed(name: &str) -> bool {
    let mut system = iced::advanced::graphics::text::font_system()
        .write()
        .unwrap_or_else(|e| e.into_inner());
    system
        .raw()
        .db()
        .faces()
        .any(|face| face.families.iter().any(|(family, _)| family == name))
}

/// CSS generic families map to iced's; anything else is a font name.
pub(super) fn family(name: &'static str) -> Family {
    match name {
        "serif" => Family::Serif,
        "sans-serif" => Family::SansSerif,
        "monospace" => Family::Monospace,
        "cursive" => Family::Cursive,
        "fantasy" => Family::Fantasy,
        _ => Family::Name(name),
    }
}

/// `iced::font::Family::Name` wants a `&'static str`; theme font names
/// are leaked once each and reused across reloads.
pub(super) fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(n) = names.iter().find(|n| **n == name) {
        return n;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.push(leaked);
    leaked
}
