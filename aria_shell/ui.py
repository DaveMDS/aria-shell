from collections.abc import Callable

from gi.repository import Gtk, GObject


class AriaWindow(Gtk.Window):
    """
    Base class for all Aria windows
    """
    def __init__(self, css_class: str = None, **kargs):
        super().__init__(**kargs)
        if css_class:
            self.add_css_class(css_class)


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
