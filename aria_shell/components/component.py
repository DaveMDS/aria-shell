from abc import ABC, abstractmethod

from gi.repository import Gtk


class AriaComponent(ABC):
    """
    Base abstract class for all aria components.

    You can raise RuntimeError in __init__ if the component cannot be run
    """
    def __init__(self, app: Gtk.Application):
        self.app = app

    def __repr__(self):
        return f'<{type(self).__name__}>'

    @abstractmethod
    def shutdown(self):
        ...