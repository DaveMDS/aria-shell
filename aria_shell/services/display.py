from gi.repository import Gdk, Gio

from aria_shell.utils import Singleton, Signalable, Timer
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class DisplayService(Signalable, metaclass=Singleton):
    """
    Get info (and stay informed) about connected/disconnected  monitors

    Signals:
      'monitor-added'(monitor: Gdk.Monitor)
      'monitor-removed'(monitor: Gdk.Monitor)
    """
    def __init__(self):
        super().__init__()

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
            monitor: Gdk.Monitor = monitors.get_item(pos + i)  # noqa
            if monitor and monitor.is_valid() and monitor.get_connector():
                # ok, monitor already populated
                self._monitors.insert(pos + i, monitor)
                self.emit('monitor-added', monitor)
            elif monitor:
                # HACK: under hyprland monitor is added with all properties
                # not set (empty), and are populated asynchrony...
                # hard to find a way to know when is all populated.
                # Going for this ugly hack for the moment:
                self.timer = Timer(0.1, self._delayed_added, pos, monitor)

        # handle removed monitors
        for i in range(removed):
            if pos < len(self._monitors):
                monitor = self._monitors.pop(pos)
                self.emit('monitor-removed', monitor)

    def _delayed_added(self, pos: int,  monitor: Gdk.Monitor) -> bool:
        if monitor and monitor.is_valid() and monitor.get_connector():
            self._monitors.insert(pos, monitor)
            self.emit('monitor-added', monitor)
        return False
