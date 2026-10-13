//! Audio gadget: the default output's volume as an icon on the bar (and
//! the percent, with `show_percent`; the default input's beside it,
//! with `show_microphone`, driven the same way); a
//! left click opens the mixer popup (outputs, inputs and the streams
//! playing, each with its mute button and volume slider; a device's
//! name makes it the default), the media players (cover, what's
//! playing, previous / play-pause / next) and a button
//! running `mixer_command`. The wheel changes the default output's
//! volume by `step`, a middle click mutes it, a right click runs
//! `mixer_command`.
//!
//! Holds no audio state: the channels and players come from
//! `ctx.audio`; what the user does goes back as `Action::Audio`.

use iced::mouse::ScrollDelta;
use iced::widget::mouse_area;
use iced_wayland_subscriber::OutputInfo;
use icssed::{Element, IconSource};

use crate::config::{RawSection, Section};
use crate::gadgets::{Action, Axis, Context, Gadget, Popup, Wheel};
use crate::locale::Locale;
use crate::process;
use crate::services::audio::{
    Channel, Command, ICON_HIGH, ICON_INPUT, ICON_INPUT_MUTED, ICON_LOW, ICON_MEDIUM, ICON_MUTED,
    Kind, PlaybackStatus, Player,
};

const ICON_STREAM: &str = "multimedia-player-symbolic";
const ICON_PREVIOUS: &str = "media-skip-backward-symbolic";
const ICON_PLAY: &str = "media-playback-start-symbolic";
const ICON_PAUSE: &str = "media-playback-pause-symbolic";
const ICON_NEXT: &str = "media-skip-forward-symbolic";

#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// A program (a real mixer) run by the popup's button and a right
    /// click; none when empty.
    pub mixer_command: String,
    /// Percent per wheel click.
    pub step: f32,
    /// The sliders' and the wheel's ceiling, percent.
    pub max_volume: f32,
    pub show_inputs: bool,
    pub show_streams: bool,
    pub show_players: bool,
    /// The percent as text after the icon on the bar.
    pub show_percent: bool,
    /// The default input's icon after the output's, with its own
    /// wheel / middle click.
    pub show_microphone: bool,
}

impl Section for AudioConfig {
    const NAME: &'static str = "Audio";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            mixer_command: raw.str_or("mixer_command", ""),
            step: raw.u64_or("step", 5).max(1) as f32,
            max_volume: raw.u64_or("max_volume", 100).max(1) as f32,
            show_inputs: raw.bool_or("show_inputs", true),
            show_streams: raw.bool_or("show_streams", true),
            show_players: raw.bool_or("show_players", true),
            show_percent: raw.bool_or("show_percent", false),
            show_microphone: raw.bool_or("show_microphone", false),
        }
    }
}

pub struct AudioGadget {
    config: AudioConfig,
    popup: Popup,
    /// One per bar button: output, input.
    wheels: [Wheel; 2],
}

#[derive(Clone, Debug)]
pub enum Message {
    /// From the output's or the input's button: the popup hangs off it.
    TogglePopup(Kind),
    ToggleMute(Kind),
    Scroll(Kind, ScrollDelta),
    RunMixer,
    /// A channel's slider, percent.
    Volume(Kind, u32, f32),
    Mute(Kind, u32, bool),
    SetDefault(Kind, u32),
    PlayPause(String),
    Previous(String),
    Next(String),
}

impl Gadget for AudioGadget {
    type Config = AudioConfig;
    type Message = Message;

    fn new(config: AudioConfig, _output: &OutputInfo) -> Self {
        Self {
            config,
            popup: Popup::new(),
            wheels: [Wheel::default(), Wheel::default()],
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::TogglePopup(kind) => self.popup.toggle_nth(anchor(kind)),
            Message::ToggleMute(kind) => Action::Audio(Command::ToggleDefaultMute(kind)),
            Message::Scroll(kind, delta) => match self.wheels[anchor(kind)].clicks(delta) {
                Some((clicks, Axis::Vertical)) => Action::Audio(Command::StepDefault {
                    kind,
                    delta: clicks as f32 * self.config.step / 100.0,
                    max: self.config.max_volume / 100.0,
                }),
                _ => Action::None,
            },
            Message::RunMixer => {
                if !self.config.mixer_command.is_empty() {
                    process::run(&self.config.mixer_command);
                }
                self.popup.close()
            }
            Message::Volume(kind, index, percent) => {
                Action::Audio(Command::SetVolume(kind, index, percent / 100.0))
            }
            Message::Mute(kind, index, mute) => Action::Audio(Command::SetMuted(kind, index, mute)),
            Message::SetDefault(kind, index) => Action::Audio(Command::SetDefault(kind, index)),
            Message::PlayPause(bus) => Action::Audio(Command::PlayPause(bus)),
            Message::Previous(bus) => Action::Audio(Command::Previous(bus)),
            Message::Next(bus) => Action::Audio(Command::Next(bus)),
        }
    }

