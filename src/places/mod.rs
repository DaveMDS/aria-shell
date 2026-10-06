//! Places: the locations a file manager's sidebar lists, for the
//! `Places` gadget. The home, the XDG user folders (`user-dirs.dirs`)
//! and the trash; the bookmarks GTK's file managers share
//! (`gtk-3.0/bookmarks`: Nautilus, Nemo, Thunar, Caja) and KDE's
//! (`user-places.xbel`: Dolphin). Read again by [`Command::Refresh`]
//! whenever the popup opens (a few small files); a click opens one in
//! `[general] file_manager` ([`Command::Open`]).

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use crate::config::{RawSection, Section};
use crate::process;

/// `[Places]` section: the gadget's keys (the daemon has none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacesConfig {
    /// The popup's sections, in this order (required).
    pub show: Vec<Group>,
    pub icon: String,
    /// Beside the icon; none when empty.
    pub label: String,
    /// The places' `-symbolic` icons, else the full colour ones.
    pub symbolic_icons: bool,
}

impl Section for PlacesConfig {
    const NAME: &'static str = "Places";

    fn from_raw(raw: &RawSection) -> Self {
        let show = raw
            .list_or("show", &[])
            .iter()
            .filter_map(|name| {
                let group = Group::from_name(name);
                if group.is_none() {
                    log::warn!("[Places] show: unknown section {name:?} (places, bookmarks)");
                }
                group
            })
            .collect();
        Self {
            show,
            icon: raw.str_or("icon", "folder-symbolic"),
            label: raw.str_or("label", ""),
            symbolic_icons: raw.bool_or("symbolic_icons", true),
        }
    }
}

/// A section of the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// The home, the XDG folders, the trash.
    Places,
    /// GTK's and KDE's.
    Bookmarks,
}

impl Group {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "places" => Some(Self::Places),
            "bookmarks" => Some(Self::Bookmarks),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Places => "places",
            Self::Bookmarks => "bookmarks",
        }
    }
}

/// What a place is, for its icon and its class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Home,
    Desktop,
    Documents,
    Download,
    Music,
    Pictures,
    Videos,
    Trash,
    /// A bookmarked local folder.
    Folder,
    /// A bookmarked URI (sftp://, smb://, ...).
    Remote,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Home,
        Kind::Desktop,
        Kind::Documents,
        Kind::Download,
        Kind::Music,
        Kind::Pictures,
        Kind::Videos,
        Kind::Trash,
        Kind::Folder,
        Kind::Remote,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Desktop => "desktop",
            Self::Documents => "documents",
            Self::Download => "download",
            Self::Music => "music",
            Self::Pictures => "pictures",
            Self::Videos => "videos",
            Self::Trash => "trash",
            Self::Folder => "folder",
            Self::Remote => "remote",
        }
    }

    /// The icon theme's name, without `-symbolic`; the trash's when
    /// empty (`user-trash-full` otherwise).
    pub fn icon(self) -> &'static str {
        match self {
            Self::Home => "user-home",
            Self::Desktop => "user-desktop",
            Self::Documents => "folder-documents",
            Self::Download => "folder-download",
            Self::Music => "folder-music",
            Self::Pictures => "folder-pictures",
            Self::Videos => "folder-videos",
            Self::Trash => "user-trash",
            Self::Folder => "folder",
            Self::Remote => "folder-remote",
        }
    }
}

/// What the file manager is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Path(PathBuf),
    /// As written: the file manager knows the scheme (gvfs, kio).
    Uri(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub kind: Kind,
    /// As shown; empty for the home and the trash, whose names are
    /// the locale's.
    pub label: String,
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Read everything again.
    Refresh,
    Open(Target),
}

#[derive(Debug, Default)]
pub struct Places {
    places: Vec<Place>,
    bookmarks: Vec<Place>,
    trash_full: bool,
}

