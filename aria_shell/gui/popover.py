from collections.abc import Callable

from gi.repository import Gtk, Gio, GLib


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
class AriaPopover:
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
