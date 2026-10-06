//! Icon theme lookup (freedesktop Icon Theme spec), backed by an
//! in-memory index: every directory `index.theme` lists is read once,
//! then a lookup is a hash probe plus picking the best-sized directory,
//! no `stat` storms. The theme chain (`Inherits`, then `hicolor`) and
//! `pixmaps/` as a last resort.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// One `[subdir]` of an `index.theme`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct IconDir {
    rel: String,
    size: u32,
    scale: u32,
    kind: DirKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirKind {
    Fixed,
    Scalable { min: u32, max: u32 },
    Threshold(u32),
}

impl IconDir {
    /// Spec: does an icon in this directory match `size` exactly?
    fn matches(&self, size: u32) -> bool {
        match self.kind {
            DirKind::Fixed => self.size == size,
            DirKind::Scalable { min, max } => (min..=max).contains(&size),
            DirKind::Threshold(t) => self.size.abs_diff(size) <= t,
        }
    }

    /// Spec: how far off is this directory for `size`?
    fn distance(&self, size: u32) -> u32 {
        match self.kind {
            DirKind::Fixed => self.size.abs_diff(size),
            DirKind::Scalable { min, max } => min.saturating_sub(size) + size.saturating_sub(max),
            DirKind::Threshold(t) => {
                let (lo, hi) = (self.size.saturating_sub(t), self.size + t);
                lo.saturating_sub(size) + size.saturating_sub(hi)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ext {
    Svg,
    Png,
}

impl Ext {
    fn of(name: &str) -> Option<(&str, Ext)> {
        let (stem, ext) = name.rsplit_once('.')?;
        let ext = match ext {
            "svg" => Ext::Svg,
            "png" => Ext::Png,
            _ => return None, // xpm and friends: iced can't decode them
        };
        Some((stem, ext))
    }
}

/// One theme, possibly spread over several base dirs
/// (`/usr/share/icons/hicolor` and `~/.local/share/icons/hicolor`).
#[derive(Debug)]
struct IconTheme {
    name: String,
    inherits: Vec<String>,
    dirs: Vec<IconDir>,
    /// `<base>/<name>` for every base dir where the theme exists.
    roots: Vec<PathBuf>,
    /// Icon name -> where it is: `(root index, dir index, ext)`.
    icons: HashMap<Box<str>, Vec<(u16, u16, Ext)>>,
}

impl IconTheme {
    /// `None` if no base dir has an `index.theme` for it.
    fn load(name: &str, bases: &[PathBuf]) -> Option<Self> {
        let roots: Vec<PathBuf> = bases
            .iter()
            .map(|b| b.join(name))
            .filter(|r| r.is_dir())
            .collect();
        let index = roots
            .iter()
            .find_map(|r| fs::read_to_string(r.join("index.theme")).ok())?;
        let (inherits, dirs) = parse_index(&index);
        let mut theme = Self {
            name: name.to_owned(),
            inherits,
            dirs,
            roots,
            icons: HashMap::new(),
        };
        theme.scan();
        Some(theme)
    }

    fn scan(&mut self) {
        for (ri, root) in self.roots.iter().enumerate() {
            for (di, dir) in self.dirs.iter().enumerate() {
                let Ok(read) = fs::read_dir(root.join(&dir.rel)) else {
                    continue;
                };
                for entry in read.flatten() {
                    let file = entry.file_name();
                    let Some((stem, ext)) = file.to_str().and_then(Ext::of) else {
                        continue;
                    };
                    self.icons
                        .entry(Box::from(stem))
                        .or_default()
                        .push((ri as u16, di as u16, ext));
                }
            }
        }
    }

    /// Best file for `name` at `size` in this theme: an exact size match
    /// (scalable first), else the closest.
    fn lookup(&self, name: &str, size: u32) -> Option<PathBuf> {
        let candidates = self.icons.get(name)?;
        let file = |&(ri, di, ext): &(u16, u16, Ext)| {
            let dir = &self.dirs[di as usize];
            let ext = match ext {
                Ext::Svg => "svg",
                Ext::Png => "png",
            };
            self.roots[ri as usize]
                .join(&dir.rel)
                .join(format!("{name}.{ext}"))
        };
        let scale1 = candidates
            .iter()
            .filter(|(_, di, _)| self.dirs[*di as usize].scale == 1);
        let best = scale1
            .clone()
            .filter(|(_, di, _)| self.dirs[*di as usize].matches(size))
            .min_by_key(|(_, _, ext)| *ext != Ext::Svg)
            .or_else(|| {
                scale1.min_by_key(|(_, di, ext)| {
                    (self.dirs[*di as usize].distance(size), *ext != Ext::Svg)
                })
            })?;
        Some(file(best))
    }
}

/// `(inherits, dirs)` from an `index.theme`.
fn parse_index(text: &str) -> (Vec<String>, Vec<IconDir>) {
    let mut inherits = Vec::new();
    let mut listed: Vec<String> = Vec::new();
    let mut groups: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(g) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = g.to_owned();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        if current == "Icon Theme" {
            match k {
                "Inherits" => inherits = csv(v),
                "Directories" | "ScaledDirectories" => {
                    for d in csv(v) {
                        if !listed.contains(&d) {
                            listed.push(d);
                        }
                    }
                }
                _ => {}
            }
        } else {
            groups
                .entry(current.clone())
                .or_default()
                .insert(k.to_owned(), v.to_owned());
        }
    }
    let dirs = listed
        .iter()
        .filter_map(|rel| {
            let g = groups.get(rel)?;
            let num = |k: &str| g.get(k).and_then(|v| v.parse::<u32>().ok());
            let size = num("Size")?;
            let kind = match g.get("Type").map(String::as_str) {
                Some("Scalable") => DirKind::Scalable {
                    min: num("MinSize").unwrap_or(size),
                    max: num("MaxSize").unwrap_or(size),
                },
                Some("Fixed") => DirKind::Fixed,
                // Threshold is the spec's default type, threshold 2.
                _ => DirKind::Threshold(num("Threshold").unwrap_or(2)),
            };
            Some(IconDir {
                rel: rel.clone(),
                size,
                scale: num("Scale").unwrap_or(1),
                kind,
            })
        })
        .collect();
    (inherits, dirs)
}

fn csv(v: &str) -> Vec<String> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The theme chain for one theme name, ready for lookups.
#[derive(Debug)]
pub struct IconIndex {
    themes: Vec<IconTheme>,
    /// `pixmaps/` dirs, the pre-theme fallback location.
    pixmaps: Vec<PathBuf>,
}

impl IconIndex {
    /// `bases` are the `icons/` dirs in precedence order; `pixmaps` the
    /// `pixmaps/` dirs. The chain is `name`, its `Inherits`
    /// (depth-first), then `hicolor`.
    pub fn load(name: &str, bases: &[PathBuf], pixmaps: &[PathBuf]) -> Self {
        let mut themes = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = vec![name.to_owned()];
        while let Some(n) = queue.first().cloned() {
            queue.remove(0);
            if !seen.insert(n.clone()) {
                continue;
            }
            match IconTheme::load(&n, bases) {
                Some(t) => {
                    // Parents go right after this theme, before hicolor.
                    let mut parents = t.inherits.clone();
                    parents.append(&mut queue);
                    queue = parents;
                    themes.push(t);
                }
                None => log::warn!("icon theme {n:?} not found"),
            }
            if queue.is_empty() && !seen.contains("hicolor") {
                queue.push("hicolor".to_owned());
            }
        }
        Self {
            themes,
            pixmaps: pixmaps.iter().filter(|p| p.is_dir()).cloned().collect(),
        }
    }

    pub fn theme_names(&self) -> Vec<&str> {
        self.themes.iter().map(|t| t.name.as_str()).collect()
    }

    pub fn icon_count(&self) -> usize {
        self.themes.iter().map(|t| t.icons.len()).sum()
    }

    /// Every directory the index was built from, to watch for changes.
    pub fn dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self
            .themes
            .iter()
            .flat_map(|t| {
                t.roots.iter().flat_map(move |r| {
                    std::iter::once(r.clone()).chain(t.dirs.iter().map(move |d| r.join(&d.rel)))
                })
            })
            .filter(|d| d.is_dir())
            .collect();
        dirs.extend(self.pixmaps.iter().cloned());
        dirs
    }

    /// The file for icon `name` at `size` pixels, first theme in the
    /// chain that has it. An absolute `name` is returned as is when it
    /// exists.
    pub fn lookup(&self, name: &str, size: u32) -> Option<PathBuf> {
        if name.starts_with('/') {
            let p = Path::new(name);
            return p.is_file().then(|| p.to_path_buf());
        }
        // `Icon=foo.png` happens; the spec says to strip it.
        let name = Ext::of(name).map_or(name, |(stem, _)| stem);
        self.themes
            .iter()
            .find_map(|t| t.lookup(name, size))
            .or_else(|| {
                self.pixmaps.iter().find_map(|dir| {
                    ["svg", "png"]
                        .iter()
                        .map(|e| dir.join(format!("{name}.{e}")))
                        .find(|p| p.is_file())
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = "[Icon Theme]\nName=T\nInherits=parent\n\
        Directories=16x16/apps,32x32/apps,scalable/apps,48x48@2x/apps\n\
        ScaledDirectories=48x48@2x/apps\n\n\
        [16x16/apps]\nSize=16\nContext=Applications\nType=Fixed\n\n\
        [32x32/apps]\nSize=32\nType=Threshold\nThreshold=4\n\n\
        [scalable/apps]\nSize=48\nType=Scalable\nMinSize=8\nMaxSize=256\n\n\
        [48x48@2x/apps]\nSize=48\nScale=2\nType=Fixed\n";

    #[test]
    fn index_parsing() {
        let (inherits, dirs) = parse_index(INDEX);
        assert_eq!(inherits, ["parent"]);
        assert_eq!(dirs.len(), 4, "a dir listed twice is parsed once");
        assert_eq!(dirs[0].kind, DirKind::Fixed);
        assert_eq!(dirs[1].kind, DirKind::Threshold(4));
        assert_eq!(dirs[2].kind, DirKind::Scalable { min: 8, max: 256 });
        assert_eq!(dirs[3].scale, 2);
        assert!(dirs[0].matches(16) && !dirs[0].matches(17));
        assert!(dirs[1].matches(28) && !dirs[1].matches(27));
        assert!(dirs[2].matches(200) && !dirs[2].matches(300));
        assert_eq!(dirs[0].distance(24), 8);
        assert_eq!(dirs[1].distance(24), 4);
        assert_eq!(dirs[2].distance(300), 44);
    }

    /// A theme tree on disk: `T` (svg at scalable, png at 16 and 32),
    /// `parent` (png at 16), `hicolor` (png at 32) and a pixmap.
    fn fixture() -> (PathBuf, IconIndex) {
        let base = std::env::temp_dir().join(format!("aria-icons-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let icons = base.join("icons");
        let mk = |theme: &str, index: &str, files: &[&str]| {
            let root = icons.join(theme);
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join("index.theme"), index).unwrap();
            for f in files {
                let p = root.join(f);
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(p, b"").unwrap();
            }
        };
        mk(
            "T",
            INDEX,
            &[
                "scalable/apps/both.svg",
                "16x16/apps/both.png",
                "32x32/apps/both.png",
                "16x16/apps/small.png",
                "48x48@2x/apps/hidpi.png",
                "16x16/apps/weird.xpm",
            ],
        );
        mk(
            "parent",
            "[Icon Theme]\nDirectories=16x16/apps\n[16x16/apps]\nSize=16\nType=Fixed\n",
            &["16x16/apps/inherited.png"],
        );
        mk(
            "hicolor",
            "[Icon Theme]\nDirectories=32x32/apps\n[32x32/apps]\nSize=32\nType=Fixed\n",
            &["32x32/apps/fallback.png", "32x32/apps/inherited.png"],
        );
        let pixmaps = base.join("pixmaps");
        fs::create_dir_all(&pixmaps).unwrap();
        fs::write(pixmaps.join("legacy.png"), b"").unwrap();
        let index = IconIndex::load("T", &[icons], &[pixmaps]);
        (base, index)
    }

    #[test]
    fn chain_and_lookup() {
        let (base, index) = fixture();
        assert_eq!(index.theme_names(), ["T", "parent", "hicolor"]);
        let rel = |p: Option<PathBuf>| {
            p.map(|p| {
                p.strip_prefix(&base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
        };
        // Exact size match: svg preferred over png of the same size.
        assert_eq!(
            rel(index.lookup("both", 16)).unwrap(),
            "icons/T/scalable/apps/both.svg"
        );
        // Only one size available: exact at 16, closest otherwise.
        assert_eq!(
            rel(index.lookup("small", 16)).unwrap(),
            "icons/T/16x16/apps/small.png"
        );
        assert_eq!(
            rel(index.lookup("small", 64)).unwrap(),
            "icons/T/16x16/apps/small.png"
        );
        // Theme chain: parent before hicolor.
        assert_eq!(
            rel(index.lookup("inherited", 32)).unwrap(),
            "icons/parent/16x16/apps/inherited.png"
        );
        assert_eq!(
            rel(index.lookup("fallback", 32)).unwrap(),
            "icons/hicolor/32x32/apps/fallback.png"
        );
        // Scale-2 dirs, xpm and unknown names don't resolve.
        assert_eq!(index.lookup("hidpi", 48), None);
        assert_eq!(index.lookup("weird", 16), None);
        assert_eq!(index.lookup("nope", 16), None);
        // Pixmaps last, extension stripped, absolute paths pass through.
        assert_eq!(
            rel(index.lookup("legacy", 16)).unwrap(),
            "pixmaps/legacy.png"
        );
        assert_eq!(
            rel(index.lookup("legacy.png", 16)).unwrap(),
            "pixmaps/legacy.png"
        );
        let abs = base.join("pixmaps/legacy.png");
        assert_eq!(index.lookup(abs.to_str().unwrap(), 16), Some(abs));
        assert_eq!(index.lookup("/nonexistent.png", 16), None);
        assert!(index.dirs().len() >= 6);
        let _ = fs::remove_dir_all(&base);
    }
}
