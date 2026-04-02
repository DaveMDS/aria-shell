"""
Aria window manager abstraction

This service keep two IndexedListModel of Windows and Workspaces
objects always updated.

"""
from abc import abstractmethod, ABC
from functools import cached_property

from gi.repository import Gio, GObject, Gtk

from aria_shell.services.hyprland import HyprlandService
from aria_shell.services.sway import SwayService, SwayMessage, MessageType as SwayMessageType
from aria_shell.utils import Singleton, IndexedListStore
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class Workspace(GObject.Object):
    """
    Class used to represent a window manager workspace.
    """
    __gtype_name__ = 'Workspace'

    # "reactive" props that can be watched/binded
    id: str = GObject.Property(type=str)
    name: str = GObject.Property(type=str)
    monitor: str = GObject.Property(type=str)
    active: bool = GObject.Property(type=bool, default=False)
    urgent: bool = GObject.Property(type=bool, default=False)

    def __repr__(self):
        return f"<Workspace id={self.id} monitor={self.monitor} name='{self.name}'>"

    @cached_property
    def windows(self) -> Gio.ListModel[Window]:
        """Get the list of Windows that belong to this Workspace."""
        filter_ = Gtk.CustomFilter.new(lambda win: win.workspace_id == self.id)
        model = Gtk.FilterListModel(model=WINDOWS_STORE, filter=filter_)
        return model

    def activate(self):
        """Ask the window manager to activate this workspace."""
        WindowManagerService().activate_workspace(self)


class Window(GObject.Object):
    """
    Class used to represent a window manager window.
    """
    __gtype_name__ = 'Window'

    # "reactive" props that can be watched/binded
    id: str = GObject.Property(type=str)
    name: str = GObject.Property(type=str) # this is the window class (class cannot be used)
    title: str = GObject.Property(type=str)
    monitor_id: str = GObject.Property(type=str)
    workspace_id: str = GObject.Property(type=str)
    active: bool = GObject.Property(type=bool, default=False)

    def __repr__(self):
        return f"<Window id={self.id} name='{self.name}' title='{self.title}'>"

    def activate(self):
        """Ask the window manager to focus this window."""
        WindowManagerService().activate_window(self)


WORKSPACES_STORE = IndexedListStore(item_type=Workspace)
WINDOWS_STORE = IndexedListStore(item_type=Window)


class WindowManagerService(metaclass=Singleton):
    """
    This is the main service to use.
    """
    def __init__(self):
        for backend in (HyprlandBackend, SwayBackend):
            try:
                self._backend = backend()
                break
            except RuntimeError as e:
                DBG(e)
        else:
            WRN('No supported window manager found!')
            return
        INF('Using WindowManager backend: %s', self._backend)

    @property
    def workspaces(self) -> IndexedListStore[Workspace]:
        """Get the list of Workspaces."""
        return WORKSPACES_STORE

    @property
    def windows(self) -> IndexedListStore[Window]:
        """Get the list of Windows."""
        return WINDOWS_STORE

    def activate_workspace(self, workspace: Workspace):
        """Ask the window manager to activate the given workspace."""
        if self._backend:
            self._backend.activate_workspace(workspace)

    def activate_window(self, window: Window):
        """Ask the window manager to focus the given window."""
        if self._backend:
            self._backend.activate_window(window)


class WindowManagerBackend(ABC):
    """
    Base abstract class for all WM backends.
    """
    def __init__(self):
        self.active_window: Window | None = None
        self.active_workspace: Workspace | None = None

    def __str__(self):
        return f'<{self.__class__.__name__}>'

    #
    # methods that must be implemented in backends
    #
    @abstractmethod
    def activate_workspace(self, workspace: Workspace):
        """Should activate the given workspace."""

    @abstractmethod
    def activate_window(self, window: Window):
        """Should activate the given window."""

    #
    # internal utilities to be used by backends
    #
    def _set_active_workspace(self, workspace: Workspace | str | None):
        if isinstance(workspace, str):
            workspace = WORKSPACES_STORE.get(workspace)
        if self.active_workspace:
            self.active_workspace.active = False
        if workspace:
            workspace.active = True
        self.active_workspace = workspace

    def _set_active_window(self, window: Window | str | None):
        if isinstance(window, str):
            window = WINDOWS_STORE.get(window)
        if self.active_window:
            self.active_window.active = False
        if window:
            window.active = True
        self.active_window = window


################################################################################
###  hyprland backend  #########################################################
################################################################################
class HyprlandBackend(WindowManagerBackend):
    ignored_events = (
        # v2 available
        'activewindow', 'focusedmon', 'movewindow', 'windowtitle',
        'workspace', 'createworkspace', 'destroyworkspace',
        # other
        'openlayer', 'focusedmonv2',
    )

    def __init__(self):
        """ Raise RuntimeError if hyprland is not available """
        super().__init__()
        self.hypr = HyprlandService()
        self.hypr.watch_events(self.hypr_events_cb)
        self.hypr.send_command('j/workspaces', self.workspaces_cb)

    def hypr_events_cb(self, event, data):
        match event:
            case 'activewindowv2':
                self._set_active_window(data)
            case 'openwindow' | 'closewindow' | 'movewindowv2':
                # TODO: how to request only the changed client?
                self.hypr.send_command('j/clients', self.clients_cb)
            case 'createworkspacev2' | 'destroyworkspacev2':
                self.hypr.send_command('j/workspaces', self.workspaces_cb)
            case _:
                if event not in self.ignored_events:
                    print("HYPR EVENT", event, data)

    def workspaces_cb(self, data):
        WORKSPACES_STORE.remove_all()

        for ws in data or []:
            wid = str(ws['id'])
            workspace = Workspace()
            workspace.id = wid
            workspace.name = ws['name']
            workspace.monitor = ws['monitor']
            WORKSPACES_STORE.append(workspace)

        self.hypr.send_command('j/clients', self.clients_cb)

    @staticmethod
    def clients_cb(data):
        WINDOWS_STORE.remove_all()

        for cli in data or []:
            cid = cli['address']
            if cid.startswith('0x'):
                cid = cid[2:]
            window = Window()
            window.id = cid
            window.name = cli['class']
            window.title = cli['title']
            window.workspace_id = str(cli['workspace']['id'])
            window.monitor_id = str(cli['monitor'])
            WINDOWS_STORE.append(window)

    def activate_workspace(self, workspace: Workspace):
        self.hypr.send_command(f'dispatch workspace {workspace.id}')

    def activate_window(self, window: Window):
        self.hypr.send_command(f'dispatch focuswindow address:0x{window.id}')


