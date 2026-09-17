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
use iced::widget::{Space, column, mouse_area, row, scrollable};
use iced::{Alignment, Element, Length};
use iced_wayland_subscriber::OutputInfo;

use crate::audio::{Channel, Command, Kind, PlaybackStatus, Player};
use crate::config::{RawSection, Section};
use crate::gadget::{Action, Axis, Context, Gadget, Popup, Wheel};
use crate::process;
use crate::theme::{self, Node};

/// Icon size when the theme doesn't set `height` on an `icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;
/// Cover size when the theme doesn't set `height` on `cover`.
const DEFAULT_COVER_SIZE: f32 = 64.0;
/// Popup width and height cap when the theme doesn't size `list`.
const DEFAULT_LIST_WIDTH: f32 = 380.0;
const DEFAULT_LIST_HEIGHT: f32 = 600.0;

const OUTPUT_TITLE: &str = "Output";
const INPUT_TITLE: &str = "Input";
const STREAMS_TITLE: &str = "Playing";
const MIXER_LABEL: &str = "Mixer";
const EMPTY: &str = "No audio devices";

const ICON_MUTED: &str = "audio-volume-muted-symbolic";
const ICON_LOW: &str = "audio-volume-low-symbolic";
const ICON_MEDIUM: &str = "audio-volume-medium-symbolic";
const ICON_HIGH: &str = "audio-volume-high-symbolic";
const ICON_INPUT: &str = "audio-input-microphone-symbolic";
const ICON_INPUT_MUTED: &str = "microphone-sensitivity-muted-symbolic";
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
        let mut buttons = vec![self.bar_button(&ctx, Kind::Output)];
        if self.config.show_microphone {
            buttons.push(self.bar_button(&ctx, Kind::Input));
        }
        ctx.theme
            .row(&ctx.node, buttons)
            .align_y(Alignment::Center)
            .into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let list = ctx.node.child("list");
        let mut rows: Vec<Element<'a, Message>> = Vec::new();
        for (kind, title) in self.groups() {
            let channels: Vec<&Channel> = ctx.audio.channels_of(kind).collect();
            if channels.is_empty() {
                continue;
            }
            let section = list.child("section").class(kind_class(kind));
            let t = section.child("title");
            rows.push(
                theme
                    .container(&section, theme.text(&t, title))
                    .width(Length::Fill)
                    .into(),
            );
            for c in channels {
                rows.push(self.channel_view(&ctx, &list, c));
            }
        }
        if self.config.show_players {
            for p in ctx.audio.players() {
                rows.push(self.player_view(&ctx, &list, p));
            }
        }
        if rows.is_empty() {
            let empty = list.child("empty");
            rows.push(
                theme
                    .container(&empty, theme.text(&empty, EMPTY))
                    .width(Length::Fill)
                    .align_x(Alignment::Center)
                    .into(),
            );
        }
        if !self.config.mixer_command.is_empty() {
            let mixer = list.child("button").class("mixer");
            let label = iced::widget::container(theme.text(&mixer.child("text"), MIXER_LABEL))
                .width(Length::Fill)
                .align_x(Alignment::Center);
            rows.push(
                theme
                    .button(&mixer, label)
                    .on_press(Message::RunMixer)
                    .width(Length::Fill)
                    .into(),
            );
        }
        let content = theme.column(&list, rows).width(Length::Fill);
        let (_, capped) = self.list_height(&ctx);
        if capped {
            scrollable(content).height(Length::Fill).into()
        } else {
            content.into()
        }
    }

    fn popup_size(&self, ctx: Context<'_>) -> (u32, u32) {
        let width = self.list_width(&ctx);
        let (height, _) = self.list_height(&ctx);
        (width.ceil().max(1.0) as u32, height.ceil().max(1.0) as u32)
    }
}

/// Which popup anchor (and wheel) a bar button is.
fn anchor(kind: Kind) -> usize {
    match kind {
        Kind::Output | Kind::Stream => 0,
        Kind::Input => 1,
    }
}

/// The volume icon for a channel's level; the microphone for an input.
fn level_icon(c: &Channel) -> &'static str {
    if c.kind == Kind::Input {
        if c.muted {
            ICON_INPUT_MUTED
        } else {
            ICON_INPUT
        }
    } else if c.muted || c.volume <= 0.0 {
        ICON_MUTED
    } else if c.volume < 0.34 {
        ICON_LOW
    } else if c.volume < 0.67 {
        ICON_MEDIUM
    } else {
        ICON_HIGH
    }
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

