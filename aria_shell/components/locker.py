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
from aria_shell.services.commands import AriaCommands, CommandFailed
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
        AriaCommands().register('lock', self.the_locker_command)

        self._lock_instance: GtkSessionLock.Instance | None = None

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
        AriaCommands().unregister('lock')
        super().shutdown()  # cleanup safe-connected signals
        if self._lock_instance and self._lock_instance.is_locked():
            self._lock_instance.unlock()
        self._lock_instance = None

    def the_locker_command(self, *_) -> None:
        """Runner for the 'locker' aria command."""
        if error := self.lock():
            raise CommandFailed(error)

    def lock(self) -> None | str:
        """Try to lock the screen, return an error str or None if success."""
        if not self._lock_instance:
            ERR('GtkSessionLock not available, cannot lock the screen')
            return 'GtkSessionLock not available, cannot lock the screen'

        INF('Sending request to lock the screen')
        if not self._lock_instance.lock():
            ERR('Cannot request screen lock')
            return 'Cannot request screen lock'
        # FOR DEBUG (without a real lock):
        # win = LockerWindow(self)
        # win.present()
        return None

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


class LockerWindow(Gtk.Window):
    """
    This is the surface that will be placed on every monitor.
    """
    def __init__(self, locker: AriaLocker):
        super().__init__()
        self.add_css_class('aria-locker')
        self.locker = locker

        # main vertical container
        vbox = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            halign=Gtk.Align.CENTER, valign=Gtk.Align.CENTER,
        )

        # show decoration widgets on request
        if locker.config.show_avatar or locker.config.show_username:
            vbox.append(UserWidget(locker.config))
        if locker.config.show_time or locker.config.show_date:
            vbox.append(DateTimeWidget(locker.config))

        # always show the entry + unlock button
        auth = AuthWidget(locker)
        vbox.append(auth)

        # show the box and set the button to be activated on Enter
        self.set_child(vbox)
        self.set_default_widget(auth.button)


class AuthWidget(CleanupHelper, Gtk.Box):
    """Show password entry, unlock button, the spinner and the error."""
    def __init__(self, locker: AriaLocker):
        super().__init__(orientation=Gtk.Orientation.VERTICAL)
        self.add_css_class('aria-locker-auth')
        self.locker = locker

        # password entry
        self.entry = Gtk.PasswordEntry(
            placeholder_text=i18n('locker.enter_password'),
            activates_default=True,
            show_peek_icon=True,
            visible=False,
        )
        self.safe_connect(self.entry, 'changed', lambda _: self.set_error(None))
        self.append(self.entry)

        # error label
        self.error = Gtk.Label(visible=False)
        self.error.add_css_class('error')
        self.append(self.error)

        # waiting spinner
        self.spinner = Gtk.Spinner(visible=False)
        self.append(self.spinner)

        # unlock button
        self.button = Gtk.Button(label=i18n('locker.unlock'), halign=Gtk.Align.CENTER)
        self.button.add_css_class('suggested-action')
        self.safe_connect(self.button, 'clicked', self.unlock_clicked_cb)
        self.append(self.button)

        # show the password entry if needed, or an error if PAM not available
        if locker.config.password_prompt:
            if PamService().available:
                self.entry.show()
            else:
                self.set_error(i18n('locker.missing_pam'))

    def do_unmap(self):
        self.locker = None
        CleanupHelper.shutdown(self)
        Gtk.Box.do_unmap(self)

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
            # make the shake animation in CSS (for 1 sec max)
            self.add_css_class('shake')
            Timer(1, lambda: self.remove_css_class('shake'))

    def set_error(self, text: str | None):
        if text is None:
            self.error.set_text('')
            self.error.hide()
            self.entry.remove_css_class('error')
        else:
            self.entry.add_css_class('error')
            self.error.set_label(text)
            self.error.show()


class UserWidget(Gtk.Box):
    """Show the avatar and the username."""
    def __init__(self, config: LockerConfig):
        super().__init__(orientation=Gtk.Orientation.VERTICAL)
        self.add_css_class('aria-locker-avatar')

        if config.show_avatar:
            img = Gtk.Image(halign=Gtk.Align.CENTER, overflow=Gtk.Overflow.HIDDEN)
            if avatar := search_user_avatar():
                img.set_from_file(avatar.as_posix())
            else:
                img.set_from_icon_name('avatar-default')  # avatar-default-symbolic?
            self.append(img)

        if config.show_username:
            lbl = Gtk.Label(label=USER_INFO.pw_gecos or USER_INFO.pw_name)
            self.append(lbl)


class DateTimeWidget(Gtk.Box):
    """Show the time and the date."""
    def __init__(self, config: LockerConfig):
        super().__init__(orientation=Gtk.Orientation.VERTICAL)
        self.add_css_class('aria-locker-datetime')
        self.config = config
        self.time_label: Gtk.Label | None = None
        self.date_label: Gtk.Label | None = None

        if config.show_time:
            self.time_label = Gtk.Label()
            self.time_label.add_css_class('aria-locker-time')
            self.append(self.time_label)

        if config.show_date:
            self.date_label = Gtk.Label()
            self.date_label.add_css_class('aria-locker-date')
            self.append(self.date_label)

        self.timer = Timer(1, self._tick, immediate=True)

    def _tick(self) -> bool:
        now = datetime.now()
        if self.time_label:
            text = now.strftime(self.config.time_format)
            self.time_label.set_text(text)
        if self.date_label:
            text = now.strftime(self.config.date_format)
            self.date_label.set_text(text)
        return True

    def do_unmap(self):
        if self.timer:
            self.timer.stop()
            self.timer = None
        Gtk.Box.do_unmap(self)
