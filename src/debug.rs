//! `aria-shell debug ...`: the daemon describing itself to a test
//! driver or a curious user, one line per reply. Where our surfaces are
//! (estimated from what we asked the compositor for), the widgets the
//! theme tagged and the pointer; the services describe themselves.

use iced::advanced::widget::operation::{Operation, Outcome};
use iced::window::Id;
use iced::{Point, Rectangle, Size, Task, widget};
use iced_wayland_subscriber::OutputId;

use crate::commands::{DebugCommand, Reply};
use crate::components::locker::Locker;
use crate::components::panel::{self, Panel};
use crate::ui::theme;
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
            DebugCommand::Widgets(filter) => widget_rects()
                .map(move |rects| Message::Widgets(reply.clone(), filter.clone(), rects)),
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
        for (&id, open) in &self.popups {
            if let Some(&(_, _, output, bar)) = list.iter().find(|(p, ..)| *p == open.panel)
                && let Some(position) = self.panels.get(&open.panel).map(Panel::position)
            {
                let rect = open.estimate(position, self.popup_room());
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
        for toast in &self.toasts {
            if let Some(rect) = self.toast_rect(toast) {
                list.push((toast.window, "notification", toast.output, rect));
            }
        }
        for (output, id) in self.osd.windows() {
            if let Some(out) = self.output_rect(output) {
                let bars = (
                    self.bar_height(output, panel::Position::Top),
                    self.bar_height(output, panel::Position::Bottom),
                );
                list.push((id, "osd", output, self.osd.rect(&self.theme, out, bars)));
            }
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
    /// `;`-separated (the protocol is one line per reply).
    fn describe_surfaces(&self) -> String {
        self.surfaces()
            .into_iter()
            .map(|(_, kind, output, r)| {
                format!(
                    "{kind} {} {},{} {}x{}",
                    self.output_name(output),
                    r.x as i32,
                    r.y as i32,
                    r.width as i32,
                    r.height as i32
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// `debug widgets [selector]`: `<element path> <x>,<y> <w>x<h>` per
    /// themed widget matching the selector (theme syntax, plus
    /// `:nth-child(n)`; all of them without one), global coordinates
    /// (as estimated by [`AriaShell::surfaces`]), `;`-separated. The
    /// surface is found from the path's root: `panel[output=..]`,
    /// `launcher`, `popup[output=..]`.
    pub(crate) fn describe_widgets(
        &self,
        filter: &Option<String>,
        rects: Vec<(String, Rectangle)>,
    ) -> String {
        let selector = match filter.as_deref().map(theme::Selector::parse) {
            None => None,
            Some(Ok(s)) => Some(s),
            Some(Err(e)) => return format!("bad selector: {e}"),
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
                .split_once('#')
                .and_then(|(_, rest)| rest.split(['.', '[', ':']).next()?.parse::<u32>().ok())
                .and_then(|id| self.toasts.iter().find(|t| t.id == id))
                .map(|t| t.window);
            surfaces
                .iter()
                .find(|(w, k, out, _)| {
                    *k == kind
                        && output.is_none_or(|o| self.output_name(*out) == o)
                        && window.is_none_or(|id| *w == id)
                })
                .map(|(_, _, _, r)| r.position())
        };
        rects
            .into_iter()
            .filter(|(path, _)| {
                selector
                    .as_ref()
                    .is_none_or(|s| theme::node_from_path(path).is_ok_and(|node| s.matches(&node)))
            })
            .filter_map(|(path, r)| {
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

/// Element path and bounds of every widget the theme helpers tagged, in
/// every window (the runtime runs the operation on all of them; the
/// path's root tells the surface apart).
fn widget_rects() -> Task<Vec<(String, Rectangle)>> {
    struct Collect(Vec<(String, Rectangle)>);

    impl Operation<Vec<(String, Rectangle)>> for Collect {
        fn traverse(
            &mut self,
            operate: &mut dyn FnMut(&mut dyn Operation<Vec<(String, Rectangle)>>),
        ) {
            operate(self);
        }

        fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
            if let Some(path) = id.and_then(theme::widget_path) {
                self.0.push((path, bounds));
            }
        }

        fn finish(&self) -> Outcome<Vec<(String, Rectangle)>> {
            Outcome::Some(self.0.clone())
        }
    }

    iced::advanced::widget::operate(Collect(Vec::new()))
}
