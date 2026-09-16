//! Typed property values. `parse(name, value)` turns one declaration
//! (after variable substitution) into [`Property`] values, or an error
//! for the user. This is where a theme gets validated, once, at load.
//!
//! Units: `px` or none. Colors: anything CSS accepts (`#rgb`, `#rrggbbaa`,
//! `rgb()`, `rgba()`, `hsl()`, names, `transparent`).

use iced::border::Radius;
use iced::font::Weight;
use iced::{Color, Padding, Shadow, Vector};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Px(f32),
    Auto,
    Fill,
}

impl From<Length> for iced::Length {
    fn from(l: Length) -> Self {
        match l {
            Length::Px(px) => iced::Length::Fixed(px),
            Length::Auto => iced::Length::Shrink,
            Length::Fill => iced::Length::Fill,
        }
    }
}

/// One property, typed. Shorthands (`border`) expand to several.
#[derive(Debug, Clone, PartialEq)]
pub enum Property {
    Color(Color),
    /// `None` for `none` / `transparent`.
    Background(Option<Color>),
    BorderWidth(f32),
    BorderColor(Color),
    BorderRadius(Radius),
    Shadow(Option<Shadow>),
    Padding(Padding),
    Gap(f32),
    Width(Length),
    Height(Length),
    MinHeight(f32),
    FontFamily(String),
    FontSize(f32),
    FontWeight(Weight),
}

pub fn parse(name: &str, value: &str) -> Result<Vec<Property>, String> {
    let words = split_words(value);
    let one = |f: fn(&str) -> Result<Property, String>| -> Result<Vec<Property>, String> {
        match words.as_slice() {
            [w] => Ok(vec![f(w)?]),
            _ => Err(format!("{name} takes a single value, got {value:?}")),
        }
    };
    match name {
        "color" => one(|w| color(w).map(Property::Color)),
        "background" | "background-color" => one(|w| background(w).map(Property::Background)),
        "border-width" => one(|w| px(w).map(Property::BorderWidth)),
        "border-color" => one(|w| color(w).map(Property::BorderColor)),
        "border-radius" => {
            let [tl, tr, br, bl] = sides(&words)?;
            Ok(vec![Property::BorderRadius(Radius {
                top_left: tl,
                top_right: tr,
                bottom_right: br,
                bottom_left: bl,
            })])
        }
        "border" => border(&words),
        "box-shadow" => shadow(&words).map(|s| vec![Property::Shadow(s)]),
        "padding" => {
            let [top, right, bottom, left] = sides(&words)?;
            Ok(vec![Property::Padding(Padding {
                top,
                right,
                bottom,
                left,
            })])
        }
        "gap" => one(|w| px(w).map(Property::Gap)),
        "width" => one(|w| length(w).map(Property::Width)),
        "height" => one(|w| length(w).map(Property::Height)),
        "min-height" => one(|w| px(w).map(Property::MinHeight)),
        "font-family" => font_family(value).map(|f| vec![Property::FontFamily(f)]),
        "font-size" => one(|w| px(w).map(Property::FontSize)),
        "font-weight" => one(|w| weight(w).map(Property::FontWeight)),
        _ => Err(format!("unknown property {name}")),
    }
}

