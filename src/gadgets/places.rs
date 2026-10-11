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

use iced_wayland_subscriber::OutputInfo;
use icssed::Element;

use crate::gadgets::{Action, Context, Gadget, Popup};
use crate::services::places::{
    Command, Device, DeviceKind, Group, Kind, Mount, Place, PlacesConfig, Target,
};

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
    MountDir(PathBuf),
    UnmountDir(PathBuf),
}

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
            Message::MountDir(dir) => Action::Many(vec![
                self.popup.close(),
                Action::Places(Command::MountDir(dir)),
            ]),
            // The popup stays: the device leaves it, or shows why not.
            Message::Eject(path) => Action::Places(Command::Eject(path)),
            Message::UnmountDir(dir) => Action::Places(Command::UnmountDir(dir)),
        }
    }

    fn icon_names(&self) -> Vec<String> {
        let mut names = vec![self.config.icon.clone()];
        names.extend(Kind::ALL.iter().map(|k| self.icon_name(*k, false)));
        names.push(self.icon_name(Kind::Trash, true));
        names.extend(DeviceKind::ALL.iter().flat_map(|k| self.device_icons(*k)));
        names.push(self.symbolic(EJECT_ICON));
        names.push(self.symbolic(SHARE_ICON));
        names.extend(ENCRYPTED_ICONS.iter().map(|n| self.symbolic(n)));
        names
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let mut content: Vec<Element<'a, Message>> =
            vec![ctx.icon(self.config.icon.as_str()).into()];
        if !self.config.label.is_empty() {
            content.push(icssed::text(self.config.label.as_str()).into());
        }
        icssed::row![
            icssed::button(content)
                .on_press(Message::TogglePopup)
                .id(self.popup.anchor())
        ]
        .into()
    }

    fn popup(&mut self) -> Option<&mut Popup> {
        Some(&mut self.popup)
    }

    fn popup_view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        icssed::column(self.rows(ctx)).kind("list").into()
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

    /// Every row of the popup: a header and the places of each section
    /// shown, the empty ones left out.
    fn rows<'a>(&'a self, ctx: Context<'a>) -> Vec<Element<'a, Message>> {
        let places = ctx.places.places();
        let shows_places = self.config.show.contains(&Group::Places);
        let mut rows = Vec::new();
        for &group in &self.config.show {
            let entries: Vec<Element<'a, Message>> = match group {
                Group::Places => places.iter().map(|p| self.item(ctx, p)).collect(),
                Group::Devices => {
                    let devices = ctx.places.devices().iter().map(|d| self.device(ctx, d));
                    let mounts = ctx.places.local_mounts().map(|m| self.mount(ctx, m));
                    devices.chain(mounts).collect()
                }
                Group::Network => ctx.places.shares().map(|s| self.mount(ctx, s)).collect(),
                // A bookmark of a place listed above isn't repeated
                // (GTK's file managers bookmark the home, often).
                Group::Bookmarks => ctx
                    .places
                    .bookmarks()
                    .iter()
                    .filter(|b| !(shows_places && places.iter().any(|p| p.target == b.target)))
                    .map(|p| self.item(ctx, p))
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
            rows.push(
                icssed::row![icssed::text(title).kind("title")]
                    .kind("header")
                    .class(group.name())
                    .into(),
            );
            rows.extend(entries);
        }
        if rows.is_empty() {
            rows.push(
                super::note(ctx.locale.tr("places.empty"))
                    .kind("empty")
                    .into(),
            );
        }
        rows
    }

    /// A place: a button with its icon and its name.
    fn item<'a>(&'a self, ctx: Context<'a>, place: &Place) -> Element<'a, Message> {
        let full = place.kind == Kind::Trash && ctx.places.trash_full();
        let label = match place.kind {
            Kind::Home => ctx.locale.tr("places.home").to_owned(),
            Kind::Trash => ctx.locale.tr("places.trash").to_owned(),
            _ => place.label.clone(),
        };
        icssed::button![
            ctx.icon(self.icon_name(place.kind, full)),
            icssed::text(label)
        ]
        .kind("item")
        .class(place.kind.name())
        .class_if("full", full)
        .on_press(Message::Open(place.target.clone()))
        .into()
    }

    /// A device: its icon, its name and, while mounted, how full it is;
    /// ⏏ beside it while mounted (as Nemo has it: nothing to eject before
    /// a mount), disabled on the root filesystem, which can't be
    /// unmounted but is mounted all the same.
    fn device<'a>(&'a self, ctx: Context<'a>, d: &Device) -> Element<'a, Message> {
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
                icons: self.device_icons(d.kind),
                name: d.name(ctx.locale),
                usage: d.usage,
                open: (!busy && !d.locked).then_some(open),
                eject,
            },
        )
        .kind("device")
        .class(d.kind.name())
        .class_if("mounted", d.mount_point.is_some())
        .class_if("locked", d.locked)
        .class_if("busy", busy)
        .into()
    }

    /// A mount UDisks2 doesn't know: a network share (`share`, no usage:
    /// `statvfs` would wait on the server) or a local FUSE or bind mount
    /// (with the devices, as a `device`); ⏏ while mounted.
    fn mount<'a>(&'a self, ctx: Context<'a>, share: &Mount) -> Element<'a, Message> {
        let busy = ctx.places.busy(&share.key());
        let dir = share.mount_point.clone();
        let open = if share.mounted {
            Some(Message::Open(Target::Path(dir.clone())))
        } else {
            // One mounted by hand, gone: nothing to mount it again with.
            share.in_fstab.then(|| Message::MountDir(dir.clone()))
        };
        let custom = if self.config.symbolic_icons {
            &share.symbolic_icon
        } else {
            &share.icon
        };
        let mut icons: Vec<String> = custom.iter().cloned().collect();
        let fallback: &[&str] = if share.network {
            &[SHARE_ICON]
        } else if share.encrypted() {
            ENCRYPTED_ICONS
        } else {
            &["folder"]
        };
        icons.extend(fallback.iter().map(|n| self.symbolic(n)));
        self.volume(
            ctx,
            Volume {
                icons,
                name: share.label.clone(),
                usage: share.usage,
                open: open.filter(|_| !busy),
                eject: share
                    .mounted
                    .then(|| (!busy).then(|| Message::UnmountDir(dir))),
            },
        )
        .kind(if share.network { "share" } else { "device" })
        .class(share.family())
        .class_if("mounted", share.mounted)
        .class_if("busy", busy)
        .into()
    }

    /// A device's or a share's row (its type and classes the caller's):
    /// a button opening it with its icon, its name and how full it is
    /// (when known), ⏏ beside it.
    fn volume<'a>(&'a self, ctx: Context<'a>, v: Volume) -> icssed::Flex<'a, Message> {
        let mut parts: Vec<Element<'a, Message>> = vec![icssed::text(v.name).into()];
        if let Some(usage) = v.usage {
            parts.push(
                icssed::meter(usage)
                    .class_if("critical", usage >= 0.9)
                    .into(),
            );
        }
        let icon: Element<'a, Message> = match ctx.icons.first_of(&v.icons) {
            Some(icon) => icssed::icon(icon.clone()).into(),
            None => ctx.icon("").into(),
        };
        // Disabled, the icon and the label take `button:disabled`'s
        // colour.
        let mut open = icssed::button![icon, icssed::column(parts).kind("info")]
            .class("open")
            .class("flat")
            .disabled_if(v.open.is_none());
        if let Some(message) = v.open {
            open = open.on_press(message);
        }
        let mut buttons: Vec<Element<'a, Message>> = vec![open.into()];
        if let Some(on_eject) = v.eject {
            let mut eject = icssed::button![ctx.icon(self.symbolic(EJECT_ICON))]
                .class("eject")
                .class("flat")
                .disabled_if(on_eject.is_none());
            if let Some(message) = on_eject {
                eject = eject.on_press(message);
            }
            buttons.push(eject.into());
        }
        icssed::row(buttons)
    }
}

/// What [`PlacesGadget::volume`] draws.
struct Volume {
    /// Best first.
    icons: Vec<String>,
    name: String,
    usage: Option<f32>,
    /// What a click does; `None`: disabled.
    open: Option<Message>,
    /// ⏏: `None` not there, `Some(None)` disabled.
    eject: Option<Option<Message>>,
}

const EJECT_ICON: &str = "media-eject";
/// A share's icon, when its fstab entry names none.
const SHARE_ICON: &str = "folder-remote";
/// An encrypted folder's (encfs, gocryptfs, ...), best first.
const ENCRYPTED_ICONS: &[&str] = &[
    "folder-encrypted",
    "folder-locked",
    "channel-secure",
    "security-high",
    "folder",
];
