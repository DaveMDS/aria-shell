//! Places: the locations a file manager's sidebar lists, for the
//! `Places` gadget. The home, the XDG user folders (`user-dirs.dirs`)
//! and the trash; the bookmarks GTK's file managers share
//! (`gtk-3.0/bookmarks`: Nautilus, Nemo, Thunar, Caja) and KDE's
//! (`user-places.xbel`: Dolphin), read by `bookmarks.rs`. Read again by [`Command::Refresh`]
//! whenever the popup opens (a few small files); a click opens one in
//! `[general] file_manager` ([`Command::Open`]).
//!
//! The devices come from UDisks2 (`udisks.rs`): followed on the system
//! bus, filtered as GVfs does (`devices.rs`), mounted and ejected with
//! [`Command::Mount`] / [`Command::Eject`]. The other mounts from fstab
//! and mountinfo (`mounts.rs`: network shares, FUSE and bind mounts),
//! read with the rest, mounted with `mount` ([`Command::MountDir`] /
//! [`Command::UnmountDir`]). A failure comes back as a [`Failure`] for
//! the daemon to notify.

mod bookmarks;
mod devices;
mod mounts;
mod udisks;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use iced::{Subscription, Task};
use zbus::Connection;

use crate::config::{RawSection, Section};
use crate::locale::Locale;
use crate::process;
use crate::services::notifications::client;
use crate::services::sysmon::format;
use bookmarks::{base_name, bookmark, parse_gtk_bookmarks, parse_user_dirs, parse_xbel, xdg_dir};
use devices::{devices, read_usage, usage};

/// `[Places]` section: the gadget's keys (the daemon has none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacesConfig {
    /// The popup's sections, in this order (required).
    pub show: Vec<Group>,
    pub icon: String,
    /// Beside the icon; none when empty.
    pub label: String,
    /// The places' `-symbolic` icons, else the full colour ones.
    pub symbolic_icons: bool,
}

impl Section for PlacesConfig {
    const NAME: &'static str = "Places";

    fn from_raw(raw: &RawSection) -> Self {
        let show = raw
            .list_or("show", &[])
            .iter()
            .filter_map(|name| {
                let group = Group::from_name(name);
                if group.is_none() {
                    log::warn!(
                        "[Places] show: unknown section {name:?} (places, devices, network, bookmarks)"
                    );
                }
                group
            })
            .collect();
        Self {
            show,
            icon: raw.str_or("icon", "folder-symbolic"),
            label: raw.str_or("label", ""),
            symbolic_icons: raw.bool_or("symbolic_icons", true),
        }
    }
}

/// A section of the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// The home, the XDG folders, the trash.
    Places,
    /// UDisks2's.
    Devices,
    /// fstab's network shares, and those mounted by hand.
    Network,
    /// GTK's and KDE's.
    Bookmarks,
}

impl Group {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "places" => Some(Self::Places),
            "devices" => Some(Self::Devices),
            "network" => Some(Self::Network),
            "bookmarks" => Some(Self::Bookmarks),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Places => "places",
            Self::Devices => "devices",
            Self::Network => "network",
            Self::Bookmarks => "bookmarks",
        }
    }
}

/// What a place is, for its icon and its class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Home,
    Desktop,
    Documents,
    Download,
    Music,
    Pictures,
    Videos,
    Trash,
    /// A bookmarked local folder.
    Folder,
    /// A bookmarked URI (sftp://, smb://, ...).
    Remote,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Home,
        Kind::Desktop,
        Kind::Documents,
        Kind::Download,
        Kind::Music,
        Kind::Pictures,
        Kind::Videos,
        Kind::Trash,
        Kind::Folder,
        Kind::Remote,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Desktop => "desktop",
            Self::Documents => "documents",
            Self::Download => "download",
            Self::Music => "music",
            Self::Pictures => "pictures",
            Self::Videos => "videos",
            Self::Trash => "trash",
            Self::Folder => "folder",
            Self::Remote => "remote",
        }
    }

    /// The icon theme's name, without `-symbolic`; the trash's when
    /// empty (`user-trash-full` otherwise).
    pub fn icon(self) -> &'static str {
        match self {
            Self::Home => "user-home",
            Self::Desktop => "user-desktop",
            Self::Documents => "folder-documents",
            Self::Download => "folder-download",
            Self::Music => "folder-music",
            Self::Pictures => "folder-pictures",
            Self::Videos => "folder-videos",
            Self::Trash => "user-trash",
            Self::Folder => "folder",
            Self::Remote => "folder-remote",
        }
    }
}