    fn icon_names(&self) -> Vec<String> {
        [
            ICON_MUTED,
            ICON_LOW,
            ICON_MEDIUM,
            ICON_HIGH,
            ICON_INPUT,
            ICON_INPUT_MUTED,
            ICON_STREAM,
            ICON_PREVIOUS,
            ICON_PLAY,
            ICON_PAUSE,
            ICON_NEXT,
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let mut buttons = vec![self.bar_button(ctx, Kind::Output)];
        if self.config.show_microphone {
            buttons.push(self.bar_button(ctx, Kind::Input));
        }
        icssed::row(buttons).into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let mut rows: Vec<Element<'a, Message>> = Vec::new();
        for (kind, title) in self.groups(ctx.locale) {
            let channels: Vec<&Channel> = ctx.audio.channels_of(kind).collect();
            if channels.is_empty() {
                continue;
            }
            rows.push(
                icssed::row![icssed::text(title).kind("title")]
                    .kind("section")
                    .class(kind_class(kind))
                    .into(),
            );
            for c in channels {
                rows.push(self.channel_view(ctx, c));
            }
        }
        if self.config.show_players {
            for p in ctx.audio.players() {
                rows.push(player_view(ctx, p));
            }
        }
        if rows.is_empty() {
            rows.push(
                super::note(ctx.locale.tr("audio.empty"))
                    .kind("empty")
                    .into(),
            );
        }
        if !self.config.mixer_command.is_empty() {
            rows.push(
                icssed::button![icssed::text(ctx.locale.tr("audio.mixer"))]
                    .class("mixer")
                    .on_press(Message::RunMixer)
                    .into(),
            );
        }
        icssed::scroll(icssed::column(rows).kind("list")).into()
    }
}

impl AudioGadget {
    /// The bar button of a default device: its level icon, the percent
    /// with `show_percent`; left click the popup, wheel and middle
    /// click on the device, right click `mixer_command`.
    fn bar_button<'a>(&'a self, ctx: Context<'a>, kind: Kind) -> Element<'a, Message> {
        let device = ctx.audio.default_of(kind);
        let muted = device.is_none_or(|c| c.muted);
        let name = match device {
            Some(c) => c.icon_name(),
            None if kind == Kind::Input => ICON_INPUT_MUTED,
            None => ICON_MUTED,
        };
        let mut content: Vec<Element<'a, Message>> = vec![ctx.icon(name).into()];
        if self.config.show_percent {
            let value = device.map_or(0.0, |c| c.volume);
            content.push(icssed::text(percent(value)).into());
        }
        let button = icssed::button(content)
            .class(kind_class(kind))
            .class_if("muted", muted)
            .on_press(Message::TogglePopup(kind))
            .id(self.popup.anchor_nth(anchor(kind)));
        mouse_area(button)
            .on_middle_press(Message::ToggleMute(kind))
            .on_right_press(Message::RunMixer)
            .on_scroll(move |delta| Message::Scroll(kind, delta))
            .into()
    }

    /// The channel groups shown, in order, with their titles.
    fn groups(&self, locale: &Locale) -> Vec<(Kind, &'static str)> {
        let mut groups = vec![(Kind::Output, locale.tr("audio.output"))];
        if self.config.show_inputs {
            groups.push((Kind::Input, locale.tr("audio.input")));
        }
        if self.config.show_streams {
            groups.push((Kind::Stream, locale.tr("audio.playing")));
        }
        groups
    }