/// The XDG folders shown, in this order (Templates and Public aren't).
const USER_DIRS: [(&str, Kind); 6] = [
    ("XDG_DESKTOP_DIR", Kind::Desktop),
    ("XDG_DOCUMENTS_DIR", Kind::Documents),
    ("XDG_DOWNLOAD_DIR", Kind::Download),
    ("XDG_MUSIC_DIR", Kind::Music),
    ("XDG_PICTURES_DIR", Kind::Pictures),
    ("XDG_VIDEOS_DIR", Kind::Videos),
];

const TRASH_URI: &str = "trash:///";

impl Places {
    pub fn run(&mut self, command: Command, file_manager: Option<&str>) {
        match command {
            Command::Refresh => self.reload(),
            Command::Open(target) => match file_manager {
                Some(file_manager) => {
                    let argv = match &target {
                        Target::Path(path) => process::on_file(file_manager, path),
                        Target::Uri(uri) => process::on_file(file_manager, uri),
                    };
                    process::run_argv(&argv);
                }
                None => log::warn!("places: no file manager ([general] file_manager)"),
            },
        }
    }

    pub fn places(&self) -> &[Place] {
        &self.places
    }

    pub fn bookmarks(&self) -> &[Place] {
        &self.bookmarks
    }

    pub fn trash_full(&self) -> bool {
        self.trash_full
    }

    fn reload(&mut self) {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            log::warn!("places: no $HOME");
            return;
        };
        let config_home = xdg_dir("XDG_CONFIG_HOME", &home, ".config");
        let data_home = xdg_dir("XDG_DATA_HOME", &home, ".local/share");
        let read = |path: PathBuf| std::fs::read_to_string(path).unwrap_or_default();

        let mut places = vec![Place {
            kind: Kind::Home,
            label: String::new(),
            target: Target::Path(home.clone()),
        }];
        for (kind, path) in parse_user_dirs(&read(config_home.join("user-dirs.dirs")), &home) {
            if path.is_dir() {
                places.push(Place {
                    kind,
                    label: base_name(&path),
                    target: Target::Path(path),
                });
            }
        }
        places.push(Place {
            kind: Kind::Trash,
            label: String::new(),
            target: Target::Uri(TRASH_URI.to_owned()),
        });
        self.places = places;

        let gtk = parse_gtk_bookmarks(&read(config_home.join("gtk-3.0/bookmarks")));
        let kde = parse_xbel(&read(data_home.join("user-places.xbel")));
        let mut bookmarks: Vec<Place> = Vec::new();
        for (uri, label) in gtk.into_iter().chain(kde) {
            let Some(place) = bookmark(&uri, label) else {
                continue;
            };
            let gone = matches!(&place.target, Target::Path(p) if !p.is_dir());
            if !gone && !bookmarks.iter().any(|b| b.target == place.target) {
                bookmarks.push(place);
            }
        }
        self.bookmarks = bookmarks;

        // The spec's home trash; the trash of other filesystems
        // (`.Trash-<uid>` on a USB stick) is the file manager's.
        self.trash_full = std::fs::read_dir(data_home.join("Trash/files"))
            .is_ok_and(|mut entries| entries.next().is_some());
    }
}

/// `$<var>`, else `<home>/<fallback>`.
fn xdg_dir(var: &str, home: &Path, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(fallback))
}

/// The last component, `/` for the root.
fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The folders of [`USER_DIRS`] `user-dirs.dirs` sets, in that order:
/// `XDG_<NAME>_DIR="$HOME/<path>"` or `"/<path>"`. One set to the home
/// itself is disabled, as the spec has it.
fn parse_user_dirs(text: &str, home: &Path) -> Vec<(Kind, PathBuf)> {
    let mut found: Vec<(Kind, PathBuf)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(&(_, kind)) = USER_DIRS.iter().find(|(k, _)| *k == key.trim()) else {
            continue;
        };
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        let path = if value == "$HOME" || value == "$HOME/" {
            continue;
        } else if let Some(rest) = value.strip_prefix("$HOME/") {
            home.join(rest)
        } else if value.starts_with('/') {
            PathBuf::from(value)
        } else {
            continue;
        };
        if path == home {
            continue;
        }
        found.retain(|(k, _)| *k != kind);
        found.push((kind, path));
    }
    found.sort_by_key(|(kind, _)| USER_DIRS.iter().position(|(_, k)| k == kind));
    // Two names for one folder: the first one.
    let mut seen: Vec<PathBuf> = Vec::new();
    found.retain(|(_, path)| {
        let new = !seen.contains(path);
        seen.push(path.clone());
        new
    });
    found
}

