from __future__ import annotations

from collections.abc import Callable

from gi.repository import Gtk


class AriaWindow(Gtk.Window):
    """
    Base class for all Aria windows
    """
    def __init__(self, css_class: str = None, **kargs):
        super().__init__(css_name='Window', **kargs)
        if css_class:
            self.add_css_class(css_class)


class AriaBox(Gtk.Box):
    """
    Simple GtkBox wrapper
    Use this when your box should be "visible" in css syles.
    """
    def __init__(self, css_class: str = None, **kargs):
        super().__init__(css_name='Box', **kargs)
        if css_class:
            self.add_css_class(css_class)


class AriaWidget(Gtk.Box):
    """
    Base class for all widgets
    A widget is an entity that can be placed in panels, docks, etc...
    """
    def __init__(self, name: str):
        super().__init__(css_name='Widget')
        self.add_css_class('aria-widget')
        self.add_css_class(f'widget-{name}')
        self.name = name

    def __repr__(self):
        return f'<AriaWidget {self.name}>'

    # def open_popup(self):
    #     raise NotImplementedError('TODO')


class AriaPopup_Popover:
    def __init__(self, content: Gtk.Widget, parent: Gtk.Widget, on_destroy: Callable = None):
        self._popo = Gtk.Popover()
        self._on_destroy = on_destroy
        self._popo.set_parent(parent)
        self._popo.set_child(content)
        self._popo.set_autohide(True)  # TODO config (general or per panel? per widget?)
        self._popo.set_has_arrow(False) # TODO config

        self._popo.connect('hide', self._on_hide)
        self._popo.connect('destroy', lambda p: print("!!! DESTROY !!!", self))

        # if on_destroy:
        #     popo.connect('hide', lambda p: on_destroy(self))

        # popo.set_pointing_to(parent)
        # popo.set_relative_to(parent)
        # popo.set_default_widget(parent)
        # popo.set_position(Gtk.PositionType.TOP)  # Do not work in sway :(
        # popo.set_constrain_to(Gtk.PopoverConstraint.WINDOW)  # WINDOW / NONE
        # content.show()

        # print(popo.get_toplevel())
        # from gi.repository import GtkLayerShell
        # win = popo.get_toplevel()
        # GtkLayerShell.init_for_window(win)

        # show
        # self._popo.show()  # no animation
        self._popo.popup()  # with animation

    def _on_hide(self, _popo):
        print('_on_hide()')
        self.destroy()

    def destroy(self):
        self._popo.popdown()
        if self._popo:
            print('UNPARENT')
            if self._on_destroy:
                self._on_destroy(self)
            # self._popo.hide()
            self._popo.unparent()
            del self._popo
            self._popo = None


    # def _on_hide(self, popo):
    #     self.destroy()





AriaPopup = AriaPopup_Popover
