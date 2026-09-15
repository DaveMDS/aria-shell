from abc import ABC, abstractmethod

from aria_shell.utils import Singleton


class AriaService(ABC, metaclass=Singleton):
    """
    Base abstract class for all aria services.

    A Service is a Singleton global utility.

    Services are lazily initialized only once on first usage.
    The shutdown() method is called only once on aria shutdown (NOT on restart).

    """
    @abstractmethod
    def __init__(self):
        """Init is called only once on first usage. No params allowed."""

    @abstractmethod
    def shutdown(self):
        """Called on aria shutdown (NOT on restart)."""

    def __repr__(self):
        return f'<{type(self).__name__}>'

