//! What the daemon draws on each window, and what it listens to.

use std::path::Path;

use iced::window::Id;
use iced::{Element, Event, Length, Subscription, widget};

use crate::components::{dialog, exiter, launcher, panel, picker};
use crate::ui::theme::Node;
use crate::{AriaShell, Message, commands, ui, watch};
use panel::Panel;
use picker::Picker;

impl AriaShell {
    pub(crate) fn view(&self, window: Id) -> Element<'_, Message> {
        let shared = self.shared();
        if let Some(locker) = &self.locker
            && locker.has_window(window)
        {
            let output = locker.output_of(window).map_or("", |o| self.output_name(o));
            return locker.view(shared, output).map(Message::Locker);
        }
        if let Some(view) = self
            .picker
            .as_ref()
            .and_then(|p| p.view(window, &self.theme, &self.locale))
        {
            return view.map(Message::Picker);
        }
        if let Some((dialog, launcher)) = &self.launcher
            && dialog.is_window(window)
        {
            let node = Node::root("launcher");
            let root = self
                .theme
                .container(&node, launcher.view(shared))
                .width(Length::Fill)
                .height(Length::Fill);
            // The shadow's room outside what takes clicks: a press there
            // is outside the launcher.
            let content = dialog::content(root, launcher::Message::Nothing);
            return Element::from(self.theme.surface(&node, content)).map(Message::Launcher);
        }
        if let Some((dialog, exiter)) = &self.exiter
            && dialog.is_window(window)
        {
            let node = Node::root("exiter");
            let root = self
                .theme
                .container(&node, exiter.view(shared))
                .width(Length::Fill)
                .height(Length::Fill);
            let content = dialog::content(root, exiter::Message::Nothing);
            return Element::from(self.theme.surface(&node, content)).map(Message::Exiter);
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
        if let Some(output) = self.wallpapers.output_of(window) {
            return self
                .wallpapers
                .view(window, &self.theme, self.output_name(output));
        }
        if let Some(output) = self.toasts.output_of(window) {
            return self
                .toasts
                .view(
                    window,
                    &self.theme,
                    &self.notifications,
                    &self.icons,
                    self.output_name(output),
                )
                .map(Message::Toast);
        }
        if let Some(output) = self.osd.output_of(window) {
            return self
                .osd
                .view(&self.theme, &self.icons, self.output_name(output));
        }
        if let Some(panel) = self.panels.get(&window) {
            return panel.view(shared).map(move |m| Message::Panel(window, m));
        }
        if let Some((&owner, panel)) = self.panels.iter().find(|(_, p)| p.has_popup(window)) {
            return panel
                .popup_view(window, shared)
                .map(move |m| Message::Panel(owner, m));
        }
        widget::Space::new().into()
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
            .chain(
                self.picker
                    .as_ref()
                    .map(|_| Picker::subscription().map(Message::Picker)),
            ),
        )
    }
}
