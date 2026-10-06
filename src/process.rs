//! Running external programs: the command lines of the config (a
//! `Custom` gadget's `command`, its `exec`) and the desktop entries the
//! launcher starts.
//!
//! A command line is a program and its arguments, split here with
//! shell-like quoting (see [`split_words`]) and run directly: no shell
//! in between, so no `$VAR`, pipes or redirections unless the user
//! writes `sh -c '...'` in the config.

use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use zbus::zvariant::Value;

/// Split a command line into words: on unquoted whitespace, with
/// single quotes taken literally, double quotes honouring `\"` and
/// `\\`, and a backslash outside quotes escaping the next character.
/// An unclosed quote runs to the end of the line.
pub fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    word.push(c);
                }
            }
            '"' => {
                in_word = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some(next @ ('"' | '\\')) => word.push(next),
                            Some(next) => {
                                word.push('\\');
                                word.push(next);
                            }
                            None => word.push('\\'),
                        },
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            c if c.is_whitespace() => {
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

/// Our own name in a command line (`command = aria-shell launcher
/// toggle`): run this very binary, on the PATH or not.
const SELF: &str = "aria-shell";

/// The [`Command`] for a config command line, or `None` when it's
/// empty.
pub fn command(line: &str) -> Option<Command> {
    let words = split_words(line);
    let (program, args) = words.split_first()?;
    let program = match (program.as_str(), std::env::current_exe()) {
        (SELF, Ok(me)) => me,
        _ => program.into(),
    };
    let mut cmd = Command::new(program);
    cmd.args(args);
    Some(cmd)
}

/// A program the config chooses (`[general] terminal`, `[Screenshot]
/// editor`, ...): `auto`, or no value, what `auto` finds; `none` or
/// `off` none; anything else the command line as written.
pub fn chosen(value: Option<&str>, auto: impl FnOnce() -> Option<String>) -> Option<String> {
    match value.map(str::trim).unwrap_or("") {
        "none" | "off" => None,
        "" | "auto" => auto(),
        line => Some(line.to_owned()),
    }
}

/// The first of `lines` (command lines) whose program is `installed`.
pub fn first_installed<'a>(lines: &[&'a str], installed: impl Fn(&str) -> bool) -> Option<&'a str> {
    lines
        .iter()
        .copied()
        .find(|line| installed(line.split_whitespace().next().unwrap_or(line)))
}

/// Whether `program` is on the PATH.
pub fn on_path(program: &str) -> bool {
    first_on_path(&[program]).is_some()
}

/// `line` split into words, `args` in place of every word that is
/// `placeholder`; when none is, `absent` then `args` last.
pub fn filled(line: &str, placeholder: &str, args: &[String], absent: &[&str]) -> Vec<String> {
    let mut argv = split_words(line);
    match argv.iter().position(|w| w == placeholder) {
        Some(_) => argv
            .into_iter()
            .flat_map(|w| match w == placeholder {
                true => args.to_vec(),
                false => vec![w],
            })
            .collect(),
        None => {
            argv.extend(absent.iter().map(|w| (*w).to_owned()));
            argv.extend_from_slice(args);
            argv
        }
    }
}

/// A command line run on `path` (an editor, a file manager): the path
/// in place of every `%f`, last when there's none.
pub fn on_file(line: &str, path: &Path) -> Vec<String> {
    filled(line, "%f", &[path.to_string_lossy().into_owned()], &[])
}

/// The terminals `[general] terminal = auto` tries, in order, after
/// `$TERMINAL`; `%c` is the program run inside.
pub const TERMINALS: &[&str] = &[
    "kitty %c",
    "alacritty -e %c",
    "foot %c",
    "terminology -e %c",
    "wezterm start -- %c",
    "ghostty -e %c",
    "gnome-terminal -- %c",
    "konsole -e %c",
    "xfce4-terminal -x %c",
    "xterm -e %c",
];

