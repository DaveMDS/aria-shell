//! The command socket: how a keybind or a terminal talks to the running
//! shell (`aria-shell launcher toggle`).
//!
//! Same protocol as the Python implementation: a unix socket at
//! `$XDG_RUNTIME_DIR/aria-shell/<WAYLAND_DISPLAY>.sock` (one shell per
//! display: a test shell in a nested compositor doesn't take over the
//! desktop's), one command per line, one reply line per command
//! starting with `OK` or `ERR`. Commands are
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
//! compositor-specific tools (`hyprctl layers`), see ARCHITECTURE.md.

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
    Launcher(ToggleCommand),
    /// The exit menu (`aria-shell exiter toggle`).
    Exiter(ToggleCommand),
    /// Lock the session (`aria-shell lock`).
    Lock,
    /// Hold idle or let it go (`aria-shell idle inhibit [toggle|on|off]`).
    Idle(crate::idle::Command),
    /// Show the OSD (`aria-shell osd show ...`).
    Osd(crate::osd::Content),
    /// Answered through the channel.
    Debug(DebugCommand, Reply),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugCommand {
    /// Every surface the shell has open, with its global rectangle.
    Surfaces,
    /// Where the pointer was last seen over one of our surfaces.
    Cursor,
    /// The theme in use: `style=<name|-> scheme=<light|dark>`.
    Theme,
    /// The language in use: `lang=<code> dates=<locale>`.
    Locale,
    /// The system monitor's last reading, in numbers.
    SysMon,
    Audio,
    /// The network: devices, networks around, profiles, active connections.
    Network,
    /// The idle stages: power source, holds, screens, timers.
    Idle,
    /// The battery, the peripherals, the power profiles.
    Power,
    /// Every themed widget (element path + global rectangle), or those
    /// whose path contains the filter.
    Widgets(Option<String>),
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
pub enum ToggleCommand {
    Toggle,
    Show,
    Hide,
}

impl ToggleCommand {
    /// `<name> [toggle|show|hide]`.
    fn parse(name: &str, args: &[&str]) -> Result<Self, String> {
        match args {
            [] | ["toggle"] => Ok(Self::Toggle),
            ["show"] => Ok(Self::Show),
            ["hide"] => Ok(Self::Hide),
            _ => Err(format!(
                "invalid arguments for <{name}>: {} (toggle | show | hide)",
                args.join(" ")
            )),
        }
    }
}

/// What a received line turns into.
#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    /// Deliver to the daemon (and reply `OK`).
    Launcher(ToggleCommand),
    Exiter(ToggleCommand),
    Lock,
    Idle(crate::idle::Command),
    Osd(crate::osd::Content),
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
        "launcher" => Ok(Parsed::Launcher(ToggleCommand::parse(name, &args)?)),
        "exiter" => Ok(Parsed::Exiter(ToggleCommand::parse(name, &args)?)),
        "lock" => match args.as_slice() {
            [] => Ok(Parsed::Lock),
            _ => Err(format!("invalid arguments for <lock>: {}", args.join(" "))),
        },
        "idle" => match args.as_slice() {
            ["inhibit"] | ["inhibit", "toggle"] => {
                Ok(Parsed::Idle(crate::idle::Command::ToggleInhibit))
            }
            ["inhibit", "on"] => Ok(Parsed::Idle(crate::idle::Command::SetInhibit(true))),
            ["inhibit", "off"] => Ok(Parsed::Idle(crate::idle::Command::SetInhibit(false))),
            _ => Err(format!(
                "invalid arguments for <idle>: {} (inhibit [toggle | on | off])",
                args.join(" ")
            )),
        },
        "osd" => match args.split_first() {
            Some((&"show", rest)) => Ok(Parsed::Osd(parse_osd_show(rest)?)),
            _ => Err(format!(
                "invalid arguments for <osd>: {} (show [--icon <name>] [--value <percent>] [text])",
                args.join(" ")
            )),
        },
        "debug" => match args.as_slice() {
            ["surfaces"] => Ok(Parsed::Debug(DebugCommand::Surfaces)),
            ["cursor"] => Ok(Parsed::Debug(DebugCommand::Cursor)),
            ["theme"] => Ok(Parsed::Debug(DebugCommand::Theme)),
            ["locale"] => Ok(Parsed::Debug(DebugCommand::Locale)),
            ["sysmon"] => Ok(Parsed::Debug(DebugCommand::SysMon)),
            ["audio"] => Ok(Parsed::Debug(DebugCommand::Audio)),
            ["network"] => Ok(Parsed::Debug(DebugCommand::Network)),
            ["idle"] => Ok(Parsed::Debug(DebugCommand::Idle)),
            ["power"] => Ok(Parsed::Debug(DebugCommand::Power)),
            ["widgets"] => Ok(Parsed::Debug(DebugCommand::Widgets(None))),
            ["widgets", filter @ ..] => {
                Ok(Parsed::Debug(DebugCommand::Widgets(Some(filter.join(" ")))))
            }
            _ => Err(format!(
                "invalid arguments for <debug>: {} (surfaces | cursor | theme | locale | sysmon | audio | network | idle | power | widgets [filter])",
                args.join(" ")
            )),
        },
        other => Err(format!("unknown command <{other}>")),
    }
}

