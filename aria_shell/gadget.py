from gi.repository import Gtk, GObject

from aria_shell.utils import CleanupHelper


class AriaGadget(CleanupHelper, Gtk.Box):
    """
    Base class for all gadgets
    A gadget is a Gtk.Widget that can be placed in panels, docks, etc...
    """
    def __init__(self, name: str, clickable=False):
        super().__init__(css_name='Gadget')
        self.add_css_class('aria-gadget')
        self.add_css_class(f'gadget-{name}')
        self.name = name

        # keep track of signals registered using safe_connect()
        self._signal_handlers: list[tuple[GObject.Object, int]] = []

        if clickable:
            self.set_cursor_from_name('pointer')
            # EventController to receive mouse events
            ec = Gtk.GestureSingle(button=0)
            def _on_click(_ec: Gtk.GestureSingle, _):
                self.mouse_click(_ec.get_current_button())
            self.safe_connect(ec, 'begin', _on_click)
            self.add_controller(ec)

    def __repr__(self):
        return f'<AriaGadget {self.name}>'

    def shutdown(self):
        """Called before removing the gadget from the bar."""
        super().shutdown()

    def mouse_click(self, button: int):
        """Called on every mouse click."""
        raise NotImplementedError(f'{self} must implement mouse_click()')

    # def open_popup(self):
    #     raise NotImplementedError('TODO')
