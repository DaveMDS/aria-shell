//! A media player for the UI scenarios: owns
//! `org.mpris.MediaPlayer2.aria-test` on the session bus, serves the
//! `org.mpris.MediaPlayer2` and `.Player` interfaces, and reports what
//! the shell asks of it on stdout, one line each:
//!
//!   play-pause | next | previous | volume <0..1>
//!
//! Stdin drives changes (one command per line, `PropertiesChanged`
//! each): `status <Playing|Paused|Stopped>`, `title <text>`,
//! `artist <text>`, `volume <0..1>`. `ready` is printed once the name
//! is owned.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, BufReader};
use zbus::interface;
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::zvariant::{OwnedValue, Value};

const NAME: &str = "org.mpris.MediaPlayer2.aria-test";
const PATH: &str = "/org/mpris/MediaPlayer2";

#[derive(Clone)]
struct State {
    status: String,
    title: String,
    artist: String,
    volume: f64,
}

struct Root;

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        "Aria test player".to_owned()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        "aria-test".to_owned()
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }
}

struct Player {
    state: Arc<Mutex<State>>,
}

impl Player {
    fn report(line: &str) {
        println!("{line}");
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play_pause(&self) {
        Self::report("play-pause");
    }

    fn next(&self) {
        Self::report("next");
    }

    fn previous(&self) {
        Self::report("previous");
    }

    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.state.lock().unwrap().status.clone()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let s = self.state.lock().unwrap();
        let mut md = HashMap::new();
        let put = |md: &mut HashMap<String, OwnedValue>, key: &str, v: Value<'static>| {
            md.insert(key.to_owned(), OwnedValue::try_from(v).unwrap());
        };
        put(&mut md, "xesam:title", s.title.clone().into());
        put(
            &mut md,
            "xesam:artist",
            Value::from(vec![s.artist.clone()]),
        );
        put(&mut md, "xesam:album", "Test album".into());
        md
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.state.lock().unwrap().volume
    }

    #[zbus(property)]
    fn set_volume(&mut self, volume: f64) {
        self.state.lock().unwrap().volume = volume;
        Self::report(&format!("volume {volume}"));
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> zbus::Result<()> {
    let state = Arc::new(Mutex::new(State {
        status: "Paused".to_owned(),
        title: "Test song".to_owned(),
        artist: "Test artist".to_owned(),
        volume: 0.5,
    }));
    let conn = zbus::connection::Builder::session()?
        .name(NAME)?
        .serve_at(PATH, Root)?
        .serve_at(
            PATH,
            Player {
                state: state.clone(),
            },
        )?
        .build()
        .await?;
    println!("ready");

    let player: InterfaceRef<Player> = conn.object_server().interface(PATH).await?;
    let emitter: &SignalEmitter<'_> = player.signal_emitter();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["status", s] => {
                state.lock().unwrap().status = (*s).to_owned();
                player.get().await.playback_status_changed(emitter).await?;
            }
            ["title", rest @ ..] => {
                state.lock().unwrap().title = rest.join(" ");
                player.get().await.metadata_changed(emitter).await?;
            }
            ["artist", rest @ ..] => {
                state.lock().unwrap().artist = rest.join(" ");
                player.get().await.metadata_changed(emitter).await?;
            }
            ["volume", v] => {
                state.lock().unwrap().volume = v.parse().unwrap_or(0.0);
                player.get().await.volume_changed(emitter).await?;
            }
            ["quit"] => break,
            _ => eprintln!("unknown command: {line}"),
        }
        println!("ok");
    }
    Ok(())
}