    /// A row: the mute button with the icon, then the name (a radio
    /// making a device the default) with the percent, over the slider.
    fn channel_view<'a>(&'a self, ctx: Context<'a>, c: &'a Channel) -> Element<'a, Message> {
        let (kind, index) = (c.kind, c.index);
        let mute = icssed::toggle(c.muted, move |m| Message::Mute(kind, index, m))
            .class("mute")
            .icon(channel_icon(ctx, c));
        // A device's name is a radio: on the default, a click makes it
        // so (on the default too, a click that changes nothing).
        let label: Element<'a, Message> = if is_device(c) {
            icssed::toggle(c.default, move |_| Message::SetDefault(kind, index))
                .radio()
                .label(c.label.as_str())
                .into()
        } else {
            icssed::text(c.label.as_str()).kind("name").into()
        };
        let head = icssed::row![label, icssed::text(percent(c.volume)).kind("value")].kind("head");
        let mut parts: Vec<Element<'a, Message>> = vec![head.into()];
        if c.has_volume {
            parts.push(
                icssed::slider(0.0..=self.config.max_volume, c.volume * 100.0, move |v| {
                    Message::Volume(kind, index, v)
                })
                .step(1.0)
                .into(),
            );
        }
        icssed::row![mute, icssed::column(parts).kind("info")]
            .kind("channel")
            .class(kind_class(c.kind))
            .class_if("muted", c.muted)
            .class_if("default", c.default)
            .attr("name", &c.name)
            .into()
    }
}

/// A channel's icon: an output's level, the microphone for an input
/// (the server's `device.icon_name`s are rarely in a theme), a stream's
/// application (its desktop entry, else the icon name it gave, else a
/// generic player).
fn channel_icon<'a>(ctx: Context<'a>, c: &'a Channel) -> IconSource<'a> {
    match c.kind {
        Kind::Output | Kind::Input => ctx.named(c.icon_name()),
        Kind::Stream => match c.app.as_deref().and_then(|app| ctx.icons.get(app)) {
            Some(icon) => icon.clone().into(),
            None => ctx.named(c.icon.as_deref().unwrap_or(ICON_STREAM)),
        },
    }
}

/// The cover (or the player's icon) beside the title, artist and album;
/// the transport buttons. No volume: the player's stream is in the
/// mixer already.
fn player_view<'a>(ctx: Context<'a>, p: &'a Player) -> Element<'a, Message> {
    let cover = match ctx
        .audio
        .cover(&p.bus)
        .or_else(|| p.desktop_entry.as_deref().and_then(|id| ctx.icons.get(id)))
    {
        Some(icon) => icssed::icon(icon.clone()),
        None => ctx.icon(""),
    };
    let title = if p.title.is_empty() {
        &p.identity
    } else {
        &p.title
    };
    let mut lines: Vec<Element<'a, Message>> =
        vec![icssed::text(title.as_str()).kind("title").into()];
    for (class, value) in [("artist", &p.artist), ("album", &p.album)] {
        if !value.is_empty() {
            lines.push(icssed::text(value.as_str()).kind(class).into());
        }
    }
    let bus = &p.bus;
    // Disabled, the icon takes `:disabled`'s colour.
    let transport = |class: &str, icon: &'a str, enabled: bool, message: Message| {
        icssed::button![ctx.icon(icon)]
            .class(class)
            .class("flat")
            .disabled_if(!enabled)
            .on_press(message)
    };
    let play_icon = if p.status == PlaybackStatus::Playing {
        ICON_PAUSE
    } else {
        ICON_PLAY
    };
    icssed::column![
        icssed::row![cover.kind("cover"), icssed::column(lines).kind("texts")].kind("info"),
        icssed::row![
            transport(
                "previous",
                ICON_PREVIOUS,
                p.can_go_previous,
                Message::Previous(bus.clone()),
            ),
            transport(
                "play",
                play_icon,
                p.can_play || p.can_pause,
                Message::PlayPause(bus.clone()),
            ),
            transport("next", ICON_NEXT, p.can_go_next, Message::Next(bus.clone())),
        ]
        .kind("controls"),
    ]
    .kind("player")
    .class(match p.status {
        PlaybackStatus::Playing => "playing",
        PlaybackStatus::Paused => "paused",
        PlaybackStatus::Stopped => "stopped",
    })
    .attr("name", &p.identity)
    .into()
}

/// An output or an input, which can be made the default; not a stream.
fn is_device(c: &Channel) -> bool {
    matches!(c.kind, Kind::Output | Kind::Input)
}

fn kind_class(kind: Kind) -> &'static str {
    match kind {
        Kind::Output => "output",
        Kind::Input => "input",
        Kind::Stream => "stream",
    }
}

fn percent(v: f32) -> String {
    format!("{}%", (v * 100.0).round() as i32)
}

/// Which popup anchor (and wheel) a bar button is.
fn anchor(kind: Kind) -> usize {
    match kind {
        Kind::Output | Kind::Stream => 0,
        Kind::Input => 1,
    }
}
