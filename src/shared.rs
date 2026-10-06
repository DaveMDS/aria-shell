//! What every view reads: the daemon's state, read-only, borrowed for
//! one `view`. Gadgets get it through their `Context`, the components
//! (the launcher, the exit menu, the lock screen) as it is.

use crate::locale::Locale;
use crate::services::audio::Audio;
use crate::services::brightness::Brightness;
use crate::services::compositor::Compositor;
use crate::services::icons::Icons;
use crate::services::idle::Idle;
use crate::services::network::Network;
use crate::services::notifications::Notifications;
use crate::services::places::Places;
use crate::services::power::Power;
use crate::services::scripts::Scripts;
use crate::services::sysmon::SysMon;
use crate::services::tray::Tray;
use crate::ui::theme::Theme;

/// The daemon's state, read-only: what every view reads.
#[derive(Clone, Copy)]
pub struct Shared<'a> {
    pub compositor: &'a Compositor,
    pub theme: &'a Theme,
    pub locale: &'a Locale,
    pub icons: &'a Icons,
    pub tray: &'a Tray,
    pub notifications: &'a Notifications,
    pub sysmon: &'a SysMon,
    pub audio: &'a Audio,
    pub network: &'a Network,
    pub idle: &'a Idle,
    pub power: &'a Power,
    pub brightness: &'a Brightness,
    pub places: &'a Places,
    pub scripts: &'a Scripts,
}
