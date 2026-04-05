from collections.abc import Callable

from gi.repository import Gtk, Gdk, Gio, GLib, GObject
from gi.repository import Gtk4LayerShell as GtkLayerShell

from aria_shell.utils import CleanupHelper


class AriaWindow(CleanupHelper, Gtk.Window):
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


class AriaBox(Gtk.Box):
    """
    A simple Gtk.Box that can be binded to a Gio.ListModel.
    Use the provided factory function to create the widgets on demand.

    Works like Gtk.FlowBox/ListBox but it's cheaper and don't wrap each
    child in an intermediate BoxRow object!

    NOTE: Don't use for huge lists.
    """
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self._model: Gio.ListModel | None = None
        self._factory: tuple[Callable[..., Gtk.Widget], tuple] | None = None
        self._childs: list[Gtk.Widget] = []

    def do_unmap(self):
        self.unbind_model()
        Gtk.Box.do_unmap(self)

    def bind_model(self,
                   model: Gio.ListModel,
                   widget_factory: Callable[..., Gtk.Widget],
                   *factory_args):
        """Bind to the given model. Previous binding is destroyed."""
        self.unbind_model()
        self._model = model
        self._factory = (widget_factory, factory_args)
        # populate the box and stay informed about added / removed items
        self._on_model_changed(model, 0, 0, len(model))
        model.connect('items-changed', self._on_model_changed)

    def unbind_model(self):
        """Remove the connection with the model and clear all the items."""
        if self._model is not None:
            self._model.disconnect_by_func(self._on_model_changed)
            self._childs.clear()
            self._factory = None
            self._model = None

    def _on_model_changed(self, model: Gio.ListModel,
                          position: int, removed: int, added: int):
        """Create/release children while the model changes."""
        for i in range(added):
            item = model.get_item(position + i)
            func, args = self._factory
            child = func(item, *args)
            self._insert_child_at_pos(position + i, child)

        for i in range(removed):
            child = self._childs[position]
            self.remove(child)
            self._childs.remove(child)

    def _insert_child_at_pos(self, position: int, child: Gtk.Widget):
        """Insert the created child at the given position."""
        if position == 0:
            self.prepend(child)
            self._childs.insert(0, child)

        elif position >= len(self._childs):
            self.append(child)
            self._childs.append(child)

        else:
            after = self._childs[position]
            self.insert_child_after(child, after)
            self._childs.insert(position, child)


class AriaPopover:
    """
    A popup menu that can be attached to any 'parent' Gtk.Widget.

    Popup content can be a manually created Gtk.Widget, or you can
    pass a Gio.MenuModel for an automatic menu in the popup.

    This widget is 'one-shot', create one and forget.
    To show again create a new one.

    Args:
        parent: a Gtk.Widget to use as reference for popup positioning
        content: a Gtk.Widget or a Gio.Menu to show in the popup
        callback: user function will be called when the popup is closed
        *args **kwargs: any other args will be passed back in callback
    """
    def __init__(self,
                 parent: Gtk.Widget,
                 content: Gtk.Widget | Gio.MenuModel = None,
                 callback: Callable[..., None] = None,
                 *args, **kwargs):
        if isinstance(content, Gio.MenuModel):
            popover = Gtk.PopoverMenu.new_from_model(content)
        else:
            popover = Gtk.Popover()
            popover.set_child(content)

        popover.set_parent(parent)
        popover.add_css_class('aria-popover')
        popover.set_autohide(True)  # TODO config (or pin icon in a corner?)
        popover.set_has_arrow(True)  # TODO config
        popover.popup()

        self._signal_handler = popover.connect('closed', self._on_closed)
        # popover.connect('destroy', lambda _: print('DESTROY !!!!  \o/'))
        self._cb_info = (callback, args, kwargs)
        self._popover = popover

    def popdown(self):
        """Close the popover."""
        self._popover.popdown()

    def _on_closed(self, _popover):
        # destroy on next tick, to let the menu actions execute...
        GLib.idle_add(self._on_closed_delayed)

    def _on_closed_delayed(self) -> bool:
        # call user callback if provided
        callback, args, kwargs = self._cb_info
        if callable(callback):
            callback(self, *args, **kwargs)

        # release children from the popover
        if isinstance(self._popover, Gtk.PopoverMenu):
            self._popover.set_menu_model(None)
        else:
            self._popover.set_child(None)

        # remove the popover from the parent widget
        self._popover.unparent()

        # release all references
        self._popover.disconnect(self._signal_handler)
        self._cb_info = None
        self._popover = None

        return False


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
                self.shutdown()

            for i, label in enumerate(buttons, 1):
                button = Gtk.Button(label=label)
                button.add_css_class('aria-dialog-button')
                self.safe_connect(
                    button, 'clicked', _button_clicked_cb, f'button-{i}'
                )
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
