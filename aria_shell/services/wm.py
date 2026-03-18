from collections.abc import Callable
from dataclasses import dataclass

from aria_shell.services.hyprland import HyprlandService
from aria_shell.services.sway import SwayService, SwayMessage, MessageType as SwayMessageType
from aria_shell.utils import Singleton
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)

"""
Aria window manager abstraction
"""

@dataclass
class Monitor:
    id: str
    name: str

    @property
    def windows(self):
        return [w for w in WINDOWS.values() if w.monitor_id == self.id]


@dataclass
class Workspace:
    id: str
    name: str
    monitor_id: str

    @property
    def windows(self):
        return [w for w in WINDOWS.values() if w.workspace_id == self.id]


@dataclass
class Window:
    id: str
    name: str  # this is the window class (class cannot be used)
    title: str
    monitor_id: str
    workspace_id: str


MONITORS: dict[str, Monitor] = {}
WORKSPACES: dict[str, Workspace] = {}
WINDOWS: dict[str, Window] = {}


class WindowManagerService(metaclass=Singleton):
    def __init__(self):
        instance = None
        for backend in (HyprlandBackend, SwayBackend):
            try:
                instance = backend()
            except RuntimeError as e:
                DBG(e)
            else:
                break
        if not instance:
            WRN('No supported wm found, lets see what we can do...')
            # TODO return (raise) the failure?
            return
        INF('Using WindowManager backend: %s', instance)
        self.backend = instance
        self.listeners = []

    def watch_events(self, callback: Callable):
        """
        Receive wm events in callback

        # TODO Put this events in a decent struct
        events:
        - 'changed': emitted every time an item is added/removed
        - 'activewin win_id': new focused window
        - 'active_ws ws_id': new focused workspace
        """
        self.backend.watch_events(callback)

    @property
    def monitors(self) -> dict[str, Monitor]:
        return MONITORS

    @property
    def workspaces(self) -> dict[str, Workspace]:
        return WORKSPACES

    @property
    def windows(self) -> dict[str, Window]:
        return WINDOWS

    def activate_workspace(self, ws_id: str):
        self.backend.activate_workspace(ws_id)

    def activate_window(self, win_id: str):
        self.backend.activate_window(win_id)


class WMBackendBase:
    def __init__(self):
        self.listeners: list[Callable] = []

    def __str__(self):
        return f'<{self.__class__.__name__}>'

    def watch_events(self, callback: Callable):
        self.listeners.append(callback)

    def emit_event(self, event: str):
        for listener in self.listeners:
            listener(event)

    def activate_workspace(self, ws_id: str):
        """ Should activate the given workspace """
        raise NotImplementedError

    def activate_window(self, win_id: str):
        """ Should activate the given window """
        raise NotImplementedError

################################################################################
###  hyprland backend  #########################################################
################################################################################
class HyprlandBackend(WMBackendBase):
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
        self.hypr.send_command('j/monitors', self.monitors_cb)

    def hypr_events_cb(self, event, data):
        match event:
            case 'activewindowv2':
                self.emit_event(f'activewin {data}')
            case 'openwindow' | 'closewindow' | 'movewindowv2':
                # TODO: how to request only the changed client?
                self.hypr.send_command('j/clients', self.clients_cb)
            case 'createworkspacev2' | 'destroyworkspacev2':
                self.hypr.send_command('j/workspaces', self.workspaces_cb)
            case _:
                if event not in self.ignored_events:
                    print("HYPR EVENT", event, data)

    def monitors_cb(self, data):
        MONITORS.clear()
        for mon in data or []:
            mid = str(mon['id'])
            MONITORS[mid] = Monitor(id=mid, name=mon['name'])
        # pprint(MONITORS)
        self.hypr.send_command('j/workspaces', self.workspaces_cb)

    def workspaces_cb(self, data):
        WORKSPACES.clear()
        for ws in data or []:
            wid = str(ws['id'])
            WORKSPACES[wid] = Workspace(
                id=wid,
                name=ws['name'],
                monitor_id=str(ws['monitorID']),
            )
        # pprint(WORKSPACES)
        self.hypr.send_command('j/clients', self.clients_cb)

    def clients_cb(self, data):
        WINDOWS.clear()
        for cli in data or []:
            cid = cli['address']
            if cid.startswith('0x'):
                cid = cid[2:]
            WINDOWS[cid] = Window(
                id=cid,
                name=cli['class'],
                title=cli['title'],
                workspace_id=str(cli['workspace']['id']),
                monitor_id=str(cli['monitor']),
            )
        # pprint(WINDOWS)
        self.emit_event('changed')

    def activate_workspace(self, ws_id: str):
        self.hypr.send_command(f'dispatch workspace {ws_id}')

    def activate_window(self, win_id: str):
        self.hypr.send_command(f'dispatch focuswindow address:0x{win_id}')