/// `[general] terminal = auto`: `$TERMINAL` (as [`TERMINALS`] has it
/// when it's one of them), else the first of [`TERMINALS`] on the PATH.
pub fn auto_terminal() -> Option<String> {
    let env = std::env::var("TERMINAL").ok();
    let terminal = pick_terminal(env.as_deref(), on_path);
    if terminal.is_none() {
        log::info!("no $TERMINAL and none of the terminals on the PATH");
    }
    terminal
}

fn pick_terminal(env: Option<&str>, installed: impl Fn(&str) -> bool) -> Option<String> {
    match env.map(str::trim).filter(|t| !t.is_empty()) {
        Some(env) => Some(
            first_installed(TERMINALS, |program| program == env)
                .unwrap_or(env)
                .to_owned(),
        ),
        None => first_installed(TERMINALS, installed).map(str::to_owned),
    }
}

/// `argv` run inside a terminal emulator: `terminal` is a command line
/// (`[general] terminal`), `argv` in place of `%c`, or `-e` and `argv`
/// last when there's none (as the desktop entry spec has terminals
/// take it).
pub fn in_terminal(terminal: &str, argv: &[String]) -> Vec<String> {
    filled(terminal, "%c", argv, &["-e"])
}

/// The file managers `[general] file_manager = auto` tries, in order;
/// `%f` is the directory.
pub const FILE_MANAGERS: &[&str] = &[
    "nautilus %f",
    "dolphin %f",
    "nemo %f",
    "thunar %f",
    "caja %f",
    "pcmanfm-qt %f",
    "pcmanfm %f",
];

/// `[general] file_manager = auto`: the first of [`FILE_MANAGERS`] on
/// the PATH.
pub fn auto_file_manager() -> Option<String> {
    let line = first_installed(FILE_MANAGERS, on_path);
    if line.is_none() {
        log::info!("none of the file managers on the PATH");
    }
    line.map(str::to_owned)
}

/// The first of `programs` found on the PATH.
pub fn first_on_path<'a>(programs: &[&'a str]) -> Option<&'a str> {
    let paths = std::env::var_os("PATH")?;
    programs
        .iter()
        .copied()
        .find(|p| std::env::split_paths(&paths).any(|dir| dir.join(p).is_file()))
}

/// [`spawn_detached`] for an argument vector, logging what happens.
pub fn run_argv(argv: &[String]) {
    let Some((program, args)) = argv.split_first() else {
        return;
    };
    let mut cmd = Command::new(program);
    cmd.args(args);
    let app = program_name(&cmd);
    match spawn_detached(cmd, &app) {
        Ok(()) => log::info!("ran {argv:?}"),
        Err(e) => log::warn!("can't run {argv:?}: {e}"),
    }
}

/// Start `cmd` detached from the shell: its own process group and no
/// stdio, so it outlives us and doesn't write on our log, and its own
/// systemd scope named after `app` (see [`move_to_scope`]); a thread
/// reaps it.
pub fn spawn_detached(mut cmd: Command, app: &str) -> io::Result<()> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = cmd.spawn()?;
    let app = app.to_owned();
    std::thread::spawn(move || {
        move_to_scope(child.id(), &app);
        let _ = child.wait();
    });
    Ok(())
}

/// [`spawn_detached`] for a config command line, logging what happens;
/// an empty line does nothing.
pub fn run(line: &str) {
    let Some(cmd) = command(line) else {
        return;
    };
    let app = program_name(&cmd);
    match spawn_detached(cmd, &app) {
        Ok(()) => log::info!("ran {line:?}"),
        Err(e) => log::warn!("can't run {line:?}: {e}"),
    }
}

/// The program's file name, what a command line's scope is named after.
fn program_name(cmd: &Command) -> String {
    Path::new(cmd.get_program())
        .file_name()
        .unwrap_or(cmd.get_program())
        .to_string_lossy()
        .into_owned()
}

