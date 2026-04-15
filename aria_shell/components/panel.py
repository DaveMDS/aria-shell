from typing import TYPE_CHECKING

from gi.repository import Gdk, Gtk

from aria_shell.components import AriaComponent
from aria_shell.services.display import DisplayService
from aria_shell.ui import AriaWindow
from aria_shell.utils import clamp
from aria_shell.module import request_module_gadget, destroy_module_gadget
from aria_shell.config import AriaConfigModel, AriaConfig
from aria_shell.utils.logger import get_loggers
if TYPE_CHECKING:
    from aria_shell.ariashell import AriaShell


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


POSITIONS = {
    'top': AriaWindow.Edge.TOP,
    'bottom': AriaWindow.Edge.BOTTOM,
}

LAYERS = {
    'top': AriaWindow.Layer.TOP,
    'bottom': AriaWindow.Layer.BOTTOM,
    'overlay': AriaWindow.Layer.OVERLAY,
}

SIZES = {
    'fill',
    'min',
}


class PanelConfig(AriaConfigModel):
    """The configuration model for a single panel."""
    outputs: list[str] = 'all'
    position: str = 'top'
    layer: str = 'bottom'
    size: str = 'fill'
    align: str = 'center'
    margin: int = 0
    opacity: int = 80
    items_start: list[str] = []
    items_center: list[str] = []
    items_end: list[str] = []

    @staticmethod
    def validate_opacity(val: int):
        return clamp(val, 0, 100)

    @staticmethod
    def validate_margin(val: int):
        return clamp(val, 0, 9999)

    @staticmethod
    def validate_size(val: str):
        if val not in SIZES:
            raise ValueError(f'Invalid size "{val}" for panel. '
                             'Allowed values: ' + ','.join(SIZES))
        return val

    @staticmethod
    def validate_position(val: str):
        if val not in POSITIONS:
            raise ValueError(f'Invalid position "{val}" for panel. '
                             'Allowed values: ' + ','.join(POSITIONS.keys()))
        return val

    @staticmethod
    def validate_layer(val: str):
        if val not in LAYERS:
            raise ValueError(f'Invalid layer "{val}" for panel. '
                             'Allowed values: ' + ','.join(LAYERS.keys()))
        return val


class AriaPanels(AriaComponent):
    """
    This is the manager component that keep track of connected monitors
    and create / destroy the needed panels.
    """
    def __init__(self, app: AriaShell):
        super().__init__(app)

        # keep track of alive panels, ex: {'DP-1': [Panel, Panel, ..]}
        self.panels: dict[str, list[AriaPanel]] = {}

        # stay informed about changed monitors
        ds = DisplayService()
        ds.connect('monitor-added', self._on_monitor_added)
        ds.connect('monitor-removed', self._on_monitor_removed)

        # inspect connected monitors, and create needed panels
        for monitor in ds.monitors:
            self._on_monitor_added(monitor)

    def shutdown(self):
        ds = DisplayService()
        ds.disconnect('monitor-added', self._on_monitor_added)
        ds.disconnect('monitor-removed', self._on_monitor_removed)
        for monitor, panels in self.panels.items():
            for panel in panels:
                panel.shutdown()
        self.panels.clear()

    def _on_monitor_added(self, monitor: Gdk.Monitor):
        if output_name := monitor.get_connector():
            INF('Monitor connected %s', output_name)
            self._create_panels_for_monitor(monitor)
        else:
            CRI('Cannot find monitor name for monitor %s', monitor)

    def _on_monitor_removed(self, monitor: Gdk.Monitor):
        name = monitor.get_connector()
        INF('Monitor disconnected %s', name)
        for panel in self.panels.pop(name, []):
            panel.shutdown()

    def _create_panels_for_monitor(self, monitor: Gdk.Monitor):
        output_name = monitor.get_connector()
        config = AriaConfig()
        for section in sorted(config.sections('panel')):
            panel_conf = config.section(section, PanelConfig)
            outputs = panel_conf.outputs
            if (not outputs) or ('all' in outputs) or (output_name in outputs):
                if ':' in section and not section.endswith(':'):
                    panel_name = section.split(':')[1]
                else:
                    panel_name = 'Aria Panel'

                panel = AriaPanel(panel_name, panel_conf, monitor, self.app)
                self.panels.setdefault(output_name, []).append(panel)


