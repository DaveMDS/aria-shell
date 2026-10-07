//! The daemon's surfaces besides the bars: opening, closing and syncing
//! the popups, the toasts, the OSD, the dialogs, the picker with the
//! state they show, and [`surface_tasks`], the runtime's messages for
//! what an owner of surfaces asks ([`Surfaces`]).

use iced::Task;
use iced::window::Id;
use iced_exwlshell::reexport::NewLayerShellSettings;

use crate::components::{dialog, exiter, launcher, locker, osd, panel};
use crate::ui::Surfaces;
use crate::{AriaShell, Message};
use dialog::Dialog;
use exiter::Exiter;
use launcher::Launcher;
use locker::Locker;
use panel::Panel;

impl AriaShell {
    /// The popups' content may have changed with the shared state: the
    /// bars take or give back the keyboard, the popups whose gadget
    /// wants another size are placed again.
    pub(crate) fn sync_popups(&mut self) -> Task<Message> {
        // `Shared` borrows the daemon but for the panels.
        let mut panels = std::mem::take(&mut self.panels);
        let surfaces = panels
            .iter_mut()
            .fold(Surfaces::default(), |all, (&id, panel)| {
                all.and(panel.sync_popups(id, self.shared()))
            });
        self.panels = panels;
        surface_tasks(surfaces)
    }

    /// New frames for the surfaces showing the shared state (`Shared`):
    /// all of them but the wallpapers.
    pub(crate) fn redraw_shared(&self) -> Task<Message> {
        let dialogs = self
            .launcher
            .iter()
            .map(|(d, _)| d)
            .chain(self.exiter.iter().map(|(d, _)| d));
        let ids = self
            .panels
            .keys()
            .copied()
            .chain(self.panels.values().flat_map(Panel::popup_windows))
            .chain(dialogs.map(|d| d.window))
            .chain(
                self.locker
                    .iter()
                    .flat_map(|l| l.windows().map(|(id, _)| id)),
            )
            .chain(self.toasts.placed().map(|(window, ..)| window))
            .chain(self.osd.windows().map(|(_, id)| id));
        Task::batch(ids.map(|id| Task::done(Message::Redraw(Some(id)))))
    }

    /// New frames for a panel's popups, which show its gadgets' state.
    pub(crate) fn redraw_popups(&self, panel: Id) -> Task<Message> {
        let ids = self
            .panels
            .get(&panel)
            .into_iter()
            .flat_map(Panel::popup_windows);
        Task::batch(ids.map(|id| Task::done(Message::Redraw(Some(id)))))
    }

    /// Make the toasts match the notifications.
    pub(crate) fn sync_toasts(&mut self) -> Task<Message> {
        let focused = self.focused_output().cloned();
        surface_tasks(self.toasts.sync(
            &self.notifications,
            &self.outputs,
            focused.as_ref(),
            &self.theme,
            &self.icons,
        ))
    }

    /// The screens' brightness changed: the OSD, the gadgets.
    pub(crate) fn brightness_changed(&mut self) -> Task<Message> {
        Task::batch([self.observe_osd(), self.sync_popups(), self.redraw_shared()])
    }

    /// Read what the OSD watches after a change of the shared state, and
    /// show what changed.
    pub(crate) fn observe_osd(&mut self) -> Task<Message> {
        match self.osd.observe(
            &self.audio,
            &self.network,
            &self.power,
            &self.idle,
            &self.brightness,
            &self.locale,
        ) {
            Some(content) => self.show_osd(content),
            None => Task::none(),
        }
    }

    /// Show `content` on the outputs it's for, until its timer runs
    /// out.
    pub(crate) fn show_osd(&mut self, content: osd::Content) -> Task<Message> {
        log::debug!("osd: {content:?}");
        let (surfaces, expiry) = self.osd.show(content, &self.outputs, &self.theme);
        self.resolve_icons();
        Task::batch([surface_tasks(surfaces), expiry.map(Message::OsdExpired)])
    }

    /// Show the launcher on the focused output (the first one if the
    /// compositor didn't say), sized by the theme's `launcher` rule, on
    /// a dialog surface.
    pub(crate) fn open_launcher(&mut self) -> Task<Message> {
        let Some(output) = self.focused_output().cloned() else {
            log::warn!("no output to show the launcher on");
            return Task::none();
        };
        let size = Launcher::size(&self.theme);
        let launcher = Launcher::new(
            self.config.section(None),
            self.config.section(None),
            self.general.terminal.clone(),
            self.icons.index(),
        );
        let (dialog, surfaces) = Dialog::open(
            "aria-launcher",
            &output,
            size,
            self.outputs.values().cloned(),
        );
        self.launcher = Some((dialog, launcher));
        self.resolve_icons();
        Task::batch([self.close_exiter(), surface_tasks(surfaces)])
    }

    pub(crate) fn close_launcher(&mut self) -> Task<Message> {
        match self.launcher.take() {
            Some((dialog, _)) => surface_tasks(dialog.close()),
            None => Task::none(),
        }
    }

