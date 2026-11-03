//! Apps: the applications pinned to the `Apps` gadgets, the same on
//! every bar, and launching desktop entries for them.
//!
//! The pinned ones are desktop ids (`firefox`, `org.gnome.nautilus`:
//! the file's name, lowercased, without `.desktop`), kept in
//! `$XDG_STATE_HOME/aria-shell/pinned-apps`, one per line, in the order
//! they were pinned: read when the shell starts, written at every
//! [`Command::Pin`] / [`Command::Unpin`]. Never in the way: a missing
//! or broken file is an empty list, a failed write is logged.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::config;
use crate::services::icons::{Index, desktop};

#[derive(Debug, Clone)]
pub enum Command {
    /// Run the desktop entry `id`, or its `[Desktop Action]` `action`.
    Launch {
        id: String,
        action: Option<String>,
    },
    /// Pin desktop id `id` on every `Apps` gadget, after the others.
    Pin(String),
    Unpin(String),
}

#[derive(Debug, Default)]
pub struct Apps {
    pinned: Vec<String>,
    path: Option<PathBuf>,
}

impl Apps {
    pub fn load() -> Self {
        Self::load_from(config::state_dir().map(|d| d.join("pinned-apps")))
    }

    fn load_from(path: Option<PathBuf>) -> Self {
        let mut apps = Self {
            pinned: Vec::new(),
            path,
        };
        let Some(path) = &apps.path else {
            return apps;
        };
        match fs::read_to_string(path) {
            Ok(text) => apps.pinned = parse(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("cannot read {}: {e}", path.display()),
        }
        apps
    }

    /// The pinned desktop ids, in their order.
    pub fn pinned(&self) -> &[String] {
        &self.pinned
    }

    /// Whether it changed the pinned ones (every bar shows them).
    pub fn run(&mut self, command: Command, index: Option<&Index>, terminal: Option<&str>) -> bool {
        match command {
            Command::Launch { id, action } => {
                launch(index, &id, action.as_deref(), terminal);
                false
            }
            Command::Pin(id) => {
                let id = desktop_id(&id);
                if self.pinned.contains(&id) {
                    return false;
                }
                self.pinned.push(id);
                self.save();
                true
            }
            Command::Unpin(id) => {
                let id = desktop_id(&id);
                let before = self.pinned.len();
                self.pinned.retain(|p| *p != id);
                if self.pinned.len() == before {
                    return false;
                }
                self.save();
                true
            }
        }
    }

    fn save(&self) {
        if let Some(path) = &self.path
            && let Err(e) = write(path, &self.pinned)
        {
            log::warn!("cannot write {}: {e}", path.display());
        }
    }
}

/// A desktop id as the index keys it: lowercased, no `.desktop`.
pub fn desktop_id(name: &str) -> String {
    let id = name.trim().to_ascii_lowercase();
    match id.strip_suffix(".desktop") {
        Some(stem) => stem.to_owned(),
        None => id,
    }
}

/// One id per line; blank lines and doubles dropped.
fn parse(text: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let id = desktop_id(line);
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// The whole file, through a temporary so a crash midway leaves the
/// old one.
fn write(path: &Path, ids: &[String]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)?;
    for id in ids {
        writeln!(file, "{id}")?;
    }
    file.flush()?;
    fs::rename(&tmp, path)
}

fn launch(index: Option<&Index>, id: &str, action: Option<&str>, terminal: Option<&str>) {
    let Some(entry) = index.and_then(|i| i.apps().for_class(id)) else {
        log::warn!("apps: no desktop entry {id:?} to launch");
        return;
    };
    let action = match action {
        Some(name) => match entry.actions.iter().find(|a| a.id == name) {
            Some(a) => Some(a),
            None => {
                log::warn!("apps: {id:?} has no action {name:?}");
                return;
            }
        },
        None => None,
    };
    if let Err(e) = desktop::launch(entry, action, terminal) {
        log::error!("cannot launch {id:?}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_the_index_keys() {
        assert_eq!(desktop_id("Firefox.desktop"), "firefox");
        assert_eq!(desktop_id(" org.gnome.Nautilus "), "org.gnome.nautilus");
        assert_eq!(
            parse("kitty\n\nfirefox.desktop\nKitty\n"),
            ["kitty", "firefox"]
        );
    }

    #[test]
    fn pin_and_unpin_are_saved() {
        let dir = std::env::temp_dir().join(format!("aria-apps-{}", std::process::id()));
        let path = dir.join("state").join("pinned-apps");
        let mut apps = Apps::load_from(Some(path.clone()));
        assert!(apps.pinned().is_empty());
        assert!(apps.run(Command::Pin("kitty".into()), None, None));
        assert!(apps.run(Command::Pin("Firefox.desktop".into()), None, None));
        assert!(!apps.run(Command::Pin("kitty".into()), None, None));
        assert_eq!(fs::read_to_string(&path).unwrap(), "kitty\nfirefox\n");
        assert!(apps.run(Command::Unpin("kitty".into()), None, None));
        assert!(!apps.run(Command::Unpin("gimp".into()), None, None));
        let apps = Apps::load_from(Some(path));
        assert_eq!(apps.pinned(), ["firefox"]);
        fs::remove_dir_all(dir).unwrap();
    }
}