class AriaPanel(AriaWindow):
    """
    A single AriaWindow for a single PanelConfig on the given monitor.
    """
    def __init__(self, name: str, conf: PanelConfig, monitor: Gdk.Monitor, app: AriaShell):
        INF('Creating Aria Panel "%s" on monitor %s', name, monitor.get_connector())

        anchors = [POSITIONS.get(conf.position)]
        if conf.size == 'fill':
            anchors.extend([AriaWindow.Edge.LEFT, AriaWindow.Edge.RIGHT])
        elif conf.size == 'min' and conf.align == 'left':
            anchors.append(AriaWindow.Edge.LEFT)
            # TODO: this is only in GtkLayerShell >= 1.4 (not yet released)
            #  should fix the auto-exclusive zone not working in the corners
            #  GtkLayerShell.set_exclusive_edge_enabled(self, GtkLayerShell.Edge.TOP, True)
        elif conf.size == 'min' and conf.align == 'right':
            anchors.append(AriaWindow.Edge.RIGHT)

        margins = (  # top, right, bottom, left
            conf.margin if conf.margin and conf.position == 'bottom' else 0,
            0,
            conf.margin if conf.margin and conf.position == 'top' else 0,
            0,
        )

        super().__init__(
            app=app,
            namespace='aria-panel',
            title='Aria panel',
            layer=LAYERS.get(conf.layer),
            anchors=anchors,
            margins=margins,
            auto_exclusive_zone=True,
            keyboard_mode=AriaWindow.KeyboardMode.NONE,
            monitor=monitor,
            opacity=conf.opacity / 100.0,
        )
        self._box1: Gtk.Box | None = None
        self._box2: Gtk.Box | None = None
        self._box3: Gtk.Box | None = None

        self.name = name
        self.conf = conf
        self.monitor = monitor
        self.setup_window()
        self.populate()

        # self.connect('destroy', self.on_destroy)
        self.show()

    def __repr__(self):
        return f'<AriaPanel name="{self.name}" on="{self.monitor.get_connector()}">'

    def shutdown(self):
        INF('Removing panel %s', self)

        # clear the 3 boxes (unparent all gadgets)
        for box in (self._box1, self._box2, self._box3):
            for gadget in list(box or []):
                destroy_module_gadget(gadget)
                box.remove(gadget)

        # destroy the AriaWindow
        super().shutdown()

    def setup_window(self):
        # create the left/center/right boxes, in a CenterBox
        cbox = Gtk.CenterBox()
        cbox.add_css_class('aria-panel-box')
        self._box1, self._box2, self._box3 = Gtk.Box(), Gtk.Box(), Gtk.Box()
        self._box1.add_css_class('aria-panel-box-start')
        self._box2.add_css_class('aria-panel-box-center')
        self._box3.add_css_class('aria-panel-box-end')
        cbox.set_start_widget(self._box1)
        cbox.set_center_widget(self._box2)
        cbox.set_end_widget(self._box3)
        self.set_child(cbox)

    def populate(self):
        # add a clock in the center for empty configs
        if not self.conf.items_start and not self.conf.items_center and not self.conf.items_end:
            self.conf.items_center = ['Clock']

        # populate box1 (start)
        for module_name in self.conf.items_start:
            if gadget := request_module_gadget(module_name, self.monitor):
                self._box1.append(gadget)

        # populate box2 (center)
        for module_name in self.conf.items_center:
            if gadget := request_module_gadget(module_name, self.monitor):
                self._box2.append(gadget)

        # populate box3 (end)
        for module_name in self.conf.items_end:
            if gadget := request_module_gadget(module_name, self.monitor):
                self._box3.append(gadget)
