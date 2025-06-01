from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass

from aria_shell.services.hyprland import HyprlandService
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
        for backend in (HyprlandBackend,):
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

        self.backend = instance
        self.listeners = []

    def watch_events(self, callback: Callable):
        """
        Receive wm events in callback

        events:
        - 'changed': emitted every time an item is added/removed
        - 'activewin win_id': new focused window
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
### TODO sway backend  #########################################################
################################################################################
