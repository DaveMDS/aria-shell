"""
NOTE:
    pywayland is the worst python library I have ever seen!

    Documentation is not really useful, the examples are full of errors, typing
    is a great mess, and issues on GitHub are mostly ignored.

    Improvement has been done 6 months ago, but the last release is from 2024...

    I hope we can find something better in the future.

"""
from collections.abc import Callable
from typing import TYPE_CHECKING, NamedTuple


# users of this module MUST NOT directly import stuff from pywayland !
__all__ = [
    # the aria service
    'WaylandService',
    # pywayland types
    'WlSeat',
    # known protocol objects
    'ExtIdleNotifierV1',
    'ExtIdleNotificationV1',
]


# OK, pywayland is a mess with typing! This is a minimal-hackish stub
if TYPE_CHECKING:
    from pywayland.protocol_core import Interface, Proxy
    from pywayland.client import EventQueue

    class Display:
        # pywayland stuff
        def connect(self) -> None: ...
        def disconnect(self) -> None: ...
        # wayland WlDisplay proxy
        def get_registry(self) -> WlRegistry: ...
        def get_fd(self) -> int: ...
        def dispatch(self, *, block: bool = False, queue: EventQueue | None = None) -> int: ...
        def roundtrip(self) -> None: ...

    class WlRegistry(Interface, Proxy):
        dispatcher: dict[str, Callable]
        def bind[T](self, name: int, interface: type[T], version: int) -> T: ...

    class WlSeat(Interface, Proxy):
        def release(self) -> None: ...

    class ExtIdleNotifierV1(Interface, Proxy):
        def destroy(self) -> None: ...
        def get_idle_notification(self, timeout: int, seat: WlSeat) -> ExtIdleNotificationV1: ...
        def get_input_idle_notification(self, timeout: int, seat: WlSeat) -> ExtIdleNotificationV1: ...

    class ExtIdleNotificationV1(Interface, Proxy):
        def destroy(self) -> None: ...
#===============================================================================


from pywayland.client import Display
from pywayland.protocol.wayland import WlSeat

from pywayland.protocol.ext_idle_notify_v1 import ExtIdleNotifierV1
from pywayland.protocol.ext_idle_notify_v1 import ExtIdleNotificationV1

from gi.repository import GLib

from aria_shell.utils import Singleton
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class Global(NamedTuple):
    """Used to keep track of global objects known to the compositor."""
    name: int
    interface: str
    version: int


class WaylandService(metaclass=Singleton):
    """
    The main wayland singleton service.
    """
    # keep track of global objects known to the compositor
    _global_objects: dict[int, Global] = {}

    def __init__(self):
        INF('Initializing WaylandService')
        self._display: Display | None = None
        self._registry: WlRegistry | None = None
        self._seat: WlSeat | None = None
        self._watch_handler: int = 0

        # connect to the Wayland display
        try:
            self._display = Display()
            self._display.connect()
        except Exception as e:
            self._display = None
            ERR('Failed to connect to Wayland display. Error: %s', e)
            return

        # set up the registry (fill the _global_objects dict)
        def _on_registry_global(registry: WlRegistry, name: int, interface: str, version: int):
            self._global_objects[name] = Global(name, interface, version)
            if interface == WlSeat.name:
                self._seat = registry.bind(name, WlSeat, version)

        def _on_registry_global_remove(_, name: int):
            self._global_objects.pop(name, None)

        self._registry = self._display.get_registry()
        self._registry.dispatcher['global'] = _on_registry_global
        self._registry.dispatcher['global_remove'] = _on_registry_global_remove

        # sync with server (after this point all the registry global events has been emitted)
        self._display.roundtrip()

        if not self._seat:
            ERR('Cannot find the Wayland Seat global object')
            self.shutdown()
            return

        # GLib mainloop integration
        def _wayland_event_source(*_):
            self._display.dispatch(block=True)
            return GLib.SOURCE_CONTINUE

        self._watch_handler = GLib.io_add_watch(
            self._display.get_fd(), GLib.IO_IN, _wayland_event_source
        )

    @property
    def connected(self) -> bool:
        return self._display is not None

    @property
    def seat(self) -> WlSeat:
        return self._seat

    def bind_object[T](self, interface: str, version: int, cls: type[T]) -> T | None:
        # NOTE: the T generic do not work in pycharm... :/
        for g in self._global_objects.values():
            if g.interface == interface and g.version >= version:
                return self._registry.bind(g.name, cls, version)
        return None

    def roundtrip(self):
        if self._display:
            self._display.roundtrip()

    def shutdown(self):
        # TODO no one is calling this!! factorize AriaService!
        INF('Shutting down WaylandService')

        if self._watch_handler:
            GLib.source_remove(self._watch_handler)
            self._watch_handler = 0

        if self._seat:
            self._seat.release()
            self._seat = None

        if self._display:
            self._display.disconnect()
            self._display = None

        self._global_objects.clear()
