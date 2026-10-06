//! Places: the locations a file manager's sidebar lists, for the
//! `Places` gadget. The home, the XDG user folders (`user-dirs.dirs`)
//! and the trash; the bookmarks GTK's file managers share
//! (`gtk-3.0/bookmarks`: Nautilus, Nemo, Thunar, Caja) and KDE's
//! (`user-places.xbel`: Dolphin). Read again by [`Command::Refresh`]
//! whenever the popup opens (a few small files); a click opens one in
//! `[general] file_manager` ([`Command::Open`]).
//!
//! The devices come from UDisks2 (`udisks.rs`): followed on the system
//! bus, filtered as GVfs does ([`devices`]), mounted and ejected with
//! [`Command::Mount`] / [`Command::Eject`]. The network shares from fstab
//! and mountinfo (`network.rs`), read with the rest, mounted with
//! `mount` ([`Command::MountShare`] / [`Command::UnmountShare`]). A
//! failure comes back as a [`Failure`] for the daemon to notify.

mod network;
mod udisks;

use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use iced::{Subscription, Task};
use zbus::Connection;

use crate::config::{RawSection, Section};
use crate::process;

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

/// A network share, as the popup lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    /// Its key.
    pub mount_point: PathBuf,
    /// `x-gvfs-name`, else the folder's name.
    pub label: String,
    /// `server:/export`, `//server/share`, `user@host:dir`.
    pub source: String,
    pub fs_type: String,
    /// `x-gvfs-icon`, `x-gvfs-symbolic-icon`.
    pub icon: Option<String>,
    pub symbolic_icon: Option<String>,
    pub mounted: bool,
    /// In fstab (it can be mounted again), not mounted by hand.
    pub in_fstab: bool,
}

impl Share {
    /// The theme's class: the protocol's family.
    pub fn family(&self) -> &'static str {
        match self.fs_type.as_str() {
            "nfs" | "nfs4" => "nfs",
            "cifs" | "smb3" | "smbfs" => "smb",
            "sshfs" | "fuse.sshfs" | "fuse" => "ssh",
            "davfs" | "fuse.davfs2" => "dav",
            _ => "other",
        }
    }

    /// What [`Places::busy`] knows it by.
    pub fn key(&self) -> String {
        share_key(&self.mount_point)
    }
}

fn share_key(mount_point: &Path) -> String {
    format!("share:{}", mount_point.display())
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
    ShareMounted(PathBuf),
    ShareUnmounted(PathBuf),
    Failed(Failure),
}

/// What failed to mount or unmount.
#[derive(Debug, Clone, PartialEq)]
pub enum Volume {
    Device(Device),
    Share(Share),
}

impl Volume {
    fn key(&self) -> String {
        match self {
            Self::Device(d) => d.path.clone(),
            Self::Share(s) => s.key(),
        }
    }
}

/// A mount or an eject refused (by UDisks2, by `mount`), for the daemon
/// to notify.
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
    MountShare(PathBuf),
    UnmountShare(PathBuf),
}

#[derive(Debug, Default)]
pub struct Places {
    places: Vec<Place>,
    bookmarks: Vec<Place>,
    trash_full: bool,
    bus: Option<Connection>,
    /// `None` while UDisks2 isn't running.
    devices: Option<Vec<Device>>,
    shares: Vec<Share>,
    /// The devices (by path) and the shares ([`Share::key`]) a mount or
    /// an eject is under way for.
    busy: BTreeSet<String>,
}