/// `osd show [--icon <name>] [--value <percent>] [text...]`: the
/// options first, then the text (every word left; the client sends its
/// arguments as one line, so this is how a text with spaces comes).
fn parse_osd_show(args: &[&str]) -> Result<crate::osd::Content, String> {
    let mut icon = None;
    let mut value = None;
    let mut words = args.iter();
    let mut text = Vec::new();
    while let Some(&word) = words.next() {
        match word {
            "--icon" => match words.next() {
                Some(name) => icon = Some((*name).to_owned()),
                None => return Err("osd show: --icon needs a name".to_owned()),
            },
            "--value" => match words.next().map(|v| v.parse::<u32>()) {
                Some(Ok(v)) => value = Some(v),
                _ => return Err("osd show: --value needs a percent (0, 40, 120)".to_owned()),
            },
            _ => {
                text.push(word);
                text.extend(words.by_ref());
            }
        }
    }
    let text = (!text.is_empty()).then(|| text.join(" "));
    if icon.is_none() && value.is_none() && text.is_none() {
        return Err(
            "osd show: nothing to show (--icon <name>, --value <percent>, text)".to_owned(),
        );
    }
    Ok(crate::osd::Content {
        kind: crate::osd::Kind::Custom,
        icon,
        value,
        text,
        muted: false,
    })
}

/// `$XDG_RUNTIME_DIR/aria-shell/<WAYLAND_DISPLAY>.sock`, so the shell
/// and its client agree on which compositor they mean.
pub fn socket_path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_owned());
    Some(
        PathBuf::from(dir)
            .join("aria-shell")
            .join(format!("{display}.sock")),
    )
}

/// Held for as long as the shell runs: one shell per display.
pub struct Instance(#[allow(dead_code)] std::fs::File);

/// Take the display's lock (`<WAYLAND_DISPLAY>.lock` next to the socket),
/// or say who has it. A lock rather than asking the socket: two shells
/// started together (a compositor's autostart and a terminal) can't
/// both get it, and it goes with its process, however that ends.
pub fn single_instance() -> Result<Instance, String> {
    let socket = socket_path().ok_or("XDG_RUNTIME_DIR not set")?;
    let path = socket.with_extension("lock");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Instance(file)),
        Err(std::fs::TryLockError::WouldBlock) => Err(format!(
            "aria-shell is already running on this display (it holds {})",
            path.display()
        )),
        Err(std::fs::TryLockError::Error(e)) => Err(format!("cannot lock {}: {e}", path.display())),
    }
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
            Ok(Parsed::Exiter(cmd)) => {
                let _ = tx.send(Command::Exiter(cmd)).await;
                "OK".to_owned()
            }
            Ok(Parsed::Lock) => {
                let _ = tx.send(Command::Lock).await;
                "OK".to_owned()
            }
            Ok(Parsed::Idle(cmd)) => {
                let _ = tx.send(Command::Idle(cmd)).await;
                "OK".to_owned()
            }
            Ok(Parsed::Osd(content)) => {
                let _ = tx.send(Command::Osd(content)).await;
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
            Ok(Parsed::Launcher(ToggleCommand::Toggle))
        );
        assert_eq!(
            parse("  aria launcher toggle \n"),
            Ok(Parsed::Launcher(ToggleCommand::Toggle))
        );
        assert_eq!(
            parse("launcher show"),
            Ok(Parsed::Launcher(ToggleCommand::Show))
        );
        assert_eq!(
            parse("launcher hide"),
            Ok(Parsed::Launcher(ToggleCommand::Hide))
        );
        assert!(parse("launcher what").is_err());
        assert_eq!(parse("exiter"), Ok(Parsed::Exiter(ToggleCommand::Toggle)));
        assert_eq!(
            parse("exiter hide"),
            Ok(Parsed::Exiter(ToggleCommand::Hide))
        );
        assert!(parse("exiter now").is_err());
    }

    #[test]
    fn parses_lock() {
        assert_eq!(parse("lock"), Ok(Parsed::Lock));
        assert_eq!(parse("aria lock"), Ok(Parsed::Lock));
        assert!(parse("lock now").is_err());
    }

    #[test]
    fn parses_osd_show() {
        let custom = |icon: Option<&str>, value, text: Option<&str>| {
            Ok(Parsed::Osd(crate::osd::Content {
                kind: crate::osd::Kind::Custom,
                icon: icon.map(str::to_owned),
                value,
                text: text.map(str::to_owned),
                muted: false,
            }))
        };
        assert_eq!(
            parse("osd show --icon display-brightness-symbolic --value 40"),
            custom(Some("display-brightness-symbolic"), Some(40), None)
        );
        assert_eq!(
            parse("osd show Caps Lock on"),
            custom(None, None, Some("Caps Lock on"))
        );
        assert_eq!(
            parse("osd show --value 120 Boost --value"),
            custom(None, Some(120), Some("Boost --value"))
        );
        assert!(parse("osd show").is_err());
        assert!(parse("osd show --icon").is_err());
        assert!(parse("osd show --value loud").is_err());
        assert!(parse("osd").is_err());
        assert!(parse("osd hide").is_err());
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
