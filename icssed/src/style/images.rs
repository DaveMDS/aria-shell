//! The images a sheet names (`url()`), loaded with it: a raster one
//! decoded (iced draws whole images, so a 9-patch is cut here, once per
//! slicing), an svg one as iced's handle.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use iced::advanced::{image, svg};
use rustc_hash::FxHashMap;

/// The 9-patches cut from a picture, by slicing and `fill`.
type Nines = Mutex<FxHashMap<([u32; 4], bool), Arc<Nine>>>;

/// An image of the sheet. Equal when it's the same load.
#[derive(Clone)]
pub struct Picture(Arc<Data>);

enum Data {
    Raster {
        handle: image::Handle,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        /// The 9-patches cut from it so far, by slicing.
        nines: Nines,
    },
    Svg(svg::Handle),
}

/// What a picture draws with.
#[derive(Debug, Clone, PartialEq)]
pub enum Drawn {
    Raster {
        handle: image::Handle,
        width: u32,
        height: u32,
    },
    /// Its size is the renderer's to measure.
    Svg(svg::Handle),
}

/// A raster picture cut in nine, row by row: the corners, the edges,
/// the middle (only with `fill`).
#[derive(Debug)]
pub struct Nine {
    pub parts: [Option<image::Handle>; 9],
    /// The slices, in image pixels: top, right, bottom, left.
    pub slices: [u32; 4],
}

impl Picture {
    /// The image at `path`: an svg by its name, else png, jpeg or webp.
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let shown = path.display();
        let svg = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
        if svg {
            if !path.is_file() {
                return Err(format!("`{shown}` isn't there"));
            }
            return Ok(Self(Arc::new(Data::Svg(svg::Handle::from_path(path)))));
        }
        let decoded = ::image::ImageReader::open(path)
            .map_err(|e| format!("`{shown}`: {e}"))?
            .with_guessed_format()
            .map_err(|e| format!("`{shown}`: {e}"))?
            .decode()
            .map_err(|e| format!("`{shown}`: {e}"))?
            .into_rgba8();
        let (width, height) = decoded.dimensions();
        let rgba = decoded.into_raw();
        Ok(Self(Arc::new(Data::Raster {
            handle: image::Handle::from_rgba(width, height, rgba.clone()),
            width,
            height,
            rgba,
            nines: Mutex::new(FxHashMap::default()),
        })))
    }

    pub fn drawn(&self) -> Drawn {
        match &*self.0 {
            Data::Raster {
                handle,
                width,
                height,
                ..
            } => Drawn::Raster {
                handle: handle.clone(),
                width: *width,
                height: *height,
            },
            Data::Svg(handle) => Drawn::Svg(handle.clone()),
        }
    }

    /// Cut in nine at `slices` (top, right, bottom, left, in pixels), the
    /// middle kept with `fill`. `None` for an svg.
    pub(crate) fn nine(&self, slices: [u32; 4], fill: bool) -> Option<Arc<Nine>> {
        let Data::Raster {
            width,
            height,
            rgba,
            nines,
            ..
        } = &*self.0
        else {
            return None;
        };
        let mut nines = nines.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(nine) = nines.get(&(slices, fill)) {
            return Some(nine.clone());
        }
        let (w, h) = (*width, *height);
        let [top, right, bottom, left] = slices;
        let (top, bottom) = (top.min(h), bottom.min(h.saturating_sub(top.min(h))));
        let (left, right) = (left.min(w), right.min(w.saturating_sub(left.min(w))));
        let columns = [(0, left), (left, w - left - right), (w - right, right)];
        let rows = [(0, top), (top, h - top - bottom), (h - bottom, bottom)];
        let mut parts: [Option<image::Handle>; 9] = Default::default();
        for (r, &(y, ph)) in rows.iter().enumerate() {
            for (c, &(x, pw)) in columns.iter().enumerate() {
                if pw == 0 || ph == 0 || (r == 1 && c == 1 && !fill) {
                    continue;
                }
                let mut part = Vec::with_capacity((pw * ph * 4) as usize);
                for row in y..y + ph {
                    let at = ((row * w + x) * 4) as usize;
                    part.extend_from_slice(&rgba[at..at + (pw * 4) as usize]);
                }
                parts[r * 3 + c] = Some(image::Handle::from_rgba(pw, ph, part));
            }
        }
        let nine = Arc::new(Nine {
            parts,
            slices: [top, right, bottom, left],
        });
        nines.insert((slices, fill), nine.clone());
        Some(nine)
    }
}

impl PartialEq for Picture {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for Picture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &*self.0 {
            Data::Raster { width, height, .. } => write!(f, "Picture({width}x{height})"),
            Data::Svg(_) => write!(f, "Picture(svg)"),
        }
    }
}

impl PartialEq for Nine {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
