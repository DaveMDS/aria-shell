from gi.repository import GLib, Gdk, Gio

from aria_shell.utils import Singleton, Signalable
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class DisplayService(Signalable, metaclass=Singleton):
    """
    Get info (and stay informed) about connected/disconnected  monitors

    Signals:
      'monitor-added'(monitor: Gdk.Monitor)
      'monitor-removed'(name: str)
    """
    def __init__(self):
        super().__init__()
        display = Gdk.Display.get_default()
        self._monitors: Gio.ListModel = display.get_monitors()
        self._monitors.connect('items-changed', self._on_listmodel_changed)
        self._map_pos_name = {}  # model-pos => name

    @property
    def monitors(self):
        return self._monitors

    def _on_listmodel_changed(self, monitors: Gio.ListStore,
                              pos: int, removed: int, added: int):
        if added:
            mon: Gdk.Monitor = monitors.get_item(pos)  # noqa
            name = mon.get_connector()
            if mon.is_valid() and name:
                # ok, monitor already populated
                self._map_pos_name[pos] = name
                self.emit('monitor-added', pos, mon)
            else:
                # HACK: under hyperland monitor is added with all properties
                # not set (empty), and are populated asynchrony...
                # hard to find a way to know when is all populated.
                # Going for this ugly hack for the moment:
                self.timer = GLib.timeout_add(500, self._delayed_added, pos, mon)
        if removed:
            if name := self._map_pos_name.pop(pos, None):
                self.emit('monitor-removed', name)

    def _delayed_added(self, pos: int,  mon: Gdk.Monitor):
        name = mon.get_connector()
        if mon and mon.is_valid() and name:
            self._map_pos_name[pos] = name
            self.emit('monitor-added', mon)
        else:
            CRI('Cannot get monitor info (after the ugly delay hack)')
        return False
