//! Audio: the mixer (outputs, inputs and the streams playing, with
//! their volume and mute) and the media players.
//!
//! One [`Audio`] lives in the daemon, shaped like `Tray`: the
//! [`Audio::subscription`] is the mixer connection (pulse.rs, libpulse
//! against `pipewire-pulse` or PulseAudio) plus the players' bus
//! watcher (mpris.rs); the [`Event`]s they yield go through
//! [`Audio::apply`], gadgets read the resulting [`Channel`]s and
//! [`Player`]s from their view context and act on them with a
//! [`Command`] the daemon runs with [`Audio::run`].

mod mpris;
mod pulse;

use std::collections::HashMap;
use std::path::PathBuf;

use iced::{Subscription, Task};
use zbus::Connection;

use crate::icons::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A sink: speakers, headphones, ...
    Output,
    /// A source: a microphone, ...
    Input,
    /// An application's stream playing to a sink.
    Stream,
}

/// A mixer channel, as last reported. Sinks, sources and streams are
/// numbered apart: `(kind, index)` is the identity.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub kind: Kind,
    pub index: u32,
    /// The server's name (`alsa_output.pci-...`), what the default is
    /// named by; a stream's media name.
    pub name: String,
    /// What to show: the device's description, the stream's application.
    pub label: String,
    /// An icon name the server suggested (`device.icon_name`,
    /// `application.icon_name`), if any.
    pub icon: Option<String>,
    /// A stream's application name, to find its desktop entry's icon.
    pub app: Option<String>,
    /// 1.0 is 100%; may go above (up to ~1.5).
    pub volume: f32,
    pub muted: bool,
    /// How many audio channels (left, right, ...) it has.
    pub channels: u8,
    /// Whether the volume can be set (some streams say no).
    pub has_volume: bool,
    /// The default device of its kind.
    pub default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

/// A media player, as last reported over MPRIS.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    /// Its bus name: `org.mpris.MediaPlayer2.spotify`.
    pub bus: String,
    pub identity: String,
    /// Its desktop entry id, to find its icon.
    pub desktop_entry: Option<String>,
    pub status: PlaybackStatus,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The cover's URL.
    pub art: Option<String>,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_play: bool,
    pub can_pause: bool,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// The mixer is up; its commands can be sent.
    Connected(pulse::Handle),
    Disconnected,
    /// A channel appeared or changed.
    Channel(Channel),
    ChannelGone(Kind, u32),
    /// The default devices, by name.
    Defaults {
        sink: String,
        source: String,
    },
    /// The session bus is up; player commands can be sent.
    Bus(Connection),
    /// A player appeared or changed.
    Player(Player),
    PlayerGone(String),
}

#[derive(Debug, Clone)]
pub enum Command {
    /// A channel's volume, 1.0 being 100%.
    SetVolume(Kind, u32, f32),
    SetMuted(Kind, u32, bool),
    /// Make a device the default of its kind.
    SetDefault(Kind, u32),
    /// The default device's volume, by this much (clamped to `max`).
    StepDefault {
        kind: Kind,
        delta: f32,
        max: f32,
    },
    /// Mute or unmute the default device.
    ToggleDefaultMute(Kind),
    PlayPause(String),
    Next(String),
    Previous(String),
}

#[derive(Default)]
pub struct Audio {
    mixer: Option<pulse::Handle>,
    bus: Option<Connection>,
    /// Outputs, then inputs, then streams, each group in the order the
    /// server listed them.
    channels: Vec<Channel>,
    default_sink: String,
    default_source: String,
    /// In appearance order.
    players: Vec<Player>,
    /// The players' covers, by bus name: the URL loaded and its icon
    /// (a new handle per change, not per view: iced caches by handle).
    covers: HashMap<String, (String, Icon)>,
}

