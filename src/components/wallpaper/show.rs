//! One slideshow: the images a `source` names, in showing order, and
//! which of them is wanted on screen. A file is a slideshow of one; a
//! folder every image in it and in its sub-folders; `auto` the first
//! `backgrounds` folder of the XDG data dirs with an image in it.
//! Bookkeeping only: [`super::Wallpapers`] decodes and shows.

use std::fs;
use std::path::{Path, PathBuf};

use super::{Order, Source, WallpaperConfig};
use crate::config;

/// The images a folder offers, by extension: decoding goes by content,
/// but a folder holds other files too (GNOME's slideshow xml).
const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// Deep enough for any collection; ends a symlink loop.
const MAX_DEPTH: usize = 8;

pub struct Show {
    pub config: WallpaperConfig,
    /// The folder or file the images come from (`auto`: the folder
    /// picked); `None` when there is no image.
    pub root: Option<PathBuf>,
    /// What to watch for images coming and going: the folders scanned,
    /// the file, and the parent of a folder that doesn't exist yet.
    pub watch: Vec<PathBuf>,
    /// The images, in showing order.
    pub files: Vec<PathBuf>,
    /// Index in `files` of the image wanted on screen.
    current: usize,
    /// The image on screen: the wanted one once it's decoded.
    pub shown: Option<PathBuf>,
    /// Bumped when the image changes by hand: the timer starts over.
    pub generation: u64,
    rng: Rng,
}

impl Show {
    pub fn new(config: WallpaperConfig) -> Self {
        Self::with_seed(config, Rng::seed())
    }

    fn with_seed(config: WallpaperConfig, seed: u64) -> Self {
        let mut show = Self {
            config,
            root: None,
            watch: Vec::new(),
            files: Vec::new(),
            current: 0,
            shown: None,
            generation: 0,
            rng: Rng(seed),
        };
        let (root, watch, mut files) = scan(&show.config.source);
        if show.config.order == Order::Random {
            show.rng.shuffle(&mut files);
        }
        show.root = root;
        show.watch = watch;
        show.files = files;
        show
    }

    /// The image to show.
    pub fn wanted(&self) -> Option<&Path> {
        self.files.get(self.current).map(PathBuf::as_path)
    }

    /// Whether there is anything to rotate.
    pub fn rotates(&self) -> bool {
        self.config.interval.is_some() && self.files.len() > 1
    }

    /// On to the next image: by name, back to the first after the last;
    /// at random, a new shuffle after every image has had its turn (not
    /// starting with the one just shown).
    pub fn next(&mut self) {
        if self.files.len() < 2 {
            return;
        }
        self.current += 1;
        if self.current < self.files.len() {
            return;
        }
        self.current = 0;
        if self.config.order == Order::Random {
            let last = self.files.last().cloned();
            self.rng.shuffle(&mut self.files);
            if self.files.first() == last.as_ref() {
                let other = 1 + self.rng.below(self.files.len() - 1);
                self.files.swap(0, other);
            }
        }
    }

    /// Past the images that can't be shown (`failed`), as long as one
    /// can.
    pub fn skip(&mut self, failed: impl Fn(&Path) -> bool) {
        for _ in 0..self.files.len() {
            match self.wanted() {
                Some(path) if failed(path) => self.next(),
                _ => return,
            }
        }
    }

    /// Look at the source again (something changed in a folder): the
    /// wanted image stays if it's still there, else the one after it
    /// comes. At random, the images not shown yet in this round stay
    /// ahead, the new ones join them at random places.
    pub fn rescan(&mut self) {
        let (root, watch, fresh) = scan(&self.config.source);
        self.root = root;
        self.watch = watch;
        let wanted = self.wanted().map(Path::to_path_buf);
        // The images before the wanted one that are still there: where
        // the one after it lands when it's gone.
        let before = self.files[..self.current.min(self.files.len())]
            .iter()
            .filter(|f| fresh.contains(f))
            .count();
        match self.config.order {
            Order::Name => self.files = fresh,
            Order::Random => {
                self.files.retain(|f| fresh.contains(f));
                for file in fresh {
                    if !self.files.contains(&file) {
                        let ahead = self.files.len() - before.min(self.files.len());
                        let at = before + 1 + self.rng.below(ahead + 1);
                        self.files.insert(at.min(self.files.len()), file);
                    }
                }
            }
        }
        self.current = wanted
            .and_then(|w| self.files.iter().position(|f| *f == w))
            .unwrap_or(before);
        if self.current >= self.files.len() {
            self.current = 0;
        }
    }
}

