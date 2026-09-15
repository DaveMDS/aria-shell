from gi.repository import Gtk, Gdk
from gi.repository import Gtk4LayerShell as GtkLayerShell

from aria_shell.utils import CleanupHelper


"""
Extend Gtk.Window with GtkLayerShell abilities.

NOTE: Always call shutdown() to destroy an AriaWindow !!!!

Args:
    app: the Gtk.Application this window belong
    namespace: unique LayerShell namespace
    hide_on_escape: automatically hide the window pressing Esc
    layer: overlay, top, bottom, background
    anchors: list of Edges to attach the window to
    auto_exclusive_zone: request to not be covered by other windows
    exclusive_zone: pixel to reserve (-1 = don't respect other exclusive)
    margins: margin in pixel (top, right, bottom, left)
    grab_display: disable interaction with other windows
    keyboard_mode: on_demand, exclusive or none
    minitor: show the window on the given Gdk.Monitor
    size_request: requested size for the window
    **kwargs: any other arguments for Gtk.Window

See: https://wmww.github.io/gtk4-layer-shell/
"""
class AriaWindow(CleanupHelper, Gtk.Window):
    Layer = GtkLayerShell.Layer
    Edge = GtkLayerShell.Edge
    KeyboardMode = GtkLayerShell.KeyboardMode

    def __init__(self,
                 app: Gtk.Application = None,
                 namespace: str = 'aria-shell',
                 hide_on_escape: bool = False,
                 layer: Layer = Layer.TOP,
                 anchors: list[Edge] = None,
                 auto_exclusive_zone: bool = False,
                 exclusive_zone: int = 0,
                 margins: tuple[int, int, int, int] = (0, 0, 0, 0),
                 grab_display=False,
                 keyboard_mode: KeyboardMode = None,
                 monitor: Gdk.Monitor = None,
                 size_request: tuple[int, int] = None,
                 **kwargs
                 ):
        super().__init__(**kwargs)
        self.set_application(app)
        self.add_css_class('aria-window')
        self.add_css_class(namespace)

        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_namespace(self, namespace)
        GtkLayerShell.set_layer(self, layer)

        if monitor:
            GtkLayerShell.set_monitor(self, monitor)

        if size_request is not None:
            self.set_size_request(*size_request)

        for anchor in anchors or []:
            GtkLayerShell.set_anchor(self, anchor, True)

        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.TOP, margins[0])
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.RIGHT, margins[1])
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.BOTTOM, margins[2])
        GtkLayerShell.set_margin(self, GtkLayerShell.Edge.LEFT, margins[3])

        if auto_exclusive_zone:
            GtkLayerShell.auto_exclusive_zone_enable(self)
        else:
            GtkLayerShell.set_exclusive_zone(self, exclusive_zone)

        if grab_display:
            GtkLayerShell.set_keyboard_mode(
                self, GtkLayerShell.KeyboardMode.EXCLUSIVE
            )
            # TODO grab mouse also,  and dim the whole bg?
            #ec = Gtk.GestureSingle(button=0)
            #self.safe_connect(ec, 'begin', lambda *_: self.hide())
            #self.add_controller(ec)
        else:
            GtkLayerShell.set_keyboard_mode(
                self, GtkLayerShell.KeyboardMode.ON_DEMAND
            )

        if keyboard_mode:
            GtkLayerShell.set_keyboard_mode(self, keyboard_mode)

        if hide_on_escape:
            ec = Gtk.EventControllerKey()
            self.safe_connect(ec, 'key-pressed', self._key_pressed)
            self.add_controller(ec)

    def show(self):
        """Show the window."""
        super().show()

    def hide(self):
        """Hide the window."""
        super().hide()

    def toggle(self):
        """Toggle window visibility."""
        self.hide() if self.is_visible() else self.show()

    def shutdown(self):
        """Destroy the window."""
        CleanupHelper.shutdown(self)
        Gtk.Window.destroy(self)

    def _key_pressed(self, _ec: Gtk.EventControllerKey, keyval: int,
                     _keycode: int, _state: Gdk.ModifierType):
        if keyval == Gdk.KEY_Escape:
            self.hide()
            return True  # handled, stop event propagation
        else:
            return False  # not handled, continue propagation