impl Audio {
    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::batch([
            Subscription::run(pulse::events),
            Subscription::run(mpris::events),
        ])
    }

    /// Apply an event; whether what gadgets see changed (the server
    /// reports a stream "changed" several times a second while it
    /// plays, mostly with nothing new for us).
    pub fn apply(&mut self, event: Event) -> bool {
        match event {
            Event::Connected(handle) => {
                self.mixer = Some(handle);
                true
            }
            Event::Disconnected => {
                self.mixer = None;
                let had = !self.channels.is_empty();
                self.channels.clear();
                had
            }
            Event::Channel(mut channel) => {
                channel.default = self.is_default(&channel);
                match self
                    .channels
                    .iter_mut()
                    .find(|c| c.kind == channel.kind && c.index == channel.index)
                {
                    Some(old) => {
                        if *old == channel {
                            return false;
                        }
                        *old = channel;
                        true
                    }
                    None => {
                        // Keep the groups together: after the last of
                        // its kind, or of the kinds before it.
                        let at = self
                            .channels
                            .iter()
                            .rposition(|c| c.kind as u8 <= channel.kind as u8)
                            .map_or(0, |i| i + 1);
                        self.channels.insert(at, channel);
                        true
                    }
                }
            }
            Event::ChannelGone(kind, index) => {
                let before = self.channels.len();
                self.channels
                    .retain(|c| !(c.kind == kind && c.index == index));
                self.channels.len() != before
            }
            Event::Defaults { sink, source } => {
                if self.default_sink == sink && self.default_source == source {
                    return false;
                }
                self.default_sink = sink;
                self.default_source = source;
                for c in &mut self.channels {
                    c.default = match c.kind {
                        Kind::Output => c.name == self.default_sink,
                        Kind::Input => c.name == self.default_source,
                        Kind::Stream => false,
                    };
                }
                true
            }
            Event::Bus(conn) => {
                self.bus = Some(conn);
                true
            }
            Event::Player(player) => {
                self.load_cover(&player);
                match self.players.iter_mut().find(|p| p.bus == player.bus) {
                    Some(old) => {
                        if *old == player {
                            return false;
                        }
                        *old = player;
                        true
                    }
                    None => {
                        self.players.push(player);
                        true
                    }
                }
            }
            Event::PlayerGone(bus) => {
                self.covers.remove(&bus);
                let before = self.players.len();
                self.players.retain(|p| p.bus != bus);
                self.players.len() != before
            }
        }
    }

    fn is_default(&self, c: &Channel) -> bool {
        match c.kind {
            Kind::Output => !c.name.is_empty() && c.name == self.default_sink,
            Kind::Input => !c.name.is_empty() && c.name == self.default_source,
            Kind::Stream => false,
        }
    }

    /// A `file://` cover becomes an icon once per URL; other schemes
    /// would need fetching and are left out.
    fn load_cover(&mut self, player: &Player) {
        let Some(url) = &player.art else {
            self.covers.remove(&player.bus);
            return;
        };
        if self.covers.get(&player.bus).is_some_and(|(u, _)| u == url) {
            return;
        }
        match url.strip_prefix("file://") {
            Some(path) => {
                let path = PathBuf::from(percent_decode(path));
                self.covers
                    .insert(player.bus.clone(), (url.clone(), Icon::from_path(path)));
            }
            None => {
                log::debug!("audio: cover {url:?} of {} not loaded", player.bus);
                self.covers.remove(&player.bus);
            }
        }
    }

    #[cfg(test)]
    fn channels(&self) -> &[Channel] {
        &self.channels
    }

    /// `debug audio`: one line per channel and player.
    pub fn describe(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        if self.mixer.is_none() {
            lines.push("mixer: not connected".to_owned());
        }
        for c in &self.channels {
            lines.push(format!(
                "{:?} {} {:?} volume={:.0}%{}{}",
                c.kind,
                c.index,
                c.label,
                c.volume * 100.0,
                if c.muted { " muted" } else { "" },
                if c.default { " default" } else { "" },
            ));
        }
        for p in &self.players {
            lines.push(format!(
                "player {} {:?} {:?} title={:?} artist={:?}",
                p.bus, p.identity, p.status, p.title, p.artist,
            ));
        }
        lines.join("; ")
    }

    pub fn channels_of(&self, kind: Kind) -> impl Iterator<Item = &Channel> {
        self.channels.iter().filter(move |c| c.kind == kind)
    }

    /// The default device of a kind, if known.
    pub fn default_of(&self, kind: Kind) -> Option<&Channel> {
        self.channels_of(kind).find(|c| c.default)
    }

    pub fn players(&self) -> &[Player] {
        &self.players
    }

    pub fn cover(&self, bus: &str) -> Option<&Icon> {
        self.covers.get(bus).map(|(_, icon)| icon)
    }

    /// Icon names the channels suggest, for the daemon to resolve.
    pub fn icon_names(&self) -> impl Iterator<Item = &str> {
        self.channels.iter().filter_map(|c| c.icon.as_deref())
    }

    /// Classes (application names, desktop ids) whose icons the daemon
    /// should resolve from the desktop entries.
    pub fn app_classes(&self) -> impl Iterator<Item = &str> {
        self.channels.iter().filter_map(|c| c.app.as_deref()).chain(
            self.players
                .iter()
                .filter_map(|p| p.desktop_entry.as_deref()),
        )
    }

    pub fn run(&self, command: Command) -> Task<Event> {
        use pulse::Request;
        let channel = |kind: Kind, index: u32| {
            self.channels
                .iter()
                .find(|c| c.kind == kind && c.index == index)
        };
        let mixer = |request: Request| match &self.mixer {
            Some(handle) => handle.send(request),
            None => log::warn!("audio: no mixer connection, dropping {request:?}"),
        };
        match command {
            Command::SetVolume(kind, index, volume) => {
                if let Some(c) = channel(kind, index) {
                    mixer(Request::SetVolume {
                        kind,
                        index,
                        channels: c.channels,
                        volume,
                    });
                }
            }
            Command::SetMuted(kind, index, mute) => mixer(Request::SetMute { kind, index, mute }),
            Command::SetDefault(kind, index) => {
                if let Some(c) = channel(kind, index) {
                    mixer(Request::SetDefault {
                        kind,
                        name: c.name.clone(),
                    });
                }
            }
            Command::StepDefault { kind, delta, max } => {
                if let Some(c) = self.default_of(kind) {
                    mixer(Request::SetVolume {
                        kind: c.kind,
                        index: c.index,
                        channels: c.channels,
                        volume: (c.volume + delta).clamp(0.0, max),
                    });
                }
            }
            Command::ToggleDefaultMute(kind) => {
                if let Some(c) = self.default_of(kind) {
                    mixer(Request::SetMute {
                        kind: c.kind,
                        index: c.index,
                        mute: !c.muted,
                    });
                }
            }
            Command::PlayPause(ref bus) | Command::Next(ref bus) | Command::Previous(ref bus) => {
                let Some(conn) = self.bus.clone() else {
                    log::warn!("audio: no bus connection, dropping {command:?}");
                    return Task::none();
                };
                let bus = bus.clone();
                return Task::future(async move {
                    let result = async {
                        let player = mpris::player_proxy(&conn, &bus).await?;
                        match command {
                            Command::PlayPause(_) => player.play_pause().await,
                            Command::Next(_) => player.next().await,
                            Command::Previous(_) => player.previous().await,
                            _ => Ok(()),
                        }
                    }
                    .await;
                    if let Err(e) = result {
                        log::warn!("audio: player {bus}: {e}");
                    }
                })
                .discard();
            }
        }
        Task::none()
    }
}