/// GTK's bookmarks: one `<uri>[ <label>]` per line.
fn parse_gtk_bookmarks(text: &str) -> Vec<(String, Option<String>)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| match line.split_once(' ') {
            Some((uri, label)) => (uri.to_owned(), Some(label.trim().to_owned())),
            None => (line.to_owned(), None),
        })
        .map(|(uri, label)| (uri, label.filter(|l| !l.is_empty())))
        .collect()
}

/// KDE's places: the `bookmark`s the user added, without Dolphin's own
/// (`isSystemItem`: home, trash, network, ...), the hidden ones and
/// the devices (`UDI`).
fn parse_xbel(text: &str) -> Vec<(String, Option<String>)> {
    if text.is_empty() {
        return Vec::new();
    }
    // KDE writes `<!DOCTYPE xbel>`.
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = match roxmltree::Document::parse_with_options(text, options) {
        Ok(doc) => doc,
        Err(e) => {
            log::warn!("places: user-places.xbel: {e}");
            return Vec::new();
        }
    };
    doc.root_element()
        .children()
        .filter(|n| n.has_tag_name("bookmark"))
        .filter(|bookmark| {
            !bookmark.descendants().any(|n| {
                let name = n.tag_name().name();
                let flag = |n: roxmltree::Node| n.text().is_some_and(|t| t.trim() == "true");
                name == "UDI" || ((name == "isSystemItem" || name == "IsHidden") && flag(n))
            })
        })
        .filter_map(|bookmark| {
            let uri = bookmark.attribute("href")?.to_owned();
            let title = bookmark
                .children()
                .find(|n| n.has_tag_name("title"))
                .and_then(|n| n.text())
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty());
            Some((uri, title))
        })
        .collect()
}

/// A bookmark's place: a local folder for a `file://` URI, a remote
/// one for any other; its label the bookmark's, else the folder's name
/// or the host.
fn bookmark(uri: &str, label: Option<String>) -> Option<Place> {
    if let Some(path) = file_uri_path(uri) {
        return Some(Place {
            kind: Kind::Folder,
            label: label.unwrap_or_else(|| base_name(&path)),
            target: Target::Path(path),
        });
    }
    let (scheme, rest) = uri.split_once(':')?;
    if scheme.is_empty() || rest.is_empty() {
        return None;
    }
    let label = label.unwrap_or_else(|| {
        let host = rest.trim_start_matches('/').split('/').next().unwrap_or("");
        let host = host.rsplit('@').next().unwrap_or(host);
        if host.is_empty() {
            uri.to_owned()
        } else {
            percent_decode(host).to_string_lossy().into_owned()
        }
    });
    Some(Place {
        kind: Kind::Remote,
        label,
        target: Target::Uri(uri.to_owned()),
    })
}

/// The path of a local `file://` URI, percent-decoded.
fn file_uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    path.starts_with('/')
        .then(|| PathBuf::from(percent_decode(path)))
}