impl AudioGadget {
    /// The bar button of a default device: its level icon, the percent
    /// with `show_percent`; left click the popup, wheel and middle
    /// click on the device, right click `mixer_command`.
    fn bar_button<'a>(&'a self, ctx: &Context<'a>, kind: Kind) -> Element<'a, Message> {
        let theme = ctx.theme;
        let device = ctx.audio.default_of(kind);
        let muted = device.is_none_or(|c| c.muted);
        let button = ctx
            .node
            .child("button")
            .class(kind_class(kind))
            .class_if("muted", muted)
            .class_if("none", device.is_none());
        let name = match device {
            Some(c) => level_icon(c),
            None if kind == Kind::Input => ICON_INPUT_MUTED,
            None => ICON_MUTED,
        };
        let icon = self.icon(ctx, &button.child("icon"), name);
        let mut content = row![icon]
            .spacing(theme.resolve(&button).gap)
            .align_y(Alignment::Center);
        if self.config.show_percent {
            let text = button.child("text");
            let value = device.map_or(0.0, |c| c.volume);
            content = content.push(theme.container(&text, theme.text(&text, percent(value))));
        }
        let button = theme
            .button(&button, content)
            .on_press(Message::TogglePopup(kind));
        mouse_area(self.popup.anchor_nth(anchor(kind), button))
            .on_middle_press(Message::ToggleMute(kind))
            .on_right_press(Message::RunMixer)
            .on_scroll(move |delta| Message::Scroll(kind, delta))
            .into()
    }

    /// The channel groups shown, in order.
    fn groups(&self) -> Vec<(Kind, &'static str)> {
        let mut groups = vec![(Kind::Output, OUTPUT_TITLE)];
        if self.config.show_inputs {
            groups.push((Kind::Input, INPUT_TITLE));
        }
        if self.config.show_streams {
            groups.push((Kind::Stream, STREAMS_TITLE));
        }
        groups
    }

    /// A themed icon by name, at the node's `height` (its `width` if
    /// there's no height), or a blank of that size.
    fn icon<'a>(&'a self, ctx: &Context<'a>, node: &Node, name: &str) -> Element<'a, Message> {
        let style = ctx.theme.resolve(node);
        let size = px(style.height.or(style.width)).unwrap_or(DEFAULT_ICON_SIZE);
        match ctx.icons.get_name(name, None) {
            Some(icon) => icon.view(size, style.color),
            None => Space::new().width(size).height(size).into(),
        }
    }

    fn icon_size(&self, ctx: &Context<'_>, node: &Node) -> f32 {
        let style = ctx.theme.resolve(node);
        px(style.height.or(style.width)).unwrap_or(DEFAULT_ICON_SIZE)
    }

