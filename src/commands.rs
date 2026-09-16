//! The command socket: how a keybind or a terminal talks to the running
//! shell (`aria-shell launcher toggle`).
//!
//! Same protocol as the Python implementation: a unix socket at
//! `$XDG_RUNTIME_DIR/aria-shell/cmd.sock`, one command per line, one
//! reply line per command starting with `OK` or `ERR`. Commands are
//! parsed here, in the listener: an unknown one is refused on the spot,
//! a valid one is acknowledged and delivered to the daemon as a
//! [`Command`] (the reply doesn't wait for it to be carried out).
//!
//! The same binary is the client: with arguments, `main` sends them as
//! one line and prints the reply.
//!
//! `debug` commands are answered by the daemon: they carry a [`Reply`]
//! channel and the listener waits for the text. They exist so a test
//! driver can ask the shell where things are without going through
//! compositor-specific tools (`hyprctl layers`), see RS-PORT.md.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use std::time::Duration;

use iced::Subscription;
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::stream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

#[derive(Debug, Clone)]
pub enum Command {
    Launcher(LauncherCommand),
    /// Answered through the channel.
    Debug(DebugCommand, Reply),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugCommand {
    /// Every surface the shell has open, with its global rectangle.
    Surfaces,
    /// Where the pointer was last seen over one of our surfaces.
    Cursor,
}

/// Where the daemon writes the answer to a [`Command::Debug`].
#[derive(Debug, Clone)]
pub struct Reply(mpsc::Sender<String>);

impl Reply {
    pub fn send(mut self, text: impl Into<String>) {
        let _ = self.0.try_send(text.into());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherCommand {
    Toggle,
    Show,
    Hide,
}

/// What a received line turns into.
#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    /// Deliver to the daemon (and reply `OK`).
    Launcher(LauncherCommand),
    /// Deliver to the daemon and relay its answer.
    Debug(DebugCommand),
    /// Answered by the listener itself.
    Reply(String),
}

/// `aria <cmd> [args]` is accepted too, it's how the Python config
/// spelled commands.
fn parse(line: &str) -> Result<Parsed, String> {
    let line = line.trim();
    let line = line.strip_prefix("aria ").unwrap_or(line);
    let mut words = line.split_whitespace();
    let Some(name) = words.next() else {
        return Err("empty command".to_owned());
    };
    let args: Vec<&str> = words.collect();
    match name {
        "ping" => Ok(Parsed::Reply(
            format!("pong {}", args.join(" ")).trim().to_owned(),
        )),
        "launcher" => {
            let cmd = match args.as_slice() {
                [] | ["toggle"] => LauncherCommand::Toggle,
                ["show"] => LauncherCommand::Show,
                ["hide"] => LauncherCommand::Hide,
                _ => {
                    return Err(format!(
                        "invalid arguments for <launcher>: {}",
                        args.join(" ")
                    ));
                }
            };
            Ok(Parsed::Launcher(cmd))
        }
        "debug" => match args.as_slice() {
            ["surfaces"] => Ok(Parsed::Debug(DebugCommand::Surfaces)),
            ["cursor"] => Ok(Parsed::Debug(DebugCommand::Cursor)),
            _ => Err(format!(
                "invalid arguments for <debug>: {} (surfaces | cursor)",
                args.join(" ")
            )),
        },
        other => Err(format!("unknown command <{other}>")),
    }
}

/// `$XDG_RUNTIME_DIR/aria-shell/cmd.sock`.
pub fn socket_path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(dir).join("aria-shell").join("cmd.sock"))
}

/// The daemon side: every command received, for as long as the shell
/// runs.
pub fn listen() -> Subscription<Command> {
    Subscription::run(serve)
}

