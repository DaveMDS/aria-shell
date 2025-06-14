"""

TODO DOC a little bit

"""
from typing import Literal

from dasbus.server.interface import dbus_interface, dbus_signal
from dasbus.server.interface import accepts_additional_arguments
from dasbus.client.observer import DBusObserver
from dasbus.typing import Bool, Int, Str, List
from dasbus.connection import SessionMessageBus

from gi.repository import Gtk, GObject, Gio, GLib, Graphene

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


class TrayIcon(Gtk.Overlay):
    """ A Gtk.Widget that is able to show a single StatusNotifierItem """
    __gtype_name__ = 'TrayIcon'

    def __init__(self):
        super().__init__(css_classes=['aria-tray-item'])
        self.image = Gtk.Image()
        self.set_child(self.image)
        self._binds: list[GObject.Binding] = []
        self._sni: StatusNotifierItem | None = None

        self.set_cursor_from_name('pointer')  # TODO giusto? ci piace?

        # EventController to receive mouse clicks
        ec = Gtk.GestureSingle(button=0)
        ec.connect('begin', self._on_mouse_down)
        self.add_controller(ec)

        # EventController to receive mouse wheel events
        ec = Gtk.EventControllerScroll.new(
            Gtk.EventControllerScrollFlags.VERTICAL
            | Gtk.EventControllerScrollFlags.DISCRETE
        )
        ec.connect('scroll', self._on_scroll)
        self.add_controller(ec)

    def bind(self, item: 'StatusNotifierItem'):
        print('BIND', self)
        if self._binds is not None:  # should never happend
            self.unbind()
        self._binds.append(
            item.bind_property(
               'icon_name', self.image, 'icon_name',
                GObject.BindingFlags.SYNC_CREATE,
            )
        )
        self._binds.append(
            item.bind_property(
                'tooltip', self, 'tooltip_markup',
                GObject.BindingFlags.SYNC_CREATE,
            )
        )
        self._sni = item

    def unbind(self):
        print('UNBIND', self)
        while self._binds and (bind := self._binds.pop()):
            bind.unbind()
        self._sni = None

    def _on_scroll(self, _ec: Gtk.EventControllerScroll, dx: float, dy: float):
        if self._sni:
            if dy != 0:
                self._sni.scroll(int(dy), 'vertical')
            elif dx != 0:
                self._sni.scroll(int(dx), 'horizontal')

    def _on_mouse_down(self, ec: Gtk.GestureSingle, _):
        # ...ok, on wayland is not really possible to get absolute pos
        # needed by Activate. Will use point relative to the parent win.
        # When the panel is on top this should work more or less...
        if self._sni:
            win = self.get_native()
            _, x, y = ec.get_point()
            _, p = self.compute_point(win, Graphene.Point(x, y)) # noqa
            x, y = int(x), int(y)
            match ec.get_current_button():
                case 1:
                    self._sni.activate(x, y)
                case 2:
                    self._sni.secondary_activate(x, y)
                case 3:
                    self._sni.context_menu(x, y)


class TrayGadget(AriaGadget):
    def __init__(self, conf: TrayConfigModel):
        super().__init__('tray')

        factory = Gtk.SignalListItemFactory()
        factory.connect('setup', self._factory_item_setup)
        factory.connect('bind', self._factory_item_bind)
        factory.connect('unbind', self._factory_item_unbind)

        model = Gtk.SingleSelection(model=ITEMS_STORE, autoselect=False)
        list_view = Gtk.ListView(
            model=model,
            factory=factory,
            orientation=Gtk.Orientation.HORIZONTAL,
        )
        self.append(list_view)

    @staticmethod
    def _factory_item_setup(_, list_item: Gtk.ListItem):
        """ create a new for the list item """
        # create a new TrayIcon and attach to ListItem
        list_item.set_child(TrayIcon())
        list_item.set_activatable(False)
        list_item.set_selectable(False)

    @staticmethod
    def _factory_item_bind(_, list_item: Gtk.ListItem):
        """ bind the previously created TrayIcon with the StatusNotifierItem """
        item: StatusNotifierItem = list_item.get_item()  # noqa
        tico: TrayIcon = list_item.get_child()  # noqa
        tico.bind(item)

    @staticmethod
    def _factory_item_unbind(_, list_item: Gtk.ListItem):
        """ unbind the previously binded StatusNotifierItem """
        tico: TrayIcon = list_item.get_child()  # noqa
        tico.unbind()



################################################################################

################################################################################

################################################################################


SESSION_BUS = SessionMessageBus()

STATUS_NOTIFIER_WATCHER_IFACE = 'org.kde.StatusNotifierWatcher'
STATUS_NOTIFIER_WATCHER_SERVICE = 'org.kde.StatusNotifierWatcher'
STATUS_NOTIFIER_WATCHER_PATH = '/StatusNotifierWatcher'


