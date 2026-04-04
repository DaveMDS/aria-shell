"""

Aria screen locker.

Implement the ext-session-lock-v1 protocol using Gtk4SessionLock.

References:
- https://wayland.app/protocols/ext-session-lock-v1
- https://wmww.github.io/gtk4-layer-shell/gtk4-layer-shell-GTK4-Session-Lock.html

"""
from datetime import datetime

from gi.repository import Gtk, Gdk
from gi.repository import Gtk4SessionLock as GtkSessionLock

from aria_shell.services.pam import PamService, AuthCallback
from aria_shell.utils import Timer, CleanupHelper
from aria_shell.config import AriaConfig, AriaConfigModel
from aria_shell.utils.env import USER_INFO, search_user_avatar
from aria_shell.utils.logger import get_loggers
from aria_shell.i18n import i18n


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class LockerConfig(AriaConfigModel):
    password_prompt: bool = True
    show_avatar: bool = True
    show_username: bool = True
    show_time: bool = True
    show_date: bool = True
    time_format: str = '%H:%M'
    date_format: str = '%A %d %B'


class AriaLocker(CleanupHelper):
    """
    Aria screen locker manager.
    """
    def __init__(self, _app: Gtk.Application):
        super().__init__()
        INF('Initialize Aria Locker')
        self.config = AriaConfig().section('locker', LockerConfig)

        if not GtkSessionLock.is_supported():
            ERR('GtkSessionLock is not supported. Locker will not work!')
            return

        if not PamService().available:
            WRN('PAM is not available, locker will not authenticate credentials!!')

        lock = GtkSessionLock.Instance()
        self.safe_connect(lock, 'monitor',  self._create_surface)
        self.safe_connect(lock, 'locked',   lambda _: INF('Screen successfully locked'))
        self.safe_connect(lock, 'unlocked', lambda _: INF('Screen successfully unlocked'))
        self.safe_connect(lock, 'failed',   lambda _: ERR('Screen lock request failed'))
        self._lock_instance = lock

    def shutdown(self):
        INF('Shutting down Aria Locker')
        super().shutdown()  # cleanup safe-connected signals
        if self._lock_instance and self._lock_instance.is_locked():
            self._lock_instance.unlock()
        self._lock_instance = None

    def lock(self):
        if not self._lock_instance:
            ERR('GtkSessionLock not available, cannot lock the screen')
            return

        INF('Sending request to lock the screen')
        if not self._lock_instance.lock():
            ERR('Cannot request screen lock')
        # FOR DEBUG (without a real lock):
        # win = LockerWindow(self)
        # win.present()

    def _create_surface(self, lock: GtkSessionLock.Instance, monitor: Gdk.Monitor):
        """Called for every monitor, must assign a new surface to the given monitor."""
        INF('Creating lock surface for monitor %s', monitor)
        win = LockerWindow(self)
        lock.assign_window_to_monitor(win, monitor)

    def unlock(self, password: str | None, callback: AuthCallback):
        """Called by a LockerWindow when the user press unlock."""
        if self.config.password_prompt is False or not PamService().available:
            # no auth requested or available, just unlock now
            INF('Unlocking without using credentials!')
            if callable(callback):
                callback(True)
            self._lock_instance.unlock()  # this will destroy all created windows on all monitors
            return

        INF('Authenticating user credentials using PAM...')
        def _auth_done(success: bool):
            if callable(callback):
                callback(success)
            if success:
                INF('PAM authentication successful')
                self._lock_instance.unlock()  # this will destroy all created windows on all monitors
            else:
                WRN('PAM authentication failed')

        PamService().authenticate(USER_INFO.pw_name, password, _auth_done)


