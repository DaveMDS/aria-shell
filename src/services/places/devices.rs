//! Which of UDisks2's filesystems the popup lists, as GVfs chooses
//! them ([`devices`]), and how full the mounted ones are.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{Block, Device, DeviceKind, Drive, DriveAction};

/// The devices worth listing, as GVfs (Nautilus, Nemo) chooses them:
/// a filesystem or a LUKS volume UDisks2 doesn't say to ignore, not
/// swap; a system one (an internal disk) only while it's mounted, or
/// set in fstab, somewhere a user looks (`/media`, `/run/media`, `/mnt`,
/// the home), or with `x-gvfs-show`, and the root filesystem (as
/// Dolphin lists it: `/` isn't among the places); `x-gvfs-hide` hides
/// any. An unlocked LUKS container is listed as its cleartext volume.
pub(super) fn devices(
    blocks: &[Block],
    drives: &HashMap<String, Drive>,
    home: &Path,
) -> Vec<Device> {
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
pub(super) fn user_visible(path: &Path, home: &Path) -> bool {
    ["/media", "/run/media", "/mnt"]
        .iter()
        .any(|dir| path.starts_with(dir) && path != Path::new(dir))
        || (!home.as_os_str().is_empty() && path.starts_with(home) && path != home)
}

/// The fraction used of each mounted device (as `df` has it: what's
/// left to users counts as free).
pub(super) fn read_usage(devices: &mut [Device]) {
    for d in devices {
        d.usage = d.mount_point.as_deref().and_then(usage);
    }
}

pub(super) fn usage(path: &Path) -> Option<f32> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
