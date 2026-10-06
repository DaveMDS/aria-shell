//! Places gadget: an icon (and a label, when set). A left click opens
//! the popup: the sections of `[Places] show`, in that order, as a
//! file manager's sidebar has them (the home, the XDG folders, the
//! trash; the GTK and KDE bookmarks), a click on one opening it in
//! `[general] file_manager`.
//!
//! Holds no places: they come from `ctx.places`, read again as the
//! popup opens (`Action::Places(Refresh)`).

use iced::widget::{Space, row};
use iced::{Alignment, Element, Length, Size};
use iced_wayland_subscriber::OutputInfo;

use crate::gadget::{Action, Context, Gadget, Popup};
use crate::places::{Command, Group, Kind, Place, PlacesConfig, Target};
use crate::theme::{self, Node};

/// Icon size when the theme doesn't set `height` on an `icon`.
const DEFAULT_ICON_SIZE: f32 = 16.0;

pub struct PlacesGadget {
    config: PlacesConfig,
    popup: Popup,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    Open(Target),
}

/// A themed part of the popup and its size, built together so the
/// popup's size and its content can't disagree.
type Block<'a> = (Element<'a, Message>, Size);

impl Gadget for PlacesGadget {
    type Config = PlacesConfig;
    type Message = Message;

    fn new(config: PlacesConfig, _output: &OutputInfo) -> Self {
        Self {
            config,
            popup: Popup::new(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::TogglePopup => {
                if self.popup.is_open() {
                    self.popup.close()
                } else {
                    Action::Many(vec![Action::Places(Command::Refresh), self.popup.toggle()])
                }
            }
            Message::Open(target) => Action::Many(vec![
                self.popup.close(),
                Action::Places(Command::Open(target)),
            ]),
        }
    }

    fn icon_names(&self) -> Vec<String> {
        let mut names = vec![self.config.icon.clone()];
        names.extend(Kind::ALL.iter().map(|k| self.icon_name(*k, false)));
        names.push(self.icon_name(Kind::Trash, true));
        names
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let button = ctx.node.child("button");
        let mut content = vec![icon(&ctx, &button.child("icon"), &self.config.icon)];
        if !self.config.label.is_empty() {
            let label = button.child("label");
            content.push(theme.text(&label, self.config.label.as_str()).into());
        }
        let content = row(content)
            .spacing(theme.resolve(&button).gap)
            .align_y(Alignment::Center);
        self.popup.anchor(
            theme
                .button(&button, content)
                .on_press(Message::TogglePopup),
        )
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let list = ctx.node.child("list");
        let rows: Vec<Element<'a, Message>> = self.rows(&ctx).into_iter().map(|(e, _)| e).collect();
        ctx.theme.column(&list, rows).width(Length::Fill).into()
    }

    fn popup_size(&self, ctx: Context<'_>) -> (u32, u32) {
        let list = ctx.node.child("list");
        let s = ctx.theme.resolve(&list);
        let rows = self.rows(&ctx);
        let content = rows.iter().map(|(_, size)| size.width).fold(0.0, f32::max);
        let width = px(s.width).unwrap_or(content + s.padding.left + s.padding.right);
        let height = rows.iter().map(|(_, size)| size.height).sum::<f32>()
            + s.gap * rows.len().saturating_sub(1) as f32
            + s.padding.top
            + s.padding.bottom;
        (width.ceil().max(1.0) as u32, height.ceil().max(1.0) as u32)
    }
}

impl PlacesGadget {
    fn icon_name(&self, kind: Kind, trash_full: bool) -> String {
        let name = match kind {
            Kind::Trash if trash_full => "user-trash-full",
            kind => kind.icon(),
        };
        if self.config.symbolic_icons {
            format!("{name}-symbolic")
        } else {
            name.to_owned()
        }
    }

