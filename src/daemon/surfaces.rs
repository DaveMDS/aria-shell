//! The daemon's surfaces besides the bars: opening, closing and syncing
//! the popups, the toasts, the OSD, the dialogs, the picker with the
//! state they show, and [`surface_tasks`], the runtime's messages for
//! what an owner of surfaces asks ([`Surfaces`]).

use iced::Task;
use iced::window::Id;
use iced_exwlshell::actions::ActionCallback;
use iced_exwlshell::reexport::NewLayerShellSettings;
use iced_wayland_subscriber::OutputInfo;

use crate::components::{dialog, exiter, launcher, locker, osd, panel};
use crate::daemon::view::dialog_surface;
use crate::ui::{self, Surfaces};
use crate::{AriaShell, Message};
use dialog::Dialog;
use exiter::Exiter;
use launcher::Launcher;
use locker::Locker;
use panel::Panel;

/// The keys the dialogs are measured under (one of each at a time).
const LAUNCHER: &str = "launcher";
const EXITER: &str = "exiter";

impl AriaShell {
    /// The popups' content may have changed with the shared state: the
    /// bars take or give back the keyboard, the popups whose gadget
    /// wants another size are placed again, and so are the bars as long
    /// as their gadgets.
    pub(crate) fn sync_popups(&mut self) -> Task<Message> {
        let m = measuring!(self);
        let mut tasks = Vec::new();
        // A bar as long as its gadgets follows them.
        for (&id, panel) in self.panels.iter_mut().filter(|(_, p)| p.fits()) {
            if let Some((anchor, size, zone_size)) = panel.resize(id, m) {
                tasks.push(Task::done(Message::LayoutChange { id, anchor, size }));
                tasks.push(Task::done(Message::ExclusiveZoneChange { id, zone_size }));
            }
        }
        let surfaces = self
            .panels
            .iter_mut()
            .fold(Surfaces::default(), |all, (&id, panel)| {
                all.and(panel.sync_popups(id, m))
            });
        tasks.push(surface_tasks(surfaces));
        // A popup closed may let its bar hide.
        tasks.push(self.autohide_timers());
        Task::batch(tasks)
    }

    /// The bars told whether their output shows a workspace without
    /// windows (a `smart` one shows then), and the timers that arms.
    pub(crate) fn observe_desktops(&mut self) -> Task<Message> {
        for panel in self.panels.values_mut() {
            panel.set_empty(self.compositor.desktop_empty(panel.output_name()));
        }
        self.autohide_timers()
    }

    /// The timers the bars that hide want armed: each goes off as a
    /// `Message::Autohide`.
    pub(crate) fn autohide_timers(&mut self) -> Task<Message> {
        Task::batch(self.panels.iter_mut().filter_map(|(&id, panel)| {
            let (serial, delay) = panel.autohide_timer()?;
            Some(Task::perform(tokio::time::sleep(delay), move |()| {
                Message::Autohide(id, serial)
            }))
        }))
    }

    /// What iced draws surface `window` at, over its output's scale: the
    /// UI's (`[general] scale`), every surface the same.
    pub(crate) fn scale_factor(&self, _window: Id) -> f32 {
        self.scales.ui()
    }

    /// The part of bar `id` that takes the pointer: all of it, or the
    /// strip at its edge while it's hidden.
    pub(crate) fn input_region(&self, id: Id) -> Task<Message> {
        let Some(panel) = self.panels.get(&id).filter(|p| p.autohide()) else {
            return Task::none();
        };
        let (x, y, width, height) = panel.input_region();
        let callback = ActionCallback::new(move |region| region.add(x, y, width, height));
        Task::done(Message::SetInputRegion { id, callback })
    }

    /// Every surface measured again (the theme or a scale changed): the
    /// bars whose thickness changed are resized, the popups, the
    /// toasts, the OSD and the dialogs too.
    pub(crate) fn relayout(&mut self) -> Task<Message> {
        let m = measuring!(self);
        let mut tasks = Vec::new();
        for (&id, panel) in &mut self.panels {
            panel.scale = self.scales.of(panel.output);
            if let Some((anchor, size, zone_size)) = panel.resize(id, m) {
                tasks.push(Task::done(Message::LayoutChange { id, anchor, size }));
                tasks.push(Task::done(Message::ExclusiveZoneChange { id, zone_size }));
            }
        }
        tasks.push(self.sync_popups());
        tasks.push(self.sync_toasts());
        tasks.push(surface_tasks(
            self.osd.relayout(&self.outputs, measuring!(self)),
        ));
        tasks.push(self.sync_launcher());
        tasks.push(self.sync_exiter());
        Task::batch(tasks)
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
            .chain(self.showcase.iter().map(|s| s.window))
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
        surface_tasks(
            self.toasts
                .sync(&self.outputs, focused.as_ref(), measuring!(self)),
        )
    }

