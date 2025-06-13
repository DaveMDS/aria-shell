"""

TODO DOC a little bit

"""
from dasbus.server.interface import dbus_interface, dbus_signal
from dasbus.server.interface import accepts_additional_arguments
from dasbus.client.observer import DBusObserver
from dasbus.typing import Bool, Int, Str, List
from dasbus.connection import SessionMessageBus

from gi.repository import Gtk, GObject, Gio

from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule, GadgetRunContext
from aria_shell.config import AriaConfigModel
from aria_shell.gadget import AriaGadget


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class TrayConfigModel(AriaConfigModel):
    pass


class TrayModule(AriaModule):
    config_model_class = TrayConfigModel

    def __init__(self):
        super().__init__()
        self.snw: StatusNotifierWatcher | None = None

    def module_init(self):
        super().module_init()

        # create the StatusNotifierWatcher (with a fake Host)
        self.snw = StatusNotifierWatcher()


    def module_shutdown(self):
        if self.snw:
            self.snw.shutdown()
        super().module_shutdown()

    def gadget_new(self, ctx: GadgetRunContext) -> AriaGadget | None:
        super().gadget_new(ctx)
        conf: TrayConfigModel = ctx.config  # noqa
        return TrayGadget(conf)


class TrayGadget(AriaGadget):
    def __init__(self, conf: TrayConfigModel):
        super().__init__('tray', clickable=True)

        lbl = Gtk.Label(label='Tray')
        self.append(lbl)

        factory = Gtk.SignalListItemFactory()
        factory.connect("setup", self._factory_item_setup)
        factory.connect("bind", self._factory_item_bind)
        model = Gtk.SingleSelection.new(ITEMS_STORE)
        list_view = Gtk.ListView(model=model, factory=factory,
                                 orientation=Gtk.Orientation.HORIZONTAL)
        self.append(list_view)

    def _factory_item_setup(self, factory, list_item):
        print("setup", list_item, self)
        lbl = Gtk.Label(label='asd')
        list_item.set_child(lbl)

    def _factory_item_bind(self, factory, list_item):
        print("bind", list_item, self)
        item = list_item.get_item()
        label = list_item.get_child()
        label.set_label(item.id)

    def on_mouse_down(self, button: int):
        print('click:', button)


################################################################################

################################################################################

################################################################################


SESSION_BUS = SessionMessageBus()

STATUS_NOTIFIER_WATCHER_IFACE = 'org.kde.StatusNotifierWatcher'
STATUS_NOTIFIER_WATCHER_SERVICE = 'org.kde.StatusNotifierWatcher'
STATUS_NOTIFIER_WATCHER_PATH = '/StatusNotifierWatcher'


class StatusNotifierItem(GObject.Object):
    """ TODO IMPLEMENT """
    def __init__(self, full_path: str):
        super().__init__()

        # :1.12520/org/ayatana/NotificationItem/nm_applet
        i = full_path.index('/')
        bus_name, object_path = full_path[:i], full_path[i:]

        self._proxy = SESSION_BUS.get_proxy(
            bus_name,  # service_name  es :1.4
            object_path,  # object_path
            'org.kde.StatusNotifierItem', # interface_name
        )

    @property
    def id(self) -> str:
        return self._proxy.Id


ITEMS_STORE = Gio.ListStore(item_type=StatusNotifierItem)


@dbus_interface(STATUS_NOTIFIER_WATCHER_IFACE)
class StatusNotifierWatcher(object):
    """ Implementation of the Watcher with a fake Host """

    def __init__(self):
        DBG(f'TRAY Publishing {STATUS_NOTIFIER_WATCHER_SERVICE} on D-Bus')

        # index of registered sni items, by full_path
        self._items: dict[str, StatusNotifierItem] = {}

        # publish self on the bus, and register the service name
        try:
            SESSION_BUS.publish_object(STATUS_NOTIFIER_WATCHER_PATH, self)
            SESSION_BUS.register_service(STATUS_NOTIFIER_WATCHER_SERVICE)
        except Exception as e:
            ERR(f'Cannot register DBUS service: {STATUS_NOTIFIER_WATCHER_SERVICE}.'
                f'Error: {e}. Is another systray running?')

        # tell the world we are ready to accept items
        self.StatusNotifierHostRegistered.emit()

    def shutdown(self):
        # tell the world we are going away
        self.StatusNotifierHostUnregistered.emit()
        # remove self from the bus
        SESSION_BUS.unregister_service(STATUS_NOTIFIER_WATCHER_SERVICE)
        SESSION_BUS.unpublish_object(STATUS_NOTIFIER_WATCHER_PATH)

    @accepts_additional_arguments
    def RegisterStatusNotifierItem(self, service: Str, *, call_info: dict) -> None:
        """ Register a StatusNotifierItem into the StatusNotifierWatcher """
        DBG(f'TRAY RegisterStatusNotifierItem({service})')
        sender = call_info['sender']

        if service.startswith('/'):
            # es: "/org/ayatana/NotificationItem/nm_applet"  (NetworkManager)
            full_path = f'{sender}{service}'
        elif service.startswith(':'):
            # es: ":1.12"  (megasync)
            full_path = f'{service}/StatusNotifierItem'
        else:
            # ?? never seen this case...what's in service?
            full_path = f'{sender}/StatusNotifierItem'

        if full_path in self._items:
            return

        # observe the sender on the bus, to know when disconnected
        observer = DBusObserver(SESSION_BUS, sender)
        observer.service_available.connect(
            lambda _: self._item_available(full_path)
        )
        observer.service_unavailable.connect(
            lambda _: self._item_unavailable(full_path)
        )
        observer.connect_once_available()

    def _item_available(self, full_path: str):
        # create a new sni, put in store and index
        sni = StatusNotifierItem(full_path)
        self._items[full_path] = sni
        ITEMS_STORE.append(sni)

        # emit the event on the bus
        self.StatusNotifierItemRegistered.emit(full_path)

    def _item_unavailable(self, full_name: str):
        # get and remove the sni from the index
        sni = self._items.pop(full_name, None)
        if sni is None:
            ERR(f'Cannot find tray item to remove: {full_name}')
            return

        # find position in store (needed to remove, hmm...somethig faster?)
        res, pos = ITEMS_STORE.find(sni)
        if not res or pos < 0:
            ERR(f'Cannot find tray item to remove: {sni}')
            return

        # remove sni from the store
        ITEMS_STORE.remove(pos)

        # emit the event on the bus
        self.StatusNotifierItemRegistered.emit(full_name)

    @staticmethod
    def RegisterStatusNotifierHost(service: Str) -> None:
        """ Register a StatusNotifierHost into the StatusNotifierWatcher """
        DBG(f'TRAY RegisterStatusNotifierHost({service})')
        pass  # we are the only Host (well, a fake host...)

    @property
    def RegisteredStatusNotifierItems(self) -> List[Str]:
        """ List containing all the registered instances of StatusNotifierItem """
        return list(self._items.keys())

    @property
    def IsStatusNotifierHostRegistered(self) -> Bool:
        """ True if at least one StatusNotifierHost has been registered """
        return True  # yes, here we are!

    @property
    def ProtocolVersion(self) -> Int:
        return 0

    @dbus_signal
    def StatusNotifierItemRegistered(self, full_path: Str): ...

    @dbus_signal
    def StatusNotifierItemUnregistered(self, full_path: Str): ...

    @dbus_signal
    def StatusNotifierHostRegistered(self): ...

    @dbus_signal
    def StatusNotifierHostUnregistered(self): ...