fn percent_decode(s: &str) -> OsString {
    OsString::from_vec(percent_encoding::percent_decode_str(s).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn config_sections_in_order() {
        let config = Config::parse("[Places]\nshow = bookmarks devices places\nlabel = Go\n");
        let places: PlacesConfig = config.section(None);
        assert_eq!(places.show, vec![Group::Bookmarks, Group::Places]);
        assert_eq!(places.label, "Go");
        assert_eq!(places.icon, "folder-symbolic");
        assert!(places.symbolic_icons);
    }

    #[test]
    fn user_dirs() {
        let home = Path::new("/home/u");
        let text = r#"
# XDG_DOCUMENTS_DIR="$HOME/Commented"
XDG_VIDEOS_DIR="$HOME/Video"
XDG_DESKTOP_DIR="$HOME/"
XDG_DOCUMENTS_DIR="$HOME"
XDG_DOWNLOAD_DIR="$HOME/Scaricati"
XDG_MUSIC_DIR="/data/music"
XDG_PICTURES_DIR="relative/pictures"
XDG_TEMPLATES_DIR="$HOME/Modelli"
XDG_PICTURES_DIR="$HOME/Scaricati"
"#;
        assert_eq!(
            parse_user_dirs(text, home),
            vec![
                (Kind::Download, PathBuf::from("/home/u/Scaricati")),
                (Kind::Music, PathBuf::from("/data/music")),
                (Kind::Videos, PathBuf::from("/home/u/Video")),
            ],
            "in sidebar order; the home, relative paths, templates and a \
             folder already listed left out"
        );
    }

    #[test]
    fn gtk_bookmarks() {
        let text =
            "file:///home/u Home\n\nfile:///home/u/My%20Projects\nsftp://u@host/srv  Server \n";
        assert_eq!(
            parse_gtk_bookmarks(text),
            vec![
                ("file:///home/u".to_owned(), Some("Home".to_owned())),
                ("file:///home/u/My%20Projects".to_owned(), None),
                ("sftp://u@host/srv".to_owned(), Some("Server".to_owned())),
            ]
        );
    }

    #[test]
    fn xbel_user_bookmarks_only() {
        let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xbel>
<xbel xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
 <info><metadata owner="http://www.kde.org"><kde_places_version>4</kde_places_version></metadata></info>
 <bookmark href="file:///home/u"><title>Home</title>
  <info>
   <metadata owner="http://freedesktop.org"><bookmark:icon name="user-home"/></metadata>
   <metadata owner="http://www.kde.org"><isSystemItem>true</isSystemItem></metadata>
  </info>
 </bookmark>
 <bookmark href="file:///home/u/Work"><title>Work &amp; stuff</title>
  <info><metadata owner="http://www.kde.org"><isSystemItem>false</isSystemItem></metadata></info>
 </bookmark>
 <bookmark href="file:///home/u/Old"><title>Old</title>
  <info><metadata owner="http://www.kde.org"><IsHidden>true</IsHidden></metadata></info>
 </bookmark>
 <bookmark href="file:///run/media/u/STICK"><title>STICK</title>
  <info><metadata owner="http://www.kde.org"><UDI>/org/kde/fstab/x</UDI></metadata></info>
 </bookmark>
 <bookmark href="smb://nas/share"><title></title></bookmark>
</xbel>"#;
        assert_eq!(
            parse_xbel(text),
            vec![
                (
                    "file:///home/u/Work".to_owned(),
                    Some("Work & stuff".to_owned())
                ),
                ("smb://nas/share".to_owned(), None),
            ]
        );
        assert!(parse_xbel("<not xml").is_empty());
        assert!(parse_xbel("").is_empty());
    }

    #[test]
    fn bookmark_places() {
        assert_eq!(
            bookmark("file:///home/u/My%20Projects", None),
            Some(Place {
                kind: Kind::Folder,
                label: "My Projects".to_owned(),
                target: Target::Path(PathBuf::from("/home/u/My Projects")),
            })
        );
        assert_eq!(
            bookmark("file://localhost/srv", Some("Srv".to_owned())).map(|p| p.target),
            Some(Target::Path(PathBuf::from("/srv")))
        );
        assert_eq!(
            bookmark("file:///", None).map(|p| p.label),
            Some("/".to_owned())
        );
        assert_eq!(
            bookmark("sftp://u@host.lan:22/srv", None),
            Some(Place {
                kind: Kind::Remote,
                label: "host.lan:22".to_owned(),
                target: Target::Uri("sftp://u@host.lan:22/srv".to_owned()),
            })
        );
        assert_eq!(
            bookmark("recent:///", None).map(|p| p.label),
            Some("recent:///".to_owned()),
            "no host: the URI itself"
        );
        assert_eq!(bookmark("no-scheme", None), None);
    }
}