    /// The screens' brightness changed: the OSD, the gadgets.
    pub(crate) fn brightness_changed(&mut self) -> Task<Message> {
        Task::batch([self.observe_osd(), self.sync_popups(), self.redraw_shared()])
    }

    /// Make the screens' brightness follow idle's dim stage, after
    /// anything that may have changed it.
    pub(crate) fn sync_dim(&mut self) -> Task<Message> {
        if self.brightness.set_dim(self.idle.dim()) {
            self.brightness_changed()
        } else {
            Task::none()
        }
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
        let (surfaces, expiry) = self.osd.show(content, &self.outputs, measuring!(self));
        self.resolve_icons();
        Task::batch([surface_tasks(surfaces), expiry.map(Message::OsdExpired)])
    }

    /// Show the launcher on the focused output (the first one if the
    /// compositor didn't say), sized by its content, on a dialog
    /// surface.
    pub(crate) fn open_launcher(&mut self) -> Task<Message> {
        let Some(output) = self.focused_output().cloned() else {
            log::warn!("no output to show the launcher on");
            return Task::none();
        };
        let launcher = Launcher::new(
            self.config.section(None),
            self.config.section(None),
            self.general.terminal.clone(),
            self.icons.index(),
        );
        self.resolve_icons();
        let size = self.launcher_size(&launcher, &output);
        let (dialog, surfaces) = Dialog::open(
            "aria-launcher",
            &output,
            size,
            self.outputs.values().cloned(),
        );
        self.launcher = Some((dialog, launcher));
        Task::batch([self.close_exiter(), surface_tasks(surfaces)])
    }

    /// The launcher's surface size on `output`: its content's.
    fn launcher_size(&self, launcher: &Launcher, output: &OutputInfo) -> (u32, u32) {
        let root = self.theme.root("launcher", self.scales.of(output.into()));
        let content = dialog_surface(
            root,
            launcher.view(self.shared()),
            launcher::Message::Nothing,
        );
        ui::size(
            measuring!(self),
            LAUNCHER,
            content,
            ui::room_on(Some(output)),
        )
    }

    /// The launcher's content changed: resize its surface when it
    /// wants another size.
    pub(crate) fn sync_launcher(&mut self) -> Task<Message> {
        let Some((dialog, launcher)) = &self.launcher else {
            return Task::none();
        };
        let Some(output) = self.outputs.get(&dialog.output) else {
            return Task::none();
        };
        let size = self.launcher_size(launcher, output);
        let Some((dialog, _)) = &mut self.launcher else {
            return Task::none();
        };
        surface_tasks(dialog.resize(size))
    }

    pub(crate) fn close_launcher(&mut self) -> Task<Message> {
        match self.launcher.take() {
            Some((dialog, _)) => {
                self.measure.borrow_mut().forget(LAUNCHER);
                surface_tasks(dialog.close())
            }
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
        let size = self.exiter_size(&exiter, &output);
        let (dialog, surfaces) =
            Dialog::open("aria-exiter", &output, size, self.outputs.values().cloned());
        self.exiter = Some((dialog, exiter));
        self.resolve_icons();
        Task::batch([self.close_launcher(), surface_tasks(surfaces)])
    }

    /// The exit menu's surface size on `output`: its content's.
    fn exiter_size(&self, exiter: &Exiter, output: &OutputInfo) -> (u32, u32) {
        let root = self.theme.root("exiter", self.scales.of(output.into()));
        let content = dialog_surface(root, exiter.view(self.shared()), exiter::Message::Nothing);
        ui::size(measuring!(self), EXITER, content, ui::room_on(Some(output)))
    }

    pub(crate) fn close_exiter(&mut self) -> Task<Message> {
        match self.exiter.take() {
            Some((dialog, _)) => {
                self.measure.borrow_mut().forget(EXITER);
                surface_tasks(dialog.close())
            }
            None => Task::none(),
        }
    }

    /// The exit menu's content changed (the grid, the confirmation, a
    /// countdown tick): resize its surface when it wants another size.
    pub(crate) fn sync_exiter(&mut self) -> Task<Message> {
        let Some((dialog, exiter)) = &self.exiter else {
            return Task::none();
        };
        let Some(output) = self.outputs.get(&dialog.output) else {
            return Task::none();
        };
        let size = self.exiter_size(exiter, output);
        let Some((dialog, _)) = &mut self.exiter else {
            return Task::none();
        };
        surface_tasks(dialog.resize(size))
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
            self.close_showcase(),
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
