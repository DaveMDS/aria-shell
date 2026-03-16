from collections.abc import Callable

from gi.repository import Gtk, Gdk, Gio, GObject
from gi.repository import Gtk4LayerShell as GtkLayerShell


class AriaWindow(Gtk.Window):
    """
    Extend Gtk.Window with GtkLayerShell abilities.

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
    Layer = GtkLayerShell.Layer
    Edge = GtkLayerShell.Edge
    KeyboardMode = GtkLayerShell.KeyboardMode

    def __init__(self,
                 app: Gtk.Application,
                 namespace: str,
                 hide_on_escape: bool = False,
                 layer = Layer.TOP,
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
            #ec.connect('begin', lambda *_: self.hide())
            #self.add_controller(ec)
        else:
            GtkLayerShell.set_keyboard_mode(
                self, GtkLayerShell.KeyboardMode.ON_DEMAND
            )

        if keyboard_mode:
            GtkLayerShell.set_keyboard_mode(self, keyboard_mode)

        if hide_on_escape:
            ec = Gtk.EventControllerKey()
            ec.connect('key-pressed', self._key_pressed)
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

    def _key_pressed(self, _ec: Gtk.EventControllerKey, keyval: int,
                     _keycode: int, _state: Gdk.ModifierType):
        if keyval == Gdk.KEY_Escape:
            self.hide()
            return True  # handled, stop event propagation
        else:
            return False  # not handled, continue propagation


class AriaBox(Gtk.Box):
    """
    Simple GtkBox wrapper
    Use this when your box should be "visible" in css syles.
    """
    def __init__(self, css_class: str = None, **kargs):
        super().__init__(**kargs)
        if css_class:
            self.add_css_class(css_class)


class AriaSlider(Gtk.Scale):
    """
    Don't know why Gtk.Scale doesn't provide the 'value' property.
    This class add the 'value' property, so it can be binded.
    """
    __gtype_name__ = 'AriaSlider'

    @GObject.Property(type=float)
    def value(self):
        return super().get_value()

    @value.setter
    def value(self, value: float):
        super().set_value(value)


class AriaPopup(Gtk.Popover):
    def __init__(self, content: Gtk.Widget, parent: Gtk.Widget,
                       on_destroy: Callable = None):
        super().__init__()
        self._on_destroy = on_destroy
        self.set_parent(parent)
        self.set_child(content)
        self.set_autohide(True)  # TODO config (or pin icon in a corner?)
        self.set_has_arrow(True)  # TODO config
        self.add_css_class('aria-popup')
        self.connect('closed', self._on_closed)
        # self.connect('destroy', lambda _: print("!!! DESTROY !!!  \o/"))
        self.popup()

    def close(self):
        self.popdown()

    def _on_closed(self, _popover):
        if self._on_destroy:
            self._on_destroy(self)
        self.set_child(None)
        self.unparent()
        # self.run_dispose()


#######################################################################
#######################################################################
#######################################################################

class AriaDialog(Gtk.AlertDialog):
    def __init__(self,
                 parent: Gtk.Window,
                 title: str = None,
                 body: str = None,
                 buttons: list[str] = None,
                 callback: Callable[[str, ...], None] = None, **kwargs
                 ):
        super().__init__(
            message=title,
            detail='TODO: Gtk.Alert dialog cannot update this text for the countdown :(',
            buttons=buttons,
            default_button=1,
            cancel_button=0,
            modal=True,
        )

        def _choose_cb(dialog: AriaDialog, result: Gio.AsyncResult):
            btn_id = dialog.choose_finish(result)
            if btn_id and callable(callback):
                callback(f'button-{btn_id+1}', **kwargs)

        self.choose(parent, callback=_choose_cb)

    def close(self):
        pass

    def set_title(self, title: str):
        super().set_message(title)

    def set_body(self, body: str):
        # TODO: questo non funziona...desktop-portal maledetto...  GRRR!!!
        super().set_detail(body)

