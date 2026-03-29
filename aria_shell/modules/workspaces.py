from gi.repository import Gtk

from aria_shell.gadget import AriaGadget
from aria_shell.ui import AriaBox
from aria_shell.services.wm import WindowManagerService, Workspace, Window
from aria_shell.services.xdg import XDGDesktopService
from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule, GadgetRunContext
from aria_shell.config import AriaConfigModel


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class WorkspacesConfigModel(AriaConfigModel):
    show_name: bool = True
    show_windows: bool = True
    all_monitors: bool = False
    focus_window_on_click: bool = False


class WorkSpacesModule(AriaModule):
    config_model_class = WorkspacesConfigModel

    def __init__(self):
        super().__init__()
        self.wm_service: WindowManagerService | None = None
        self.gadgets: list[WorkSpacesGadget] = [] # just for typing

    def module_init(self):
        self.wm_service = WindowManagerService()
        self.wm_service.connect('changed', self._changed_event_cb)
        self.wm_service.connect('activated', self._activated_event_cb)

    def module_shutdown(self):
        self.wm_service.disconnect('changed', self._changed_event_cb)
        self.wm_service.disconnect('activated', self._activated_event_cb)

    def gadget_factory(self, ctx: GadgetRunContext) -> AriaGadget | None:
        conf: WorkspacesConfigModel = ctx.config  # noqa
        return WorkSpacesGadget(conf, ctx.monitor.get_connector())

    def _changed_event_cb(self):
        for instance in self.gadgets:
            instance.update()

    def _activated_event_cb(self, item: Window | Workspace):
        if isinstance(item, Window):
            for instance in self.gadgets:
                instance.set_active_window(item.id)
                if item and item.workspace_id:
                    instance.set_active_workspace(item.workspace_id)
        elif isinstance(item, Workspace):
            for instance in self.gadgets:
                instance.set_active_window(None)
                instance.set_active_workspace(item.id)


class WorkSpacesGadget(AriaGadget):
    def __init__(self, conf: WorkspacesConfigModel, monitor_name: str):
        super().__init__('workspaces')
        self.conf = conf
        self.monitor_name = monitor_name
        self._win_index: dict[str, Gtk.Widget] = {}
        self._ws_index: dict[str, Gtk.Widget] = {}
        self._active_win_id: str | None = None
        self._active_ws_id: str | None = None
        self.icon_service = XDGDesktopService()
        self.wm_service = WindowManagerService()
        self.update()

    def clear(self):
        while child := self.get_last_child():
            self.remove(child)
        self._win_index.clear()
        self._ws_index.clear()

    def update(self):
        self.clear()

        # get the id of the monitor this panel belong to
        for mon_id, mon in self.wm_service.monitors.items():
            if mon.name == self.monitor_name:
                break
        else:
            mon_id = None

        for ws_id, ws in self.wm_service.workspaces.items():
            # skip workspaces from other monitors
            if not self.conf.all_monitors and mon_id and ws.monitor_id != mon_id:
                continue

            # the workspace box
            box = AriaBox('aria-workspace')
            box.set_cursor_from_name('pointer')
            self._ws_index[ws.id] = box

            # make the box clickable
            ges = Gtk.GestureSingle()
            self.safe_connect(ges, 'begin', self.on_workspace_click, ws)
            box.add_controller(ges)

            # the workspace label
            if self.conf.show_name:
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
                        self.safe_connect(ges, 'begin', self.on_window_click, win)
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

    def set_active_workspace(self, wid: str|None):
        # deactivate currently active workspace object
        if obj := self._ws_index.get(self._active_ws_id):
            obj.remove_css_class('active')
            self._active_ws_id = None

        # activate new workspace object
        if obj := self._ws_index.get(wid):
            obj.add_css_class('active')
            self._active_ws_id = wid

    def set_active_window(self, wid: str|None):
        # deactivate currently active window object
        if obj := self._win_index.get(self._active_win_id):
            obj.remove_css_class('active')
            self._active_win_id = None

        # activate new window object
        if obj := self._win_index.get(wid):
            obj.add_css_class('active')
            self._active_win_id = wid
