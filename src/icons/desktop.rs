//! The `.desktop` entries installed on the system, indexed for the
//! lookups the shell does: by id, by `StartupWMClass`, by executable.
//! Only the keys we use are parsed. [`launch`] runs one.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::process;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    /// File name without `.desktop`, lowercased: `firefox`,
    /// `org.kde.dolphin`.
    pub id: String,
    pub name: String,
    pub comment: Option<String>,
    /// An icon name, or an absolute path.
    pub icon: Option<String>,
    /// Basename of the program in `Exec`, lowercased.
    pub exec: Option<String>,
    /// The full `Exec` line, field codes included.
    pub exec_line: Option<String>,
    /// `Terminal=true`: run it inside a terminal emulator.
    pub terminal: bool,
    /// `Path=`: working directory to run it in.
    pub working_dir: Option<PathBuf>,
    /// `StartupWMClass`, lowercased.
    pub wm_class: Option<String>,
    pub no_display: bool,
    pub path: PathBuf,
}

#[derive(Debug, Default)]
pub struct DesktopDb {
    entries: Vec<DesktopEntry>,
    by_id: HashMap<String, usize>,
    /// Last dot-segment of a reverse-DNS id: `dolphin` for
    /// `org.kde.dolphin`. First entry wins.
    by_id_suffix: HashMap<String, usize>,
    by_wm_class: HashMap<String, usize>,
    by_exec: HashMap<String, usize>,
}

impl DesktopDb {
    /// Scan `dirs` in precedence order: the first file with a given id
    /// wins, as the spec says. `Hidden=true` entries are dropped.
    pub fn load(dirs: &[PathBuf]) -> Self {
        let mut db = Self::default();
        for dir in dirs {
            let Ok(read) = fs::read_dir(dir) else {
                continue;
            };
            let mut paths: Vec<PathBuf> = read
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "desktop"))
                .collect();
            paths.sort();
            for path in paths {
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if db.by_id.contains_key(&id) {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                if let Some(entry) = parse(&text, id, path) {
                    db.insert(entry);
                }
            }
        }
        db
    }

    fn insert(&mut self, entry: DesktopEntry) {
        let i = self.entries.len();
        self.by_id.insert(entry.id.clone(), i);
        if let Some(suffix) = entry.id.rsplit('.').next()
            && suffix != entry.id
        {
            self.by_id_suffix.entry(suffix.to_owned()).or_insert(i);
        }
        if let Some(c) = &entry.wm_class {
            self.by_wm_class.entry(c.clone()).or_insert(i);
        }
        if let Some(e) = &entry.exec {
            self.by_exec.entry(e.clone()).or_insert(i);
        }
        self.entries.push(entry);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Every entry, in scan order.
    pub fn entries(&self) -> &[DesktopEntry] {
        &self.entries
    }

    /// The entry for a window class / app id: exact id, then
    /// `StartupWMClass`, then the executable name, then the last segment
    /// of a reverse-DNS id.
    pub fn for_class(&self, class: &str) -> Option<&DesktopEntry> {
        let key = class.to_ascii_lowercase();
        [
            &self.by_id,
            &self.by_wm_class,
            &self.by_exec,
            &self.by_id_suffix,
        ]
        .into_iter()
        .find_map(|map| map.get(&key))
        .map(|&i| &self.entries[i])
    }
}

