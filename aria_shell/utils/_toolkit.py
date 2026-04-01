from collections.abc import Callable
from typing import TypeVar, Iterator, Hashable, Any, Iterable

from gi.repository import Gio, GObject


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
