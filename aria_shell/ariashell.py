#!/usr/bin/env python3
from pathlib import Path
import argparse

# For GTK4 Layer Shell to get linked before libwayland-client
# we must explicitly load it before importing with gi
from ctypes import CDLL
CDLL('libgtk4-layer-shell.so')

import gi
gi.require_version('Gdk', '4.0')
gi.require_version('Gtk', '4.0')
gi.require_version('Gtk4LayerShell', '1.0')
from gi.repository import Gio, Gdk, Gtk, Gtk4LayerShell as LayerShell  # noqa

from aria_shell.i18n import setup_locale
from aria_shell.utils.logger import get_loggers
from aria_shell.utils.env import lookup_config_file, ARIA_ASSETS_DIR
from aria_shell.module import load_modules, unload_all_modules
from aria_shell.config import AriaConfig
from aria_shell.services.display import DisplayService
from aria_shell.components.commands import AriaCommands
from aria_shell.components.cmd_socket import AriaCommandSocket
from aria_shell.components.panel import AriaPanel, PanelConfig
from aria_shell.components.launcher import AriaLauncher
from aria_shell.components.terminal import AriaTerminal


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class AriaShell(Gtk.Application):
    def __init__(self, args: argparse.Namespace):
        super().__init__(application_id='org.davemds.aria-shell')
        self.args = args
        self.conf = AriaConfig()

        self.panels: dict[str, list[AriaPanel]] = {}  # es: {'eDP-1': [Panels]}

        # components instances
        self.command_socket: AriaCommandSocket | None = None
        self.commands: AriaCommands | None = None
        self.launcher: AriaLauncher | None = None
        self.terminal: AriaTerminal | None = None

        # app lifecycle
        self.connect('startup', self._on_app_startup)
        self.connect('activate', self._on_app_active)
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

    def _on_app_startup(self, app: Gtk.Application):
        # setup i18n locale
        setup_locale()

        # load config file
        self.conf.load_conf(self.args.config)

        # load css files
        self._load_css_styles(self.args.style)

        # init commands
        self.commands = AriaCommands(app)

        # load and init all requested modules
        load_modules(self.conf.general.modules)

    def _on_app_shutdown(self, _app: Gtk.Application):
        INF(f'Shutting down... {self}')
        unload_all_modules()
        INF('Bye bye o/')
        # TODO shutdown components, windows, sockets ...

    def _on_app_active(self, app: Gtk.Application):
        if not Gdk.Display.get_default():
            raise RuntimeError('Cannot find wayland display')

        # prevent multiple instances
        if self.first_instance_created:
            return
        self.first_instance_created = True

        # inspect connected monitors, and create needed panels
        ds = DisplayService()
        ds.connect('monitor-added', self._on_monitor_added)
        ds.connect('monitor-removed', self._on_monitor_removed)
        for monitor in ds.monitors:
            self._on_monitor_added(monitor)

        # start command socket listener
        self.command_socket = AriaCommandSocket(app)

        # init the launcher
        self.launcher = AriaLauncher(app)
        # self.launcher.show()

        # init the terminal
        try:
            self.terminal = AriaTerminal(app)
            # self.terminal.show()
        except RuntimeError:
            WRN('Vte4 not available, embedded terminal is disabled!')

        return 0

    def _load_css_styles(self, user_css: Path | None):
        # load base.css from python package only (base shoud never be edited)
        self._load_css_file(ARIA_ASSETS_DIR / 'base.css')

        # load stylesheet given on command line (file path)
        if user_css:
            self._load_css_file(user_css)

        # load style from config (can be named and searched in sys dirs)
        style = self.conf.general.style
        if style:
            if not style.endswith('.css'):
                style += '.css'
            if '/' in style:
                css_path = Path(user_css)
            else:
                css_path = lookup_config_file(style)
            if css_path:
                self._load_css_file(css_path)
            else:
                ERR(f'Cannot find requested style: {style}')

    @staticmethod
    def _load_css_file(css: Path):
        if css.exists() or css.is_file():
            INF(f'Loading css file: {css}')
            css_provider = Gtk.CssProvider()
            css_provider.load_from_path(css.as_posix())
            Gtk.StyleContext.add_provider_for_display(
                Gdk.Display.get_default(), css_provider,
                Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
            )
        else:
            ERR(f'Cannot find css file: {css}')

    def _on_monitor_added(self, monitor: Gdk.Monitor):
        output_name = monitor.get_connector()
        geom = monitor.get_geometry()

        DBG(f'==== MONITOR ADDED {monitor}')
        DBG(f'MONITOR: {monitor.get_model()} - {output_name} - scale={monitor.get_scale_factor()} valid={monitor.is_valid()}')
        DBG(f'GEOMETRY: size={geom.width}x{geom.height} x={geom.x} y={geom.y}')

        if not output_name:
            CRI(f'Cannot find monitor name for monitor {monitor}')
            return

        self._create_panels_for_monitor(monitor)

    def _on_monitor_removed(self, name: str):
        DBG(f'====  MONITOR REMOVED {name}')
        for panel in self.panels.pop(name, []):
            panel.destroy()

    def _create_panels_for_monitor(self, monitor: Gdk.Monitor):
        output_name = monitor.get_connector()
        DBG(f'Creating panels for monitor {output_name}')
        for section in sorted(self.conf.sections('panel')):
            panel_conf = self.conf.section(section, PanelConfig)
            outputs = panel_conf.outputs
            if (not outputs) or ('all' in outputs) or (output_name in outputs):
                if ':' in section and not section.endswith(':'):
                    panel_name = section.split(':')[1]
                else:
                    panel_name = 'Aria Panel'
                INF(f'Running panel "{panel_name}" on monitor {output_name}')
                panel = AriaPanel(panel_name, panel_conf, monitor, self)
                self.panels.setdefault(output_name, []).append(panel)
