from abc import ABC, abstractmethod
from typing import TYPE_CHECKING


if TYPE_CHECKING:
    from aria_shell.ariashell import AriaShell


class AriaComponent(ABC):
    """
    Base abstract class for all aria components.

    All AriaComponent subclasses are automatically initialized on
    aria startup, and the shutdown() method is called on exit.

    shutdown() / __init__() sequence are also called on aria reload,
    for example when the config file change.

    Implementations should raise RuntimeError(msg) inside __init__ in case
    the component cannot run for some reason.
    """
    def __init__(self, app: AriaShell):
        self.app = app

    def __repr__(self):
        return f'<{type(self).__name__}>'

    @abstractmethod
    def shutdown(self):
        ...