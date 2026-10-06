//! Network shares: the fstab entries of network filesystems (NFS, SMB,
//! sshfs, WebDAV, ...) and the ones mounted by hand, as GVfs lists them
//! for Nautilus and Nemo; mounted and unmounted with `mount <dir>` /
//! `umount <dir>` (fstab's `user`/`users` lets a user do it, the setuid
//! `mount` does the rest), never through UDisks2, which only knows block
//! devices. Read from `/etc/fstab` and `/proc/self/mountinfo`: local
//! files, nothing here waits on the network (no `statvfs`: a slow or
//! dead server would hang it).

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use super::Share;

/// The system's fstab, unless `ARIA_SHELL_FSTAB` names another (the
/// UI scenarios', tests/ui: they can't mount a share for real).
pub fn fstab_path() -> PathBuf {
    std::env::var_os("ARIA_SHELL_FSTAB").map_or_else(|| PathBuf::from("/etc/fstab"), PathBuf::from)
}

/// The mounts, unless `ARIA_SHELL_MOUNTINFO` names another file (the
/// UI scenarios', which their fake `mount` writes).
pub fn mountinfo_path() -> PathBuf {
    std::env::var_os("ARIA_SHELL_MOUNTINFO")
        .map_or_else(|| PathBuf::from("/proc/self/mountinfo"), PathBuf::from)
}

/// Filesystem types that are network shares.
const NETWORK_TYPES: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "sshfs",
    "fuse.sshfs",
    "davfs",
    "fuse.davfs2",
    "9p",
    "ceph",
    "glusterfs",
    "fuse.glusterfs",
    "afs",
    "fuse.rclone",
];

/// One line of fstab, or of mountinfo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub source: String,
    pub dir: PathBuf,
    pub fs_type: String,
    pub options: String,
}

impl Entry {
    fn is_network(&self) -> bool {
        NETWORK_TYPES.contains(&self.fs_type.as_str())
            // The old sshfs form: `sshfs#user@host:/dir /mnt fuse ...`.
            || (self.fs_type == "fuse" && self.source.starts_with("sshfs#"))
    }

    /// `name=value` among the options.
    fn option(&self, name: &str) -> Option<&str> {
        self.options
            .split(',')
            .find_map(|o| o.strip_prefix(name).and_then(|rest| rest.strip_prefix('=')))
    }

    fn has_option(&self, name: &str) -> bool {
        self.options.split(',').any(|o| o == name)
    }
}

/// fstab's entries: `source dir type options [dump pass]`, with `\040`
/// for a space (as the kernel and `getmntent` escape them).
pub fn parse_fstab(text: &str) -> Vec<Entry> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let source = unescape(fields.next()?);
            let dir = unescape(fields.next()?);
            let fs_type = fields.next()?.to_owned();
            let options = fields.next().unwrap_or("defaults").to_owned();
            Some(Entry {
                source: source.to_string_lossy().into_owned(),
                dir: PathBuf::from(dir),
                fs_type,
                options,
            })
        })
        .collect()
}

/// mountinfo's mounts: `id parent major:minor root dir options
/// [optional...] - type source super-options`.
pub fn parse_mountinfo(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let left: Vec<&str> = left.split(' ').collect();
            let mut right = right.split(' ');
            let fs_type = right.next()?.to_owned();
            let source = unescape(right.next().unwrap_or(""));
            Some(Entry {
                source: source.to_string_lossy().into_owned(),
                dir: PathBuf::from(unescape(left.get(4)?)),
                fs_type,
                options: left.get(5).copied().unwrap_or("").to_owned(),
            })
        })
        .collect()
}

/// `\ooo` octal escapes undone.
fn unescape(field: &str) -> OsString {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && bytes[i + 1..i + 4]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            let n = bytes[i + 1..i + 4]
                .iter()
                .fold(0u32, |n, b| n * 8 + u32::from(b - b'0'));
            out.push(n as u8);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    OsString::from_vec(out)
}

/// The shares worth listing, as GVfs chooses them: a network entry of
/// fstab with `x-gvfs-show` or somewhere a user looks (`/media`,
/// `/run/media`, `/mnt`, the home), not `x-gvfs-hide`; then a network
/// filesystem mounted by hand in such a place. Named by `x-gvfs-name`,
/// else the folder; `x-gvfs-icon` / `x-gvfs-symbolic-icon` kept.
pub fn shares(fstab: &[Entry], mounts: &[Entry], home: &Path) -> Vec<Share> {
    let visible = |dir: &Path| super::user_visible(dir, home);
    let mounted = |dir: &Path| mounts.iter().any(|m| m.dir == dir);
    let mut shares: Vec<Share> = fstab
        .iter()
        .filter(|e| e.is_network() && !e.has_option("x-gvfs-hide"))
        .filter(|e| e.has_option("x-gvfs-show") || visible(&e.dir))
        .map(|e| share(e, mounted(&e.dir), true))
        .collect();
    for m in mounts {
        if m.is_network() && visible(&m.dir) && !shares.iter().any(|s| s.mount_point == m.dir) {
            shares.push(share(m, true, false));
        }
    }
    shares
}

fn share(e: &Entry, mounted: bool, in_fstab: bool) -> Share {
    let label = e
        .option("x-gvfs-name")
        .map(|n| super::percent_decode(n).to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| super::base_name(&e.dir));
    Share {
        mount_point: e.dir.clone(),
        label,
        source: e.source.clone(),
        fs_type: e.fs_type.clone(),
        icon: e.option("x-gvfs-icon").map(str::to_owned),
        symbolic_icon: e.option("x-gvfs-symbolic-icon").map(str::to_owned),
        mounted,
        in_fstab,
    }
}