/// What the file manager is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Path(PathBuf),
    /// As written: the file manager knows the scheme (gvfs, kio).
    Uri(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub kind: Kind,
    /// As shown; empty for the home and the trash, whose names are
    /// the locale's.
    pub label: String,
    pub target: Target,
}

/// A filesystem UDisks2 has, as the popup lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// UDisks2's object path of the block with the filesystem (the
    /// cleartext one, for an unlocked LUKS volume): its key.
    pub path: String,
    /// `/dev/sdb1`, for `debug places` and the order.
    pub device: PathBuf,
    /// The filesystem's label; empty when it has none (the gadget then
    /// words it by size).
    pub label: String,
    pub size: u64,
    pub kind: DeviceKind,
    /// Where it's mounted (the first place, when several).
    pub mount_point: Option<PathBuf>,
    /// A LUKS volume not unlocked (it can't be opened yet).
    pub locked: bool,
    /// On a drive that comes out (USB, SD card, optical): an eject
    /// also ejects or powers off the drive.
    pub removable: bool,
    /// The drive's object path; empty for none.
    pub drive: String,
    pub drive_action: Option<DriveAction>,
    /// The LUKS container of an unlocked volume, locked again by an
    /// eject.
    pub crypto_backing: Option<String>,
    /// The fraction used, read with `statvfs` while mounted.
    pub usage: Option<f32>,
}

impl Device {
    /// What it's called: its label, else its size ("32 GB volume").
    pub fn name(&self, locale: &Locale) -> String {
        if self.label.is_empty() {
            locale.fmt(
                "places.volume",
                &[("size", &format::bytes(locale, self.size))],
            )
        } else {
            self.label.clone()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    HardDisk,
    /// A disk on USB (or FireWire): an external hard disk, a USB stick
    /// that doesn't say it is one.
    UsbDisk,
    /// A USB stick (`Media` `thumb`).
    Thumb,
    /// An SD card, a flash reader's media.
    Flash,
    Optical,
    /// Any other drive that comes out.
    Removable,
}

impl DeviceKind {
    pub const ALL: [DeviceKind; 6] = [
        DeviceKind::HardDisk,
        DeviceKind::UsbDisk,
        DeviceKind::Thumb,
        DeviceKind::Flash,
        DeviceKind::Optical,
        DeviceKind::Removable,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::HardDisk => "harddisk",
            Self::UsbDisk => "usb",
            Self::Thumb => "thumb",
            Self::Flash => "flash",
            Self::Optical => "optical",
            Self::Removable => "removable",
        }
    }

    /// The icon theme's names, without `-symbolic`, best first: the
    /// first the theme has is used (as libudisks names them for
    /// Nautilus and Nemo; the last one every theme has).
    pub fn icons(self) -> &'static [&'static str] {
        match self {
            Self::HardDisk => &["drive-harddisk"],
            Self::UsbDisk => &[
                "drive-harddisk-usb",
                "drive-removable-media-usb",
                "drive-removable-media",
            ],
            Self::Thumb => &[
                "media-removable",
                "drive-removable-media-usb",
                "drive-removable-media",
            ],
            Self::Flash => &["media-flash", "drive-removable-media"],
            Self::Optical => &["media-optical", "drive-optical"],
            Self::Removable => &["drive-removable-media"],
        }
    }
}