    /// Show the exit menu on the focused output, sized from its
    /// content, on a dialog surface; the launcher (if open) goes.
    pub(crate) fn open_exiter(&mut self, exiter: Exiter) -> Task<Message> {
        let Some(output) = self.focused_output().cloned() else {
            log::warn!("no output to show the exiter on");
            return Task::none();
        };
        let size = exiter.size(&self.theme, &self.locale);
        let (dialog, surfaces) =
            Dialog::open("aria-exiter", &output, size, self.outputs.values().cloned());
        self.exiter = Some((dialog, exiter));
        self.resolve_icons();
        Task::batch([self.close_launcher(), surface_tasks(surfaces)])
    }

    pub(crate) fn close_exiter(&mut self) -> Task<Message> {
        match self.exiter.take() {
            Some((dialog, _)) => surface_tasks(dialog.close()),
            None => Task::none(),
        }
    }

    /// The exit menu's content changed (the grid, the confirmation, a
    /// countdown tick): resize its surface when it wants another size.
    pub(crate) fn sync_exiter(&mut self) -> Task<Message> {
        let Some((dialog, exiter)) = &mut self.exiter else {
            return Task::none();
        };
        surface_tasks(dialog.resize(exiter.size(&self.theme, &self.locale)))
    }

    /// Close every open popup but `except` (one opening), telling its
    /// panel (the runtime's `Closed` won't, the popup is forgotten here
    /// first).
    pub(crate) fn close_popups(&mut self, except: Option<Id>) -> Task<Message> {
        let surfaces = self
            .panels
            .values_mut()
            .fold(Surfaces::default(), |all, panel| {
                all.and(panel.close_popups(except))
            });
        surface_tasks(surfaces)
    }

    /// Take the screenshot picker down.
    pub(crate) fn close_picker(&mut self) -> Task<Message> {
        match self.picker.take() {
            Some(picker) => surface_tasks(picker.close()),
            None => Task::none(),
        }
    }

    /// The outputs changed under the picker: it closes, its pictures
    /// are of outputs no longer so.
    pub(crate) fn picker_outputs_changed(&mut self) -> Task<Message> {
        if self.picker.is_some() {
            log::info!("screenshot: the outputs changed, picking cancelled");
        }
        self.close_picker()
    }

    pub(crate) fn lock(&mut self) -> Task<Message> {
        if self.locker.is_some() {
            log::info!("lock requested while locked, ignored");
            return Task::none();
        }
        log::info!("locking the session");
        self.locker = Some(Locker::new(self.config.section(None)));
        self.resolve_icons();
        Task::batch([
            self.close_launcher(),
            self.close_exiter(),
            self.close_popups(None),
            Task::done(Message::Lock),
        ])
    }
}

/// Open the layer surfaces a [`Dialog`] asked for.
pub(crate) fn open_surfaces(surfaces: Vec<(Id, NewLayerShellSettings)>) -> Task<Message> {
    Task::batch(
        surfaces
            .into_iter()
            .map(|(id, settings)| Task::done(Message::NewLayerShell { settings, id })),
    )
}

/// Open, close, resize and redraw the surfaces a component asks for.
pub(crate) fn surface_tasks(surfaces: Surfaces) -> Task<Message> {
    let keyboard = surfaces
        .keyboard
        .into_iter()
        .map(|(id, keyboard_interactivity)| {
            Task::done(Message::KeyboardInteractivityChange {
                id,
                keyboard_interactivity,
            })
        });
    let popup = surfaces
        .popup
        .into_iter()
        .map(|(id, settings)| Task::done(Message::NewPopUp { settings, id }));
    let close = surfaces
        .close
        .into_iter()
        .map(|id| Task::done(Message::RemoveWindow(id)));
    let resize = surfaces
        .resize
        .into_iter()
        .map(|(id, anchor, size)| Task::done(Message::LayoutChange { id, anchor, size }));
    let margin = surfaces
        .margin
        .into_iter()
        .map(|(id, margin)| Task::done(Message::MarginChange { id, margin }));
    let reposition = surfaces
        .reposition
        .into_iter()
        .map(|(id, settings)| Task::done(Message::PopUpReposition { settings, id }));
    let redraw = surfaces
        .redraw
        .into_iter()
        .map(|id| Task::done(Message::Redraw(Some(id))));
    Task::batch(
        keyboard
            .chain(std::iter::once(open_surfaces(surfaces.open)))
            .chain(popup)
            .chain(close)
            .chain(resize)
            .chain(margin)
            .chain(reposition)
            .chain(redraw),
    )
}

/// What the wallpapers asked for: surfaces opened or closed, images to
/// decode.
pub(crate) fn wallpaper_tasks(
    (surfaces, load): (Surfaces, Task<crate::components::wallpaper::Event>),
) -> Task<Message> {
    Task::batch([surface_tasks(surfaces), load.map(Message::Wallpaper)])
}
