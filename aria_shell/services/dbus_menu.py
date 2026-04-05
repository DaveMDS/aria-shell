"""

This module create a Gio.MenuModel representing the items from a DBus Menu

The D-Bus interface is 'com.canonical.dbusmenu' as used in systray

The implementation is not complete, there are not handled advanced features,
like sending AboutToShow(mid) and create the sub menu dynamically.
Didn't find a way to implement AboutToShow with Gio.Menu...

Hopefully this is enough for the majority of the cases.


Usage:
> menu = Gtk.PopoverMenu()  # any widget that support MenuModel
>
> model = CanonicalDBusMenuModel(
>     bus_name=":1.3",
>     object_path="/org/ayatana/NotificationItem/nm_applet/Menu",
>     parent_widget=menu,
> )
>
> menu.set_model(model)
>

Reference:
dbus-menu(-docs).xml in the doc/ source directory
https://code.launchpad.net/~dbusmenu-team/libdbusmenu/trunk

"""
import time
from typing import Literal

from gi.repository import Gio, Gtk
from gi.repository.GLib import Variant

from dasbus.client.proxy import ObjectProxy, disconnect_proxy
from dasbus.connection import SessionMessageBus

from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class MenuItem:
    """ Decode a menu item received from D-Bus in the recursive format:

        a(ia{sv}av)  =>  [(id, props, childs), (id, props, childs), ...]

        The format is recursive, where the second 'v' is in the same format
        as the original 'a(ia{sv}av)'.
    """
    def __init__(self, mid: int, props: dict, childs: list):
        """ Args are the 3 fields of the struct:  ia{sv}av """
        self.mid = mid
        self._props = props
        self._childs = childs

    def __repr__(self):
        return (
            f"<MenuItem {self.mid} '{self.label}'"
            f"{' SUBMENU' if self.is_submenu else ''}"
            f"{' SEPARATOR' if self.is_separator else ''}"
            f"{' HIDDEN' if not self.visible else ''}"
            f">"
        )

    # def __del__(self):
    #     print("DEL !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!", self)

    @property
    def childs(self) -> list['MenuItem']:
        return [MenuItem(*x) for x in self._childs]

    @property
    def is_separator(self) -> bool:
        return self._props.get('type', None) == 'separator'

    @property
    def label(self) -> str:
        return self._props.get('label', '')
        # return f'{self.mid} - {self._props.get('label', '')}'

    @property
    def enabled(self) -> bool:
        return self._props.get('enabled', True)

    @property
    def visible(self) -> bool:
        return self._props.get('visible', True)

    @property
    def icon_name(self) -> str:
        return self._props.get('icon-name', '')

    @property
    def icon_data(self) -> list[int]:  # TODO bytes ??
        return self._props.get('icon-data', [])

    # @property
    # def shortcut(self) -> ???:
    #     return self._props.get('shortcut', ???)

    @property
    def is_check(self) -> bool:
        return self._props.get('toggle-type', None) == 'checkmark'

    @property
    def is_radio(self) -> bool:
        return self._props.get('toggle-type', None) == 'radio'

    @property
    def toggle_state(self) -> int:
        # 0 = off, 1 = on, anything-else = indeterminate
        return self._props.get('toggle-state', -1)  # noqa

    @property
    def is_submenu(self) -> bool:
        return self._props.get('children-display', None) == 'submenu'

    @property
    def disposition(self) -> Literal['normal', 'informative', 'warning', 'alert']:
        return self._props.get('dispositon', 'normal')  # noqa