/// A mount UDisks2 doesn't know (no block device under it), as the
/// popup lists it: a network share, in its own section; a FUSE or bind
/// mount (encfs, gocryptfs, bindfs, ...), with the devices.
#[derive(Debug, Clone, PartialEq)]
pub struct Mount {
    /// Its key.
    pub mount_point: PathBuf,
    /// `x-gvfs-name`, else the folder's name.
    pub label: String,
    /// `server:/export`, `//server/share`, `user@host:dir`, `encfs`.
    pub source: String,
    pub fs_type: String,
    /// A network filesystem (no usage read: `statvfs` would wait on the
    /// server).
    pub network: bool,
    /// The fraction used of a local one, while mounted.
    pub usage: Option<f32>,
    /// `x-gvfs-icon`, `x-gvfs-symbolic-icon`.
    pub icon: Option<String>,
    pub symbolic_icon: Option<String>,
    pub mounted: bool,
    /// In fstab (it can be mounted again), not mounted by hand.
    pub in_fstab: bool,
}

impl Mount {
    /// The theme's class: a share's protocol (`nfs`, `smb`, `ssh`,
    /// `dav`, `other`), a local mount's kind (`encrypted`, `fuse`).
    pub fn family(&self) -> &'static str {
        if !self.network {
            return if self.encrypted() {
                "encrypted"
            } else {
                "fuse"
            };
        }
        match self.fs_type.as_str() {
            "nfs" | "nfs4" => "nfs",
            "cifs" | "smb3" | "smbfs" => "smb",
            "sshfs" | "fuse.sshfs" | "fuse" => "ssh",
            "davfs" | "fuse.davfs2" => "dav",
            _ => "other",
        }
    }

    /// An encrypted folder: encfs, gocryptfs, CryFS, securefs, eCryptfs.
    pub fn encrypted(&self) -> bool {
        matches!(
            self.fs_type.as_str(),
            "fuse.encfs" | "encfs" | "fuse.gocryptfs" | "fuse.cryfs" | "fuse.securefs" | "ecryptfs"
        )
    }

    /// What [`Places::busy`] knows it by.
    pub fn key(&self) -> String {
        mount_key(&self.mount_point)
    }
}

fn mount_key(mount_point: &Path) -> String {
    format!("mount:{}", mount_point.display())
}

/// What an eject does to the drive, after unmounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveAction {
    /// Optical media out of the tray.
    Eject,
    /// Safe removal: the drive spun down and switched off.
    PowerOff,
}

/// A UDisks2 block, as `udisks.rs` read it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Block {
    path: String,
    device: PathBuf,
    size: u64,
    id_usage: String,
    id_type: String,
    id_label: String,
    hint_ignore: bool,
    hint_system: bool,
    /// Empty for none.
    drive: String,
    /// Empty for none.
    crypto_backing: String,
    /// The fstab entries: `dir`, `opts`.
    fstab: Vec<(PathBuf, String)>,
    /// `Some` when it has a filesystem.
    mount_points: Option<Vec<PathBuf>>,
    /// `Some` when it's a LUKS container: its cleartext block, empty
    /// while locked.
    cleartext: Option<String>,
}

/// A UDisks2 drive, as `udisks.rs` read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drive {
    removable: bool,
    ejectable: bool,
    can_power_off: bool,
    optical: bool,
    connection_bus: String,
    media: String,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// The system bus.
    Bus(Connection),
    /// UDisks2's blocks and drives; `None` while it isn't running.
    Objects(Option<(Vec<Block>, HashMap<String, Drive>)>),
    /// A mount went through: the device is opened there.
    Mounted {
        path: String,
        mount_point: PathBuf,
    },
    Ejected(String),
    /// `mount` went through: the share is opened.
    DirMounted(PathBuf),
    DirUnmounted(PathBuf),
    Failed(Failure),
}

/// What failed to mount or unmount.
#[derive(Debug, Clone, PartialEq)]
pub enum Volume {
    Device(Device),
    Mount(Mount),
}

