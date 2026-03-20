from gi.repository import Gdk, Gtk

from aria_shell.ui import AriaBox, AriaWindow
from aria_shell.utils import clamp
from aria_shell.module import request_module_gadget, destroy_module_gadget
from aria_shell.config import AriaConfigModel
from aria_shell.utils.logger import get_loggers


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
    outputs: list[str] = 'all'
    position: str = 'top'
    layer: str = 'bottom'
    size: str = 'fill'
    align: str = 'center'
    margin: int = 0
    opacity: int = 80
    ontheleft: list[str] = []
    inthecenter: list[str] = []
    ontheright: list[str] = []

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


class AriaPanel(AriaWindow):
    def __init__(self, name: str, conf: PanelConfig, monitor: Gdk.Monitor, app):
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
            # decorated=False,
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

    def destroy(self):
        INF('Removing panel %s', self)

        # clear the 3 boxes (unparent all gadgets)
        for box in (self._box1, self._box2, self._box3):
            for gadget in list(box or []):
                destroy_module_gadget(gadget)
                box.remove(gadget)

        # destroy the Gtk.Window
        super().destroy()

    def setup_window(self):
        # create the left/center/right boxes, in a CenterBox
        cbox = Gtk.CenterBox()
        cbox.add_css_class('aria-panel-box')
        self._box1 = AriaBox(css_class='aria-panel-box-start')
        self._box2 = AriaBox(css_class='aria-panel-box-center')
        self._box3 = AriaBox(css_class='aria-panel-box-end')
        cbox.set_start_widget(self._box1)
        cbox.set_center_widget(self._box2)
        cbox.set_end_widget(self._box3)
        self.set_child(cbox)

    def populate(self):
        # add a clock in the center for empty configs
        if not self.conf.ontheleft and not self.conf.ontheright and not self.conf.inthecenter:
            self.conf.inthecenter = ['Clock']

        # populate box1 (start)
        for module_name in self.conf.ontheleft:
            if gadget := request_module_gadget(module_name, self.monitor):
                self._box1.append(gadget)

        # populate box2 (center)
        for module_name in self.conf.inthecenter:
            if gadget := request_module_gadget(module_name, self.monitor):
                self._box2.append(gadget)

        # populate box3 (end)
        for module_name in self.conf.ontheright:
            if gadget := request_module_gadget(module_name, self.monitor):
                self._box3.append(gadget)