/// `[Desktop Entry]` group only; localized keys (`Name[it]`) are
/// ignored, we want the untranslated `Name`.
pub fn parse(text: &str, id: String, path: PathBuf) -> Option<DesktopEntry> {
    let mut in_entry = false;
    let mut name = None;
    let mut comment = None;
    let mut icon = None;
    let mut exec = None;
    let mut exec_line = None;
    let mut terminal = false;
    let mut working_dir = None;
    let mut wm_class = None;
    let mut no_display = false;
    let mut is_app = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(group) = line.strip_prefix('[') {
            in_entry = group.strip_suffix(']') == Some("Desktop Entry");
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Type" => is_app = value == "Application",
            "Name" => name = Some(value.to_owned()),
            "Comment" if !value.is_empty() => comment = Some(value.to_owned()),
            "Icon" if !value.is_empty() => icon = Some(value.to_owned()),
            "Exec" => {
                exec = exec_basename(value);
                exec_line = Some(value.to_owned()).filter(|v| !v.is_empty());
            }
            "Terminal" => terminal = value == "true",
            "Path" if !value.is_empty() => working_dir = Some(PathBuf::from(value)),
            "StartupWMClass" if !value.is_empty() => wm_class = Some(value.to_ascii_lowercase()),
            "NoDisplay" => no_display = value == "true",
            "Hidden" if value == "true" => return None,
            _ => {}
        }
    }
    if !is_app {
        return None;
    }
    Some(DesktopEntry {
        name: name.unwrap_or_else(|| id.clone()),
        id,
        comment,
        icon,
        exec,
        exec_line,
        terminal,
        working_dir,
        wm_class,
        no_display,
        path,
    })
}

/// The program `Exec` runs, skipping `env VAR=x` prefixes: `firefox`
/// for `/usr/lib/firefox/firefox %u`, `code` for `env FOO=1 code`.
fn exec_basename(exec: &str) -> Option<String> {
    let mut words = exec.split_whitespace();
    let mut word = words.next()?;
    if word == "env" {
        word = words.find(|w| !w.contains('='))?;
    }
    let base = Path::new(word).file_name()?.to_str()?;
    Some(base.to_ascii_lowercase())
}

/// Run `entry` as the spec's `Exec` key says: quoting and escapes
/// unwound, field codes expanded (no files or URLs to pass, so
/// `%f %F %u %U` vanish; `%i` is the icon, `%c` the name, `%k` the
/// file), in `Path=` if set, inside `terminal` (a command line, given
/// `-e`) when `Terminal=true`, detached (see [`process::spawn_detached`]).
/// `DBusActivatable` is not honoured.
pub fn launch(entry: &DesktopEntry, terminal: &str) -> io::Result<()> {
    let line = entry
        .exec_line
        .as_deref()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no Exec"))?;
    let mut argv = exec_argv(line, entry);
    if entry.terminal {
        argv = process::in_terminal(terminal, argv);
    }
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty Exec"))?;
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(dir) = &entry.working_dir {
        cmd.current_dir(dir);
    }
    process::spawn_detached(cmd)?;
    log::info!("launched {:?}: {argv:?}", entry.id);
    Ok(())
}

/// The argument vector for an `Exec` line: unquote and unescape as the
/// spec says, then expand or drop the field codes.
fn exec_argv(line: &str, entry: &DesktopEntry) -> Vec<String> {
    let mut argv = Vec::new();
    for word in split_exec(line) {
        let mut out = String::new();
        let mut chars = word.chars();
        let mut only_code = None;
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('%') => out.push('%'),
                Some('c') => out.push_str(&entry.name),
                Some('k') => out.push_str(&entry.path.to_string_lossy()),
                Some(code @ ('i' | 'f' | 'F' | 'u' | 'U')) => only_code = Some(code),
                // Deprecated codes: removed.
                Some(_) | None => {}
            }
        }
        match only_code {
            Some('i') => {
                if let Some(icon) = &entry.icon {
                    argv.push("--icon".to_owned());
                    argv.push(icon.clone());
                }
            }
            Some(_) => {}
            // A word that was nothing but a removed code goes away too.
            None if out.is_empty() && !word.is_empty() => {}
            None => argv.push(out),
        }
    }
    argv
}