/// What `source` gives: the root picked, the paths to watch, the images
/// (by name).
fn scan(source: &Source) -> (Option<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
    let roots = match source {
        Source::None => Vec::new(),
        Source::Auto => config::xdg_data_dirs()
            .into_iter()
            .map(|dir| dir.join("backgrounds"))
            .collect(),
        Source::Path(path) => vec![path.clone()],
    };
    let mut watch = Vec::new();
    for root in roots {
        match fs::metadata(&root) {
            Ok(meta) if meta.is_file() => {
                watch.push(root.clone());
                return (Some(root.clone()), watch, vec![root]);
            }
            Ok(meta) if meta.is_dir() => {
                let mut files = Vec::new();
                collect(&root, 0, &mut watch, &mut files);
                if !files.is_empty() {
                    files.sort();
                    return (Some(root), watch, files);
                }
            }
            // Not there (yet): its parent, to see it come.
            _ => {
                if let Some(parent) = root.parent().filter(|p| p.is_dir()) {
                    watch.push(parent.to_path_buf());
                }
            }
        }
    }
    watch.dedup();
    (None, watch, Vec::new())
}

/// The images under `dir`, and every folder visited; hidden entries
/// skipped, symlinks followed.
fn collect(dir: &Path, depth: usize, dirs: &mut Vec<PathBuf>, files: &mut Vec<PathBuf>) {
    dirs.push(dir.to_path_buf());
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        match fs::metadata(&path) {
            Ok(meta) if meta.is_dir() && depth < MAX_DEPTH => {
                collect(&path, depth + 1, dirs, files)
            }
            Ok(meta) if meta.is_file() && is_image(&path) => files.push(path),
            _ => {}
        }
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// xorshift64: a shuffle needs no more.
struct Rng(u64);

impl Rng {
    fn seed() -> u64 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        nanos ^ (u64::from(std::process::id()) << 32) | 1
    }

    fn step(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// In `0..n` (`n > 0`).
    fn below(&mut self, n: usize) -> usize {
        (self.step() % n as u64) as usize
    }

    /// Fisher-Yates.
    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Fit;
    use super::*;
    use std::time::Duration;

    /// A folder of files named `names` (sub/folders created), fresh for
    /// each test.
    fn folder(test: &str, names: &[&str]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("aria-wallpaper-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for name in names {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"").unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn show(source: Source, order: Order) -> Show {
        Show::with_seed(
            WallpaperConfig {
                source,
                fit: Fit::Cover,
                interval: Some(Duration::from_secs(60)),
                order,
            },
            42,
        )
    }

    fn names(show: &Show, root: &Path) -> Vec<String> {
        show.files
            .iter()
            .map(|f| f.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_folder_is_its_images_by_name_sub_folders_too() {
        let dir = folder(
            "scan",
            &[
                "b.jpg",
                "a.PNG",
                "notes.txt",
                "slides.xml",
                ".hidden.png",
                "sub/c.webp",
                ".git/d.png",
            ],
        );
        let s = show(Source::Path(dir.clone()), Order::Name);
        assert_eq!(names(&s, &dir), ["a.PNG", "b.jpg", "sub/c.webp"]);
        assert_eq!(s.root.as_deref(), Some(dir.as_path()));
        assert!(s.watch.contains(&dir) && s.watch.contains(&dir.join("sub")));
        assert_eq!(s.wanted(), Some(dir.join("a.PNG").as_path()));
    }

    #[test]
    fn a_file_is_itself_whatever_its_name() {
        let dir = folder("file", &["picture"]);
        let s = show(Source::Path(dir.join("picture")), Order::Name);
        assert_eq!(s.files, [dir.join("picture")]);
        assert_eq!(s.watch, [dir.join("picture")]);
        assert!(!s.rotates());
    }

    #[test]
    fn a_missing_folder_watches_its_parent() {
        let dir = folder("missing", &[]);
        let s = show(Source::Path(dir.join("walls")), Order::Name);
        assert!(s.files.is_empty() && s.root.is_none());
        assert_eq!(s.watch, std::slice::from_ref(&dir));
        assert_eq!(s.wanted(), None);
        assert!(show(Source::None, Order::Name).files.is_empty());
    }

    #[test]
    fn by_name_round_and_round() {
        let dir = folder("name", &["a.png", "b.png", "c.png"]);
        let mut s = show(Source::Path(dir.clone()), Order::Name);
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(s.wanted().unwrap().file_name().unwrap().to_owned());
            s.next();
        }
        assert_eq!(seen, ["a.png", "b.png", "c.png", "a.png"]);
    }

    #[test]
    fn at_random_every_image_once_per_round_no_repeat_across() {
        let all: Vec<String> = (0..7).map(|i| format!("{i}.png")).collect();
        let refs: Vec<&str> = all.iter().map(String::as_str).collect();
        let dir = folder("random", &refs);
        let mut s = show(Source::Path(dir.clone()), Order::Random);
        let mut last = None;
        for _ in 0..5 {
            let mut round = Vec::new();
            for _ in 0..all.len() {
                let now = s.wanted().unwrap().to_path_buf();
                assert_ne!(Some(&now), last.as_ref(), "the same image twice in a row");
                round.push(now.clone());
                last = Some(now);
                s.next();
            }
            round.sort();
            round.dedup();
            assert_eq!(round.len(), all.len(), "a round shows every image");
        }
    }

    #[test]
    fn rescan_keeps_the_wanted_or_takes_the_next() {
        let dir = folder("rescan", &["a.png", "b.png", "c.png"]);
        let mut s = show(Source::Path(dir.clone()), Order::Name);
        s.next();
        assert_eq!(s.wanted(), Some(dir.join("b.png").as_path()));
        fs::write(dir.join("0.png"), b"").unwrap();
        s.rescan();
        assert_eq!(names(&s, &dir), ["0.png", "a.png", "b.png", "c.png"]);
        assert_eq!(s.wanted(), Some(dir.join("b.png").as_path()));
        fs::remove_file(dir.join("b.png")).unwrap();
        s.rescan();
        assert_eq!(s.wanted(), Some(dir.join("c.png").as_path()));
        fs::remove_file(dir.join("c.png")).unwrap();
        s.rescan();
        assert_eq!(
            s.wanted(),
            Some(dir.join("0.png").as_path()),
            "past the end: the first"
        );
        for f in ["0.png", "a.png"] {
            fs::remove_file(dir.join(f)).unwrap();
        }
        s.rescan();
        assert_eq!(s.wanted(), None);
        assert_eq!(s.root, None);
    }

    #[test]
    fn rescan_at_random_keeps_the_round() {
        let dir = folder("rescan-random", &["a.png", "b.png", "c.png", "d.png"]);
        let mut s = show(Source::Path(dir.clone()), Order::Random);
        s.next();
        let wanted = s.wanted().unwrap().to_path_buf();
        let shown: Vec<PathBuf> = s.files[..2].to_vec();
        fs::write(dir.join("e.png"), b"").unwrap();
        s.rescan();
        assert_eq!(s.wanted(), Some(wanted.as_path()));
        assert_eq!(s.files[..2], shown[..], "what was shown stays behind");
        assert_eq!(s.files.len(), 5);
        assert!(
            s.files[2..].contains(&dir.join("e.png")),
            "the new one comes in this round"
        );
    }

    #[test]
    fn skip_passes_the_failed_ones() {
        let dir = folder("skip", &["a.png", "b.png", "c.png"]);
        let mut s = show(Source::Path(dir.clone()), Order::Name);
        s.skip(|p| p.ends_with("a.png") || p.ends_with("b.png"));
        assert_eq!(s.wanted(), Some(dir.join("c.png").as_path()));
        // None can be shown: it stops somewhere, it doesn't spin.
        s.skip(|_| true);
        assert!(s.wanted().is_some());
    }
}
