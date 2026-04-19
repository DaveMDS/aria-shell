from gi.repository import Gtk, GObject


"""
Dunno why Gtk.Scale doesn't provide the 'value' property.
This class add the 'value' property, so it can be binded.
"""
class AriaSlider(Gtk.Scale):
    __gtype_name__ = 'AriaSlider'

    @GObject.Property(type=float)
    def value(self):
        return super().get_value()

    @value.setter
    def value(self, value: float):
        super().set_value(value)
