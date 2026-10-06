//! What becomes of the captured buffers: each output's pixels turned
//! upright as RGBA, then the asked rectangle of the global space cut
//! out of them, across outputs and scales, and encoded as PNG.
//! No Wayland here, only pixels: testable as it is.

use std::sync::Arc;

use image::imageops::{self, FilterType};
use image::{ImageEncoder, RgbaImage};
use wayland_client::protocol::wl_output::Transform;

/// A rectangle in the global logical space, or in an image's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        (right > x && bottom > y).then(|| Rect::new(x, y, right - x, bottom - y))
    }

    /// The smallest rectangle holding both.
    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Rect::new(x, y, right - x, bottom - y)
    }
}

impl std::fmt::Display for Rect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{},{} {}x{}", self.x, self.y, self.width, self.height)
    }
}

/// The shm formats we read: 8 bits per channel, little-endian words
/// (`Xrgb` is B, G, R, X in memory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Xrgb,
    Argb,
    Xbgr,
    Abgr,
}

/// One output's buffer as the compositor filled it.
#[derive(Debug)]
pub struct RawFrame {
    /// The output's global name.
    pub output: u32,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    /// What the compositor applied to the contents (the output's own).
    pub transform: Transform,
    /// `width * 4` bytes per row.
    pub data: Vec<u8>,
}

pub type Frames = Arc<Vec<RawFrame>>;

/// An output's picture, upright, and where the output is: its scale is
/// the image's pixels over the rectangle's.
pub struct Shot {
    /// The output's global name.
    pub output: u32,
    pub rect: Rect,
    pub image: RgbaImage,
}

impl Shot {
    fn scale(&self) -> f64 {
        self.image.width() as f64 / self.rect.width as f64
    }
}

/// The frame as RGBA, the right way up.
pub fn upright(frame: &RawFrame) -> RgbaImage {
    let mut data = frame.data.clone();
    for px in data.chunks_exact_mut(4) {
        match frame.format {
            Format::Xrgb | Format::Argb => px.swap(0, 2),
            Format::Xbgr | Format::Abgr => {}
        }
        if matches!(frame.format, Format::Xrgb | Format::Xbgr) {
            px[3] = 255;
        }
    }
    let image = RgbaImage::from_raw(frame.width, frame.height, data)
        .expect("a frame holds width * height * 4 bytes");
    untransform(image, frame.transform)
}

/// Undo what the compositor applied for the output's transform.
fn untransform(image: RgbaImage, transform: Transform) -> RgbaImage {
    match transform {
        Transform::_90 => imageops::rotate90(&image),
        Transform::_180 => imageops::rotate180(&image),
        Transform::_270 => imageops::rotate270(&image),
        Transform::Flipped => imageops::flip_horizontal(&image),
        Transform::Flipped90 => imageops::rotate90(&imageops::flip_horizontal(&image)),
        Transform::Flipped180 => imageops::flip_vertical(&image),
        Transform::Flipped270 => imageops::rotate270(&imageops::flip_horizontal(&image)),
        _ => image,
    }
}

/// `rect` of the global space, out of the shots it touches, at the
/// largest scale among them (a smaller one is stretched to it); `None`
/// when it touches none.
pub fn compose(shots: &[Shot], rect: Rect) -> Option<RgbaImage> {
    let parts: Vec<(&Shot, Rect)> = shots
        .iter()
        .filter_map(|s| Some((s, s.rect.intersection(&rect)?)))
        .collect();
    let scale = parts.iter().map(|(s, _)| s.scale()).reduce(f64::max)?;
    let px = |v: i32, origin: i32, scale: f64| ((v - origin) as f64 * scale).round() as i64;
    let mut out = RgbaImage::new(
        px(rect.width, 0, scale) as u32,
        px(rect.height, 0, scale) as u32,
    );
    for (shot, part) in parts {
        let s = shot.scale();
        let (sx0, sy0) = (px(part.x, shot.rect.x, s), px(part.y, shot.rect.y, s));
        let (sx1, sy1) = (
            px(part.x + part.width, shot.rect.x, s),
            px(part.y + part.height, shot.rect.y, s),
        );
        let (dx0, dy0) = (px(part.x, rect.x, scale), px(part.y, rect.y, scale));
        let (dx1, dy1) = (
            px(part.x + part.width, rect.x, scale),
            px(part.y + part.height, rect.y, scale),
        );
        let piece = imageops::crop_imm(
            &shot.image,
            sx0 as u32,
            sy0 as u32,
            (sx1 - sx0) as u32,
            (sy1 - sy0) as u32,
        )
        .to_image();
        let (w, h) = ((dx1 - dx0) as u32, (dy1 - dy0) as u32);
        let piece = if piece.dimensions() == (w, h) {
            piece
        } else {
            imageops::resize(&piece, w, h, FilterType::Triangle)
        };
        imageops::replace(&mut out, &piece, dx0, dy0);
    }
    Some(out)
}

pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn frame(width: u32, height: u32, format: Format, px: [u8; 4]) -> RawFrame {
        RawFrame {
            output: 1,
            width,
            height,
            format,
            transform: Transform::Normal,
            data: px.repeat((width * height) as usize),
        }
    }

    fn solid(rect: Rect, scale: u32, color: [u8; 4]) -> Shot {
        Shot {
            output: 1,
            rect,
            image: RgbaImage::from_pixel(
                rect.width as u32 * scale,
                rect.height as u32 * scale,
                Rgba(color),
            ),
        }
    }

    #[test]
    fn formats_become_rgba() {
        let xrgb = upright(&frame(1, 1, Format::Xrgb, [3, 2, 1, 0]));
        assert_eq!(xrgb.get_pixel(0, 0), &Rgba([1, 2, 3, 255]));
        let argb = upright(&frame(1, 1, Format::Argb, [3, 2, 1, 7]));
        assert_eq!(argb.get_pixel(0, 0), &Rgba([1, 2, 3, 7]));
        let xbgr = upright(&frame(1, 1, Format::Xbgr, [1, 2, 3, 0]));
        assert_eq!(xbgr.get_pixel(0, 0), &Rgba([1, 2, 3, 255]));
    }

    #[test]
    fn rotated_outputs_come_back_upright() {
        let mut f = frame(4, 2, Format::Abgr, [0, 0, 0, 255]);
        f.transform = Transform::_90;
        assert_eq!(upright(&f).dimensions(), (2, 4));
        f.transform = Transform::_180;
        assert_eq!(upright(&f).dimensions(), (4, 2));
    }

    #[test]
    fn rect_algebra() {
        let a = Rect::new(0, 0, 100, 50);
        let b = Rect::new(80, 40, 100, 50);
        assert_eq!(a.intersection(&b), Some(Rect::new(80, 40, 20, 10)));
        assert_eq!(a.intersection(&Rect::new(100, 0, 10, 10)), None);
        assert_eq!(a.union(&b), Rect::new(0, 0, 180, 90));
    }

    #[test]
    fn a_region_of_one_output_at_its_scale() {
        let shots = [solid(Rect::new(0, 0, 100, 50), 2, [9, 9, 9, 255])];
        let out = compose(&shots, Rect::new(10, 10, 30, 20)).unwrap();
        assert_eq!(out.dimensions(), (60, 40));
        assert_eq!(compose(&shots, Rect::new(200, 0, 10, 10)), None);
    }

    #[test]
    fn across_outputs_at_the_largest_scale() {
        // a 1x output left of a 2x one
        let shots = [
            solid(Rect::new(0, 0, 100, 50), 1, [255, 0, 0, 255]),
            solid(Rect::new(100, 0, 100, 50), 2, [0, 0, 255, 255]),
        ];
        let all = shots[0].rect.union(&shots[1].rect);
        let out = compose(&shots, all).unwrap();
        assert_eq!(out.dimensions(), (400, 100));
        assert_eq!(out.get_pixel(10, 10), &Rgba([255, 0, 0, 255]));
        assert_eq!(out.get_pixel(390, 90), &Rgba([0, 0, 255, 255]));
        // the seam is where the outputs meet
        assert_eq!(out.get_pixel(199, 50), &Rgba([255, 0, 0, 255]));
        assert_eq!(out.get_pixel(200, 50), &Rgba([0, 0, 255, 255]));
    }

    #[test]
    fn outputs_apart_leave_the_gap_transparent() {
        let shots = [
            solid(Rect::new(0, 0, 10, 10), 1, [255, 0, 0, 255]),
            solid(Rect::new(10, 5, 10, 10), 1, [0, 0, 255, 255]),
        ];
        let out = compose(&shots, shots[0].rect.union(&shots[1].rect)).unwrap();
        assert_eq!(out.dimensions(), (20, 15));
        assert_eq!(out.get_pixel(15, 0), &Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn png_roundtrip() {
        let img = RgbaImage::from_pixel(3, 2, Rgba([1, 2, 3, 255]));
        let png = encode_png(&img).unwrap();
        let back = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(back, img);
    }
}