impl Volume {
    fn key(&self) -> String {
        match self {
            Self::Device(d) => d.path.clone(),
            Self::Mount(s) => s.key(),
        }
    }
}

/// A mount or an eject refused (by UDisks2, by `mount`), for the daemon
/// to notify ([`Failure::notify`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub eject: bool,
    pub volume: Volume,
    /// UDisks2's message, `mount`'s stderr.
    pub message: String,
    /// Polkit said no: no agent to ask for the password, or the wrong
    /// one.
    pub not_authorized: bool,
}

impl Failure {
    /// Say so in a notification, in the user's language, with UDisks2's
    /// reason (polkit's refusal worded: no agent to ask for the
    /// password, usually).
    pub fn notify(self, locale: &Locale) -> Task<Event> {
        let (name, icon) = match &self.volume {
            Volume::Device(d) => (d.name(locale), "drive-harddisk-symbolic"),
            Volume::Mount(s) => (s.label.clone(), "folder-remote-symbolic"),
        };
        let key = match (&self.volume, self.eject) {
            (_, false) => "places.mount_failed",
            (Volume::Device(_), true) => "places.eject_failed",
            (Volume::Mount(_), true) => "places.unmount_failed",
        };
        let summary = locale.fmt(key, &[("name", &name)]);
        let body = if self.not_authorized {
            locale.tr("places.not_authorized").to_owned()
        } else {
            self.message
        };
        let icon = icon.to_owned();
        Task::future(async move {
            if let Err(e) = client::notify(0, icon, summary, body, false).await {
                log::warn!("places: can't notify: {e}");
            }
        })
        .discard()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Read everything again.
    Refresh,
    Open(Target),
    /// Mount a device (by its path), then open it.
    Mount(String),
    /// Unmount it (and lock it, when it's an unlocked LUKS volume),
    /// then eject or power off its drive when it's removable.
    Eject(String),
    /// `mount` a share (by its mount point), then open it.
    MountDir(PathBuf),
    UnmountDir(PathBuf),
}

#[derive(Debug, Default)]
pub struct Places {
    places: Vec<Place>,
    bookmarks: Vec<Place>,
    trash_full: bool,
    bus: Option<Connection>,
    /// `None` while UDisks2 isn't running.
    devices: Option<Vec<Device>>,
    /// The mounts UDisks2 doesn't know: shares and local ones.
    mounts: Vec<Mount>,
    /// The devices (by path) and the shares ([`Mount::key`]) a mount or
    /// an eject is under way for.
    busy: BTreeSet<String>,
}

const TRASH_URI: &str = "trash:///";

impl Places {
    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::run(udisks::events)
    }

    /// Apply an event: whether what gadgets see changed, and a failure
    /// to notify. A mount that went through is opened in `file_manager`.
    pub fn apply(&mut self, event: Event, file_manager: Option<&str>) -> (bool, Option<Failure>) {
        match event {
            Event::Bus(conn) => {
                self.bus = Some(conn);
                (false, None)
            }
            Event::Objects(objects) => {
                let devices = objects.map(|(blocks, drives)| {
                    let home = std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_default();
                    let mut devices = devices(&blocks, &drives, &home);
                    read_usage(&mut devices);
                    devices
                });
                if devices == self.devices {
                    return (false, None);
                }
                self.devices = devices;
                (true, None)
            }
            Event::Mounted { path, mount_point } => {
                log::info!("places: {path} mounted at {}", mount_point.display());
                self.busy.remove(&path);
                open(file_manager, &Target::Path(mount_point));
                (true, None)
            }
            Event::Ejected(path) => {
                log::info!("places: {path} ejected");
                self.busy.remove(&path);
                (true, None)
            }
            Event::DirMounted(dir) => {
                log::info!("places: {} mounted", dir.display());
                self.busy.remove(&mount_key(&dir));
                self.reload_mounts();
                open(file_manager, &Target::Path(dir));
                (true, None)
            }
            Event::DirUnmounted(dir) => {
                log::info!("places: {} unmounted", dir.display());
                self.busy.remove(&mount_key(&dir));
                self.reload_mounts();
                (true, None)
            }
            Event::Failed(failure) => {
                let key = failure.volume.key();
                log::warn!(
                    "places: {} {key}: {}",
                    if failure.eject { "eject" } else { "mount" },
                    failure.message
                );
                self.busy.remove(&key);
                if matches!(failure.volume, Volume::Mount(_)) {
                    self.reload_mounts();
                }
                (true, Some(failure))
            }
        }
    }

