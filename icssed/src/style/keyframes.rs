//! `@keyframes`: an animation's steps, each the paint it sets. What a
//! step leaves out comes from the steps around it that set it, or from
//! the element's own paint at the ends, as CSS's implicit keyframes.

use std::fmt;
use std::sync::Arc;

use super::computed::Shadow;
use super::value::{ColorValue, Decl, Transform};
use crate::anim::{NO_SHADOW, Paint};

pub struct Keyframes {
    pub name: Arc<str>,
    /// By offset.
    pub(crate) frames: Vec<Frame>,
    /// What any of them sets, as `Moving` bits.
    pub(crate) sets: u16,
}

pub(crate) struct Frame {
    /// 0 to 1.
    pub offset: f32,
    pub decls: Arc<[Decl]>,
    pub sets: u16,
}

impl Frame {
    /// `base` with what this step sets.
    pub fn paint(&self, base: &Paint) -> Paint {
        let mut paint = *base;
        for decl in self.decls.iter() {
            let current = paint.color;
            let resolve = |c: &ColorValue| match c {
                ColorValue::Rgba(c) => *c,
                ColorValue::Current => current,
            };
            match decl {
                Decl::Color(ColorValue::Rgba(c)) => paint.color = *c,
                Decl::Background(c) => paint.background = resolve(c),
                Decl::BorderColor(side, c) => paint.border_color[*side as usize] = resolve(c),
                Decl::OutlineColor(c) => paint.outline_color = resolve(c),
                Decl::Opacity(v) => paint.opacity = *v,
                Decl::BoxShadow(shadows) => {
                    paint.shadow = shadows.outer.map_or(NO_SHADOW, |s| Shadow {
                        x: s.x,
                        y: s.y,
                        blur: s.blur,
                        spread: s.spread,
                        color: resolve(&s.color),
                    })
                }
                Decl::Transform(t) => paint.transform = t.unwrap_or(Transform::IDENTITY),
                Decl::Rotate(degrees) => paint.rotate = *degrees,
                _ => {}
            }
        }
        paint
    }
}

impl PartialEq for Keyframes {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl fmt::Debug for Keyframes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Keyframes({}, {} steps)", self.name, self.frames.len())
    }
}
