from gi.repository import Gdk, Gio

from aria_shell.utils import Singleton, Signalable
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class DisplayService(Signalable, metaclass=Singleton):
    """
    Get info (and stay informed) about connected/disconnected monitors.

    Signals:
      'monitor-added'(monitor: Gdk.Monitor)
      'monitor-removed'(monitor: Gdk.Monitor)
    """
    __signals__ = ['monitor-added', 'monitor-removed']

    def __init__(self):
        super().__init__()
        INF('Initializing DisplayService')

        # get the monitors list-model from the default display
        display = Gdk.Display.get_default()
        self._list_model: Gio.ListModel = display.get_monitors()

        # keep an internal list of connected monitors
        self._monitors: list[Gdk.Monitor] = []
        for monitor in self._list_model:
            self._monitors.append(monitor)  # noqa

        # stay informed about monitors connected/disconnected
        self._list_model.connect('items-changed', self._on_listmodel_changed)

    @property
    def monitors(self) -> list[Gdk.Monitor]:
        return self._monitors

    def _on_listmodel_changed(self, monitors: Gio.ListModel,
                              pos: int, removed: int, added: int):
        DBG('Monitors changed pos=%d remove=%d added=%d', pos, removed, added)

        # handle added monitors
        for i in range(added):
            # NOTE: sometimes monitor are added with all the properties empty,
            # they are populated later. In that case we need to wait for
            # the 'connector' property to be available...
            monitor: Gdk.Monitor = monitors.get_item(pos + i)  # noqa
            if monitor and not self._monitor_try_insert(monitor, pos + 1):
                monitor.connect(
                    'notify::connector',
                    lambda mon, _: self._monitor_try_insert(mon, pos + i)
                )

        # handle removed monitors
        for i in range(removed):
            if pos < len(self._monitors):
                monitor = self._monitors.pop(pos)
                self.emit('monitor-removed', monitor)

    def _monitor_try_insert(self, monitor: Gdk.Monitor, pos: int) -> bool:
        if monitor in self._monitors:
            return True
        if monitor.is_valid() and monitor.get_connector():
            self._monitors.insert(pos, monitor)
            self.emit('monitor-added', monitor)
            return True
        return False
