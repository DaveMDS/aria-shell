//! The daemon and the outputs: monitors coming, changing and going,
//! and the surfaces that follow them (the bars, the wallpapers, the
//! lock screen's); the config and the theme loaded again.

use iced::window::Id;
use iced::{Point, Rectangle, Size, Task};
use iced_exwlshell::shell::{ShellEvent, ShellType};
use iced_wayland_subscriber::{OutputId, OutputInfo};

use crate::components::panel;
use crate::config::Config;
use crate::daemon::{surface_tasks, wallpaper_tasks};
use crate::locale::Locale;
use crate::services::{icons, idle};
use crate::ui::theme::Theme;
use crate::{AriaShell, Message};

/// Ask the compositor's scale of surface `window`.
fn ask_scale(window: Id) -> Task<Message> {
    iced::window::scale_factor(window).map(move |scale| Message::Scale(window, scale))
}
use icons::Icons;
use panel::{Panel, PanelConfig};

impl AriaShell {
    pub(crate) fn on_shell_event(&mut self, event: ShellEvent) -> Task<Message> {
        // Every surface tells the scale it's drawn at once it exists.
        let scale = match &event {
            ShellEvent::NewShell(info) => ask_scale(info.window),
            _ => Task::none(),
        };
        Task::batch([scale, self.on_shell_event_inner(event)])
    }

