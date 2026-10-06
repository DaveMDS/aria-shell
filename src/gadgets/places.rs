//! Places gadget: an icon (and a label, when set). A left click opens
//! the popup: the sections of `[Places] show`, in that order, as a
//! file manager's sidebar has them (the home, the XDG folders, the
//! trash; the disks, USB sticks and cards UDisks2 has; the GTK and KDE
//! bookmarks), a click on one opening it in `[general] file_manager`
//! (a device not mounted is mounted first), ⏏ on a device unmounting
//! it (and powering its drive off, when it's removable).
//!
//! Holds no places: they come from `ctx.places`, read again as the
//! popup opens (`Action::Places(Refresh)`).

use std::path::PathBuf;

use iced::widget::{Space, button, row};
use iced::{Alignment, Element, Length, Size};
use iced_wayland_subscriber::OutputInfo;

use crate::gadget::{Action, Context, Gadget, Popup};
use crate::locale::Locale;
use crate::places::{Command, Device, DeviceKind, Group, Kind, Place, PlacesConfig, Share, Target};
use crate::sysmon::format;
use crate::theme::{self, Node};
use crate::widgets::graph;

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
    /// Mount a device (by its path), then open it.
    Mount(String),
    Eject(String),
    /// Mount a share (by its mount point), then open it.
    MountShare(PathBuf),
    UnmountShare(PathBuf),
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
            Message::Mount(path) => Action::Many(vec![
                self.popup.close(),
                Action::Places(Command::Mount(path)),
            ]),
            Message::MountShare(dir) => Action::Many(vec![
                self.popup.close(),
                Action::Places(Command::MountShare(dir)),
            ]),
            // The popup stays: the device leaves it, or shows why not.
            Message::Eject(path) => Action::Places(Command::Eject(path)),
            Message::UnmountShare(dir) => Action::Places(Command::UnmountShare(dir)),
        }
    }

    fn icon_names(&self) -> Vec<String> {
        let mut names = vec![self.config.icon.clone()];
        names.extend(Kind::ALL.iter().map(|k| self.icon_name(*k, false)));
        names.push(self.icon_name(Kind::Trash, true));
        names.extend(DeviceKind::ALL.iter().flat_map(|k| self.device_icons(*k)));
        names.push(self.symbolic(EJECT_ICON));
        names.push(self.symbolic(SHARE_ICON));
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
        self.symbolic(match kind {
            Kind::Trash if trash_full => "user-trash-full",
            kind => kind.icon(),
        })
    }

    /// The kind's icons, best first.
    fn device_icons(&self, kind: DeviceKind) -> Vec<String> {
        kind.icons().iter().map(|n| self.symbolic(n)).collect()
    }

    /// `name`, `-symbolic` with `symbolic_icons`.
    fn symbolic(&self, name: &str) -> String {
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
            let entries: Vec<Block<'a>> = match group {
                Group::Places => places.iter().map(|p| self.item(ctx, &list, p)).collect(),
                Group::Devices => ctx
                    .places
                    .devices()
                    .iter()
                    .map(|d| self.device(ctx, &list, d))
                    .collect(),
                Group::Network => ctx
                    .places
                    .shares()
                    .iter()
                    .map(|s| self.share(ctx, &list, s))
                    .collect(),
                // A bookmark of a place listed above isn't repeated
                // (GTK's file managers bookmark the home, often).
                Group::Bookmarks => ctx
                    .places
                    .bookmarks()
                    .iter()
                    .filter(|b| !(shows_places && places.iter().any(|p| p.target == b.target)))
                    .map(|p| self.item(ctx, &list, p))
                    .collect(),
            };
            if entries.is_empty() {
                continue;
            }
            let title = match group {
                Group::Places => ctx.locale.tr("places.places"),
                Group::Devices => ctx.locale.tr("places.devices"),
                Group::Network => ctx.locale.tr("places.network"),
                Group::Bookmarks => ctx.locale.tr("places.bookmarks"),
            };
            rows.push(header(ctx, &list, group, title));
            rows.extend(entries);
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

impl PlacesGadget {
    /// A device: its icon, its name and, while mounted, how full it is;
    /// ⏏ beside it while mounted (as Nemo has it: nothing to eject before
    /// a mount), disabled on the root filesystem, which can't be
    /// unmounted but is mounted all the same.
    fn device<'a>(&'a self, ctx: &Context<'a>, list: &Node, d: &Device) -> Block<'a> {
        let busy = ctx.places.busy(&d.path);
        let open = match &d.mount_point {
            Some(mount_point) => Message::Open(Target::Path(mount_point.clone())),
            None => Message::Mount(d.path.clone()),
        };
        let eject = d.mount_point.as_ref().map(|mount_point| {
            let root = mount_point == std::path::Path::new("/");
            (!busy && !root).then(|| Message::Eject(d.path.clone()))
        });
        self.volume(
            ctx,
            Volume {
                node: list
                    .child("device")
                    .class(d.kind.name())
                    .class_if("mounted", d.mount_point.is_some())
                    .class_if("locked", d.locked)
                    .class_if("busy", busy),
                icons: self.device_icons(d.kind),
                name: device_name(ctx.locale, d),
                usage: d.usage,
                open: (!busy && !d.locked).then_some(open),
                eject,
            },
        )
    }