    pub fn run(&mut self, command: Command, file_manager: Option<&str>) -> Task<Event> {
        match command {
            Command::Refresh => {
                self.reload();
                self.reload_mounts();
                if let Some(devices) = &mut self.devices {
                    read_usage(devices);
                }
                Task::none()
            }
            Command::Open(target) => {
                open(file_manager, &target);
                Task::none()
            }
            Command::Mount(path) => {
                let (Some(conn), Some(device)) = (self.bus.clone(), self.device(&path).cloned())
                else {
                    return Task::none();
                };
                if !self.busy.insert(path.clone()) {
                    return Task::none();
                }
                log::info!("places: mounting {path}");
                Task::future(async move {
                    match udisks::mount(conn, path.clone()).await {
                        Ok(mount_point) => Event::Mounted { path, mount_point },
                        Err(e) => {
                            let (message, not_authorized) = udisks::failure(&e);
                            Event::Failed(Failure {
                                eject: false,
                                volume: Volume::Device(device),
                                message,
                                not_authorized,
                            })
                        }
                    }
                })
            }
            Command::Eject(path) => {
                let (Some(conn), Some(device)) = (self.bus.clone(), self.device(&path).cloned())
                else {
                    return Task::none();
                };
                if !self.busy.insert(path.clone()) {
                    return Task::none();
                }
                let plan = self.eject_plan(&device);
                log::info!("places: ejecting {path}: {plan:?}");
                Task::future(async move {
                    match udisks::eject(conn, plan).await {
                        Ok(()) => Event::Ejected(path),
                        Err(e) => {
                            let (message, not_authorized) = udisks::failure(&e);
                            Event::Failed(Failure {
                                eject: true,
                                volume: Volume::Device(device),
                                message,
                                not_authorized,
                            })
                        }
                    }
                })
            }
            Command::MountDir(dir) => {
                let Some(share) = self.share(&dir).cloned() else {
                    return Task::none();
                };
                if !self.busy.insert(share.key()) {
                    return Task::none();
                }
                log::info!("places: mounting {}", dir.display());
                Task::future(async move {
                    match mounts::mount(dir.clone()).await {
                        Ok(()) => Event::DirMounted(dir),
                        Err(message) => Event::Failed(Failure {
                            eject: false,
                            volume: Volume::Mount(share),
                            message,
                            not_authorized: false,
                        }),
                    }
                })
            }
            Command::UnmountDir(dir) => {
                let Some(share) = self.share(&dir).cloned() else {
                    return Task::none();
                };
                if !self.busy.insert(share.key()) {
                    return Task::none();
                }
                log::info!("places: unmounting {}", dir.display());
                Task::future(async move {
                    match mounts::unmount(share.clone()).await {
                        Ok(()) => Event::DirUnmounted(dir),
                        Err(message) => Event::Failed(Failure {
                            eject: true,
                            volume: Volume::Mount(share),
                            message,
                            not_authorized: false,
                        }),
                    }
                })
            }
        }
    }

    pub fn places(&self) -> &[Place] {
        &self.places
    }

    pub fn bookmarks(&self) -> &[Place] {
        &self.bookmarks
    }

    pub fn trash_full(&self) -> bool {
        self.trash_full
    }

    pub fn devices(&self) -> &[Device] {
        self.devices.as_deref().unwrap_or_default()
    }

