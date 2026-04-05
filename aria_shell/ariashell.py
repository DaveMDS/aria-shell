#!/usr/bin/env python3
from pathlib import Path
import argparse

# For GTK4 Layer Shell to get linked before libwayland-client
# we must explicitly load it before importing with gi
from ctypes import CDLL
try:
    CDLL('libgtk4-layer-shell.so')
except Exception as err:
    print(f'ERROR: Cannot preload the gtk4-layer-shell library. {err}')
    raise SystemExit(1)

# Check required gtk libraries
import gi
gi.require_version('Gio', '2.0')
gi.require_version('GioUnix', '2.0')
gi.require_version('GLib', '2.0')
gi.require_version('Gdk', '4.0')
gi.require_version('Gtk', '4.0')
gi.require_version('Gtk4LayerShell', '1.0')
gi.require_version('Gtk4SessionLock', '1.0')

from gi.repository import Gdk, Gtk
# import platform
# print(f'Python: {platform.python_version()}')
# print(f'PyGObject: {gi.__version__}')
# print(f'GLib {'.'.join(map(str, GLib.glib_version))}')
# print(f'Gtk: {Gtk.get_major_version()}.{Gtk.get_minor_version()}.{Gtk.get_micro_version()}')


from aria_shell.i18n import setup_locale
from aria_shell.utils.logger import get_loggers
from aria_shell.utils.env import lookup_config_file, ARIA_ASSETS_DIR
from aria_shell.utils import Timer, FileMonitor, exec_detached
from aria_shell.module import preload_all_modules, unload_all_modules
from aria_shell.config import AriaConfig
from aria_shell.services.display import DisplayService
from aria_shell.services.commands import AriaCommands
from aria_shell.components.cmd_socket import AriaCommandSocket
from aria_shell.components.panel import AriaPanel, PanelConfig
from aria_shell.components.launcher import AriaLauncher
from aria_shell.components.terminal import AriaTerminal
from aria_shell.components.exiter import AriaExiter
from aria_shell.components.locker import AriaLocker
from aria_shell.components.notificator import AriaNotificator


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class AriaShell(Gtk.Application):
    def __init__(self, args: argparse.Namespace):
        super().__init__(application_id='it.gurumeditation.aria-shell')
        self.args = args
        self.config = AriaConfig()

        # keep track of all alive panels, ex: {'eDP-1': [Panel,Panel,..]}
        self.panels: dict[str, list[AriaPanel]] = {}

        # keep track of all loaded CSS
        self.css_providers: list[Gtk.CssProvider] = []

        # components instances
        self.command_socket: AriaCommandSocket | None = None
        self.launcher: AriaLauncher | None = None
        self.terminal: AriaTerminal | None = None
        self.exiter: AriaExiter | None = None
        self.locker: AriaLocker | None = None

        # monitors for config and CSS files change
        self.file_monitors: list[FileMonitor] = []

        # app lifecycle signals
        self.connect('startup', self._on_app_startup)
        self.connect('activate', self._on_app_activate)
        self.connect('shutdown', self._on_app_shutdown)

        # do not allow to run aria-shell multiple times.
        # Gtk.Application automatically run the 'active' signal on the first
        # process when another aria-shell process is executed
        self.first_instance_created = False

    def start(self) -> int:
        try:
            return super().run(None)
        except KeyboardInterrupt:
            return 0
        except Exception as e:
            ERR(e)
            return 5

    def reload(self):
        """Reload config file and restart everything."""
        # TODO test config file before restart !!!!!!!!!!!!

        def _reload_on_next_tick():
            self._shutdown_everything()
            self._setup_everything()

        # actually reload on the next tick, to release the current context,
        # ex: to give "time" to reply on the command socket to the 'reload' cmd.
        Timer(0, _reload_on_next_tick)

    #---------------------------------------------------------------------------
    # region: Gtk.Application lifecycle
    #---------------------------------------------------------------------------
    def _on_app_startup(self, app: Gtk.Application):
        """Startup signal is emitted exactly once on the first app instance."""
        # setup i18n locale
        setup_locale()

        # start command socket listener
        self.command_socket = AriaCommandSocket(self)

    def _on_app_activate(self, app: Gtk.Application):
        """Activate signal is emitted every time the application is launched."""
        if not Gdk.Display.get_default():
            raise RuntimeError('Cannot find wayland display')

        # prevent multiple instances
        if self.first_instance_created:
            return
        self.first_instance_created = True

        # HACK: a fake window to keep the app alive while reloading!
        # (gtk close the whole app when the last window is closed...)
        self.keep_alive_win = Gtk.Window()
        self.add_window(self.keep_alive_win)

        # stay informed about changed monitors
        ds = DisplayService()
        ds.connect('monitor-added', self._on_monitor_added)
        ds.connect('monitor-removed', self._on_monitor_removed)

        # now prepare all the stuff that can be reloaded
        self._setup_everything()

    def _on_app_shutdown(self, _app: Gtk.Application):
        """Shutdown signal is emitted when the application is exiting."""
        self._shutdown_everything()
        # TODO more stuff to shutdown here? the command socket?
        INF('Bye bye o/')
    # endregion

    #---------------------------------------------------------------------------
    # region: Aria global setup / teardown
    #---------------------------------------------------------------------------
    def _setup_everything(self):
        INF('=========================================')
        INF('Warming up aria-shell...')

        # load config file, and reload the whole shell when it changes
        config_file = self.config.load_conf(self.args.config)
        if self.config.general.reload_config:
            monitor = FileMonitor(config_file, lambda _: self.reload())
            self.file_monitors.append(monitor)

        # load CSS files, and reload the styles when they changes
        loaded_files = self._load_css_styles(self.args.style)
        if self.config.general.reload_style:
            for css_file in loaded_files:
                monitor = FileMonitor(css_file, lambda _: self._reload_css_styles())
                self.file_monitors.append(monitor)

        # register the reload command
        AriaCommands().register('reload', lambda c,p: self.reload())

        # create instances of all components
        self.launcher = AriaLauncher(self)
        self.exiter = AriaExiter(self)
        self.locker = AriaLocker(self)
        self.notificator = AriaNotificator(self)
        try:
            self.terminal = AriaTerminal(self)
        except RuntimeError:
            WRN('Vte4 not available, embedded terminal is disabled!')

        # preload all modules (the gadgets)
        preload_all_modules()

        # inspect connected monitors, and create needed panels
        for monitor in DisplayService().monitors:
            self._on_monitor_added(monitor)

        # run user applications from the [autostart] config section
        for command in self.config.autostart():
            exec_detached(command)

    def _shutdown_everything(self):
        INF('-----------------------------------------------------------------')
        INF('Shutting down aria-shell...')

        # destroy file monitors
        while self.file_monitors:
            monitor = self.file_monitors.pop()
            monitor.destroy()

        # destroy all panels
        for monitor, panels in self.panels.items():
            for panel in panels:
                panel.shutdown()
        self.panels = {}

        # shutdown all modules
        unload_all_modules()

        # shutdown all components, and release their references
        if self.launcher:
            self.launcher.shutdown()
            self.launcher = None
        if self.terminal:
            self.terminal.shutdown()
            self.terminal = None
        if self.exiter:
            self.exiter.shutdown()
            self.exiter = None
        if self.locker:
            self.locker.shutdown()
            self.locker = None
        if self.notificator:
            self.notificator.shutdown()
            self.notificator = None

        # un-register basic commands
        AriaCommands().unregister('reload')

        # clear all loaded CSS styles
        self._clear_css_styles()

        # clear the loaded config
        self.config.clear()
    # endregion

    #---------------------------------------------------------------------------
    # region: CSS styles
    #---------------------------------------------------------------------------
    def _load_css_styles(self, user_css: Path | None) -> list[Path]:
        # return the list of successfully loaded files
        loaded_files = []

        # load base.css only from python package (base should never be edited)
        base_css = ARIA_ASSETS_DIR / 'base.css'
        if self._load_css_file(base_css):
            loaded_files.append(base_css)

        # load stylesheet given on command line (file path)
        if self._load_css_file(user_css):
            loaded_files.append(user_css)

        # load style from config (can be named and searched in sys dirs)
        style = self.config.general.style
        if style:
            if not style.endswith('.css'):
                style += '.css'
            if '/' in style:
                css_path = Path(user_css)
            else:
                css_path = lookup_config_file(style)
            if css_path:
                if self._load_css_file(css_path):
                    loaded_files.append(css_path)
            else:
                ERR(f'Cannot find requested style: {style}')

        return loaded_files

    def _load_css_file(self, css: Path | None) -> bool:
        if css and css.exists() and css.is_file():
            INF(f'Loading css file: {css}')

            css_provider = Gtk.CssProvider()
            css_provider.load_from_path(css.as_posix())
            self.css_providers.append(css_provider)

            Gtk.StyleContext.add_provider_for_display(
                Gdk.Display.get_default(), css_provider,
                Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
            )
            return True
        elif css is not None:
            ERR(f'Cannot find css file: {css}')
        return False

    def _clear_css_styles(self):
        INF(f'Clearing CSS styles')
        for provider in self.css_providers:
            Gtk.StyleContext.remove_provider_for_display(
                Gdk.Display.get_default(),
                provider
            )
        self.css_providers.clear()

    def _reload_css_styles(self):
        self._clear_css_styles()
        self._load_css_styles(self.args.style)
    # endregion

    #---------------------------------------------------------------------------
    # region: Manage monitors plugged and unplugged, create necessary Panels
    #---------------------------------------------------------------------------
    def _on_monitor_added(self, monitor: Gdk.Monitor):
        output_name = monitor.get_connector()
        if not output_name:
            CRI('Cannot find monitor name for monitor %s', monitor)
            return

        INF('Monitor connected %s', output_name)
        # geom = monitor.get_geometry()
        # DBG(f'MONITOR: {monitor.get_model()} - {output_name} - scale={monitor.get_scale_factor()} valid={monitor.is_valid()}')
        # DBG(f'GEOMETRY: size={geom.width}x{geom.height} x={geom.x} y={geom.y}')

        self._create_panels_for_monitor(monitor)

    def _on_monitor_removed(self, monitor: Gdk.Monitor):
        name = monitor.get_connector()
        INF(f'Monitor disconnected {name}')
        for panel in self.panels.pop(name, []):
            panel.shutdown()

    def _create_panels_for_monitor(self, monitor: Gdk.Monitor):
        output_name = monitor.get_connector()
        for section in sorted(self.config.sections('panel')):
            panel_conf = self.config.section(section, PanelConfig)
            outputs = panel_conf.outputs
            if (not outputs) or ('all' in outputs) or (output_name in outputs):
                if ':' in section and not section.endswith(':'):
                    panel_name = section.split(':')[1]
                else:
                    panel_name = 'Aria Panel'

                panel = AriaPanel(panel_name, panel_conf, monitor, self)
                self.panels.setdefault(output_name, []).append(panel)
    # endregion