/// The XDG folders shown, in this order (Templates and Public aren't).
const USER_DIRS: [(&str, Kind); 6] = [
    ("XDG_DESKTOP_DIR", Kind::Desktop),
    ("XDG_DOCUMENTS_DIR", Kind::Documents),
    ("XDG_DOWNLOAD_DIR", Kind::Download),
    ("XDG_MUSIC_DIR", Kind::Music),
    ("XDG_PICTURES_DIR", Kind::Pictures),
    ("XDG_VIDEOS_DIR", Kind::Videos),
];

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
            Event::ShareMounted(dir) => {
                log::info!("places: {} mounted", dir.display());
                self.busy.remove(&share_key(&dir));
                self.reload_shares();
                open(file_manager, &Target::Path(dir));
                (true, None)
            }
            Event::ShareUnmounted(dir) => {
                log::info!("places: {} unmounted", dir.display());
                self.busy.remove(&share_key(&dir));
                self.reload_shares();
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
                if matches!(failure.volume, Volume::Share(_)) {
                    self.reload_shares();
                }
                (true, Some(failure))
            }
        }
    }

    pub fn run(&mut self, command: Command, file_manager: Option<&str>) -> Task<Event> {
        match command {
            Command::Refresh => {
                self.reload();
                self.reload_shares();
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
            Command::MountShare(dir) => {
                let Some(share) = self.share(&dir).cloned() else {
                    return Task::none();
                };
                if !self.busy.insert(share.key()) {
                    return Task::none();
                }
                log::info!("places: mounting {}", dir.display());
                Task::future(async move {
                    match network::mount(dir.clone()).await {
                        Ok(()) => Event::ShareMounted(dir),
                        Err(message) => Event::Failed(Failure {
                            eject: false,
                            volume: Volume::Share(share),
                            message,
                            not_authorized: false,
                        }),
                    }
                })
            }
            Command::UnmountShare(dir) => {
                let Some(share) = self.share(&dir).cloned() else {
                    return Task::none();
                };
                if !self.busy.insert(share.key()) {
                    return Task::none();
                }
                log::info!("places: unmounting {}", dir.display());
                Task::future(async move {
                    match network::unmount(share.clone()).await {
                        Ok(()) => Event::ShareUnmounted(dir),
                        Err(message) => Event::Failed(Failure {
                            eject: true,
                            volume: Volume::Share(share),
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

    pub fn shares(&self) -> &[Share] {
        &self.shares
    }

    /// Whether a mount or an eject is under way for a device (by its
    /// path) or a share ([`Share::key`]).
    pub fn busy(&self, key: &str) -> bool {
        self.busy.contains(key)
    }

    /// The icons the shares' fstab entries name, for the daemon to
    /// resolve.
    pub fn icon_names(&self) -> impl Iterator<Item = &str> {
        self.shares
            .iter()
            .flat_map(|s| s.icon.iter().chain(s.symbolic_icon.iter()))
            .map(String::as_str)
    }

    fn device(&self, path: &str) -> Option<&Device> {
        self.devices().iter().find(|d| d.path == path)
    }

    fn share(&self, mount_point: &Path) -> Option<&Share> {
        self.shares.iter().find(|s| s.mount_point == mount_point)
    }

    /// fstab and mountinfo read again: local files, quick.
    fn reload_shares(&mut self) {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let read = |path| std::fs::read_to_string(path).unwrap_or_default();
        self.shares = network::shares(
            &network::parse_fstab(&read(network::fstab_path())),
            &network::parse_mountinfo(&read(network::mountinfo_path())),
            &home,
        );
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
        parts.extend(self.shares.iter().map(|s| {
            format!(
                "share {} {:?} label={:?} type={} mounted={} fstab={} busy={}",
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

/// The devices worth listing, as GVfs (Nautilus, Nemo) chooses them:
/// a filesystem or a LUKS volume UDisks2 doesn't say to ignore, not
/// swap; a system one (an internal disk) only while it's mounted, or
/// set in fstab, somewhere a user looks (`/media`, `/run/media`, `/mnt`,
/// the home), or with `x-gvfs-show`, and the root filesystem (as
/// Dolphin lists it: `/` isn't among the places); `x-gvfs-hide` hides
/// any. An unlocked LUKS container is listed as its cleartext volume.
fn devices(blocks: &[Block], drives: &HashMap<String, Drive>, home: &Path) -> Vec<Device> {
    let by_path: HashMap<&str, &Block> = blocks.iter().map(|b| (b.path.as_str(), b)).collect();
    let mut devices: Vec<Device> = blocks
        .iter()
        .filter(|b| shown(b, home))
        .map(|b| {
            let backing = by_path.get(b.crypto_backing.as_str());
            // A cleartext volume's drive is its container's.
            let drive_path = match backing {
                Some(c) if b.drive.is_empty() => c.drive.clone(),
                _ => b.drive.clone(),
            };
            let drive = drives.get(&drive_path);
            let removable = drive.is_some_and(|d| {
                d.removable || matches!(d.connection_bus.as_str(), "usb" | "sdio" | "ieee1394")
            });
            let kind = match drive {
                Some(d) if d.optical => DeviceKind::Optical,
                Some(d) if d.media.starts_with("flash") || d.connection_bus == "sdio" => {
                    DeviceKind::Flash
                }
                Some(d) if d.media == "thumb" => DeviceKind::Thumb,
                Some(d) if matches!(d.connection_bus.as_str(), "usb" | "ieee1394") => {
                    DeviceKind::UsbDisk
                }
                _ if removable => DeviceKind::Removable,
                _ => DeviceKind::HardDisk,
            };
            let drive_action = drive.and_then(|d| {
                if d.ejectable && d.optical {
                    Some(DriveAction::Eject)
                } else if d.can_power_off {
                    Some(DriveAction::PowerOff)
                } else if d.ejectable {
                    Some(DriveAction::Eject)
                } else {
                    None
                }
            });
            Device {
                path: b.path.clone(),
                device: b.device.clone(),
                label: b.id_label.clone(),
                size: b.size,
                kind,
                mount_point: b.mount_points.as_ref().and_then(|m| main_mount(m)),
                locked: b.cleartext.as_deref() == Some(""),
                removable,
                drive: drive_path,
                drive_action,
                crypto_backing: backing.map(|c| c.path.clone()),
                usage: None,
            }
        })
        .collect();
    devices.sort_by(|a, b| {
        a.removable
            .cmp(&b.removable)
            .then_with(|| a.device.cmp(&b.device))
    });
    devices
}

fn shown(b: &Block, home: &Path) -> bool {
    let unlocked_container = b.cleartext.as_deref().is_some_and(|c| !c.is_empty());
    let has_content = b.mount_points.is_some() || (b.cleartext.is_some() && !unlocked_container);
    if b.hint_ignore || !has_content || b.id_type == "swap" {
        return false;
    }
    if b.fstab
        .iter()
        .any(|(_, opts)| has_option(opts, "x-gvfs-hide"))
    {
        return false;
    }
    if b.fstab
        .iter()
        .any(|(_, opts)| has_option(opts, "x-gvfs-show"))
    {
        return true;
    }
    if !b.hint_system {
        return true;
    }
    let mounted = b.mount_points.iter().flatten();
    if mounted.clone().any(|p| p == Path::new("/")) {
        return true;
    }
    let in_fstab = b.fstab.iter().map(|(dir, _)| dir);
    mounted.chain(in_fstab).any(|p| user_visible(p, home))
}

/// Where a device mounted in several places opens: `/` for the root
/// filesystem (btrfs subvolumes mount it at `/home`, `/var/log`, ...
/// as well), else the first.
fn main_mount(mount_points: &[PathBuf]) -> Option<PathBuf> {
    mount_points
        .iter()
        .find(|p| *p == Path::new("/"))
        .or_else(|| mount_points.first())
        .cloned()
}

fn has_option(opts: &str, name: &str) -> bool {
    opts.split(',').any(|o| o.trim() == name)
}

/// Somewhere a user looks for mounted things.
fn user_visible(path: &Path, home: &Path) -> bool {
    ["/media", "/run/media", "/mnt"]
        .iter()
        .any(|dir| path.starts_with(dir) && path != Path::new(dir))
        || (!home.as_os_str().is_empty() && path.starts_with(home) && path != home)
}

/// The fraction used of each mounted device (as `df` has it: what's
/// left to users counts as free).
fn read_usage(devices: &mut [Device]) {
    for d in devices {
        d.usage = d.mount_point.as_deref().and_then(usage);
    }
}

fn usage(path: &Path) -> Option<f32> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `c` is a NUL-terminated path, `st` is written by the call.
    if unsafe { libc::statvfs(c.as_ptr(), st.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statvfs succeeded, so it filled `st`.
    let st = unsafe { st.assume_init() };
    let used = st.f_blocks.saturating_sub(st.f_bfree) as f64;
    let total = used + st.f_bavail as f64;
    (total > 0.0).then(|| (used / total) as f32)
}

/// `$<var>`, else `<home>/<fallback>`.
fn xdg_dir(var: &str, home: &Path, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(fallback))
}

/// The last component, `/` for the root.
fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The folders of [`USER_DIRS`] `user-dirs.dirs` sets, in that order:
/// `XDG_<NAME>_DIR="$HOME/<path>"` or `"/<path>"`. One set to the home
/// itself is disabled, as the spec has it.
fn parse_user_dirs(text: &str, home: &Path) -> Vec<(Kind, PathBuf)> {
    let mut found: Vec<(Kind, PathBuf)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(&(_, kind)) = USER_DIRS.iter().find(|(k, _)| *k == key.trim()) else {
            continue;
        };
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        let path = if value == "$HOME" || value == "$HOME/" {
            continue;
        } else if let Some(rest) = value.strip_prefix("$HOME/") {
            home.join(rest)
        } else if value.starts_with('/') {
            PathBuf::from(value)
        } else {
            continue;
        };
        if path == home {
            continue;
        }
        found.retain(|(k, _)| *k != kind);
        found.push((kind, path));
    }
    found.sort_by_key(|(kind, _)| USER_DIRS.iter().position(|(_, k)| k == kind));
    // Two names for one folder: the first one.
    let mut seen: Vec<PathBuf> = Vec::new();
    found.retain(|(_, path)| {
        let new = !seen.contains(path);
        seen.push(path.clone());
        new
    });
    found
}

/// GTK's bookmarks: one `<uri>[ <label>]` per line.
fn parse_gtk_bookmarks(text: &str) -> Vec<(String, Option<String>)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| match line.split_once(' ') {
            Some((uri, label)) => (uri.to_owned(), Some(label.trim().to_owned())),
            None => (line.to_owned(), None),
        })
        .map(|(uri, label)| (uri, label.filter(|l| !l.is_empty())))
        .collect()
}

/// KDE's places: the `bookmark`s the user added, without Dolphin's own
/// (`isSystemItem`: home, trash, network, ...), the hidden ones and
/// the devices (`UDI`).
fn parse_xbel(text: &str) -> Vec<(String, Option<String>)> {
    if text.is_empty() {
        return Vec::new();
    }
    // KDE writes `<!DOCTYPE xbel>`.
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = match roxmltree::Document::parse_with_options(text, options) {
        Ok(doc) => doc,
        Err(e) => {
            log::warn!("places: user-places.xbel: {e}");
            return Vec::new();
        }
    };
    doc.root_element()
        .children()
        .filter(|n| n.has_tag_name("bookmark"))
        .filter(|bookmark| {
            !bookmark.descendants().any(|n| {
                let name = n.tag_name().name();
                let flag = |n: roxmltree::Node| n.text().is_some_and(|t| t.trim() == "true");
                name == "UDI" || ((name == "isSystemItem" || name == "IsHidden") && flag(n))
            })
        })
        .filter_map(|bookmark| {
            let uri = bookmark.attribute("href")?.to_owned();
            let title = bookmark
                .children()
                .find(|n| n.has_tag_name("title"))
                .and_then(|n| n.text())
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty());
            Some((uri, title))
        })
        .collect()
}

/// A bookmark's place: a local folder for a `file://` URI, a remote
/// one for any other; its label the bookmark's, else the folder's name
/// or the host.
fn bookmark(uri: &str, label: Option<String>) -> Option<Place> {
    if let Some(path) = file_uri_path(uri) {
        return Some(Place {
            kind: Kind::Folder,
            label: label.unwrap_or_else(|| base_name(&path)),
            target: Target::Path(path),
        });
    }
    let (scheme, rest) = uri.split_once(':')?;
    if scheme.is_empty() || rest.is_empty() {
        return None;
    }
    let label = label.unwrap_or_else(|| {
        let host = rest.trim_start_matches('/').split('/').next().unwrap_or("");
        let host = host.rsplit('@').next().unwrap_or(host);
        if host.is_empty() {
            uri.to_owned()
        } else {
            percent_decode(host).to_string_lossy().into_owned()
        }
    });
    Some(Place {
        kind: Kind::Remote,
        label,
        target: Target::Uri(uri.to_owned()),
    })
}

/// The path of a local `file://` URI, percent-decoded.
fn file_uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    path.starts_with('/')
        .then(|| PathBuf::from(percent_decode(path)))
}

fn percent_decode(s: &str) -> OsString {
    OsString::from_vec(percent_encoding::percent_decode_str(s).collect())
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
    fn user_dirs() {
        let home = Path::new("/home/u");
        let text = r#"
# XDG_DOCUMENTS_DIR="$HOME/Commented"
XDG_VIDEOS_DIR="$HOME/Video"
XDG_DESKTOP_DIR="$HOME/"
XDG_DOCUMENTS_DIR="$HOME"
XDG_DOWNLOAD_DIR="$HOME/Scaricati"
XDG_MUSIC_DIR="/data/music"
XDG_PICTURES_DIR="relative/pictures"
XDG_TEMPLATES_DIR="$HOME/Modelli"
XDG_PICTURES_DIR="$HOME/Scaricati"
"#;
        assert_eq!(
            parse_user_dirs(text, home),
            vec![
                (Kind::Download, PathBuf::from("/home/u/Scaricati")),
                (Kind::Music, PathBuf::from("/data/music")),
                (Kind::Videos, PathBuf::from("/home/u/Video")),
            ],
            "in sidebar order; the home, relative paths, templates and a \
             folder already listed left out"
        );
    }

    #[test]
    fn gtk_bookmarks() {
        let text =
            "file:///home/u Home\n\nfile:///home/u/My%20Projects\nsftp://u@host/srv  Server \n";
        assert_eq!(
            parse_gtk_bookmarks(text),
            vec![
                ("file:///home/u".to_owned(), Some("Home".to_owned())),
                ("file:///home/u/My%20Projects".to_owned(), None),
                ("sftp://u@host/srv".to_owned(), Some("Server".to_owned())),
            ]
        );
    }

    #[test]
    fn xbel_user_bookmarks_only() {
        let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xbel>
<xbel xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
 <info><metadata owner="http://www.kde.org"><kde_places_version>4</kde_places_version></metadata></info>
 <bookmark href="file:///home/u"><title>Home</title>
  <info>
   <metadata owner="http://freedesktop.org"><bookmark:icon name="user-home"/></metadata>
   <metadata owner="http://www.kde.org"><isSystemItem>true</isSystemItem></metadata>
  </info>
 </bookmark>
 <bookmark href="file:///home/u/Work"><title>Work &amp; stuff</title>
  <info><metadata owner="http://www.kde.org"><isSystemItem>false</isSystemItem></metadata></info>
 </bookmark>
 <bookmark href="file:///home/u/Old"><title>Old</title>
  <info><metadata owner="http://www.kde.org"><IsHidden>true</IsHidden></metadata></info>
 </bookmark>
 <bookmark href="file:///run/media/u/STICK"><title>STICK</title>
  <info><metadata owner="http://www.kde.org"><UDI>/org/kde/fstab/x</UDI></metadata></info>
 </bookmark>
 <bookmark href="smb://nas/share"><title></title></bookmark>
</xbel>"#;
        assert_eq!(
            parse_xbel(text),
            vec![
                (
                    "file:///home/u/Work".to_owned(),
                    Some("Work & stuff".to_owned())
                ),
                ("smb://nas/share".to_owned(), None),
            ]
        );
        assert!(parse_xbel("<not xml").is_empty());
        assert!(parse_xbel("").is_empty());
    }

    #[test]
    fn bookmark_places() {
        assert_eq!(
            bookmark("file:///home/u/My%20Projects", None),
            Some(Place {
                kind: Kind::Folder,
                label: "My Projects".to_owned(),
                target: Target::Path(PathBuf::from("/home/u/My Projects")),
            })
        );
        assert_eq!(
            bookmark("file://localhost/srv", Some("Srv".to_owned())).map(|p| p.target),
            Some(Target::Path(PathBuf::from("/srv")))
        );
        assert_eq!(
            bookmark("file:///", None).map(|p| p.label),
            Some("/".to_owned())
        );
        assert_eq!(
            bookmark("sftp://u@host.lan:22/srv", None),
            Some(Place {
                kind: Kind::Remote,
                label: "host.lan:22".to_owned(),
                target: Target::Uri("sftp://u@host.lan:22/srv".to_owned()),
            })
        );
        assert_eq!(
            bookmark("recent:///", None).map(|p| p.label),
            Some("recent:///".to_owned()),
            "no host: the URI itself"
        );
        assert_eq!(bookmark("no-scheme", None), None);
    }

    /// A partition with a filesystem on drive `drive`.
    fn fs(name: &str, label: &str, drive: &str, mounts: &[&str]) -> Block {
        Block {
            path: format!("/b/{name}"),
            device: PathBuf::from(format!("/dev/{name}")),
            size: 1000,
            id_usage: "filesystem".to_owned(),
            id_type: "ext4".to_owned(),
            id_label: label.to_owned(),
            drive: drive.to_owned(),
            mount_points: Some(mounts.iter().map(PathBuf::from).collect()),
            ..Block::default()
        }
    }

    fn system(b: Block) -> Block {
        Block {
            hint_system: true,
            ..b
        }
    }

    fn labels(devices: &[Device]) -> Vec<&str> {
        devices.iter().map(|d| d.label.as_str()).collect()
    }

    #[test]
    fn devices_as_gvfs_chooses() {
        let home = Path::new("/home/u");
        let blocks = vec![
            system(fs(
                "nvme0n1p7",
                "Root",
                "/d/nvme",
                &["/var/log", "/", "/home"],
            )),
            system(fs("nvme0n1p5", "Boot", "/d/nvme", &["/boot"])),
            system(fs("nvme0n1p3", "Windows", "/d/nvme", &[])),
            system(fs("sda1", "Data", "/d/sda", &["/media/Data"])),
            system(Block {
                fstab: vec![(PathBuf::from("/mnt/Backup"), "noauto,users".to_owned())],
                ..fs("sda2", "Backup", "/d/sda", &[])
            }),
            system(Block {
                fstab: vec![(PathBuf::from("/srv"), "defaults,x-gvfs-show".to_owned())],
                ..fs("sda3", "Shown", "/d/sda", &[])
            }),
            Block {
                fstab: vec![(PathBuf::from("/x"), "x-gvfs-hide".to_owned())],
                ..fs("sdb1", "Hidden", "/d/usb", &[])
            },
            Block {
                hint_ignore: true,
                ..fs("sdb2", "Ignored", "/d/usb", &[])
            },
            Block {
                id_type: "swap".to_owned(),
                id_usage: "other".to_owned(),
                mount_points: None,
                ..fs("sdb3", "Swap", "/d/usb", &[])
            },
            fs("sdb4", "Stick", "/d/usb", &[]),
            // A whole disk with a partition table: nothing to mount.
            Block {
                mount_points: None,
                ..fs("sdb", "", "/d/usb", &[])
            },
        ];
        let drives = HashMap::from([(
            "/d/usb".to_owned(),
            Drive {
                removable: true,
                can_power_off: true,
                connection_bus: "usb".to_owned(),
                media: "thumb".to_owned(),
                ..Drive::default()
            },
        )]);
        let devices = devices(&blocks, &drives, home);
        assert_eq!(
            labels(&devices),
            vec!["Root", "Data", "Backup", "Shown", "Stick"],
            "the root, mounted or set in fstab where a user looks, x-gvfs-show, \
             anything not a system disk; internal ones first"
        );
        assert_eq!(
            devices[0].mount_point,
            Some(PathBuf::from("/")),
            "the root opens at /"
        );
        assert_eq!(devices[0].kind, DeviceKind::HardDisk);
        let stick = &devices[4];
        assert!(stick.removable);
        assert_eq!(stick.kind, DeviceKind::Thumb);
        assert_eq!(stick.drive_action, Some(DriveAction::PowerOff));
        assert_eq!(stick.mount_point, None);
    }

    #[test]
    fn luks_volumes() {
        let home = Path::new("/home/u");
        let locked = Block {
            id_usage: "crypto".to_owned(),
            id_type: "crypto_LUKS".to_owned(),
            mount_points: None,
            cleartext: Some(String::new()),
            ..fs("sdc1", "", "/d/ext", &[])
        };
        let container = Block {
            cleartext: Some("/b/dm-0".to_owned()),
            ..locked.clone()
        };
        // The cleartext volume is on no drive of its own.
        let cleartext = Block {
            crypto_backing: container.path.clone(),
            ..fs("dm-0", "Vault", "", &["/run/media/u/Vault"])
        };
        let drives = HashMap::from([(
            "/d/ext".to_owned(),
            Drive {
                removable: true,
                can_power_off: true,
                connection_bus: "usb".to_owned(),
                ..Drive::default()
            },
        )]);
        let devices1 = devices(&[locked], &drives, home);
        assert_eq!(devices1.len(), 1);
        assert!(devices1[0].locked, "locked: listed, can't be opened");
        assert_eq!(devices1[0].kind, DeviceKind::UsbDisk);

        let devices2 = devices(&[container.clone(), cleartext], &drives, home);
        assert_eq!(
            labels(&devices2),
            vec!["Vault"],
            "the cleartext volume, not its container"
        );
        let vault = &devices2[0];
        assert!(!vault.locked);
        assert_eq!(vault.drive, "/d/ext", "its container's drive");
        assert!(vault.removable);
        assert_eq!(
            vault.crypto_backing.as_deref(),
            Some(container.path.as_str())
        );
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
