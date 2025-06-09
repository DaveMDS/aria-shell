from typing import Mapping

from gi.repository import Gtk, Gdk

from aria_shell.ui import AriaGadget, AriaBox
from aria_shell.services.wm import WindowManagerService, Workspace, Window
from aria_shell.services.xdg import XDGDesktopService
from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule
from aria_shell.config import AriaConfigModel


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class WorkspacesConfig(AriaConfigModel):
    show_windows: bool = True
    all_monitors: bool = False
    focus_window_on_click: bool = False


class WorkspacesModule(AriaModule):
    def __init__(self):
        super().__init__()
        self.wm_service: WindowManagerService | None = None
        self.instances: list[WorkspacesGadget] = [] # just for typing

    def module_init(self):
        super().module_init()
        self.wm_service = WindowManagerService()
        self.wm_service.watch_events(self.wm_event_cb)

    def module_shutdown(self):
        # TODO shutdown WindowManagerService
        super().module_shutdown()

    def module_gadget_new(self, user_settings: Mapping[str, str], monitor: Gdk.Monitor):
        super().module_gadget_new(user_settings, monitor)
        DBG(f'AriaModule module_instance_new {self.__class__.__name__}')
        conf = WorkspacesConfig(user_settings)
        return WorkspacesGadget(conf, monitor.get_connector())

    def wm_event_cb(self, event):
        if event == 'changed':
            for instance in self.instances:
                instance.update(
                    self.wm_service.monitors,
                    self.wm_service.workspaces,
                    self.wm_service.windows,
                )
        elif event.startswith('activewin ') and len(event) > 10:
            _, wid = event.split()
            win = self.wm_service.windows.get(wid)
            for instance in self.instances:
                instance.set_active_window(wid)
                if win and win.workspace_id:
                    instance.set_active_workspace(win.workspace_id)


class WorkspacesGadget(AriaGadget):
    def __init__(self, conf: WorkspacesConfig, monitor_name: str):
        super().__init__('workspaces')
        self.conf = conf
        self.monitor_name = monitor_name
        self._win_index: dict[str, Gtk.Widget] = {}
        self._ws_index: dict[str, Gtk.Widget] = {}
        self._active_win_id: str | None = None
        self._active_ws_id: str | None = None
        self.icon_service = XDGDesktopService()
        self.wm_service = WindowManagerService()

    def clear(self):
        while child := self.get_last_child():
            self.remove(child)
        self._win_index.clear()
        self._ws_index.clear()

    def update(self, monitors, workspaces, _windows):
        self.clear()

        # get the id of the monitor this panel belong to
        for mon_id, mon in monitors.items():
            if mon.name == self.monitor_name:
                break
        else:
            mon_id = None

        for ws_id in sorted(workspaces):
            ws = workspaces[ws_id]

            # skip workspaces from other monitors
            if not self.conf.all_monitors and mon_id and ws.monitor_id != mon_id:
                continue

            # the workspace box
            box = AriaBox('aria-workspace')
            box.set_cursor_from_name('pointer')
            self._ws_index[ws.id] = box

            # make the box clickable
            ges = Gtk.GestureSingle()
            ges.connect('begin', self.on_workspace_click, ws)
            box.add_controller(ges)

            # the workspace label
            label = Gtk.Label()
            label.add_css_class('aria-workspace-label')
            label.set_tooltip_text(f'Workspace\n{ws.id}: {ws.name}')
            label.set_text(ws.name)
            box.append(label)

            # the workspace windows
            if self.conf.show_windows:
                for win in ws.windows:
                    # label = Gtk.Label()
                    # label.set_text(win.name)
                    # box.append(label)

                    icon = self.icon_service.get_icon_for_window_class(win.name)
                    icon.add_css_class('aria-workspace-window')
                    icon.set_tooltip_text(f'{win.name}\n{win.title}')

                    if self.conf.focus_window_on_click:
                        ges = Gtk.GestureSingle()
                        ges.connect('begin', self.on_window_click, win)
                        icon.add_controller(ges)

                    self._win_index[win.id] = icon
                    box.append(icon)

            self.append(box)

        if self._active_win_id:
            self.set_active_window(self._active_win_id)
        if self._active_ws_id:
            self.set_active_workspace(self._active_ws_id)

    def on_workspace_click(self, _ges: Gtk.GestureSingle, _, ws: Workspace):
        self.wm_service.activate_workspace(ws.id)

    def on_window_click(self, ges: Gtk.GestureSingle, _, win: Window):
        self.wm_service.activate_window(win.id)
        ges.set_state(Gtk.EventSequenceState.CLAIMED)

    def set_active_workspace(self, wid):
        # deactivate old workspace object
        if obj := self._ws_index.get(self._active_ws_id):
            obj.remove_css_class('active')
        # activate new workspace object
        if obj := self._ws_index.get(wid):
            obj.add_css_class('active')
            self._active_ws_id = wid

    def set_active_window(self, wid):
        # deactivate old window object
        if obj := self._win_index.get(self._active_win_id):
            obj.remove_css_class('active')
        # activate new window object
        if obj := self._win_index.get(wid):
            obj.add_css_class('active')
            self._active_win_id = wid