    /// A channel's icon: an output's level, the microphone for an
    /// input (the server's `device.icon_name`s are rarely in a theme),
    /// a stream's application (its desktop entry, else the icon name
    /// it gave, else a generic player).
    fn channel_icon<'a>(
        &'a self,
        ctx: &Context<'a>,
        node: &Node,
        c: &Channel,
    ) -> Element<'a, Message> {
        let style = ctx.theme.resolve(node);
        let size = px(style.height.or(style.width)).unwrap_or(DEFAULT_ICON_SIZE);
        let icon = match c.kind {
            Kind::Output | Kind::Input => ctx.icons.get_name(level_icon(c), None),
            Kind::Stream => c
                .app
                .as_deref()
                .and_then(|app| ctx.icons.get(app))
                .or_else(|| c.icon.as_deref().and_then(|n| ctx.icons.get_name(n, None)))
                .or_else(|| ctx.icons.get_name(ICON_STREAM, None)),
        };
        match icon {
            Some(icon) => icon.view(size, style.color),
            None => Space::new().width(size).height(size).into(),
        }
    }

    fn channel_node(&self, list: &Node, c: &Channel) -> Node {
        list.child("channel")
            .class(kind_class(c.kind))
            .class_if("muted", c.muted)
            .class_if("default", c.default)
            .attr("name", c.name.clone())
    }

    /// A row: the mute button with the icon, then the name (a button
    /// making a device the default) with the percent, over the slider.
    fn channel_view<'a>(
        &'a self,
        ctx: &Context<'a>,
        list: &Node,
        c: &'a Channel,
    ) -> Element<'a, Message> {
        let theme = ctx.theme;
        let node = self.channel_node(list, c);
        let mute = node.child("button").class("mute").class_if("on", c.muted);
        let mute_button = theme
            .button(&mute, self.channel_icon(ctx, &mute.child("icon"), c))
            .on_press(Message::Mute(c.kind, c.index, !c.muted));
        let name = node.child("name");
        let label: Element<'a, Message> = match c.kind {
            Kind::Output | Kind::Input if !c.default => theme
                .button(&name, theme.text(&name.child("text"), &c.label))
                .on_press(Message::SetDefault(c.kind, c.index))
                .into(),
            _ => theme
                .container(&name, theme.text(&name.child("text"), &c.label))
                .into(),
        };
        let value = node.child("value");
        let head = row![
            label,
            Space::new().width(Length::Fill),
            theme.text(&value, percent(c.volume)),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill);
        let mut parts: Vec<Element<'a, Message>> = vec![head.into()];
        if c.has_volume {
            let (kind, index) = (c.kind, c.index);
            parts.push(
                theme
                    .slider(
                        &node.child("slider"),
                        0.0..=self.config.max_volume,
                        c.volume * 100.0,
                        1.0,
                        move |v| Message::Volume(kind, index, v),
                    )
                    .into(),
            );
        }
        let body = column(parts)
            .spacing(theme.resolve(&node).gap)
            .width(Length::Fill);
        theme
            .container(
                &node,
                row![mute_button, body]
                    .spacing(theme.resolve(&node).gap)
                    .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .into()
    }

    /// The height of a channel's row, padding included.
    fn channel_height(&self, ctx: &Context<'_>, list: &Node, c: &Channel) -> f32 {
        let theme = ctx.theme;
        let node = self.channel_node(list, c);
        let s = theme.resolve(&node);
        let mute = node.child("button").class("mute");
        let ms = theme.resolve(&mute);
        let button = self.icon_size(ctx, &mute.child("icon")) + ms.padding.top + ms.padding.bottom;
        let name = node.child("name");
        let ns = theme.resolve(&name);
        let text = name.child("text");
        let head = (theme
            .measure(&text, &c.label)
            .height
            .max(theme.line_height(&text))
            + ns.padding.top
            + ns.padding.bottom)
            .max(theme.line_height(&node.child("value")));
        let mut body = head;
        if c.has_volume {
            body += s.gap + self.slider_height(ctx, &node.child("slider"));
        }
        button.max(body) + s.padding.top + s.padding.bottom
    }

    fn slider_height(&self, ctx: &Context<'_>, node: &Node) -> f32 {
        let s = ctx.theme.resolve(node);
        let handle = ctx.theme.resolve(&node.child("handle"));
        px(s.height)
            .unwrap_or(4.0)
            .max(px(handle.width).unwrap_or(12.0))
    }

    fn player_node(&self, list: &Node, p: &Player) -> Node {
        list.child("player")
            .class(match p.status {
                PlaybackStatus::Playing => "playing",
                PlaybackStatus::Paused => "paused",
                PlaybackStatus::Stopped => "stopped",
            })
            .attr("name", p.identity.clone())
    }

    /// The cover (or the player's icon) beside the title, artist and
    /// album; the transport buttons. No volume: the player's stream is
    /// in the mixer already.
    fn player_view<'a>(
        &'a self,
        ctx: &Context<'a>,
        list: &Node,
        p: &'a Player,
    ) -> Element<'a, Message> {
        let theme = ctx.theme;
        let node = self.player_node(list, p);
        let s = theme.resolve(&node);
        let cover_node = node.child("cover");
        let cover_style = theme.resolve(&cover_node);
        let cover_size = px(cover_style.height.or(cover_style.width)).unwrap_or(DEFAULT_COVER_SIZE);
        let cover: Element<'a, Message> = match ctx
            .audio
            .cover(&p.bus)
            .or_else(|| p.desktop_entry.as_deref().and_then(|id| ctx.icons.get(id)))
        {
            Some(icon) => icon.view(cover_size, cover_style.color),
            None => Space::new().width(cover_size).height(cover_size).into(),
        };
        let mut texts: Vec<Element<'a, Message>> = Vec::new();
        let title = node.child("title");
        texts.push(
            theme
                .text(
                    &title,
                    if p.title.is_empty() {
                        &p.identity
                    } else {
                        &p.title
                    },
                )
                .into(),
        );
        for (class, value) in [("artist", &p.artist), ("album", &p.album)] {
            if !value.is_empty() {
                texts.push(theme.text(&node.child(class), value).into());
            }
        }
        let info = row![
            cover,
            column(texts).spacing(s.gap / 2.0).width(Length::Fill)
        ]
        .spacing(s.gap)
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let controls = node.child("controls");
        let bus = p.bus.clone();
        let transport = |class: &'static str, icon: &str, enabled: bool, message: Message| {
            // A class, not `:disabled`: the icon's colour is resolved
            // when the view is built, not in the button's style closure.
            let b = controls
                .child("button")
                .class(class)
                .class_if("disabled", !enabled);
            let mut button = theme.button(&b, self.icon(ctx, &b.child("icon"), icon));
            if enabled {
                button = button.on_press(message);
            }
            button
        };
        let play_icon = if p.status == PlaybackStatus::Playing {
            ICON_PAUSE
        } else {
            ICON_PLAY
        };
        let buttons = row![
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
        .spacing(theme.resolve(&controls).gap)
        .align_y(Alignment::Center);
        let parts: Vec<Element<'a, Message>> = vec![
            info.into(),
            theme
                .container(&controls, buttons)
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .into(),
        ];
        theme
            .container(&node, column(parts).spacing(s.gap).width(Length::Fill))
            .width(Length::Fill)
            .into()
    }

    /// The height of a player's block, padding included.
    fn player_height(&self, ctx: &Context<'_>, list: &Node, p: &Player) -> f32 {
        let theme = ctx.theme;
        let node = self.player_node(list, p);
        let s = theme.resolve(&node);
        let cover = node.child("cover");
        let cs = theme.resolve(&cover);
        let cover_size = px(cs.height.or(cs.width)).unwrap_or(DEFAULT_COVER_SIZE);
        let mut lines = theme.line_height(&node.child("title"));
        for (class, value) in [("artist", &p.artist), ("album", &p.album)] {
            if !value.is_empty() {
                lines += s.gap / 2.0 + theme.line_height(&node.child(class));
            }
        }
        let info = cover_size.max(lines);
        let controls = node.child("controls");
        let ctl = theme.resolve(&controls);
        let b = controls.child("button");
        let bs = theme.resolve(&b);
        let buttons = self.icon_size(ctx, &b.child("icon"))
            + bs.padding.top
            + bs.padding.bottom
            + ctl.padding.top
            + ctl.padding.bottom;
        info + s.gap + buttons + s.padding.top + s.padding.bottom
    }

    fn list_width(&self, ctx: &Context<'_>) -> f32 {
        px(ctx.theme.resolve(&ctx.node.child("list")).width).unwrap_or(DEFAULT_LIST_WIDTH)
    }

    /// The popup height: the sections, rows, players and the mixer
    /// button (or the empty text) with the gaps, within the list's
    /// padding, capped by its `height`; whether it was capped (then
    /// the content scrolls).
    fn list_height(&self, ctx: &Context<'_>) -> (f32, bool) {
        let theme = ctx.theme;
        let list = ctx.node.child("list");
        let s = theme.resolve(&list);
        let mut heights: Vec<f32> = Vec::new();
        for (kind, title) in self.groups() {
            let channels: Vec<&Channel> = ctx.audio.channels_of(kind).collect();
            if channels.is_empty() {
                continue;
            }
            let section = list.child("section").class(kind_class(kind));
            let ss = theme.resolve(&section);
            let t = section.child("title");
            heights.push(
                theme.measure(&t, title).height.max(theme.line_height(&t))
                    + ss.padding.top
                    + ss.padding.bottom,
            );
            for c in channels {
                heights.push(self.channel_height(ctx, &list, c));
            }
        }
        if self.config.show_players {
            for p in ctx.audio.players() {
                heights.push(self.player_height(ctx, &list, p));
            }
        }
        if heights.is_empty() {
            let empty = list.child("empty");
            let es = theme.resolve(&empty);
            heights.push(
                theme
                    .measure(&empty, EMPTY)
                    .height
                    .max(theme.line_height(&empty))
                    + es.padding.top
                    + es.padding.bottom,
            );
        }
        if !self.config.mixer_command.is_empty() {
            let mixer = list.child("button").class("mixer");
            let ms = theme.resolve(&mixer);
            let t = mixer.child("text");
            heights.push(
                theme
                    .measure(&t, MIXER_LABEL)
                    .height
                    .max(theme.line_height(&t))
                    + ms.padding.top
                    + ms.padding.bottom,
            );
        }
        let height = heights.iter().sum::<f32>()
            + s.gap * heights.len().saturating_sub(1) as f32
            + s.padding.top
            + s.padding.bottom;
        let cap = px(s.height).unwrap_or(DEFAULT_LIST_HEIGHT);
        if height > cap {
            (cap, true)
        } else {
            (height, false)
        }
    }
}

fn px(l: Option<theme::Length>) -> Option<f32> {
    match l {
        Some(theme::Length::Px(px)) => Some(px),
        _ => None,
    }
}