    fn on_shell_event_inner(&mut self, event: ShellEvent) -> Task<Message> {
        match event {
            ShellEvent::NewShell(info) if info.shell == ShellType::SessionLock => {
                // The runtime made a lock surface for an output: ours to
                // draw. The password field can only take focus once the
                // surface exists.
                match &mut self.locker {
                    Some(locker) => {
                        log::debug!("lock surface {:?}", info.window);
                        locker.add_window(info.window);
                        locker.focus().map(Message::Locker)
                    }
                    None => {
                        log::warn!("a lock surface without a locker, closing it");
                        Task::done(Message::UnLock)
                    }
                }
            }
            ShellEvent::NewShell(info) => match (&self.launcher, &self.exiter) {
                // The search field can only take focus once its surface
                // exists; the exit menu's default button the same.
                (Some((dialog, launcher)), _) if dialog.is_window(info.window) => {
                    launcher.focus().map(Message::Launcher)
                }
                (_, Some((dialog, exiter))) if dialog.is_window(info.window) => {
                    exiter.focus().map(Message::Exiter)
                }
                // A bar that hides starts hidden: only its edge's strip
                // takes the pointer.
                _ => self.input_region(info.window),
            },
            ShellEvent::WindowOutputChanged {
                window,
                output: Some(output),
            } => {
                if let Some(locker) = &mut self.locker {
                    locker.set_output(window, OutputId::from(&output));
                }
                Task::none()
            }
            ShellEvent::Locked => {
                log::info!("session locked");
                if let Some(locker) = &mut self.locker {
                    locker.locked = true;
                }
                self.idle.lock_settled();
                Task::none()
            }
            ShellEvent::LockDenied => {
                log::error!("the compositor denied the session lock");
                self.locker = None;
                self.idle.lock_settled();
                Task::none()
            }
            ShellEvent::LockedFinished => {
                log::info!("the compositor ended the session lock");
                self.locker = None;
                self.idle.lock_settled();
                Task::none()
            }
            ShellEvent::OutputAdded(output) => {
                log::debug!(
                    "output {:?}: logical position {:?}, size {:?}",
                    output.name,
                    output.logical_position,
                    output.logical_size
                );
                // Announced again to late subscribers: only a new one
                // may bring a monitor.
                let mut tasks = Vec::new();
                if self
                    .outputs
                    .insert(OutputId::from(&output), output.clone())
                    .is_none()
                {
                    self.scales
                        .output_added(OutputId::from(&output), output.scale_factor);
                    self.brightness.outputs_changed();
                    tasks.push(self.picker_outputs_changed());
                }
                tasks.extend([self.open_panels(&output), self.open_wallpaper(&output)]);
                Task::batch(tasks)
            }
            // Moved, rotated, rescaled: where it is now (debug surfaces,
            // screenshots).
            ShellEvent::OutputUpdated(output) => {
                let Some(known) = self.outputs.get_mut(&OutputId::from(&output)) else {
                    return Task::none();
                };
                let place = |o: &OutputInfo| {
                    (
                        o.logical_position,
                        o.logical_size,
                        o.scale_factor,
                        o.transform,
                    )
                };
                let moved = place(known) != place(&output);
                *known = output;
                if !moved {
                    return Task::none();
                }
                // Rescaled, maybe: its bars tell.
                let id = OutputId::from(&*known);
                let bars: Vec<Id> = self
                    .panels
                    .iter()
                    .filter(|(_, p)| p.output == id)
                    .map(|(&bar, _)| bar)
                    .collect();
                Task::batch(
                    bars.into_iter()
                        .map(ask_scale)
                        .chain([self.picker_outputs_changed()]),
                )
            }
            ShellEvent::OutputRemoved(output) => {
                let gone = OutputId::from(&output);
                let mut closing = Task::none();
                if self.outputs.remove(&gone).is_some() {
                    self.scales.output_removed(gone);
                    self.brightness.outputs_changed();
                    closing = self.picker_outputs_changed();
                }
                let ids: Vec<Id> = self
                    .panels
                    .iter()
                    .filter(|(_, p)| p.output == gone)
                    .map(|(id, _)| *id)
                    .collect();
                log::info!(
                    "output {:?} removed, closing {} panel(s)",
                    output.name,
                    ids.len()
                );
                let mut tasks: Vec<Task<Message>> = ids
                    .into_iter()
                    .map(|id| {
                        self.panels.remove(&id);
                        Task::done(Message::RemoveWindow(id))
                    })
                    .collect();
                tasks.push(surface_tasks(self.wallpapers.output_removed(gone)));
                // Its toasts move to another output.
                tasks.push(surface_tasks(self.toasts.output_removed(gone)));
                tasks.push(self.sync_toasts());
                tasks.push(surface_tasks(
                    self.osd.output_removed(OutputId::from(&output)),
                ));
                tasks.push(closing);
                Task::batch(tasks)
            }
            ShellEvent::Closed(id) => {
                self.measure.borrow_mut().forget(id);
                if let Some(picker) = &self.picker
                    && picker.has_window(id)
                {
                    // Its output went: it was showing every output.
                    log::info!("screenshot: a picker surface closed, picking cancelled");
                    return self.close_picker();
                }
                if let Some(locker) = &mut self.locker
                    && locker.remove_window(id)
                {
                    return Task::none();
                }
                if let Some((dialog, _)) = &self.launcher
                    && dialog.owns(id)
                {
                    // One of the launcher's surfaces went away (on our
                    // request, or not): the rest follows.
                    let rest = dialog.closed(id);
                    self.launcher = None;
                    return surface_tasks(rest);
                }
                if let Some((dialog, _)) = &self.exiter
                    && dialog.owns(id)
                {
                    let rest = dialog.closed(id);
                    self.exiter = None;
                    return surface_tasks(rest);
                }
                if self.showcase.as_ref().is_some_and(|s| s.window == id) {
                    self.showcase = None;
                    return Task::none();
                }
                if self.toasts.closed(id) {
                    return self.sync_toasts();
                }
                if self.wallpapers.closed(id) {
                    return Task::none();
                }
                if self.osd.closed(id) {
                    return Task::none();
                }
                if self.panels.remove(&id).is_none()
                    && self.panels.values_mut().any(|p| p.popup_closed(id))
                {
                    return self.sync_popups();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// Open every configured panel that wants this output and isn't
    /// already shown on it (the shell broadcast replays outputs to late
    /// subscribers, so an output can be announced more than once).
    pub(crate) fn open_panels(&mut self, output: &OutputInfo) -> Task<Message> {
        let output_id = OutputId::from(output);
        let mut tasks = Vec::new();
        for (section, cfg) in PanelConfig::all(&self.config) {
            let shown = self
                .panels
                .values()
                .any(|p| p.output == output_id && p.section == section);
            if shown || !cfg.wants_output(output) {
                continue;
            }
            log::info!("opening [{section}] on output {:?}", output.name);
            let scale = self.scales.of(output_id);
            let mut panel = Panel::new(section, cfg, &self.config, output, scale);
            let id = Id::unique();
            panel.resize(id, measuring!(self));
            tasks.push(Task::done(Message::NewLayerShell {
                settings: panel.layer_settings(),
                id,
            }));
            self.panels.insert(id, panel);
        }
        // New gadgets may draw icons of their own.
        self.resolve_icons();
        tasks.push(self.observe_desktops());
        Task::batch(tasks)
    }

    /// The wallpaper this output is configured for, unless it has one.
    pub(crate) fn open_wallpaper(&mut self, output: &OutputInfo) -> Task<Message> {
        let changes = self.wallpapers.add_output(&self.config, output);
        wallpaper_tasks(changes)
    }

    /// Logical rectangle of an output in the global space (xdg-output).
    pub(crate) fn output_rect(&self, output: OutputId) -> Option<Rectangle> {
        let info = self.outputs.get(&output)?;
        let (x, y) = info.logical_position?;
        let (w, h) = info.logical_size?;
        Some(Rectangle::new(
            Point::new(x as f32, y as f32),
            Size::new(w as f32, h as f32),
        ))
    }

    pub(crate) fn output_name(&self, output: OutputId) -> &str {
        self.outputs
            .get(&output)
            .and_then(|o| o.name.as_deref())
            .unwrap_or("?")
    }

    /// The output new toasts go to: the focused one, else the first.
    pub(crate) fn focused_output(&self) -> Option<&OutputInfo> {
        self.outputs
            .values()
            .find(|o| o.name.is_some() && o.name == self.compositor.focused_output)
            .or_else(|| self.outputs.values().next())
    }

    /// Re-read the config (the user's, the theme's over it when it's
    /// read, and the theme: it may name another one), then close every
    /// panel and open them again for the monitors we know: same path as
    /// a monitor being plugged in. With `scheme`, its `color_scheme`
    /// too (a theme's config just read, or left).
    pub(crate) fn reload_config(&mut self, scheme: bool) -> Task<Message> {
        log::info!("config changed, rebuilding panels");
        self.config = Config::load().themed(self.theme_name.as_deref(), self.theme_config);
        self.general = self.config.section(None);
        // Before anything is measured again below.
        self.scales.set_ui(self.general.scale);
        if scheme {
            self.scheme = self.general.color_scheme;
        }
        self.set_theme(Theme::load(
            &self.config,
            self.theme_name.as_deref(),
            self.scheme,
        ));
        self.locale = Locale::new(&self.general.language);
        self.notifications.set_config(self.config.section(None));
        self.sysmon.set_config(self.config.section(None));
        self.idle.set_config(idle::IdleConfig::load(&self.config));
        self.power.set_config(self.config.section(None));
        self.brightness.set_config(self.config.section(None));
        self.osd.set_config(self.config.section(None));
        self.screenshot.set_config(&self.config);
        let mut icons = Icons::new(&self.config, self.locale.languages());
        icons.keep_index_of(&self.icons);
        self.icons = icons;
        let mut tasks: Vec<Task<Message>> = self
            .panels
            .iter()
            .flat_map(|(&id, panel)| std::iter::once(id).chain(panel.popup_windows()))
            .map(|id| Task::done(Message::RemoveWindow(id)))
            .collect();
        self.panels.clear();
        tasks.push(self.close_launcher());
        tasks.push(self.close_exiter());
        // Its gadgets are the config's: built again with it.
        let showcase = self.showcase.is_some();
        tasks.push(self.close_showcase());
        self.cursor = None;
        let outputs: Vec<OutputInfo> = self.outputs.values().cloned().collect();
        tasks.extend(outputs.iter().map(|o| self.open_panels(o)));
        tasks.push(wallpaper_tasks(self.wallpapers.set_config(&self.config)));
        tasks.push(self.icons.load().map(Message::Icons));
        // The corner or the theme may have changed: reopen the toasts.
        tasks.push(surface_tasks(self.toasts.close()));
        tasks.push(self.sync_toasts());
        // The position may have changed.
        tasks.push(surface_tasks(self.osd.close()));
        if showcase {
            tasks.push(self.open_showcase());
        }
        tasks.push(self.sync_dim());
        Task::batch(tasks)
    }

    /// Re-read the theme files; views pick the new rules up on their
    /// next redraw, every surface is measured again (a bar whose
    /// thickness changed is resized). A file that doesn't parse
    /// (mid-edit, typically) keeps the current theme.
    pub(crate) fn reload_theme(&mut self) -> Task<Message> {
        match Theme::try_load(&self.config, self.theme_name.as_deref(), self.scheme) {
            Ok(theme) => self.set_theme(theme),
            Err(e) => {
                log::error!("{}, keeping the current theme", e.message);
                return Task::none();
            }
        }
        self.relayout()
    }

    /// A new theme: the trees measured with the old one go with it (its
    /// nodes are its own).
    pub(crate) fn set_theme(&mut self, mut theme: Theme) {
        theme.set_config_on(self.theme_config);
        self.theme = theme;
        *self.measure.borrow_mut() = icssed::Measure::new();
    }
}