################################################################################
### Sway backend  ##############################################################
################################################################################
class SwayBackend(WindowManagerBackend):
    sway_events = ['window', 'workspace']

    def __init__(self):
        """ Raise RuntimeError if sway is not available """
        super().__init__()
        self.sway = SwayService()
        self.sway.subscribe(self.sway_events, self._sway_events_cb)
        self.sway.get_tree(self._tree_cb)

    def activate_workspace(self, workspace: Workspace):
        self.sway.run_command(f'workspace "{workspace.name}"')

    def activate_window(self, window: Window):
        self.sway.run_command(f'[con_id={window.id}] focus')

    def _tree_cb(self, root_node: dict | None):
        if root_node is None:
            return

        parent_monitor: str = ''
        parent_workspace: Workspace | None = None

        WORKSPACES_STORE.remove_all()
        WINDOWS_STORE.remove_all()

        self._set_active_window(None)
        self._set_active_workspace(None)

        # list of nodes to precess, will be recursively filled in the loop
        nodes: list[dict] = [root_node]
        while len(nodes) > 0:
            # pop a node to process from the list of nodes
            node = nodes.pop(0)

            # recursively add child nodes to the list of nodes
            nodes = node['nodes'] + node['floating_nodes'] + nodes

            # monitor
            if node['type'] == 'output':
                parent_monitor = node.get('name', '')

            # workspace
            elif node['type'] == 'workspace':
                if workspace := self._make_workspace(node):
                    parent_workspace = workspace
                    if node.get('focused', False):
                        self._set_active_workspace(workspace)

            # window
            elif node['type'] in ('con', 'floating_con'):
                if win := self._make_window(node, parent_monitor,
                                            parent_workspace):
                    if node.get('focused', False):
                        self._set_active_window(win.id)

        # ensure a workspace is active
        if not self.active_workspace and self.active_window.workspace_id:
            self._set_active_workspace(self.active_window.workspace_id)

    @staticmethod
    def _make_workspace(node: dict) -> Workspace | None:
        wid = node.get('id', None)
        name = node.get('name', None)
        output = node.get('output', None)
        if not (wid and name and output):
            return None

        workspace =  Workspace()
        workspace.id = str(wid)
        workspace.name = node['name']
        workspace.monitor = node['output']
        WORKSPACES_STORE.append(workspace)
        return workspace

    @staticmethod
    def _make_window(node: dict, monitor: str, workspace: Workspace) -> Window | None:
        wid = node.get('id', None)
        pid = node.get('pid', None)
        if not (wid and pid):
            return None

        app_id = node.get('app_id', None)
        if not app_id:
            app_id = node.get('window_properties', {}).get('class', None)
        if not app_id:
            return None

        # visible = node.get('visible', False)
        # urgent = node.get('urgent', False)
        # focused = node.get('focused', False)
        window = Window()
        window.id=str(wid)
        window.name = app_id
        window.title = node['name']
        window.workspace_id = workspace.id
        window.monitor_id = monitor
        WINDOWS_STORE.append(window)

        return window

    def _sway_events_cb(self, event: SwayMessage):
        change = event.data.get('change', None)
        DBG('Received Sway event: %s %s', event.type.name, change)

        if event.type == SwayMessageType.EVT_WORKSPACE:
            if ws_id := str(event.data.get('current', {}).get('id', '')):
                match change:
                    case 'focus':
                        self._set_active_workspace(ws_id)
                    case 'init':
                        self._make_workspace(event.data['current'])
                    case 'empty':
                        WORKSPACES_STORE.remove_key(ws_id)
                    case 'rename':
                        if ws := WORKSPACES_STORE.get(ws_id):
                            ws.name = event.data['current'].get('name', '')
                    case 'move':
                        # refetch the whole tree  # TODO better?
                        self.sway.get_tree(self._tree_cb)
                    case _:
                        INF('NOT MANAGED WORKSPACE CHANGE %s', change)

        elif event.type == SwayMessageType.EVT_WINDOW:
            if win_id := str(event.data.get('container', {}).get('id', '')):
                match change:
                    case 'focus':
                        self._set_active_window(win_id)
                    case 'close':
                        WINDOWS_STORE.remove_key(win_id)
                    case 'new'|'move':
                        # NOTE: data do not contain the ws this new win belong
                        # I'm not able to create the win here, need to
                        # refetch the whole tree (async!)  grrr...
                        # self._make_window(event.data['container'], None, None)
                        self.sway.get_tree(self._tree_cb)  # TODO better ??
                    case 'title':
                        if win := WINDOWS_STORE.get(win_id):
                            win.title = event.data['container'].get('name', '')
                    case 'floating':
                        pass  # something to do?
                    case _:
                        WRN('NOT MANAGED WINDOW CHANGE %s', change)
