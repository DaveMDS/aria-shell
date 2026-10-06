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

/// `argv` run inside a terminal emulator: `terminal` is a command line
/// (`[launcher] terminal`), given `-e` and the program, as the desktop
/// entry spec has terminals take.
pub fn in_terminal(terminal: &str, argv: Vec<String>) -> Vec<String> {
    let mut term: Vec<String> = terminal.split_whitespace().map(str::to_owned).collect();
    term.push("-e".to_owned());
    term.extend(argv);
    term
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
        assert_eq!(
            in_terminal("kitty --single", vec!["btop".into()]),
            ["kitty", "--single", "-e", "btop"]
        );
        assert_eq!(first_on_path(&["no-such-program-xyz", "sh"]), Some("sh"));
        assert_eq!(first_on_path(&["no-such-program-xyz"]), None);
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
