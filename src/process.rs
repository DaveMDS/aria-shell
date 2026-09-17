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
use std::process::{Command, Stdio};

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
    match spawn_detached(cmd) {
        Ok(()) => log::info!("ran {argv:?}"),
        Err(e) => log::warn!("can't run {argv:?}: {e}"),
    }
}

/// Start `cmd` detached from the shell: its own process group and no
/// stdio, so it outlives us and doesn't write on our log; a thread
/// reaps it.
pub fn spawn_detached(mut cmd: Command) -> io::Result<()> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || {
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
    match spawn_detached(cmd) {
        Ok(()) => log::info!("ran {line:?}"),
        Err(e) => log::warn!("can't run {line:?}: {e}"),
    }
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