/// Move `pid` out of the shell's cgroup into a scope of its own under
/// the user's systemd, `app-aria\x2dshell-<app>-<pid>.scope` in
/// `app.slice` (the desktop's naming for launched apps): otherwise
/// whatever the shell started would end with it when the shell is a
/// service (`systemctl --user restart aria-shell`). Best effort: with
/// no systemd user manager on the session bus it stays where it is. A
/// child that forks before the move leaves that fork behind, the race
/// of any launcher moving a pid after the spawn.
fn move_to_scope(pid: u32, app: &str) {
    static SESSION: OnceLock<Option<zbus::blocking::Connection>> = OnceLock::new();
    let session = SESSION.get_or_init(|| {
        zbus::blocking::Connection::session()
            .inspect_err(|e| log::debug!("no session bus, programs stay in our cgroup: {e}"))
            .ok()
    });
    let Some(conn) = session else {
        return;
    };
    let unit = scope_name(app, pid);
    let properties: Vec<(&str, Value)> = vec![
        ("PIDs", Value::from(vec![pid])),
        ("Slice", Value::from("app.slice")),
        ("CollectMode", Value::from("inactive-or-failed")),
    ];
    let aux: Vec<(&str, Vec<(&str, Value)>)> = Vec::new();
    if let Err(e) = conn.call_method(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        Some("org.freedesktop.systemd1.Manager"),
        "StartTransientUnit",
        &(unit.as_str(), "fail", properties, aux),
    ) {
        log::debug!("can't move {pid} to {unit}: {e}");
    }
}

