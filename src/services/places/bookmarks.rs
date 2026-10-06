//! The places read from files: the XDG user folders
//! (`user-dirs.dirs`), GTK's bookmarks (`gtk-3.0/bookmarks`) and KDE's
//! (`user-places.xbel`), parsed as the file managers do.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use super::{Kind, Place, Target};

/// The XDG folders shown, in this order (Templates and Public aren't).
const USER_DIRS: [(&str, Kind); 6] = [
    ("XDG_DESKTOP_DIR", Kind::Desktop),
    ("XDG_DOCUMENTS_DIR", Kind::Documents),
    ("XDG_DOWNLOAD_DIR", Kind::Download),
    ("XDG_MUSIC_DIR", Kind::Music),
    ("XDG_PICTURES_DIR", Kind::Pictures),
    ("XDG_VIDEOS_DIR", Kind::Videos),
];

/// `$<var>`, else `<home>/<fallback>`.
pub(super) fn xdg_dir(var: &str, home: &Path, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(fallback))
}

/// The last component, `/` for the root.
pub(super) fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The folders of [`USER_DIRS`] `user-dirs.dirs` sets, in that order:
/// `XDG_<NAME>_DIR="$HOME/<path>"` or `"/<path>"`. One set to the home
/// itself is disabled, as the spec has it.
pub(super) fn parse_user_dirs(text: &str, home: &Path) -> Vec<(Kind, PathBuf)> {
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
pub(super) fn parse_gtk_bookmarks(text: &str) -> Vec<(String, Option<String>)> {
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
pub(super) fn parse_xbel(text: &str) -> Vec<(String, Option<String>)> {
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
pub(super) fn bookmark(uri: &str, label: Option<String>) -> Option<Place> {
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

pub(super) fn percent_decode(s: &str) -> OsString {
    OsString::from_vec(percent_encoding::percent_decode_str(s).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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
