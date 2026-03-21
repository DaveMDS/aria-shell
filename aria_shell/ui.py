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
        self._signal_handler = self.connect('closed', self._on_closed)
        # self.connect('destroy', lambda _: print("!!! DESTROY !!!  \o/"))
        self.popup()

    def close(self):
        self.popdown()

    def _on_closed(self, _popover):
        # call user callback
        if self._on_destroy:
            self._on_destroy(self)

        # break reference cycles
        self.disconnect(self._signal_handler)
        self._on_destroy = None

        # unparent
        self.unparent()
        self.set_child(None)
        # self.get_child().unparent()


class AriaDialog(AriaWindow):
    """
    Fake dialog implementation using a normal window and the
    LayerShell for positioning

    NOTE: Still not happy with this, see all the other tests below...

    callback response: 'cancel' or 'button-1' 'button-2' etc...
    """
    def __init__(self,
                 parent: Gtk.Window,
                 heading: str = None,
                 body: str = None,
                 buttons: list[str] = None,
                 callback: Callable[[str, ...], None] = None, **kwargs
                 ):
        super().__init__(
            transient_for=parent,
            namespace='aria-dialog',
            layer=AriaWindow.Layer.OVERLAY,
            grab_display=True,  # TODO giusto??
        )
        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.set_child(vbox)

        self.heading_label = Gtk.Label(label=heading)
        self.heading_label.add_css_class('aria-dialog-heading')
        self.heading_label.add_css_class('title-1')
        vbox.append(self.heading_label)

        self.body_label = Gtk.Label(label=body)
        self.body_label.add_css_class('aria-dialog-body')
        vbox.append(self.body_label)

        if buttons:
            hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL,
                           hexpand=True, halign=Gtk.Align.CENTER)
            vbox.append(hbox)

            def _button_clicked_cb(_btn: Gtk.Button, btn_id: str):
                if callable(callback):
                    callback(btn_id, **kwargs)
                self.close()

            for i, label in enumerate(buttons, 1):
                button = Gtk.Button(label=label)
                button.add_css_class('aria-dialog-button')
                button.connect('clicked', _button_clicked_cb, f'button-{i}')
                hbox.append(button)

        def _key_pressed_cb(_ec: Gtk.EventControllerKey, keyval: int,
                            _keycode: int, _state: Gdk.ModifierType):
            if keyval == Gdk.KEY_Escape:
                if callable(callback):
                    callback('cancel', **kwargs)
                return True  # handled, stop event propagation
            else:
                return False  # not handled, continue propagation

        ec = Gtk.EventControllerKey()
        ec.connect('key-pressed', _key_pressed_cb)
        self.add_controller(ec)

        self.show()

    def set_heading(self, text: str):
        self.heading_label.set_label(text)

    def set_body(self, text: str):
        self.body_label.set_label(text)

    def close(self):
        super().close()

'''
class AriaDialogGTK(Gtk.AlertDialog):
    """
    THIS SHOULD BE THE ONE TO USE
    SADLY DOES NOT SUPPORT BODY CHANGE AT RUNTIME !!!
    """
    def __init__(self,
                 parent: Gtk.Window,
                 heading: str = None,
                 body: str = None,
                 buttons: list[str] = None,
                 callback: Callable[[str, ...], None] = None, **kwargs
                 ):
        super().__init__(
            message=heading,
            detail='TODO: Gtk.Alert dialog cannot update this text for the countdown :(',
            buttons=buttons,
            modal=True,
        )

        def _choose_cb(dialog: AriaDialogGTK, result: Gio.AsyncResult):
            try:
                btn_num = dialog.choose_finish(result)
                btn_id = f'button-{btn_num+1}'
            except GLib.GError:
                btn_id = 'cancel'
            if callable(callback):
                callback(btn_id, **kwargs)

        self._cancellable = Gio.Cancellable()
        self.choose(parent, callback=_choose_cb, cancellable=self._cancellable)

    def set_heading(self, text: str):
        super().set_message(text)

    def set_body(self, text: str):
        # TODO: questo non funziona...desktop-portal maledetto...  GRRR!!!
        # super().set_detail(body)
        pass

    def close(self):
        if not self._cancellable.is_cancelled():
            self._cancellable.cancel()
'''



# class AriaDialogGTKDeprecated(Gtk.MessageDialog):
#     def __init__(self,
#                  parent: Gtk.Window,
#                  title: str = None,
#                  body: str = None,
#                  buttons: list[str] = None,
#                  callback: Callable = None  # TODO type
#                  ):
#         # super().__init__(
#         Gtk.MessageDialog.__init__(
#             self,
#             # parent=parent,
#             use_header_bar=1,
#             use_markup=False,
#
#             text=title,
#             secondary_text=body,
#             # buttons=buttons,
#             # default_button=0,
#             # cancel_button=1,
#             modal=True,
#         )
#         self.show()
#
#     @property
#     def title(self):
#         return self.text
#
#     @title.setter
#     def title(self, title: str):
#         # NON FUNZIONA !!!!!!!!1111
#         self.set_markup(title)
#
#     @property
#     def body(self):
#         return self.get_detail()
#
#     @body.setter
#     def body(self, body: str):
#         # NON FUNZIONA !!!!!!!!1
#         self.text = body
#         # self.set_secondary_text(body)
#         # self.set_mark
#
#
#
# import gi
# gi.require_version('Adw', '1')  # TODO move in main
# from gi.repository import Adw
#
# class AriaDialogAdw(Adw.AlertDialog):
#     """TODO DOC"""
#     def __init__(self,
#                  parent: Gtk.Window,
#                  heading: str = None,
#                  body: str = None,
#                  buttons: list[str] = None,
#                  callback: Callable[[str, ...], None] = None, **kwargs
#                  ):
#         super().__init__()
#         if heading:
#             super().set_heading(heading)
#         if body:
#             super().set_body(body)
#
#         for i, label in enumerate(buttons or []):
#             self.add_response(f'button-{i+1}', label)
#
#         def _choose_cb(dialog: AriaDialogAdw, result: Gio.AsyncResult):
#             btn_id = dialog.choose_finish(result)
#             if btn_id and callable(callback):
#                 callback(btn_id, **kwargs)
#
#         self.choose(parent, callback=_choose_cb)
#
#     def set_heading(self, text: str | None = None):
#         super().set_heading(text)
#
#     def set_body__(self, body: str):
#         super().set_body(body)
