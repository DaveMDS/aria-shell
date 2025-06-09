from __future__ import annotations
from typing import Mapping, Optional

from gi.repository import Gdk, Gtk, Gtk4LayerShell as GtkLayerShell  # noqa

from aria_shell.ui import AriaBox, AriaWindow
from aria_shell.utils import clamp
from aria_shell.module import request_module_gadget
from aria_shell.config import AriaConfigModel
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


POSITIONS = {
    'top': GtkLayerShell.Edge.TOP,
    'bottom': GtkLayerShell.Edge.BOTTOM,
}

LAYERS = {
    'top': GtkLayerShell.Layer.TOP,
    'bottom': GtkLayerShell.Layer.BOTTOM,
    'overlay': GtkLayerShell.Layer.OVERLAY,
}

SIZES = {
    'fill',
    'min',
}


class PanelConfig(AriaConfigModel):
    outputs: list[str] = 'all'
    position: str = 'top'
    layer: str = 'bottom'
    size: str = 'fill'
    align: str = 'center'
    margin: int = 0
    spacing: int = 6
    opacity: int = 80
    ontheleft: list[str] = []
    inthecenter: list[str] = []
    ontheright: list[str] = []

    @staticmethod
    def validate_opacity(val: int):
        return clamp(val, 0, 100)

    @staticmethod
    def validate_spacing(val: int):
        return clamp(val, 0, 9999)

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


class AriaPanel(AriaWindow):
    def __init__(self, name: str, user_settings: Mapping[str, str], monitor: Gdk.Monitor, app):
        super().__init__(title='Aria panel')
        self.set_application(app)
        self._box1: Optional[Gtk.Box] = None
        self._box2: Optional[Gtk.Box] = None
        self._box3: Optional[Gtk.Box] = None

        self.name = name
        self.monitor = monitor
        self.conf = PanelConfig(user_settings)
        self.setup_window()
        self.populate()

        # self.connect('destroy', self.on_destroy)
        self.show()

    def __repr__(self):
        return f'<AriaPanel name="{self.name}" on="{self.monitor.get_connector()}">'

    def setup_window(self):
        # configure the window
        self.set_opacity(self.conf.opacity / 100.0)
        self.set_decorated(False)
        self.add_css_class('aria-panel')

        if self.conf.size == 'fill':
            geom = self.monitor.get_geometry()
            self.set_size_request(geom.width, -1)

        # GtkLayerShell stuff (this is the only Wayland code for now)
        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.NONE)
        GtkLayerShell.set_monitor(self, self.monitor)
        GtkLayerShell.set_namespace(self, 'aria-panel')
        GtkLayerShell.set_layer(self, LAYERS.get(self.conf.layer))
        GtkLayerShell.set_anchor(self, POSITIONS.get(self.conf.position), True)
        if self.conf.size == 'fill':
            GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.LEFT, True)
            GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.RIGHT, True)
        if self.conf.margin and self.conf.position == 'top':
            GtkLayerShell.set_margin(self, GtkLayerShell.Edge.BOTTOM, self.conf.margin)
        if self.conf.margin and self.conf.position == 'bottom':
            GtkLayerShell.set_margin(self, GtkLayerShell.Edge.TOP, self.conf.margin)
        GtkLayerShell.auto_exclusive_zone_enable(self)
        # End Wayland code

        # create the left/center/right boxes, in a CenterBox
        cbox = Gtk.CenterBox()
        cbox.add_css_class('aria-panel-box')
        s = self.conf.spacing
        self._box1 = AriaBox(css_class='aria-panel-box-start', spacing=s)
        self._box2 = AriaBox(css_class='aria-panel-box-center', spacing=s)
        self._box3 = AriaBox(css_class='aria-panel-box-end', spacing=s)
        cbox.set_start_widget(self._box1)
        cbox.set_center_widget(self._box2)
        cbox.set_end_widget(self._box3)
        self.set_child(cbox)

    def populate(self):
        # add a clock in the center for empty configs
        if not self.conf.ontheleft and not self.conf.ontheright and not self.conf.inthecenter:
            self.conf.inthecenter = ['clock']
        # populate left
        for module_name in self.conf.ontheleft:
            if gadget := request_module_gadget(module_name, self.monitor):
                self._box1.append(gadget)
        # populate center
        for module_name in self.conf.inthecenter:
            if gadget := request_module_gadget(module_name, self.monitor):
                self._box2.append(gadget)
        # populate right
        for module_name in self.conf.ontheright:
            if gadget := request_module_gadget(module_name, self.monitor):
                self._box3.append(gadget)

    def destroy(self):
        DBG(f'panel destroy {self}')
        # TODO more cleanups? remove all gadgets?
        super().destroy()