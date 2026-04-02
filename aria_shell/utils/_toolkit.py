from collections.abc import Callable
from typing import TypeVar, Iterator, Hashable, Any, Iterable

from gi.repository import Gio, GObject


################################################################################
class CleanupHelper:
    """
    When subclassing GObject the lifetime is not tied to the python object,
    signal connections held references to cb and args that will make the
    python object stay alive forever!

    The trick to manage this lifetime difference seems to be to always cleanup
    all connected signals and bindings (they hold ref to cb and args). This
    means we need an explicit shutdown() function in every object.

    Can be used as the primary superclass of a GObject subclass, es:
        class MyWidget(CleanupHelper, Gtk.Box):
            def __init__(...):
                super().__init__(...)
                self.safe_connect(obj, 'signal', self._callback1)
                self.safe_bind(obj1, 'prop1', obj2, 'prop2',...)

            def shutdown():
                # shutdown automatically disconnect all safe-connected signals
                super().shutdown()

    Or used as a simple standalone helper:
        helper = CleanupHelper()
        helper.safe_connect(obj, 'signal', _callback)
        ...
        helper.shutdown()  # to disconnect all signals and bindings

    """
    def __init__(self, **kwargs):
        super().__init__(**kwargs)  # cooperate with other parents (es: GObject)
        self._signal_handlers: list[tuple[GObject.Object, int]] = []
        self._bindings: list[GObject.Binding] = []

    def safe_connect(self, obj: GObject.Object, signal: str, cb: Callable, *a):
        """Connect a signal that will be automatically disconnected on shutdown."""
        handler = obj.connect(signal, cb, *a)
        self._signal_handlers.append((obj, handler))

    def safe_bind(self,
                  source: GObject.Object, source_property: str,
                  target: GObject.Object, target_property: str,
                  flags: GObject.BindingFlags = GObject.BindingFlags.SYNC_CREATE,
                  transform_to: Callable[[GObject.Binding, ...], Any] | None = None,
                  transform_from: Callable[[GObject.Binding, ...], Any] | None = None,
                  user_data: Any = None):
        """Bind a property that will be automatically unbinded on shutdown."""
        binding = source.bind_property(
            source_property, target, target_property,
            flags, transform_to, transform_from, user_data
        )
        self._bindings.append(binding)

    def shutdown(self):
        """Disconnect all connected signals and unbind all bindings."""
        # print('CleanupHelper shutdown', self)
        for obj, handler in self._signal_handlers:
            # print('  -> disconnect', obj, handler)
            obj.disconnect(handler)
        self._signal_handlers.clear()

        for binding in self._bindings:
            # print('  -> unbind', binding)
            binding.unbind()
        self._bindings.clear()


################################################################################
ItemObjectT = TypeVar('ItemObjectT', bound=GObject.Object)
ItemKeyT = TypeVar('ItemKeyT', bound=Hashable)
ItemCompareFunc = Callable[[ItemObjectT, ItemObjectT, Any], int] # this is wrong also in gio?

class IndexedListStore[ItemObjectT, KeyPropT=str](Gio.ListStore):
    """
    A Gio.ListStore that keep a dict index for the items,
    and factorize some repetitive remove operations.

    The index key is taken from a property of the item itself, by
    default the 'id' property is used. For better typing you can also
    specify the type of the key property (str by default).

    Args:
        item_type: the type of the items in the store
        key_prop: name of the item property to use as index (default: id)
        key_type: type of the key property (default: str)
    """
    def __init__(self, *,
                 item_type: type[ItemObjectT],
                 key_prop: str = 'id',
                 key_type: type[KeyPropT] = str,
                 ):
        super().__init__(item_type=item_type)
        self._index: dict[KeyPropT, ItemObjectT] = {}
        self._key_prop = key_prop
        self._key_type = key_type

    def __iter__(self) -> Iterator[ItemObjectT]:
        return super().__iter__()

    def get_item(self, position: int) -> ItemObjectT | None:
        """Get the item at position."""
        return super().get_item(position)

    def get(self, key: KeyPropT) -> ItemObjectT | None:
        """Get the item with the given index key (id by default)."""
        return self._index.get(key, None)

    def keys(self) -> Iterable[KeyPropT]:
        """Return a set-like object providing a view on the index keys."""
        return self._index.keys()

    def _gkey(self, item: ItemObjectT) -> KeyPropT:
        """Return the index key to be used for item (id by default)."""
        return getattr(item, self._key_prop)

    def append(self, item: ItemObjectT):
        """Append item to the store."""
        super().append(item)
        self._index[self._gkey(item)] = item

    def insert(self, position: int, item: ItemObjectT):
        """Insert item into store at position."""
        super().insert(position, item)
        self._index[self._gkey(item)] = item

    def insert_sorted(self, item: ItemObjectT, compare: ItemCompareFunc, *args):
        super().insert_sorted(item, compare, *args)
        self._index[self._gkey(item)] = item

    def remove(self, position: int):
        """Removes the item at position from the store. FAST."""
        item = self.get_item(position)
        del self._index[self._gkey(item)]
        super().remove(position)

    def remove_item(self, item: ItemObjectT):
        """Remove the item from the store. SLOW."""
        found, position = self.find(item)
        if found:
            del self._index[self._gkey(item)]
            super().remove(position)

    def remove_key(self, key: KeyPropT):
        """Remove the item with the given key from the store. SLOW."""
        if item := self._index.get(key):
            self.remove_item(item)

    def remove_all(self):
        """Remove all items from the store."""
        super().remove_all()
        self._index.clear()