    /// The network shares.
    pub fn shares(&self) -> impl Iterator<Item = &Mount> {
        self.mounts.iter().filter(|m| m.network)
    }

    /// The local mounts UDisks2 doesn't know (FUSE, bind), listed with
    /// the devices.
    pub fn local_mounts(&self) -> impl Iterator<Item = &Mount> {
        self.mounts.iter().filter(|m| !m.network)
    }

    /// Whether a mount or an eject is under way for a device (by its
    /// path) or a share ([`Mount::key`]).
    pub fn busy(&self, key: &str) -> bool {
        self.busy.contains(key)
    }

    /// The icons the shares' fstab entries name, for the daemon to
    /// resolve.
    pub fn icon_names(&self) -> impl Iterator<Item = &str> {
        self.mounts
            .iter()
            .flat_map(|s| s.icon.iter().chain(s.symbolic_icon.iter()))
            .map(String::as_str)
    }

    fn device(&self, path: &str) -> Option<&Device> {
        self.devices().iter().find(|d| d.path == path)
    }

    fn share(&self, mount_point: &Path) -> Option<&Mount> {
        self.mounts.iter().find(|s| s.mount_point == mount_point)
    }

    /// fstab and mountinfo read again: local files, quick.
    fn reload_mounts(&mut self) {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let read = |path| std::fs::read_to_string(path).unwrap_or_default();
        let mut mounts = mounts::mounts(
            &mounts::parse_fstab(&read(mounts::fstab_path())),
            &mounts::parse_mountinfo(&read(mounts::mountinfo_path())),
            &home,
        );
        for m in &mut mounts {
            if m.mounted && !m.network {
                m.usage = usage(&m.mount_point);
            }
        }
        self.mounts = mounts;
    }

    /// Unmount, lock, then the drive: only when it's removable and
    /// nothing else on it stays mounted (the user unmounts the other
    /// partitions first).
    fn eject_plan(&self, device: &Device) -> udisks::Eject {
        let others_mounted = self
            .devices()
            .iter()
            .any(|d| d.path != device.path && d.drive == device.drive && d.mount_point.is_some());
        udisks::Eject {
            unmount: device.mount_point.as_ref().map(|_| device.path.clone()),
            lock: device.crypto_backing.clone(),
            drive: device
                .drive_action
                .filter(|_| device.removable && !device.drive.is_empty() && !others_mounted)
                .map(|action| (device.drive.clone(), action)),
        }
    }

    /// For `aria-shell debug places`.
    pub fn describe(&self) -> String {
        let mut parts = vec![format!(
            "places={} bookmarks={} trash_full={}",
            self.places.len(),
            self.bookmarks.len(),
            self.trash_full
        )];
        parts.extend(self.mounts.iter().map(|s| {
            format!(
                "{} {} {:?} label={:?} type={} mounted={} fstab={} busy={}",
                if s.network { "share" } else { "mount" },
                s.mount_point.display(),
                s.source,
                s.label,
                s.fs_type,
                s.mounted,
                s.in_fstab,
                self.busy(&s.key()),
            )
        }));
        match &self.devices {
            None => parts.push("udisks=false".to_owned()),
            Some(devices) => parts.extend(devices.iter().map(|d| {
                format!(
                    "{} {:?} label={:?} kind={} mounted={} usage={} removable={} drive_action={:?} locked={} busy={}",
                    d.device.display(),
                    d.path,
                    d.label,
                    d.kind.name(),
                    d.mount_point
                        .as_ref()
                        .map_or("-".to_owned(), |p| p.display().to_string()),
                    d.usage.map_or("-".to_owned(), |u| format!("{:.0}%", u * 100.0)),
                    d.removable,
                    d.drive_action,
                    d.locked,
                    self.busy(&d.path),
                )
            })),
        }
        parts.join("; ")
    }