/// `app-<launcher>-<app>-<pid>.scope`, each part escaped as systemd
/// wants a unit name's parts: what isn't `[A-Za-z0-9:_.]` (or a leading
/// `.`) becomes `\xNN`, so the dashes are left as separators.
fn scope_name(app: &str, pid: u32) -> String {
    fn escape(part: &str) -> String {
        part.bytes()
            .enumerate()
            .map(|(i, b)| match b {
                b'.' if i > 0 => ".".to_owned(),
                b if b.is_ascii_alphanumeric() || b == b':' || b == b'_' => {
                    char::from(b).to_string()
                }
                b => format!("\\x{b:02x}"),
            })
            .collect()
    }
    format!("app-{}-{}-{pid}.scope", escape(SELF), escape(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_a_shell() {
        assert_eq!(split_words("  a  b "), ["a", "b"]);
        assert_eq!(
            split_words("sh -c 'a | b \"c\"'"),
            ["sh", "-c", "a | b \"c\""]
        );
        assert_eq!(
            split_words(r#"x "a b" "q\"\\z" d\ e"#),
            ["x", "a b", "q\"\\z", "d e"]
        );
        assert_eq!(split_words(r#""a\nb""#), ["a\\nb"], "other escapes stay");
        assert_eq!(split_words("'' a"), ["", "a"], "an empty word");
        assert_eq!(
            split_words("'open"),
            ["open"],
            "unclosed quote runs to the end"
        );
        assert!(split_words("   ").is_empty());
    }

    #[test]
    fn terminal_wrapping() {
        let argv = ["btop".to_owned(), "-p".to_owned()];
        assert_eq!(
            in_terminal("kitty --single", &argv),
            ["kitty", "--single", "-e", "btop", "-p"]
        );
        assert_eq!(
            in_terminal("wezterm start -- %c", &argv),
            ["wezterm", "start", "--", "btop", "-p"]
        );
        assert_eq!(first_on_path(&["no-such-program-xyz", "sh"]), Some("sh"));
        assert_eq!(first_on_path(&["no-such-program-xyz"]), None);
    }

    #[test]
    fn chosen_none_off_auto_or_a_command_line() {
        let auto = || Some("found".to_owned());
        assert_eq!(chosen(Some("none"), auto), None);
        assert_eq!(chosen(Some("off"), auto), None);
        assert_eq!(chosen(None, auto).as_deref(), Some("found"));
        assert_eq!(chosen(Some("auto"), auto).as_deref(), Some("found"));
        assert_eq!(
            chosen(Some("  gimp --new  "), auto).as_deref(),
            Some("gimp --new")
        );
    }

    #[test]
    fn first_installed_by_program() {
        let lines = ["a -x %f", "b %f", "c"];
        assert_eq!(first_installed(&lines, |_| false), None);
        assert_eq!(first_installed(&lines, |_| true), Some("a -x %f"));
        assert_eq!(
            first_installed(&lines, |p| p == "b" || p == "c"),
            Some("b %f")
        );
        assert!(TERMINALS.iter().all(|line| line.contains("%c")));
        assert!(FILE_MANAGERS.iter().all(|line| line.contains("%f")));
    }

    #[test]
    fn terminal_from_the_environment_first() {
        assert_eq!(
            pick_terminal(Some("foot"), |_| true).as_deref(),
            Some("foot %c")
        );
        assert_eq!(
            pick_terminal(Some("myterm --x"), |_| true).as_deref(),
            Some("myterm --x")
        );
        assert_eq!(
            pick_terminal(Some(" "), |p| p == "foot").as_deref(),
            Some("foot %c")
        );
        assert_eq!(pick_terminal(None, |_| false), None);
    }

    #[test]
    fn on_file_puts_the_path_for_every_percent_f() {
        let path = Path::new("/tmp/my shots/a.png");
        assert_eq!(
            on_file("satty --filename %f --output-filename %f", path),
            [
                "satty",
                "--filename",
                "/tmp/my shots/a.png",
                "--output-filename",
                "/tmp/my shots/a.png"
            ]
        );
    }

    #[test]
    fn on_file_without_percent_f_appends_the_path() {
        let path = Path::new("/tmp/a.png");
        assert_eq!(
            on_file("satty --filename", path),
            ["satty", "--filename", "/tmp/a.png"]
        );
        assert_eq!(
            on_file("sh -c 'echo \"$1\"' sh", path),
            ["sh", "-c", "echo \"$1\"", "sh", "/tmp/a.png"]
        );
    }

    #[test]
    fn placeholders_only_whole_words() {
        let path = Path::new("/tmp/a.png");
        assert_eq!(
            on_file("tool --in=%f", path),
            ["tool", "--in=%f", "/tmp/a.png"]
        );
    }

    #[test]
    fn scope_names() {
        assert_eq!(
            scope_name("org.mozilla.firefox", 42),
            r"app-aria\x2dshell-org.mozilla.firefox-42.scope"
        );
        assert_eq!(
            scope_name("nm-applet", 7),
            r"app-aria\x2dshell-nm\x2dapplet-7.scope"
        );
        assert_eq!(
            scope_name(".x y", 1),
            r"app-aria\x2dshell-\x2ex\x20y-1.scope"
        );
    }

    /// Talks to the user's systemd: `cargo test -- --ignored scope`.
    #[test]
    #[ignore]
    fn spawned_in_its_own_scope() {
        let mut cmd = Command::new("sleep");
        cmd.arg("2");
        let mut child = cmd.spawn().unwrap();
        move_to_scope(child.id(), "sleep");
        // The call queues a job: the move lands a moment later.
        let wanted = format!("/app.slice/{}", scope_name("sleep", child.id()));
        let mut cgroup = String::new();
        for _ in 0..20 {
            cgroup = std::fs::read_to_string(format!("/proc/{}/cgroup", child.id())).unwrap();
            if cgroup.trim_end().ends_with(&wanted) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(cgroup.trim_end().ends_with(&wanted), "{cgroup}");
    }

    #[test]
    fn empty_line_is_no_command() {
        assert!(command("").is_none());
        assert_eq!(command("ls -l").unwrap().get_program(), "ls");
        assert_eq!(
            command("aria-shell ping").unwrap().get_program(),
            std::env::current_exe().unwrap().as_os_str(),
            "our name is this binary"
        );
    }
}