fn serve() -> impl Stream<Item = Command> {
    stream::channel(16, async move |tx| {
        let Some(path) = socket_path() else {
            log::error!("XDG_RUNTIME_DIR not set, no command socket");
            return;
        };
        if let Some(dir) = path.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            log::error!("cannot create {}: {e}", dir.display());
            return;
        }
        // A stale socket from a previous run.
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                log::error!("cannot listen on {}: {e}", path.display());
                return;
            }
        };
        log::info!("listening for commands on {}", path.display());
        loop {
            match listener.accept().await {
                Ok((conn, _)) => {
                    tokio::spawn(handle(conn, tx.clone()));
                }
                Err(e) => log::error!("command socket: accept failed: {e}"),
            }
        }
    })
}

/// One client: a line in, a reply out, until it hangs up.
async fn handle(conn: UnixStream, mut tx: mpsc::Sender<Command>) {
    let (read, mut write) = conn.into_split();
    let mut lines = tokio::io::BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        log::debug!("command: {line:?}");
        let reply = match parse(&line) {
            Ok(Parsed::Launcher(cmd)) => {
                let _ = tx.send(Command::Launcher(cmd)).await;
                "OK".to_owned()
            }
            Ok(Parsed::Debug(cmd)) => {
                let (reply_tx, mut reply_rx) = mpsc::channel(1);
                let _ = tx.send(Command::Debug(cmd, Reply(reply_tx))).await;
                match tokio::time::timeout(Duration::from_secs(2), reply_rx.next()).await {
                    Ok(Some(text)) => format!("OK {text}").trim_end().to_owned(),
                    _ => "ERR no answer from the shell".to_owned(),
                }
            }
            Ok(Parsed::Reply(text)) => format!("OK {text}"),
            Err(e) => format!("ERR {e}"),
        };
        if write
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .is_err()
        {
            break;
        }
    }
}

/// The client side: send `words` as one command, return the reply
/// (`Ok` for `OK ...`, `Err` for `ERR ...` or a connection problem).
pub fn send(words: &[String]) -> Result<String, String> {
    let path = socket_path().ok_or("XDG_RUNTIME_DIR not set")?;
    let mut sock = std::os::unix::net::UnixStream::connect(&path).map_err(|e| {
        format!(
            "cannot connect to {}: {e} (is aria-shell running?)",
            path.display()
        )
    })?;
    sock.write_all(format!("{}\n", words.join(" ")).as_bytes())
        .map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(&sock)
        .read_line(&mut reply)
        .map_err(|e| e.to_string())?;
    let reply = reply.trim();
    match reply.split_once(' ').unwrap_or((reply, "")) {
        ("OK", rest) => Ok(rest.to_owned()),
        ("ERR", rest) => Err(rest.to_owned()),
        _ => Err(format!("bad reply {reply:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_launcher_commands() {
        assert_eq!(
            parse("launcher"),
            Ok(Parsed::Launcher(LauncherCommand::Toggle))
        );
        assert_eq!(
            parse("  aria launcher toggle \n"),
            Ok(Parsed::Launcher(LauncherCommand::Toggle))
        );
        assert_eq!(
            parse("launcher show"),
            Ok(Parsed::Launcher(LauncherCommand::Show))
        );
        assert_eq!(
            parse("launcher hide"),
            Ok(Parsed::Launcher(LauncherCommand::Hide))
        );
        assert!(parse("launcher what").is_err());
    }

    #[test]
    fn parses_debug_commands() {
        assert_eq!(
            parse("debug surfaces"),
            Ok(Parsed::Debug(DebugCommand::Surfaces))
        );
        assert_eq!(
            parse("debug cursor"),
            Ok(Parsed::Debug(DebugCommand::Cursor))
        );
        assert!(parse("debug").is_err());
        assert!(parse("debug nope").is_err());
    }

    #[test]
    fn ping_and_errors() {
        assert_eq!(parse("ping"), Ok(Parsed::Reply("pong".into())));
        assert_eq!(parse("ping a b"), Ok(Parsed::Reply("pong a b".into())));
        assert!(parse("").is_err());
        assert!(parse("nope").is_err());
    }
}
