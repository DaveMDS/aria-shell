//! What the daemon draws on each window, and what it listens to.

use std::path::Path;

use iced::window::Id;
use iced::{Event, Subscription};
use iced_wayland_subscriber::OutputId;
use icssed::{Element, Node};

use crate::components::{dialog, exiter, launcher, panel, picker};
use crate::daemon::showcase;
use crate::{AriaShell, Message, commands, ui, watch};
use panel::Panel;
use picker::Picker;

impl AriaShell {
    pub(crate) fn view(&self, window: Id) -> Element<'_, Message> {
        let shared = self.shared();
        let output = self.window_output(window);
        let scale = output.map_or(1.0, |o| self.scales.of(o));
        let name = output.map_or("", |o| self.output_name(o));
        if let Some(locker) = &self.locker
            && locker.has_window(window)
        {
            let root = self.theme.root("locker", scale).attr("output", name);
            return locker.view(shared, root).map(Message::Locker);
        }
        if let Some(view) = self
            .picker
            .as_ref()
            .and_then(|p| p.view(window, &self.theme, &self.locale, scale))
        {
            return view.map(Message::Picker);
        }
        if let Some(showcase) = &self.showcase
            && showcase.window == window
        {
            return self.showcase_view(showcase, scale);
        }
        if let Some((dialog, launcher)) = &self.launcher
            && dialog.is_window(window)
        {
            let root = self.theme.root("launcher", scale);
            return dialog_surface(
                root,
                launcher.view(shared, root),
                launcher::Message::Nothing,
            )
            .map(Message::Launcher);
        }
        if let Some((dialog, exiter)) = &self.exiter
            && dialog.is_window(window)
        {
            let root = self.theme.root("exiter", scale);
            return dialog_surface(root, exiter.view(shared, root), exiter::Message::Nothing)
                .map(Message::Exiter);
        }
        if self
            .launcher
            .iter()
            .map(|(d, _)| d)
            .chain(self.exiter.iter().map(|(d, _)| d))
            .any(|d| d.is_grab(window))
        {
            return dialog::grab_view();
        }
        if self.wallpapers.output_of(window).is_some() {
            return self.wallpapers.view(window, &self.theme, name, scale);
        }
        if self.toasts.output_of(window).is_some() {
            return self
                .toasts
                .view(
                    window,
                    &self.theme,
                    &self.notifications,
                    &self.icons,
                    name,
                    scale,
                )
                .map(Message::Toast);
        }
        if self.osd.output_of(window).is_some() {
            return self.osd.view(&self.theme, &self.icons, name, scale);
        }
        if let Some(panel) = self.panels.get(&window) {
            return panel.view(shared).map(move |m| Message::Panel(window, m));
        }
        if let Some((&owner, view)) = self
            .panels
            .iter()
            .find_map(|(id, p)| Some((id, p.popup_view(window, shared)?)))
        {
            return view.map(move |m| Message::Panel(owner, m));
        }
        dialog::grab_view()
    }

    /// The output surface `window` is on, as far as we know.
    pub(crate) fn window_output(&self, window: Id) -> Option<OutputId> {
        if let Some(panel) = self
            .panels
            .get(&window)
            .or_else(|| self.panels.values().find(|p| p.has_popup(window)))
        {
            return Some(panel.output);
        }
        let dialogs = self
            .launcher
            .iter()
            .map(|(d, _)| d)
            .chain(self.exiter.iter().map(|(d, _)| d));
        for dialog in dialogs {
            if dialog.is_window(window) {
                return Some(dialog.output);
            }
            if let Some(&(_, output)) = dialog.grabs.iter().find(|(id, _)| *id == window) {
                return Some(output);
            }
        }
        self.toasts
            .output_of(window)
            .or_else(|| self.osd.output_of(window))
            .or_else(|| self.wallpapers.output_of(window))
            .or_else(|| self.locker.as_ref()?.output_of(window))
            .or_else(|| {
                let picker = self.picker.as_ref()?;
                picker
                    .windows()
                    .find(|(id, _)| *id == window)
                    .map(|(_, o)| o)
            })
            .or_else(|| {
                let showcase = self.showcase.as_ref()?;
                (showcase.window == window).then_some(showcase.output)
            })
    }

    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let panels = self.panels.iter().map(|(id, panel)| {
            panel
                .subscription()
                .with(*id)
                .map(|(id, m)| Message::Panel(id, m))
        });
        let mut files = Vec::new();
        if self.general.reload_config {
            files.extend(self.config.path().map(Path::to_path_buf));
        }
        if self.general.reload_style {
            files.extend(self.theme.files().iter().cloned());
        }
        files.extend(self.icons.watch_dirs().iter().cloned());
        files.extend(self.wallpapers.watched().cloned());
        let popups = self
            .panels
            .values()
            .any(Panel::has_popups)
            .then(|| ui::popup::presses_outside().map(Message::PressedOutside));
        let launcher = self.launcher.iter().flat_map(|(_, launcher)| {
            [
                launcher
                    .subscription()
                    .map(|(w, m)| Message::LauncherEvent(w, m)),
                dialog::pointer_events().map(|(w, e)| Message::DialogPointer(w, e)),
            ]
        });
        let exiter = self.exiter.iter().flat_map(|(_, exiter)| {
            [
                exiter
                    .subscription()
                    .map(|(w, m)| Message::ExiterEvent(w, m)),
                dialog::pointer_events().map(|(w, e)| Message::DialogPointer(w, e)),
            ]
        });
        let locker = self
            .locker
            .iter()
            .map(|l| l.subscription().map(Message::Locker));
        let showcase = self
            .showcase
            .iter()
            .map(|_| showcase::subscription().map(Message::Showcase));
        Subscription::batch(
            [
                self.shell_events.listen().map(Message::Shell),
                self.compositor.subscription().map(Message::Compositor),
                self.tray.subscription().map(Message::Tray),
                self.notifications
                    .subscription()
                    .map(Message::Notifications),
                self.sysmon.subscription().map(Message::SysMon),
                self.audio.subscription().map(Message::Audio),
                self.network.subscription().map(Message::Network),
                self.idle.subscription().map(Message::Idle),
                self.power.subscription().map(Message::Power),
                self.brightness.subscription().map(Message::Brightness),
                self.screenshot.subscription().map(Message::Screenshot),
                self.places.subscription().map(Message::Places),
                self.wallpapers.subscription().map(Message::Wallpaper),
                self.scripts
                    .subscription(self.panels.values().flat_map(Panel::scripts))
                    .map(Message::Scripts),
                commands::listen().map(Message::Command),
                watch::watch(&files).map(Message::Files),
                iced::event::listen_with(|event, _, window| match event {
                    Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                        Some(Message::Cursor(window, position))
                    }
                    Event::Mouse(
                        iced::mouse::Event::CursorEntered | iced::mouse::Event::CursorLeft,
                    ) => Some(Message::PointerCrossed(window)),
                    Event::Keyboard(k) => Some(Message::PanelKey(window, k)),
                    _ => None,
                }),
            ]
            .into_iter()
            .chain(panels)
            .chain(popups)
            .chain(launcher)
            .chain(exiter)
            .chain(locker)
            .chain(showcase)
            .chain(
                self.picker
                    .as_ref()
                    .map(|_| Picker::subscription().map(Message::Picker)),
            ),
        )
    }
}

/// A dialog's surface content: its box `content`, of root `root`, with
/// its shadow's room around. The box takes every press (`nothing`): one
/// in the shadow's room is outside the dialog.
pub(crate) fn dialog_surface<'a, M: Clone + 'a>(
    root: Node<'a>,
    content: Element<'a, M>,
    nothing: M,
) -> Element<'a, M> {
    icssed::surface(root, dialog::content(content, nothing)).into()
}
