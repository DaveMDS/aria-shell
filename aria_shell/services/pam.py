"""

A super simple service to integrate python-pam authentication with
the glib mainloop using a thread.

"""
from collections.abc import Callable
from threading import Thread

from gi.repository import GLib

from aria_shell.utils import Singleton

# python-pam is an optional dependency
try:
    import pam
except ImportError:
    pam = None


AuthCallback = Callable[[bool], None]


class PamService(metaclass=Singleton):

    @property
    def available(self) -> bool:
        """Check if PAM is available and the service can authenticate."""
        return pam is not None

    def authenticate(self, username: str, password: str, callback: AuthCallback):
        """Check the give credentials. Callback will be called with the result."""
        if pam is None:
            callback(False)  # cannot authenticate
        else:
            t = Thread(target=self._thread_started,
                       args=(username, password, callback))
            t.start()


    def _thread_started(self, username, password, callback):
        success = pam.authenticate(username, password)
        GLib.idle_add(self._thread_done, success, callback)

    @staticmethod
    def _thread_done(success, callback):
        if callable(callback):
            callback(success)