    fn reload(&mut self) {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            log::warn!("places: no $HOME");
            return;
        };
        let config_home = xdg_dir("XDG_CONFIG_HOME", &home, ".config");
        let data_home = xdg_dir("XDG_DATA_HOME", &home, ".local/share");
        let read = |path: PathBuf| std::fs::read_to_string(path).unwrap_or_default();

        let mut places = vec![Place {
            kind: Kind::Home,
            label: String::new(),
            target: Target::Path(home.clone()),
        }];
        for (kind, path) in parse_user_dirs(&read(config_home.join("user-dirs.dirs")), &home) {
            if path.is_dir() {
                places.push(Place {
                    kind,
                    label: base_name(&path),
                    target: Target::Path(path),
                });
            }
        }
        places.push(Place {
            kind: Kind::Trash,
            label: String::new(),
            target: Target::Uri(TRASH_URI.to_owned()),
        });
        self.places = places;

        let gtk = parse_gtk_bookmarks(&read(config_home.join("gtk-3.0/bookmarks")));
        let kde = parse_xbel(&read(data_home.join("user-places.xbel")));
        let mut bookmarks: Vec<Place> = Vec::new();
        for (uri, label) in gtk.into_iter().chain(kde) {
            let Some(place) = bookmark(&uri, label) else {
                continue;
            };
            let gone = matches!(&place.target, Target::Path(p) if !p.is_dir());
            if !gone && !bookmarks.iter().any(|b| b.target == place.target) {
                bookmarks.push(place);
            }
        }
        self.bookmarks = bookmarks;

        // The spec's home trash; the trash of other filesystems
        // (`.Trash-<uid>` on a USB stick) is the file manager's.
        self.trash_full = std::fs::read_dir(data_home.join("Trash/files"))
            .is_ok_and(|mut entries| entries.next().is_some());
    }
}

/// Run `[general] file_manager` on a place.
fn open(file_manager: Option<&str>, target: &Target) {
    match file_manager {
        Some(file_manager) => {
            let argv = match target {
                Target::Path(path) => process::on_file(file_manager, path),
                Target::Uri(uri) => process::on_file(file_manager, uri),
            };
            process::run_argv(&argv);
        }
        None => log::warn!("places: no file manager ([general] file_manager)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn config_sections_in_order() {
        let config = Config::parse("[Places]\nshow = bookmarks devices nope places\nlabel = Go\n");
        let places: PlacesConfig = config.section(None);
        assert_eq!(
            places.show,
            vec![Group::Bookmarks, Group::Devices, Group::Places]
        );
        assert_eq!(places.label, "Go");
        assert_eq!(places.icon, "folder-symbolic");
        assert!(places.symbolic_icons);
    }

    #[test]
    fn eject_plans() {
        let device = |path: &str, mount: Option<&str>| Device {
            path: path.to_owned(),
            device: PathBuf::new(),
            label: String::new(),
            size: 0,
            kind: DeviceKind::Thumb,
            mount_point: mount.map(PathBuf::from),
            locked: false,
            removable: true,
            drive: "/d/usb".to_owned(),
            drive_action: Some(DriveAction::PowerOff),
            crypto_backing: None,
            usage: None,
        };
        let places = Places {
            devices: Some(vec![
                device("/b/sdb1", Some("/run/media/u/A")),
                device("/b/sdb2", None),
            ]),
            ..Places::default()
        };
        let a = places.device("/b/sdb1").unwrap().clone();
        assert_eq!(
            places.eject_plan(&a),
            udisks::Eject {
                unmount: Some("/b/sdb1".to_owned()),
                lock: None,
                drive: Some(("/d/usb".to_owned(), DriveAction::PowerOff)),
            },
            "unmounted, then the drive off"
        );
        let b = places.device("/b/sdb2").unwrap().clone();
        assert_eq!(
            places.eject_plan(&b).drive,
            None,
            "the other partition is still mounted: the drive stays on"
        );
        assert_eq!(places.eject_plan(&b).unmount, None, "nothing to unmount");
    }
}