################################################################################
### Sway backend  ##############################################################
################################################################################
class SwayBackend(WMBackendBase):
    EVENTS = ['window', 'workspace']

    def __init__(self):
        """ Raise RuntimeError if sway is not available """
        super().__init__()
        self.sway = SwayService()
        self.sway.subscribe(self.EVENTS, self._sway_events_cb)
        self.sway.get_tree(self._tree_cb)

    def activate_workspace(self, ws_id: str):
        if ws := WORKSPACES.get(ws_id, None):
            self.sway.run_command(f'workspace "{ws.name}"')

    def activate_window(self, win_id: str):
        self.sway.run_command(f'[con_id={win_id}] focus')

    def _tree_cb(self, root_node: dict | None):
        if root_node is None:
            return

        parent_monitor: Monitor | None = None
        parent_workspace: Workspace | None = None
        focused_win: Window | None = None
        focused_workspace: Workspace | None = None

        MONITORS.clear()
        WORKSPACES.clear()
        WINDOWS.clear()

        # list of nodes to precess, will be recursively filled in the loop
        nodes: list[dict] = [root_node]
        while len(nodes) > 0:
            # pop a node to process from the list of nodes
            node = nodes.pop(0)

            # recursively add child nodes to the list of nodes
            nodes = node['nodes'] + node['floating_nodes'] + nodes

            # monitor
            if node['type'] == 'output':
                if monitor := self._make_monitor(node):
                    parent_monitor = monitor

            # workspace
            elif node['type'] == 'workspace':
                if workspace := self._make_workspace(node):
                    parent_workspace = workspace
                    if node.get('focused', False):
                        focused_workspace = workspace

            # window
            elif node['type'] in ('con', 'floating_con'):
                if win := self._make_window(node, parent_monitor, parent_workspace):
                    if node.get('focused', False):
                        focused_win = win

        # send events
        self.emit_event('changed')
        if focused_workspace:
            self.emit_event(f'active_ws {focused_workspace.id}')
        if focused_win:
            self.emit_event(f'activewin {focused_win.id}')

    @staticmethod
    def _make_monitor(node: dict) -> Monitor | None:
        name = node.get('name', None)
        if not name:
            return None

        monitor = Monitor(id=name, name=name)
        MONITORS[monitor.id] = monitor
        return monitor

    @staticmethod
    def _make_workspace(node: dict) -> Workspace | None:
        wid = node.get('id', None)
        name = node.get('name', None)
        output = node.get('output', None)
        if not (wid and name and output):
            return None

        workspace =  Workspace(
            id=str(wid),
            name=node['name'],
            monitor_id=node['output'],
        )
        WORKSPACES[workspace.id] = workspace
        return workspace

    @staticmethod
    def _remove_workspace(workspace_id: str):
        if workspace_id in WORKSPACES:
            del WORKSPACES[workspace_id]

    @staticmethod
    def _make_window(node: dict, monitor: Monitor, workspace: Workspace) -> Window | None:
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
        window = Window(
            id=str(wid),
            name=app_id,
            title=node['name'],
            workspace_id=workspace.id,
            monitor_id=monitor.id,
        )
        WINDOWS[window.id] = window
        return window

    @staticmethod
    def _remove_window(window_id: str):
        if window_id in WINDOWS:
            del WINDOWS[window_id]

    def _sway_events_cb(self, event: SwayMessage):
        change = event.data.get('change', None)
        DBG('Received Sway event: %s %s', event.type.name, change)

        if event.type == SwayMessageType.EVT_WORKSPACE:
            if ws_id := event.data.get('current', {}).get('id', None):
                match change:
                    case 'focus':
                        self.emit_event(f'active_ws {ws_id}')
                    case 'init':
                        self._make_workspace(event.data['current'])
                        self.emit_event('changed')
                    case 'empty':
                        self._remove_workspace(str(ws_id))
                        self.emit_event('changed')
                    case 'rename':
                        if ws := WORKSPACES.get(ws_id, None):
                            ws.name = event.data['current'].get('name', '')
                            self.emit_event('changed')
                    case 'move':
                        # refetch the whole tree
                        self.sway.get_tree(self._tree_cb)
                    case _:
                        INF('NOT MANAGED WORKSPACE CHANGE %s', change)

        elif event.type == SwayMessageType.EVT_WINDOW:
            if win_id := event.data.get('container', {}).get('id', None):
                match change:
                    case 'focus':
                        self.emit_event(f'activewin {win_id}')
                    case 'close':
                        self._remove_window(str(win_id))
                        self.emit_event('changed')
                    case 'new'|'move':
                        # NOTE: data do not contain the ws this new win belong
                        # I'm not able to create the win here, need to
                        # refetch the whole tree (async!)  grrr...
                        # self._make_window(event.data['container'], None, None)
                        self.sway.get_tree(self._tree_cb)
                    case 'title':
                        pass  # TODO
                    case 'floating':
                        pass  # something to do?
                    case _:
                        INF('NOT MANAGED WINDOW CHANGE %s', change)