class LockerWindow(CleanupHelper, Gtk.Window):
    """
    This is the surface that will be placed on every monitor.
    """
    def __init__(self, locker: AriaLocker):
        super().__init__()
        self.add_css_class('aria-locker')
        self.locker = locker

        self.entry: Gtk.PasswordEntry
        self.button: Gtk.Button
        self.spinner: Gtk.Spinner
        self.error: Gtk.Label

        # main vertical container
        vbox = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            halign=Gtk.Align.CENTER, valign=Gtk.Align.CENTER,
        )

        # show decoration widgets on request
        if locker.config.show_avatar:
            vbox.append(AvatarWidget())
        if locker.config.show_username:
            vbox.append(UsernameWidget())
        if locker.config.show_time:
            vbox.append(TimeWidget(locker.config))
        if locker.config.show_date:
            vbox.append(DateWidget(locker.config))

        # password entry
        self.entry = Gtk.PasswordEntry(
            placeholder_text=i18n('locker.enter_password'),
            activates_default=True,
            show_peek_icon=True,
            visible=False,
        )
        self.entry.add_css_class('aria-locker-entry')
        self.safe_connect(self.entry, 'changed',
                          lambda _: self.set_error(None))
        vbox.append(self.entry)

        # error label
        self.error = Gtk.Label(visible=False)
        self.error.add_css_class('aria-locker-error')
        self.error.add_css_class('error')
        vbox.append(self.error)

        # waiting spinner
        self.spinner = Gtk.Spinner(visible=False)
        vbox.append(self.spinner)

        # unlock button
        self.button = Gtk.Button(label=i18n('locker.unlock'), halign=Gtk.Align.CENTER)
        self.button.add_css_class('suggested-action')
        self.button.add_css_class('aria-locker-unlock')
        self.safe_connect(self.button, 'clicked', self.unlock_clicked_cb)
        vbox.append(self.button)

        # show the password entry if needed, or an error if PAM not available
        if locker.config.password_prompt:
            if PamService().available:
                self.entry.show()
            else:
                self.set_error(i18n('locker.missing_pam'))

        self.set_child(vbox)
        self.set_default_widget(self.button)

    def unlock_clicked_cb(self, _button):
        self.entry.set_sensitive(False)
        self.entry.set_show_peek_icon(False)
        self.entry.set_show_peek_icon(True)
        self.button.set_sensitive(False)
        self.set_error(None)
        self.spinner.show()
        self.spinner.start()
        self.locker.unlock(self.entry.get_text(), self.unlock_done_cb)

    def unlock_done_cb(self, success: bool):
        self.spinner.stop()
        self.spinner.hide()
        if not success:
            self.entry.set_sensitive(True)
            self.button.set_sensitive(True)
            self.entry.set_text('')
            self.entry.grab_focus()
            self.set_error(i18n('locker.auth_failed'))
            # make the entry shake in CSS (for 1 sec max)
            self.entry.add_css_class('shake')
            Timer(1, lambda: self.entry.remove_css_class('shake'))

    def set_error(self, text: str | None):
        if text is None:
            self.error.set_text('')
            self.error.hide()
            self.entry.remove_css_class('error')
        else:
            self.entry.add_css_class('error')
            self.error.set_label(text)
            self.error.show()

    def do_unmap(self):
        self.locker = None
        CleanupHelper.shutdown(self)
        Gtk.Window.do_unmap(self)


class AvatarWidget(Gtk.Image):
    """Show the user avatar."""
    def __init__(self):
        super().__init__(halign=Gtk.Align.CENTER)
        self.set_overflow(Gtk.Overflow.HIDDEN)
        self.add_css_class('aria-locker-avatar')

        if avatar := search_user_avatar():
            self.set_from_file(avatar.as_posix())
        else:
            self.set_from_icon_name('avatar-default')  # avatar-default-symbolic?


class UsernameWidget(Gtk.Label):
    """Show the username."""
    def __init__(self):
        super().__init__()
        self.add_css_class('aria-locker-username')
        self.set_label(USER_INFO.pw_gecos or USER_INFO.pw_name)


class TimeWidget(Gtk.Label):
    """Show the current time."""
    def __init__(self, config: LockerConfig):
        super().__init__()
        self.config = config
        self.add_css_class('aria-locker-time')
        self._timer = Timer(1, self._tick, immediate=True)

    def _tick(self) -> bool:
        text = datetime.now().strftime(self.config.time_format)
        self.set_text(text)
        return True

    def do_unmap(self):
        self._timer.stop()
        self._timer = None
        Gtk.Label.do_unmap(self)


class DateWidget(Gtk.Label):
    """Show the current date."""
    def __init__(self, config: LockerConfig):
        super().__init__()
        self.add_css_class('aria-locker-date')
        self.config = config
        self._timer = Timer(60, self._timer_tick, immediate=True)

    def _timer_tick(self) -> bool:
        text = datetime.now().strftime(self.config.date_format)
        self.set_text(text)
        return True

    def do_unmap(self):
        self._timer.stop()
        self._timer = None
        Gtk.Label.do_unmap(self)
