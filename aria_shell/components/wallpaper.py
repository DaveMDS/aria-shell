from typing import TYPE_CHECKING, Literal
from pathlib import  Path

from gi.repository import Gdk, Gtk

from aria_shell.components import AriaComponent
from aria_shell.services.display import DisplayService
from aria_shell.config import AriaConfigModel, AriaConfig
from aria_shell.gui import AriaWindow, AriaMediaPicture
from aria_shell.utils.logger import get_loggers
if TYPE_CHECKING:
    from aria_shell.ariashell import AriaShell


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


SIZES = {
    'fill': Gtk.ContentFit.FILL,
    'contain': Gtk.ContentFit.CONTAIN,
    'cover': Gtk.ContentFit.COVER,
    'scaledown': Gtk.ContentFit.SCALE_DOWN,
}


class WallpaperConfig(AriaConfigModel):
    __section__ = 'wallpaper'

    source: Path = ''
    size: Literal['fill', 'contain', 'cover', 'scaledown'] = 'fill'


class AriaWallpaper(AriaComponent):
    """
    This is the manager component that keep track of connected monitors
    and create / destroy the needed WallpaperWindow
    """
    def __init__(self, app: AriaShell):
        super().__init__(app)

        # keep track of created windows per monitor, ex: {'DP-1': LiveWindow, ..}
        self.windows: dict[str, WallpaperWindow] = {}

        # stay informed about changed monitors
        ds = DisplayService()
        ds.connect('monitor-added', self._on_monitor_added)
        ds.connect('monitor-removed', self._on_monitor_removed)

        # inspect connected monitors, and create needed wallpapers
        for monitor in ds.monitors:
            self._on_monitor_added(monitor)

    def shutdown(self):
        ds = DisplayService()
        ds.disconnect('monitor-added', self._on_monitor_added)
        ds.disconnect('monitor-removed', self._on_monitor_removed)
        for window in self.windows.values():
            window.shutdown()
        self.windows.clear()

    def _on_monitor_added(self, monitor: Gdk.Monitor):
        # make sure we never build two windows on a single monitor
        output_name = monitor.get_connector()
        if output_name in self.windows:
            self._on_monitor_removed(monitor)

        # search the correct config section (monitor specific or generic)
        config = AriaConfig().section(WallpaperConfig, f'wallpaper:{output_name}')
        if not config or not config.source:
            config = AriaConfig().section(WallpaperConfig)

        # create the window for this monitor
        if config and config.source:
            win = WallpaperWindow(config, monitor, self.app)
            self.windows[output_name] = win

    def _on_monitor_removed(self, monitor: Gdk.Monitor):
        if window := self.windows.pop(monitor.get_connector(), None):
            window.shutdown()


class WallpaperWindow(AriaWindow):
    """
    The AriaWindow that get created on each monitor.
    """
    def __init__(self, config: WallpaperConfig, monitor: Gdk.Monitor, app: AriaShell):
        INF('Creating wallpaper "%s" on monitor: %s', config.source, monitor.get_connector())
        super().__init__(
            app=app,
            namespace='aria-wallpaper',
            title='Aria LivePaper',
            layer=AriaWindow.Layer.BACKGROUND,
            monitor=monitor,
            exclusive_zone=-1,
            keyboard_mode=AriaWindow.KeyboardMode.NONE,
            anchors=[
                AriaWindow.Edge.TOP, AriaWindow.Edge.BOTTOM,
                AriaWindow.Edge.RIGHT, AriaWindow.Edge.LEFT
            ],
        )
        self.monitor_name = monitor.get_connector()
        self.config = config

        if not config.source or not config.source.exists():
            ERR('Cannot find wallpaper source: %s', config.source)
            return

        picture = AriaMediaPicture(
            source=config.source,
            content_fit=SIZES[config.size]
        )
        self.set_child(picture)
        self.show()

    def shutdown(self):
        INF('Removing wallpaper "%s" from monitor: %s', self.config.source, self.monitor_name)
        super().shutdown()
