from collections.abc import Callable

from gi.repository import Gtk, Gio


"""
A simple Gtk.Box that can be binded to a Gio.ListModel.
Use the provided factory function to create the widgets on demand.

Works like Gtk.FlowBox/ListBox but it's cheaper and don't wrap each
child in an intermediate BoxRow object!

NOTE: Don't use for huge lists.
"""
class AriaBox(Gtk.Box):
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