/// Whitespace-separated words, keeping `f(a, b)` together.
fn split_words(value: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut depth = 0;
    let mut start = None;
    for (i, c) in value.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                words.push(&value[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        words.push(&value[s..]);
    }
    words
}

fn color(word: &str) -> Result<Color, String> {
    csscolorparser::parse(word)
        .map(|c| Color::from_rgba(c.r, c.g, c.b, c.a))
        .map_err(|_| format!("invalid color {word:?}"))
}

fn background(word: &str) -> Result<Option<Color>, String> {
    match word {
        "none" | "transparent" => Ok(None),
        _ => color(word).map(Some),
    }
}

/// A pixel size: `12px`, `12`, `1.5`, `-3px`.
fn px(word: &str) -> Result<f32, String> {
    let digits = word.strip_suffix("px").unwrap_or(word);
    digits
        .parse()
        .map_err(|_| format!("invalid size {word:?}, expected pixels"))
}

fn length(word: &str) -> Result<Length, String> {
    match word {
        "auto" => Ok(Length::Auto),
        "fill" => Ok(Length::Fill),
        _ => px(word).map(Length::Px),
    }
}

/// CSS 1-to-4 value order: top, right, bottom, left.
fn sides(words: &[&str]) -> Result<[f32; 4], String> {
    let v: Vec<f32> = words.iter().map(|w| px(w)).collect::<Result<_, _>>()?;
    Ok(match v[..] {
        [a] => [a, a, a, a],
        [a, b] => [a, b, a, b],
        [a, b, c] => [a, b, c, b],
        [a, b, c, d] => [a, b, c, d],
        _ => return Err("expected 1 to 4 sizes".to_owned()),
    })
}

/// `<width> [solid] <color>` in any order, or `none`.
fn border(words: &[&str]) -> Result<Vec<Property>, String> {
    if words == ["none"] {
        return Ok(vec![Property::BorderWidth(0.0)]);
    }
    let mut props = Vec::new();
    for w in words {
        if *w == "solid" {
            continue;
        }
        if let Ok(width) = px(w) {
            props.push(Property::BorderWidth(width));
        } else {
            props.push(Property::BorderColor(color(w)?));
        }
    }
    if props.is_empty() {
        return Err("expected `<width> solid <color>` or `none`".to_owned());
    }
    Ok(props)
}

/// `<x> <y> [blur] <color>`, or `none`.
fn shadow(words: &[&str]) -> Result<Option<Shadow>, String> {
    if words == ["none"] {
        return Ok(None);
    }
    let (x, y, blur, c) = match words {
        [x, y, c] => (px(x)?, px(y)?, 0.0, color(c)?),
        [x, y, blur, c] => (px(x)?, px(y)?, px(blur)?, color(c)?),
        _ => return Err("expected `<x> <y> [blur] <color>` or `none`".to_owned()),
    };
    Ok(Some(Shadow {
        color: c,
        offset: Vector::new(x, y),
        blur_radius: blur,
    }))
}

/// The first family of a comma list, unquoted. iced has no fallback
/// list, the font system falls back on its own.
fn font_family(value: &str) -> Result<String, String> {
    let first = value.split(',').next().unwrap_or_default().trim();
    let name = first
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| first.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(first);
    if name.is_empty() {
        return Err("empty font-family".to_owned());
    }
    Ok(name.to_owned())
}

fn weight(word: &str) -> Result<Weight, String> {
    Ok(match word {
        "normal" | "400" => Weight::Normal,
        "bold" | "700" => Weight::Bold,
        "100" => Weight::Thin,
        "200" => Weight::ExtraLight,
        "300" => Weight::Light,
        "500" => Weight::Medium,
        "600" => Weight::Semibold,
        "800" => Weight::ExtraBold,
        "900" => Weight::Black,
        _ => return Err(format!("invalid font-weight {word:?}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(name: &str, value: &str) -> Property {
        let mut props = parse(name, value).unwrap_or_else(|e| panic!("{name}: {value}: {e}"));
        assert_eq!(props.len(), 1);
        props.pop().unwrap()
    }

    #[test]
    fn colors() {
        assert_eq!(
            one("color", "#ff0000"),
            Property::Color(Color::from_rgb(1.0, 0.0, 0.0))
        );
        assert_eq!(
            one("color", "rgba(0, 255, 0, 0.5)"),
            Property::Color(Color::from_rgba(0.0, 1.0, 0.0, 0.5))
        );
        assert_eq!(one("color", "white"), Property::Color(Color::WHITE));
        assert_eq!(one("background", "none"), Property::Background(None));
        assert_eq!(
            one("background-color", "transparent"),
            Property::Background(None)
        );
        assert_eq!(
            one("background", "#0008"),
            Property::Background(Some(Color::from_rgba8(0, 0, 0, 0.53333336)))
        );
        assert!(parse("color", "notacolor").is_err());
        assert!(parse("color", "red blue").is_err());
    }

    #[test]
    fn sizes() {
        assert_eq!(one("gap", "4px"), Property::Gap(4.0));
        assert_eq!(one("font-size", "13"), Property::FontSize(13.0));
        assert_eq!(one("min-height", "32px"), Property::MinHeight(32.0));
        assert_eq!(one("width", "fill"), Property::Width(Length::Fill));
        assert_eq!(one("height", "auto"), Property::Height(Length::Auto));
        assert_eq!(one("height", "20px"), Property::Height(Length::Px(20.0)));
        assert!(parse("gap", "1em").is_err());
        assert!(parse("gap", "50%").is_err());
    }

    #[test]
    fn padding_and_radius() {
        assert_eq!(one("padding", "4"), Property::Padding(Padding::new(4.0)));
        assert_eq!(
            one("padding", "1px 2px"),
            Property::Padding(Padding {
                top: 1.0,
                right: 2.0,
                bottom: 1.0,
                left: 2.0
            })
        );
        assert_eq!(
            one("padding", "1 2 3"),
            Property::Padding(Padding {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 2.0
            })
        );
        assert_eq!(
            one("border-radius", "1 2 3 4"),
            Property::BorderRadius(Radius {
                top_left: 1.0,
                top_right: 2.0,
                bottom_right: 3.0,
                bottom_left: 4.0
            })
        );
        assert!(parse("padding", "1 2 3 4 5").is_err());
    }

    #[test]
    fn border_shorthand() {
        assert_eq!(
            parse("border", "1px solid red").unwrap(),
            [
                Property::BorderWidth(1.0),
                Property::BorderColor(Color::from_rgb(1.0, 0.0, 0.0))
            ]
        );
        assert_eq!(
            parse("border", "2px").unwrap(),
            [Property::BorderWidth(2.0)]
        );
        assert_eq!(
            parse("border", "none").unwrap(),
            [Property::BorderWidth(0.0)]
        );
        assert!(parse("border", "1px dotted").is_err());
    }

    #[test]
    fn shadows() {
        assert_eq!(one("box-shadow", "none"), Property::Shadow(None));
        assert_eq!(
            one("box-shadow", "0 2px 6px rgba(0,0,0,0.5)"),
            Property::Shadow(Some(Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 6.0
            }))
        );
        assert_eq!(
            one("box-shadow", "-1 -1 black"),
            Property::Shadow(Some(Shadow {
                color: Color::BLACK,
                offset: Vector::new(-1.0, -1.0),
                blur_radius: 0.0
            }))
        );
        assert!(parse("box-shadow", "1 2").is_err());
    }

    #[test]
    fn fonts() {
        assert_eq!(
            one("font-family", "\"Fira Code\", monospace"),
            Property::FontFamily("Fira Code".to_owned())
        );
        assert_eq!(
            one("font-family", "monospace"),
            Property::FontFamily("monospace".to_owned())
        );
        assert_eq!(
            one("font-weight", "bold"),
            Property::FontWeight(Weight::Bold)
        );
        assert_eq!(
            one("font-weight", "300"),
            Property::FontWeight(Weight::Light)
        );
        assert!(parse("font-weight", "450").is_err());
    }

    #[test]
    fn unknown_property() {
        assert!(parse("margin", "1").unwrap_err().contains("unknown"));
    }
}