/// `%20` and friends in a `file://` URL.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(kind: Kind, index: u32, name: &str) -> Channel {
        Channel {
            kind,
            index,
            name: name.into(),
            label: name.into(),
            icon: None,
            app: None,
            volume: 0.5,
            muted: false,
            channels: 2,
            has_volume: true,
            default: false,
        }
    }

    #[test]
    fn channels_grouped_by_kind_and_defaults_marked() {
        let mut a = Audio::default();
        assert!(a.apply(Event::Channel(channel(Kind::Stream, 7, "song"))));
        assert!(
            !a.apply(Event::Channel(channel(Kind::Stream, 7, "song"))),
            "unchanged: not a change"
        );
        a.apply(Event::Channel(channel(Kind::Output, 1, "spk")));
        a.apply(Event::Channel(channel(Kind::Input, 3, "mic")));
        a.apply(Event::Channel(channel(Kind::Output, 2, "hp")));
        let order: Vec<_> = a.channels().iter().map(|c| (c.kind, c.index)).collect();
        assert_eq!(
            order,
            [
                (Kind::Output, 1),
                (Kind::Output, 2),
                (Kind::Input, 3),
                (Kind::Stream, 7)
            ]
        );
        a.apply(Event::Defaults {
            sink: "hp".into(),
            source: "mic".into(),
        });
        assert_eq!(a.default_of(Kind::Output).map(|c| c.index), Some(2));
        assert!(a.channels_of(Kind::Input).next().unwrap().default);
        // An update keeps the default flag.
        a.apply(Event::Channel(channel(Kind::Output, 2, "hp")));
        assert_eq!(a.default_of(Kind::Output).map(|c| c.index), Some(2));
        a.apply(Event::ChannelGone(Kind::Output, 2));
        assert!(a.default_of(Kind::Output).is_none());
        a.apply(Event::Disconnected);
        assert!(a.channels().is_empty());
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("/a%20b/c%C3%A8.png"), "/a b/cè.png");
        assert_eq!(percent_decode("/plain"), "/plain");
        assert_eq!(percent_decode("/bad%2"), "/bad%2");
    }
}