class StatusNotifierItem(GObject.Object):
    """ Implement the remote object StatusNotifierItem """
    __gtype_name__ = 'StatusNotifierItem'

    IFACE = 'org.kde.StatusNotifierItem'

    # "reactive" properties that can be watched/binded
    id = GObject.Property(type=str)
    status = GObject.Property(type=str)  # Literal['Passive','Active','NeedsAttention']
    category = GObject.Property(type=str)
    title = GObject.Property(type=str, default='')
    tooltip = GObject.Property(type=str, default='')
    # item_is_menu = GObject.Property(type=Bool, default=False) # not usefull
    menu = GObject.Property(type=str, default='')
    # TODO WindowId  ??
    icon_name = GObject.Property(type=str, default='')
    overlay_icon_name = GObject.Property(type=str, default='')
    attention_icon_name = GObject.Property(type=str, default='')
    # TODO IconPixmap
    # TODO OverlayIconPixmap
    # TODO AttentionIconPixmap
    # TODO AttentionMovieName !!

    def __init__(self, full_path: str):
        super().__init__()

        # full_path: ":1.12520/org/ayatana/NotificationItem/nm_applet"
        i = full_path.index('/')
        bus_name, object_path = full_path[:i], full_path[i:]

        # create the object proxy for this path
        self._proxy = SESSION_BUS.get_proxy(bus_name, object_path)

        # async read all the properties from the remote object
        self._proxy.GetAll(
            self.IFACE,
            callback=self._get_all_callback,
            # timeout=2000,
        )
        # self.connect('destroy', lambda *_: print("DESTROY -- "*10))
        # watch properties for changes (NEEDED?)
        # if hasattr(self._proxy, 'PropertiesChanged'):
        #     self._proxy.PropertiesChanged.connect(
        #         lambda ifa, props, inv: self._sync_properties(props.keys())
        #     )

        # connect to all various New* signals
        if hasattr(self._proxy, 'NewStatus'):
            self._proxy.NewStatus.connect(
                lambda: self._request_properties(['Status']),
            )
        if hasattr(self._proxy, 'NewTitle'):
            self._proxy.NewTitle.connect(
                lambda: self._request_properties(['Title'])
            )
        if hasattr(self._proxy, 'NewToolTip'):
            self._proxy.NewToolTip.connect(
                lambda: self._request_properties(['ToolTip'])
            )
        if hasattr(self._proxy, 'NewIcon'):
            self._proxy.NewIcon.connect(
                lambda: self._request_properties(['IconName', 'IconPixmap'])
            )
        if hasattr(self._proxy, 'NewAttentionIcon'):
            self._proxy.NewAttentionIcon.connect(
                lambda: self._request_properties(['AttentionIconName', 'AttentionIconPixmap'])
            )

    def __repr__(self):
        return f"<SNI id='{self.id}' status='{self.status}' icon='{self.icon_name}' menu='{self.menu}'>"

    # keep track of prop Get async requests, to not request the same prop
    # while it is already being requested. The set keep the names of props.
    _alive_async_requests = set()

    def _request_properties(self, props: list[str]):
        """ request the given properties from the remote object """
        INF(f'SyncProps: {props} {self.id}')
        for prop_name in props:
            if prop_name not in self._alive_async_requests:
                self._proxy.Get(self.IFACE, prop_name,
                                callback=self._get_callback,
                                callback_args=(prop_name,),
                                timeout=100)  # ms!
                self._alive_async_requests.add(prop_name)
            else:
                WRN(f'PROP ALREADY REQUESTED {prop_name}')

    def _get_all_callback(self, call):
        """ async props GetAll() method response """
        try:
            vals: dict = call()
        except Exception as e:
            ERR(f'XXX {e} {self.id}')
            return
        for prop_name, variant_val in vals.items():
            # reuse the callback for single prop get
            self._get_callback(None, prop_name, variant_val)

    def _get_callback(self, call, prop_name, val=None):
        """ async prop Get() method response """
        self._alive_async_requests.discard(prop_name)
        if callable(call):
            try:
                INF(f'Get {prop_name} _')
                val = call()
                INF(f'Get {prop_name} VAL {repr(val)})')
            except Exception as e:
                ERR(f'YYY {e} {self.id} {prop_name}')
                return

        if val is not None:
            self._update_internal_property(prop_name, val)

    def _update_internal_property(self, prop_name: str, val):
        """ Update our "reactive" properties with new values """
        if val is None:
            return

        if isinstance(val, GLib.Variant):
            val = val.unpack()

        match prop_name:
            case 'Id':
                if val != self.id:
                    self.id = val
            case 'Category':
                if val != self.category:
                    self.category = val
            case 'Status':
                if val != self.status:
                    self.status = val
            case 'Title':
                if val != self.title:
                    self.title = val
            case 'Menu':
                if val != self.menu:
                    self.menu = val
            case 'ToolTip':
                # ToolTip: (icon_name, icon_data, title, descriptive_text)
                if isinstance(val, tuple) and len(val) == 4:
                    title, text = val[2:4]
                    if title and text:
                        full = f'<b>{title}</b>\n{text}'
                    else:
                        full = title or text or ''
                    if full != self.tooltip:
                        self.tooltip = full
                elif self.tooltip:
                    self.tooltip = ''
            case 'IconName':
                if val != self.icon_name:
                    self.icon_name = val
            case 'OverlayIconName':
                if val != self.overlay_icon_name:
                    self.overlay_icon_name = val
            case 'AttentionIconName':
                if val != self.attention_icon_name:
                    self.attention_icon_name = val

    def activate(self, x: int, y: int):
        try:
            self._proxy.Activate(x, y)
        except AttributeError:
            pass

    def context_menu(self, x: int, y: int):
        # TODO use dbus menu here if available, otherwise call ContextMenu
        try:
            self._proxy.ContextMenu(x, y)
        except AttributeError:
            pass

    def secondary_activate(self, x: int, y: int):
        try:
            self._proxy.SecondaryActivate(x, y)
        except AttributeError:
            pass

    def scroll(self, delta: int, orientation: Literal['horizontal', 'vertical']):
        try:
            self._proxy.Scroll(delta, orientation)
        except AttributeError:
            pass

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