/// Split on unquoted whitespace, honouring double quotes and the
/// backslash escapes the spec allows inside them.
fn split_exec(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quoted = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                in_word = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str, id: &str) -> Option<DesktopEntry> {
        parse(
            text,
            id.to_owned(),
            PathBuf::from(format!("/x/{id}.desktop")),
        )
    }

    #[test]
    fn parses_the_keys_we_need() {
        let e = entry(
            "[Desktop Entry]\nType=Application\nName=Firefox\nName[it]=Volpe\n\
             Icon=firefox\nExec=/usr/lib/firefox/firefox %u\nStartupWMClass=Firefox\n\
             [Desktop Action new-window]\nName=New Window\nIcon=other\n",
            "firefox",
        )
        .unwrap();
        assert_eq!(e.name, "Firefox");
        assert_eq!(e.icon.as_deref(), Some("firefox"));
        assert_eq!(e.exec.as_deref(), Some("firefox"));
        assert_eq!(e.wm_class.as_deref(), Some("firefox"));
        assert_eq!(e.exec_line.as_deref(), Some("/usr/lib/firefox/firefox %u"));
        assert!(!e.no_display);
        assert!(!e.terminal);
        assert!(e.comment.is_none());
    }

    #[test]
    fn exec_argv_expands_field_codes() {
        let e = entry(
            "[Desktop Entry]\nType=Application\nName=My App\nIcon=myicon\n\
             Exec=/opt/my\\ app/run \"a b\" %F --name=%c %i --k=%k 100%% %z\nTerminal=true\nPath=/tmp\nComment=Does things\n",
            "myapp",
        )
        .unwrap();
        assert!(e.terminal);
        assert_eq!(e.working_dir.as_deref(), Some(Path::new("/tmp")));
        assert_eq!(e.comment.as_deref(), Some("Does things"));
        assert_eq!(
            exec_argv(e.exec_line.as_deref().unwrap(), &e),
            [
                "/opt/my\\",
                "app/run",
                "a b",
                "--name=My App",
                "--icon",
                "myicon",
                "--k=/x/myapp.desktop",
                "100%",
            ]
        );
    }

    #[test]
    fn split_exec_quotes() {
        assert_eq!(split_exec("a  b"), ["a", "b"]);
        assert_eq!(split_exec("\"a b\" c"), ["a b", "c"]);
        assert_eq!(split_exec("\"a\\\"b\" \"\""), ["a\"b", ""]);
    }

    #[test]
    fn skips_non_apps_and_hidden() {
        assert!(entry("[Desktop Entry]\nType=Link\nName=x\n", "x").is_none());
        assert!(entry("[Desktop Entry]\nType=Application\nHidden=true\n", "x").is_none());
        let e = entry("[Desktop Entry]\nType=Application\nNoDisplay=true\n", "x").unwrap();
        assert!(e.no_display);
        assert_eq!(e.name, "x", "name falls back to the id");
    }

    #[test]
    fn exec_basenames() {
        assert_eq!(exec_basename("kitty").as_deref(), Some("kitty"));
        assert_eq!(
            exec_basename("/usr/bin/Code --foo %F").as_deref(),
            Some("code")
        );
        assert_eq!(
            exec_basename("env GDK_BACKEND=x11 app %u").as_deref(),
            Some("app")
        );
        assert_eq!(exec_basename(""), None);
    }

    #[test]
    fn class_lookup_order() {
        let mut db = DesktopDb::default();
        let mk = |id: &str, extra: &str| {
            entry(
                &format!("[Desktop Entry]\nType=Application\nName={id}\n{extra}"),
                id,
            )
            .unwrap()
        };
        db.insert(mk("org.kde.dolphin", "Exec=dolphin"));
        db.insert(mk(
            "code-oss",
            "Exec=/usr/bin/code-oss\nStartupWMClass=Code",
        ));
        db.insert(mk("firefox", "Exec=firefox"));
        assert_eq!(db.for_class("Firefox").unwrap().id, "firefox");
        assert_eq!(db.for_class("code").unwrap().id, "code-oss");
        assert_eq!(db.for_class("dolphin").unwrap().id, "org.kde.dolphin");
        assert_eq!(
            db.for_class("org.kde.dolphin").unwrap().id,
            "org.kde.dolphin"
        );
        assert!(db.for_class("nope").is_none());
        assert_eq!(db.for_class("CODE-OSS").unwrap().id, "code-oss");
        assert_eq!(db.len(), 3);
    }
}
