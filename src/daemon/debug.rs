//! `aria-shell debug ...`: the daemon describing itself to a test
//! driver or a curious user, one line per reply. Where our surfaces are
//! (estimated from what we asked the compositor for), the widgets (the
//! toolkit's query) and the pointer; the services describe themselves.

use iced::window::Id;
use iced::{Point, Rectangle, Size, Task};
use iced_wayland_subscriber::OutputId;
use icssed::query::{Found, Report};

use crate::commands::{DebugCommand, Reply};
use crate::components::locker::Locker;
use crate::components::panel;
use crate::ui::toast;
use crate::{AriaShell, Message};

impl AriaShell {
    /// Answer a `debug` command: what the daemon and the services see.
    pub(crate) fn debug(&self, cmd: DebugCommand, reply: Reply) -> Task<Message> {
        match cmd {
            DebugCommand::Surfaces => {
                reply.send(self.describe_surfaces());
                Task::none()
            }
            DebugCommand::Cursor => {
                reply.send(self.describe_cursor());
                Task::none()
            }
            DebugCommand::SysMon => {
                reply.send(self.sysmon.describe());
                Task::none()
            }
            DebugCommand::Audio => {
                reply.send(self.audio.describe());
                Task::none()
            }
            DebugCommand::Network => {
                reply.send(self.network.describe());
                Task::none()
            }
            DebugCommand::Idle => {
                reply.send(self.idle.describe());
                Task::none()
            }
            DebugCommand::Power => {
                reply.send(self.power.describe());
                Task::none()
            }
            DebugCommand::Brightness => {
                reply.send(self.brightness.describe());
                Task::none()
            }
            DebugCommand::Screenshot => {
                let picker = match &self.picker {
                    Some(p) => p.describe(),
                    None => "picker=closed".to_owned(),
                };
                reply.send(self.screenshot.describe(&picker));
                Task::none()
            }
            DebugCommand::Places => {
                reply.send(self.places.describe());
                Task::none()
            }
            DebugCommand::Wallpaper => {
                reply.send(self.wallpapers.describe());
                Task::none()
            }
            DebugCommand::Locale => {
                reply.send(self.locale.describe());
                Task::none()
            }
            DebugCommand::Theme => {
                reply.send(format!(
                    "style={} scheme={}",
                    self.theme.name().unwrap_or("-"),
                    self.theme.scheme().name()
                ));
                Task::none()
            }
            DebugCommand::Widgets(filter) => iced::advanced::widget::operate(Report::default())
                .map(move |found| Message::Widgets(reply.clone(), filter.clone(), found)),
        }
    }

    /// Every surface we have open: its kind, output and global
    /// rectangle, computed from what we asked the compositor for.
    pub(crate) fn surfaces(&self) -> Vec<(Id, &'static str, OutputId, Rectangle)> {
        let mut list = Vec::new();
        for (&id, panel) in &self.panels {
            let Some(out) = self.output_rect(panel.output) else {
                continue;
            };
            let h = panel.height() as f32;
            let y = match panel.position() {
                panel::Position::Top => out.y,
                panel::Position::Bottom => out.y + out.height - h,
            };
            list.push((
                id,
                "panel",
                panel.output,
                Rectangle::new(Point::new(out.x, y), Size::new(out.width, h)),
            ));
        }
        let bars: Vec<(Id, OutputId, Rectangle)> = list
            .iter()
            .map(|&(id, _, output, r)| (id, output, r))
            .collect();
        for (bar_id, output, bar) in bars {
            let Some(panel) = self.panels.get(&bar_id) else {
                continue;
            };
            for (id, rect) in panel.popup_rects(&self.theme) {
                list.push((id, "popup", output, rect + iced::Vector::new(bar.x, bar.y)));
            }
        }
        let dialogs = self
            .launcher
            .iter()
            .map(|(d, _)| ("launcher", d))
            .chain(self.exiter.iter().map(|(d, _)| ("exiter", d)));
        for (kind, dialog) in dialogs {
            for &(id, output) in &dialog.grabs {
                if let Some(out) = self.output_rect(output) {
                    list.push((id, "grab", output, out));
                }
            }
            if let Some(out) = self.output_rect(dialog.output) {
                list.push((dialog.window, kind, dialog.output, dialog.rect(out)));
            }
        }
        let corner = self.notifications.config().position;
        for (window, _, output, size, margin) in self.toasts.placed() {
            if let Some(out) = self.output_rect(output) {
                let bars = (
                    self.bar_height(output, panel::Position::Top),
                    self.bar_height(output, panel::Position::Bottom),
                );
                let rect = toast::rect(corner, out, bars, size, margin);
                list.push((window, "notification", output, rect));
            }
        }
        for (output, id) in self.osd.windows() {
            if let Some(out) = self.output_rect(output) {
                let bars = (
                    self.bar_height(output, panel::Position::Top),
                    self.bar_height(output, panel::Position::Bottom),
                );
                if let Some(rect) = self.osd.rect(output, out, bars) {
                    list.push((id, "osd", output, rect));
                }
            }
        }
        if let Some(showcase) = &self.showcase
            && let Some(out) = self.output_rect(showcase.output)
        {
            let top = self.bar_height(showcase.output, panel::Position::Top);
            let bottom = self.bar_height(showcase.output, panel::Position::Bottom);
            let rect = Rectangle::new(
                Point::new(out.x, out.y + top),
                Size::new(out.width, out.height - top - bottom),
            );
            list.push((showcase.window, "showcase", showcase.output, rect));
        }
        for (id, output) in self.picker.iter().flat_map(|p| p.windows()) {
            if let Some(out) = self.output_rect(output) {
                list.push((id, "screenshot", output, out));
            }
        }
        for (id, output) in self.locker.iter().flat_map(Locker::windows) {
            if let Some(out) = self.output_rect(output) {
                list.push((id, "locker", output, out));
            }
        }
        for (id, output) in self.wallpapers.windows() {
            if let Some(out) = self.output_rect(output) {
                list.push((id, "wallpaper", output, out));
            }
        }
        list
    }