    /// Every row of the popup with its size: a header and the places
    /// of each section shown, the empty ones left out.
    fn rows<'a>(&'a self, ctx: &Context<'a>) -> Vec<Block<'a>> {
        let list = ctx.node.child("list");
        let places = ctx.places.places();
        let shows_places = self.config.show.contains(&Group::Places);
        let mut rows = Vec::new();
        for &group in &self.config.show {
            let entries: Vec<&Place> = match group {
                Group::Places => places.iter().collect(),
                // A bookmark of a place listed above isn't repeated
                // (GTK's file managers bookmark the home, often).
                Group::Bookmarks => ctx
                    .places
                    .bookmarks()
                    .iter()
                    .filter(|b| !(shows_places && places.iter().any(|p| p.target == b.target)))
                    .collect(),
            };
            if entries.is_empty() {
                continue;
            }
            let title = match group {
                Group::Places => ctx.locale.tr("places.places"),
                Group::Bookmarks => ctx.locale.tr("places.bookmarks"),
            };
            rows.push(header(ctx, &list, group, title));
            rows.extend(entries.into_iter().map(|p| self.item(ctx, &list, p)));
        }
        if rows.is_empty() {
            let empty = list.child("empty");
            let text = ctx.locale.tr("places.empty");
            let size = padded(ctx.theme, &empty, ctx.theme.measure(&empty, text));
            rows.push((
                ctx.theme
                    .container(&empty, ctx.theme.text(&empty, text))
                    .width(Length::Fill)
                    .into(),
                size,
            ));
        }
        rows
    }

    /// A place: a button with its icon and its name.
    fn item<'a>(&'a self, ctx: &Context<'a>, list: &Node, place: &Place) -> Block<'a> {
        let theme = ctx.theme;
        let full = place.kind == Kind::Trash && ctx.places.trash_full();
        let node = list
            .child("item")
            .class(place.kind.name())
            .class_if("full", full);
        let i = node.child("icon");
        let l = node.child("label");
        let label = match place.kind {
            Kind::Home => ctx.locale.tr("places.home").to_owned(),
            Kind::Trash => ctx.locale.tr("places.trash").to_owned(),
            _ => place.label.clone(),
        };
        let s = theme.resolve(&node);
        let icon_size = padded(theme, &i, Size::new(icon_size(ctx, &i), icon_size(ctx, &i)));
        let text = theme.measure(&l, &label);
        let text = padded(
            theme,
            &l,
            Size::new(text.width, text.height.max(theme.line_height(&l))),
        );
        let size = Size::new(
            icon_size.width
                + s.gap
                + text.width
                + s.padding.left
                + s.padding.right
                + 2.0 * s.border_width,
            icon_size.height.max(text.height)
                + s.padding.top
                + s.padding.bottom
                + 2.0 * s.border_width,
        );
        let content = row![
            icon(ctx, &i, &self.icon_name(place.kind, full)),
            theme.text(&l, label).width(Length::Fill),
        ]
        .spacing(s.gap)
        .align_y(Alignment::Center)
        .width(Length::Fill);
        (
            theme
                .button(&node, content)
                .width(Length::Fill)
                .on_press(Message::Open(place.target.clone()))
                .into(),
            size,
        )
    }
}

/// A section's title.
fn header<'a>(ctx: &Context<'a>, list: &Node, group: Group, title: &'a str) -> Block<'a> {
    let theme = ctx.theme;
    let header = list.child("header").class(group.name());
    let t = header.child("title");
    let text = theme.measure(&t, title);
    let text = padded(
        theme,
        &t,
        Size::new(text.width, text.height.max(theme.line_height(&t))),
    );
    (
        theme
            .container(
                &header,
                theme
                    .container(&t, theme.text(&t, title))
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .into(),
        padded(theme, &header, text),
    )
}

fn px(l: Option<theme::Length>) -> Option<f32> {
    match l {
        Some(theme::Length::Px(px)) => Some(px),
        _ => None,
    }
}

/// `content` with the node's padding around.
fn padded(theme: &theme::Theme, node: &Node, content: Size) -> Size {
    let s = theme.resolve(node);
    Size::new(
        content.width + s.padding.left + s.padding.right,
        content.height + s.padding.top + s.padding.bottom,
    )
}

fn icon_size(ctx: &Context<'_>, node: &Node) -> f32 {
    let style = ctx.theme.resolve(node);
    px(style.height.or(style.width)).unwrap_or(DEFAULT_ICON_SIZE)
}

/// A themed icon by name, at the node's `height` (its `width` if
/// there's no height), or a blank of that size.
fn icon<'a>(ctx: &Context<'a>, node: &Node, name: &str) -> Element<'a, Message> {
    let style = ctx.theme.resolve(node);
    let size = icon_size(ctx, node);
    let icon = match ctx.icons.get_name(name, None) {
        Some(icon) => icon.view(size, style.color),
        None => Space::new().width(size).height(size).into(),
    };
    ctx.theme.container(node, icon).into()
}
