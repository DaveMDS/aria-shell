from gi.repository import Gtk


class AriaGadget(Gtk.Box):
    """
    Base class for all gadgets
    A gadget is a Gtk.Widget that can be placed in panels, docks, etc...
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

    def destroy(self):
        print('DESTROY NOT IMPLEMENTED !!!!', self)
    def mouse_click(self, button: int):
        """Called on every mouse click."""
        raise NotImplementedError(f'{self} must implement mouse_click()')

    def _on_mouse_down(self, ec: Gtk.GestureSingle, _):
        self.mouse_click(ec.get_current_button())

    # def open_popup(self):
    #     raise NotImplementedError('TODO')
