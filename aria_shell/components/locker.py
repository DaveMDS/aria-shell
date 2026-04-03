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

from aria_shell.utils import Timer, CleanupHelper
from aria_shell.config import AriaConfig, AriaConfigModel
from aria_shell.utils.env import USER_INFO, search_user_avatar
from aria_shell.utils.logger import get_loggers
from aria_shell.i18n import i18n


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class LockerConfig(AriaConfigModel):
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
        # win = LockerWindow(self.config, None)
        # win.present()

    def _create_surface(self, lock: GtkSessionLock.Instance, monitor: Gdk.Monitor):
        """Called for every monitor, must assign a new surface to the given monitor."""
        INF('Creating lock surface for monitor %s', monitor)
        win = LockerWindow(self.config, lock)
        lock.assign_window_to_monitor(win, monitor)


class LockerWindow(CleanupHelper, Gtk.Window):
    """
    This is the surface that will be placed on every monitor.
    """
    def __init__(self, config: LockerConfig, lock: GtkSessionLock.Instance):
        super().__init__()

        vbox = Gtk.Box(
            orientation=Gtk.Orientation.VERTICAL,
            halign=Gtk.Align.CENTER, valign=Gtk.Align.CENTER,
        )
        self.add_css_class('aria-locker')

        if config.show_avatar:
            vbox.append(AvatarWidget())
        if config.show_username:
            vbox.append(UsernameWidget())
        if config.show_time:
            vbox.append(TimeWidget(config))
        if config.show_date:
            vbox.append(DateWidget(config))

        # unlock button
        btn = Gtk.Button(label=i18n('unlock'), halign=Gtk.Align.CENTER)
        btn.add_css_class('suggested-action')
        btn.add_css_class('aria-locker-unlock')
        self.safe_connect(btn, 'clicked', lambda _: lock.unlock())
        vbox.append(btn)

        self.set_child(vbox)

    def do_unmap(self):
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
            self.set_from_icon_name('avatar-default')  # or 'avatar-default-symbolic'


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