/// `mount <dir>`, as fstab has it.
pub async fn mount(dir: PathBuf) -> Result<(), String> {
    run("mount", &[dir.into_os_string()]).await
}

/// `umount <dir>`; a FUSE share mounted by hand (sshfs, not in fstab):
/// `fusermount3 -u` (`fusermount -u` where there's no 3), as only the
/// user who mounted it may.
pub async fn unmount(share: Share) -> Result<(), String> {
    let dir = share.mount_point.into_os_string();
    if share.fs_type.starts_with("fuse") && !share.in_fstab {
        let fusermount =
            crate::process::first_on_path(&["fusermount3", "fusermount"]).unwrap_or("fusermount");
        run(fusermount, &["-u".into(), dir]).await
    } else {
        run("umount", &[dir]).await
    }
}

/// Run `program args`, waiting; what it said on stderr when it fails.
async fn run(program: &str, args: &[OsString]) -> Result<(), String> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("{program}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if stderr.is_empty() {
        format!("{program}: {}", output.status)
    } else {
        stderr
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fstab_entries() {
        let text = "\
# /etc/fstab
UUID=ad32 / btrfs subvol=/@,defaults 0 0

nas:/volume1/My\\040Photos /media/NAS/My\\040Photos nfs users,noauto,x-gvfs-show 0 0
//nas/docs /mnt/docs cifs
";
        assert_eq!(
            parse_fstab(text),
            vec![
                Entry {
                    source: "UUID=ad32".to_owned(),
                    dir: PathBuf::from("/"),
                    fs_type: "btrfs".to_owned(),
                    options: "subvol=/@,defaults".to_owned(),
                },
                Entry {
                    source: "nas:/volume1/My Photos".to_owned(),
                    dir: PathBuf::from("/media/NAS/My Photos"),
                    fs_type: "nfs".to_owned(),
                    options: "users,noauto,x-gvfs-show".to_owned(),
                },
                Entry {
                    source: "//nas/docs".to_owned(),
                    dir: PathBuf::from("/mnt/docs"),
                    fs_type: "cifs".to_owned(),
                    options: "defaults".to_owned(),
                },
            ],
            "comments and blank lines skipped, \\040 a space, options defaulted"
        );
    }

    #[test]
    fn mountinfo_mounts() {
        let text = "\
22 1 0:21 / / rw,relatime shared:1 - btrfs /dev/nvme0n1p7 rw,ssd
461 22 0:60 / /media/GAARA/Film rw,relatime shared:254 - nfs4 192.168.1.4:/volume1/Film rw,vers=4.1
90 22 0:90 / /home/u/remote\\040box rw,nosuid master:3 shared:9 - fuse.sshfs dave@box:/ rw
broken line
";
        let mounts = parse_mountinfo(text);
        assert_eq!(mounts.len(), 3, "a line without ` - ` skipped");
        assert_eq!(mounts[1].dir, PathBuf::from("/media/GAARA/Film"));
        assert_eq!(mounts[1].fs_type, "nfs4");
        assert_eq!(mounts[1].source, "192.168.1.4:/volume1/Film");
        assert_eq!(
            mounts[2].dir,
            PathBuf::from("/home/u/remote box"),
            "optional fields before ` - `, \\040 a space"
        );
        assert_eq!(mounts[2].fs_type, "fuse.sshfs");
    }

    #[test]
    fn shares_as_gvfs_lists_them() {
        let home = Path::new("/home/u");
        let fstab = parse_fstab(
            "\
/dev/sdb1 /media/data ext4 defaults,users 0 2
nas:/Backup /media/NAS/Backup nfs users,noauto 0 0
nas:/Srv /srv/nas nfs users,noauto,x-gvfs-show,x-gvfs-name=NAS%20Srv,x-gvfs-symbolic-icon=network-server-symbolic 0 0
nas:/Hidden /mnt/hidden nfs users,x-gvfs-hide 0 0
nas:/Elsewhere /srv/elsewhere nfs users 0 0
sshfs#u@box:/ /home/u/box fuse users,noauto 0 0
",
        );
        let mounts = parse_mountinfo(
            "\
461 22 0:60 / /media/NAS/Backup rw - nfs4 nas:/Backup rw
90 22 0:90 / /home/u/remote rw - fuse.sshfs u@other:/ rw
91 22 0:91 / /var/lib/x rw - nfs4 nas:/x rw
92 22 0:92 / /run/media/u/smb rw - cifs //nas/share rw
",
        );
        let shares = shares(&fstab, &mounts, home);
        let names: Vec<(&str, bool, bool)> = shares
            .iter()
            .map(|s| (s.label.as_str(), s.mounted, s.in_fstab))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Backup", true, true),
                ("NAS Srv", false, true),
                ("box", false, true),
                ("remote", true, false),
                ("smb", true, false),
            ],
            "fstab's network entries under /media, the home, or x-gvfs-show (not \\
             x-gvfs-hide, not a local filesystem, not where nobody looks), then \\
             those mounted by hand where a user looks"
        );
        assert_eq!(
            shares[1].symbolic_icon.as_deref(),
            Some("network-server-symbolic")
        );
        assert_eq!(shares[1].icon, None);
        assert_eq!(
            shares.iter().map(Share::family).collect::<Vec<_>>(),
            vec!["nfs", "nfs", "ssh", "ssh", "smb"]
        );
    }
}