    /// A network share: its icon and its name (no usage: `statvfs` would
    /// wait on the server), ⏏ while mounted.
    fn share<'a>(&'a self, ctx: &Context<'a>, list: &Node, share: &Share) -> Block<'a> {
        let busy = ctx.places.busy(&share.key());
        let dir = share.mount_point.clone();
        let open = if share.mounted {
            Some(Message::Open(Target::Path(dir.clone())))
        } else {
            // One mounted by hand, gone: nothing to mount it again with.
            share.in_fstab.then(|| Message::MountShare(dir.clone()))
        };
        let custom = if self.config.symbolic_icons {
            &share.symbolic_icon
        } else {
            &share.icon
        };
        let mut icons: Vec<String> = custom.iter().cloned().collect();
        icons.push(self.symbolic(SHARE_ICON));
        self.volume(
            ctx,
            Volume {
                node: list
                    .child("share")
                    .class(share.family())
                    .class_if("mounted", share.mounted)
                    .class_if("busy", busy),
                icons,
                name: share.label.clone(),
                usage: None,
                open: open.filter(|_| !busy),
                eject: share
                    .mounted
                    .then(|| (!busy).then(|| Message::UnmountShare(dir))),
            },
        )
    }

    /// A device's or a share's row: a button opening it with its icon,
    /// its name and how full it is (when known), ⏏ beside it.
    fn volume<'a>(&'a self, ctx: &Context<'a>, v: Volume) -> Block<'a> {
        let theme = ctx.theme;
        let node = v.node;
        let s = theme.resolve(&node);

        // Disabled already here, not only once iced draws it: the icon
        // and the label take their colour from `button:disabled` too.
        let open = disabled(node.child("button").class("open"), v.open.is_none());
        let os = theme.resolve(&open);
        let i = open.child("icon");
        let info = open.child("info");
        let l = info.child("label");
        let meter = info.child("meter");
        let icon_box = padded(theme, &i, Size::new(icon_size(ctx, &i), icon_size(ctx, &i)));
        let text = theme.measure(&l, &v.name);
        let text = padded(
            theme,
            &l,
            Size::new(text.width, text.height.max(theme.line_height(&l))),
        );
        let mut info_height = text.height;
        let mut parts: Vec<Element<'a, Message>> =
            vec![theme.text(&l, v.name).width(Length::Fill).into()];
        if let Some(usage) = v.usage {
            let meter = meter.class_if("critical", usage >= 0.9);
            info_height +=
                theme.resolve(&info).gap + px(theme.resolve(&meter).height).unwrap_or(8.0);
            parts.push(graph::meter(theme, &meter, usage));
        }
        let info_size = padded(theme, &info, Size::new(text.width, info_height));
        let chrome = |s: &theme::Style| {
            Size::new(
                s.padding.left + s.padding.right + 2.0 * s.border_width,
                s.padding.top + s.padding.bottom + 2.0 * s.border_width,
            )
        };
        let open_chrome = chrome(&os);
        let mut size = Size::new(
            icon_box.width + os.gap + info_size.width + open_chrome.width,
            icon_box.height.max(info_size.height) + open_chrome.height,
        );
        let content = row![
            icon_view(ctx, &i, ctx.icons.first_of(&v.icons)),
            theme.column(&info, parts).width(Length::Fill),
        ]
        .spacing(os.gap)
        .align_y(Alignment::Center)
        .width(Length::Fill);
        let mut buttons: Vec<Element<'a, Message>> = vec![
            theme
                .button(&open, content)
                .width(Length::Fill)
                .on_press_maybe(v.open)
                .into(),
        ];

        if let Some(on_eject) = v.eject {
            let eject = disabled(node.child("button").class("eject"), on_eject.is_none());
            let ei = eject.child("icon");
            let es = chrome(&theme.resolve(&eject));
            let ei_size = padded(
                theme,
                &ei,
                Size::new(icon_size(ctx, &ei), icon_size(ctx, &ei)),
            );
            size.width += s.gap + ei_size.width + es.width;
            size.height = size.height.max(ei_size.height + es.height);
            buttons.push(
                theme
                    .button(&eject, icon(ctx, &ei, &self.symbolic(EJECT_ICON)))
                    .on_press_maybe(on_eject)
                    .into(),
            );
        }
        let content = row(buttons)
            .spacing(s.gap)
            .align_y(Alignment::Center)
            .width(Length::Fill);
        (
            theme.container(&node, content).width(Length::Fill).into(),
            Size::new(
                size.width + s.padding.left + s.padding.right,
                size.height + s.padding.top + s.padding.bottom,
            ),
        )
    }
}

const EJECT_ICON: &str = "media-eject";
/// A share's icon, when its fstab entry names none.
const SHARE_ICON: &str = "folder-remote";

/// What [`PlacesGadget::volume`] draws.
struct Volume {
    node: Node,
    /// Best first.
    icons: Vec<String>,
    name: String,
    usage: Option<f32>,
    /// What a click does; `None`: disabled.
    open: Option<Message>,
    /// ⏏: `None` not there, `Some(None)` disabled.
    eject: Option<Option<Message>>,
}

/// What a device is called: its label, else its size ("32 GB volume").
pub fn device_name(locale: &Locale, d: &Device) -> String {
    if d.label.is_empty() {
        locale.fmt("places.volume", &[("size", &format::bytes(locale, d.size))])
    } else {
        d.label.clone()
    }
}

/// `node` as a disabled button, when it is one.
fn disabled(node: Node, disabled: bool) -> Node {
    if disabled {
        node.status(button::Status::Disabled)
    } else {
        node
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
    icon_view(ctx, node, ctx.icons.get_name(name, None))
}

/// [`icon`] for an icon already looked up.
fn icon_view<'a>(
    ctx: &Context<'a>,
    node: &Node,
    icon: Option<&crate::icons::Icon>,
) -> Element<'a, Message> {
    let style = ctx.theme.resolve(node);
    let size = icon_size(ctx, node);
    let icon = match icon {
        Some(icon) => icon.view(size, style.color),
        None => Space::new().width(size).height(size).into(),
    };
    ctx.theme.container(node, icon).into()
}
