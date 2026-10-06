//! A laptop's panel: `/sys/class/backlight/<device>/` (`max_brightness`,
//! `actual_brightness`), written through logind's
//! `Session.SetBrightness` (the file is root's, the session's user may
//! ask logind). The kernel notifies `actual_brightness` on every change
//! (`backlight_generate_event`: a write, the firmware's hotkeys), which
//! a thread of ours waits for with `poll(POLLPRI)`.
//!
//! One backlight is used when there are several (an ACPI one and the
//! GPU's for the same panel): firmware, then platform, then raw, as
//! GNOME's settings daemon does. It's tied to its output by its
//! `device` link when that is a DRM connector (`card0-eDP-1`), else to
//! the one internal panel connected (eDP, LVDS, DSI).

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use zbus::{Connection, proxy};

use super::{Display, Kind, Level};

const CLASS: &str = "/sys/class/backlight";
const DRM: &str = "/sys/class/drm";

#[proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1/session/auto"
)]
trait Session {
    fn set_brightness(&self, subsystem: &str, name: &str, brightness: u32) -> zbus::Result<()>;
}

/// The backlight to use, if any.
pub fn find() -> Option<Display> {
    let mut found: Vec<(u8, String)> = fs::read_dir(CLASS)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let kind = fs::read_to_string(e.path().join("type")).ok()?;
            let rank = match kind.trim() {
                "firmware" => 0,
                "platform" => 1,
                _ => 2,
            };
            Some((rank, name))
        })
        .collect();
    found.sort();
    let (_, name) = found.into_iter().next()?;
    let output = connector(&Path::new(CLASS).join(&name)).or_else(internal_panel);
    log::info!(
        "brightness: backlight {name} on {}",
        output.as_deref().unwrap_or("an unknown output")
    );
    Some(Display {
        id: id(&name),
        kind: Kind::Backlight,
        output,
        model: String::new(),
        level: read(&name),
    })
}

pub fn id(name: &str) -> String {
    format!("backlight:{name}")
}

/// The device name of a display id (`backlight:intel_backlight`).
pub fn name(id: &str) -> Option<&str> {
    id.strip_prefix("backlight:")
}

fn path(name: &str, file: &str) -> PathBuf {
    Path::new(CLASS).join(name).join(file)
}

fn read_u32(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// The level as the hardware has it (`actual_brightness`, else what was
/// asked).
pub fn read(name: &str) -> Option<Level> {
    let max = read_u32(&path(name, "max_brightness")).filter(|m| *m > 0)?;
    let value = read_u32(&path(name, "actual_brightness"))
        .or_else(|| read_u32(&path(name, "brightness")))?;
    Some(Level {
        value: value.min(max),
        max,
    })
}

pub async fn set(conn: &Connection, name: &str, value: u32) -> zbus::Result<()> {
    SessionProxy::new(conn)
        .await?
        .set_brightness("backlight", name, value)
        .await
}

/// The connector of a backlight whose `device` is one (`card0-eDP-1`).
fn connector(dir: &Path) -> Option<String> {
    let device = fs::read_link(dir.join("device")).ok()?;
    let name = device.file_name()?.to_string_lossy().into_owned();
    let (card, connector) = name.split_once('-')?;
    card.starts_with("card").then(|| connector.to_owned())
}

/// The one internal panel connected, by its DRM connector.
fn internal_panel() -> Option<String> {
    let panels: Vec<String> = fs::read_dir(DRM)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let (_, connector) = name.split_once('-')?;
            let internal = ["eDP-", "LVDS-", "DSI-"]
                .iter()
                .any(|p| connector.starts_with(p));
            let status = fs::read_to_string(e.path().join("status")).ok()?;
            (internal && status.trim() == "connected").then(|| connector.to_owned())
        })
        .collect();
    match panels.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

/// Wait for the kernel's notifications on `actual_brightness` of
/// backlight `name`, calling `changed` after each, until it returns
/// false. Blocking: run on a thread of its own.
pub fn watch(name: &str, mut changed: impl FnMut() -> bool) {
    let path = path(name, "actual_brightness");
    let mut file = match fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("brightness: {}: {e}", path.display());
            return;
        }
    };
    let mut buf = [0u8; 32];
    loop {
        // A sysfs attribute is armed by reading it to the end.
        if file.seek(SeekFrom::Start(0)).is_err() || file.read(&mut buf).is_err() {
            log::warn!("brightness: {} unreadable, not watched", path.display());
            return;
        }
        let mut fd = libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLPRI | libc::POLLERR,
            revents: 0,
        };
        // SAFETY: one valid pollfd, owned by this frame.
        let n = unsafe { libc::poll(&mut fd, 1, -1) };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            log::warn!("brightness: polling {}: failed", path.display());
            return;
        }
        if !changed() {
            return;
        }
    }
}