class CanonicalDBusMenu(Gio.Menu):
    """ A MenuModel representing the items from a DBusMenu """
    __gtype_name__ = 'CanonicalDBusMenu'

    INTERFACE = 'com.canonical.dbusmenu'

    def __repr__(self):
        return f'<CanonicalDBusMenu {self.service_name} {self.object_path} root={self.root_node}>'

    def __init__(self, *,
                 service_name: str,
                 object_path: str,
                 parent_widget: Gtk.Widget,
                 root_node: int = 0,
                 proxy: ObjectProxy = None):
        """
        Args:
            service_name: the name on the bus, es  :1.117
            object_path: the object path, es: /MenuBar
            parent_widget: the menu using the model
            root_node: node-id to start from (leave 0)
            proxy: used in recursive calls to reuse the same proxy
        """
        super().__init__()
        self.parent_widget = parent_widget  # needed to attach the ActionGroup :/
        self.service_name = service_name
        self.object_path = object_path
        self.root_node = root_node

        # Action group
        self._action_group = Gio.SimpleActionGroup()
        self.parent_widget.insert_action_group(f'menu-{root_node}', self._action_group)

        # create the dbus proxy only for the first (root) node
        if proxy is None and root_node == 0:
            try:
                bus = SessionMessageBus()
                self._proxy = bus.get_proxy(service_name, object_path,
                                            interface_name=self.INTERFACE)
                # get the version now, to test the connection
                version = self._proxy.Version
                # print(' TextDirection', self._proxy.TextDirection)
                # print(' Status', self._proxy.Status)
                # print(' IconThemePath', self._proxy.IconThemePath)
                DBG('Connected to DBusMenu %s %s v=%s', service_name, object_path, version)
            except Exception as e:
                ERR(f'Cannot connect to DBusMenu over the SessionBus. '
                    f'bus=%s path=%s Error: %s', self.service_name, self.object_path, e)
                return

            self._proxy.LayoutUpdated.connect(self._on_layout_updated)
            # self._proxy.ItemsPropertiesUpdated.connect(self._on_items_properties_updated)

        # or use the proxy from the caller (root node)
        elif proxy:
            self._proxy = proxy
        else:
            raise RuntimeError('Dont have a proxy for the DBusMenu')

        self._build_menu(root_node)

    def destroy(self):
        # remove all the menu items and all the actions
        self._clear_menu()

        # cleanup the ActionGroup
        self.parent_widget.insert_action_group(f'menu-{self.root_node}', None)

        # disconnect the proxy, only if we are the root node
        if self.root_node == 0 and self._proxy:
            DBG('Disconnect from DBusMenu %s %s', self.service_name, self.object_path)
            disconnect_proxy(self._proxy)
            del self._proxy

        # TODO: need to destroy the children ?  :/

    # def __del__(self):
    #     print("DEL MenuModel --------------------------", self)

    def _on_layout_updated(self, revision: int, parent: int):
        WRN(f'LayoutUpdated !!!! {revision=} {parent=}')
        # TODO debounce, to not redraw 3 times in a row
        self._clear_menu()
        self._build_menu(0)

    def _clear_menu(self):
        # remove all the menu items
        self.remove_all()

        # remove all the actions
        for action_name in self._action_group.list_actions():
            self._action_group.remove_action(action_name)

    def _build_menu(self, root_node: int = 0):
        DBG('Building DBus menu %d', root_node)
        # request the menu layout
        try:
            layout = self._proxy.GetLayout(root_node, 1, [])
            assert isinstance(layout, tuple) and len(layout) == 2
        except Exception as e:
            ERR(f'Error at GetLayout() for menu node: {root_node}. Error: {e}')
            return

        # create the Menu Item
        revision, main_item = layout
        main_item = MenuItem(*main_item)

        # Gio use sections to represent separators, while on D-Bus separators
        # are special items. We start with the root section, when we meet
        # a separator we close the section and open a new one.
        section = Gio.Menu()  # root section

        for item in main_item.childs:
            if not item.visible:
                continue

            if item.is_separator:
                # "close" the current section and create a new one
                self.append_section(None, section)
                section = Gio.Menu()
                continue

            if item.is_submenu:
                # create another instance of self, and set as submenu
                submenu = CanonicalDBusMenu(
                    service_name=self.service_name, object_path=self.object_path,
                    root_node=item.mid, parent_widget=self.parent_widget,
                    proxy=self._proxy,
                )
                section.append_submenu(item.label, submenu)
                continue

            # normal items, build a suitable Action
            action_name = f'action-for-menu{item.mid}'

            if item.is_check:
                action = Gio.SimpleAction.new_stateful(
                    name=action_name, parameter_type=None,
                    state=Variant.new_boolean(item.toggle_state == 1),
                )

            elif item.is_radio:
                # TODO find an app that use radio to test
                INF('Radio items not implemented, please report the app!!')
                action = Gio.SimpleAction.new(name=action_name)

            else:
                action = Gio.SimpleAction.new(name=action_name)

            # finalize the action
            action.set_enabled(item.enabled)
            action.connect('activate', self._on_action_activated, item)
            self._action_group.add_action(action)

            # create the new menu item
            section.append(item.label, f'menu-{root_node}.{action_name}')

        # "close" the last section
        self.append_section(None, section)

    def _on_action_activated(self,
                             action: Gio.SimpleAction,
                             param: Variant | None,
                             item: MenuItem):
        # events: 'clicked', 'hovered', 'opened', 'closed']
        timestamp = int(time.time())
        self._proxy.Event(item.mid, 'clicked', Variant('s', ''), timestamp)
