from gi.repository import Gtk


class AriaGadget(Gtk.Box):
    """
    Base class for all gadgets
    A gadget is an entity that can be placed in panels, docks, etc...
    """
    def __init__(self, name: str, clickable=False):
        super().__init__(css_name='Gadget')
        self.add_css_class('aria-gadget')
        self.add_css_class(f'gadget-{name}')
        self.name = name

        if clickable:
            # EventController to receive mouse events
            ec = Gtk.GestureSingle(button=0)
            ec.connect('begin', self._on_mouse_down)
            self.add_controller(ec)
            self.set_cursor_from_name('pointer')

    def __repr__(self):
        return f'<AriaGadget {self.name}>'

    def _on_mouse_down(self, ec: Gtk.GestureSingle, _):
        self.on_mouse_down(ec.get_current_button())

    def on_mouse_down(self, button: int):
        raise NotImplementedError('Gadget must implement on_mouse_down')

    # def open_popup(self):
    #     raise NotImplementedError('TODO')