    /// `debug surfaces`: `<kind> <output> <x>,<y> <w>x<h>` per surface,
    /// then `keyboard` on the one the keyboard is on, `;`-separated (the
    /// protocol is one line per reply).
    fn describe_surfaces(&self) -> String {
        self.surfaces()
            .into_iter()
            .map(|(window, kind, output, r)| {
                format!(
                    "{kind} {} {},{} {}x{}{}",
                    self.output_name(output),
                    r.x as i32,
                    r.y as i32,
                    r.width as i32,
                    r.height as i32,
                    if self.keyboard == Some(window) {
                        " keyboard"
                    } else {
                        ""
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// `debug widgets [selector]`: `<element path> <x>,<y> <w>x<h>` per
    /// widget the selector matches (the theme's syntax, positions
    /// included; all of them without one), global coordinates (as
    /// estimated by [`AriaShell::surfaces`]), `;`-separated. The surface
    /// is found from the path's root: `panel[output=..]`, `launcher`,
    /// `popup[output=..]`; the showcase's, for a root with no surface of
    /// its kind.
    pub(crate) fn describe_widgets(&self, filter: &Option<String>, found: Vec<Found>) -> String {
        let sheet = self.theme.sheet();
        let picked = match filter.as_deref() {
            None => (0..found.len()).collect(),
            Some(selector) => match sheet.matching(selector, &found) {
                Ok(picked) => picked,
                Err(e) => return format!("bad selector: {e}"),
            },
        };
        let surfaces = self.surfaces();
        let origin = |path: &str| -> Option<Point> {
            let root = path.split(" > ").next()?;
            let output = root
                .split_once("[output=\"")
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(name, _)| name);
            let kind = root.split(['.', '#', '[', ':']).next()?;
            // Several notifications may show on one output: their
            // root carries the notification id.
            let window = root
                .split_once("[id=\"")
                .and_then(|(_, rest)| rest.split('"').next()?.parse::<u32>().ok())
                .and_then(|id| self.toasts.placed().find(|(_, n, ..)| *n == id))
                .map(|(window, ..)| window);
            surfaces
                .iter()
                .find(|(w, k, out, _)| {
                    *k == kind
                        && output.is_none_or(|o| self.output_name(*out) == o)
                        && window.is_none_or(|id| *w == id)
                })
                // The showcase draws every kind of root on its surface:
                // one with no surface of its own is there.
                .or_else(|| surfaces.iter().find(|(_, k, ..)| *k == "showcase"))
                .map(|(_, _, _, r)| r.position())
        };
        picked
            .into_iter()
            .filter_map(|i| {
                let r = found[i].bounds;
                let path = found[i].path(sheet)?;
                let o = origin(&path)?;
                Some(format!(
                    "{path} {},{} {}x{}",
                    (o.x + r.x) as i32,
                    (o.y + r.y) as i32,
                    r.width as i32,
                    r.height as i32
                ))
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// `debug cursor`: where the pointer was last seen over one of our
    /// surfaces, `<kind> <output> local <x>,<y> global <x>,<y>`. The
    /// local position is what the surface got; the global one assumes
    /// the surface is where [`AriaShell::surfaces`] thinks (another
    /// client's exclusive zone can shift a bar without us knowing), so a
    /// driver that placed the pointer itself can compare the two and
    /// learn the surface's real origin.
    fn describe_cursor(&self) -> String {
        let Some((window, p)) = self.cursor else {
            return "unknown".to_owned();
        };
        match self.surfaces().into_iter().find(|(id, ..)| *id == window) {
            Some((_, kind, output, r)) => format!(
                "{kind} {} local {},{} global {},{}",
                self.output_name(output),
                p.x as i32,
                p.y as i32,
                (r.x + p.x) as i32,
                (r.y + p.y) as i32
            ),
            None => "unknown".to_owned(),
        }
    }

    /// The thickest bar at `position` on `output`: the exclusive zone
    /// a surface anchored to that edge is pushed past.
    fn bar_height(&self, output: OutputId, position: panel::Position) -> f32 {
        self.panels
            .values()
            .filter(|p| p.output == output && p.position() == position)
            .map(|p| p.height() as f32)
            .fold(0.0, f32::max)
    }
}
