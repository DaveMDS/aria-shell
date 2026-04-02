from gi.repository import Gtk, GObject

from aria_shell.gadget import AriaGadget
from aria_shell.ui import AriaBox
from aria_shell.utils import CleanupHelper
from aria_shell.services.wm import WindowManagerService, Workspace, Window
from aria_shell.services.xdg import XDGDesktopService
from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule, GadgetRunContext
from aria_shell.config import AriaConfigModel


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class WorkspacesConfig(AriaConfigModel):
    show_name: bool = True
    show_windows: bool = True
    all_monitors: bool = False
    focus_window_on_click: bool = False


class WorkSpacesModule(AriaModule):
    config_model_class = WorkspacesConfig

    def gadget_factory(self, ctx: GadgetRunContext) -> AriaGadget | None:
        conf: WorkspacesConfig = ctx.config  # noqa
        return WorkSpacesGadget(conf, ctx.monitor.get_connector())


class WorkSpacesGadget(AriaGadget):
    def __init__(self, config: WorkspacesConfig, monitor_name: str):
        super().__init__('workspaces')

        # get the list model full of Workspaces
        model = WindowManagerService().workspaces

        if not config.all_monitors:
            # filter the model based on the current monitor
            model = Gtk.FilterListModel(
                model=WindowManagerService().workspaces,
                filter=Gtk.CustomFilter.new(
                    lambda ws: ws.monitor == monitor_name
                )
            )

        # horizontal box binded to the workspaces list model
        box = AriaBox(orientation=Gtk.Orientation.HORIZONTAL)
        box.bind_model(model, WorkspaceView, config)

        self.append(box)


class WorkspaceView(CleanupHelper, Gtk.Box):
    """
    A Gtk.Widget that show a Workspace object.
    """
    def __init__(self, workspace: Workspace, config: WorkspacesConfig):
        super().__init__(orientation=Gtk.Orientation.HORIZONTAL)
        self.add_css_class('aria-workspace')
        self.set_cursor_from_name('pointer')

        # workspace tooltip (on the box itself)
        self.safe_bind(
            workspace, 'name', self, 'tooltip_text',
            transform_to=lambda _,val: f'Workspace\n{val}'
        )

        # workspace name label
        if config.show_name:
            label = Gtk.Label()
            label.add_css_class('aria-workspace-label')
            self.safe_bind(workspace, 'name', label, 'label')
            self.append(label)

        # windows list
        if config.show_windows:
            windows_box = AriaBox(orientation=Gtk.Orientation.HORIZONTAL)
            windows_box.bind_model(workspace.windows, WindowView, config)
            self.append(windows_box)

        # toggle the 'active' and 'urgent' CSS classes following props
        self.safe_connect(workspace, 'notify::active', sync_css_class, self)
        self.safe_connect(workspace, 'notify::urgent', sync_css_class, self)

        # controller to receive mouse click
        ges = Gtk.GestureSingle()
        self.safe_connect(ges, 'begin', lambda *_: workspace.activate())
        self.add_controller(ges)

    def do_unmap(self):
        CleanupHelper.shutdown(self)
        Gtk.Widget.do_unmap(self)


class WindowView(CleanupHelper, Gtk.Image):
    """
    A Gtk.Widget that show a Window object.
    """
    def __init__(self, window: Window, config: WorkspacesConfig):
        super().__init__()
        self.add_css_class('aria-workspace-window')

        icon = XDGDesktopService().get_icon_name_for_window_class(window.name)
        self.set_from_icon_name(icon)

        # window tooltip
        self.safe_bind(
            window, 'title', self, 'tooltip_text',
            transform_to=lambda _,val: f'{window.name}\n{window.title}'
        )

        # activate the window on click
        if config.focus_window_on_click:
            ges = Gtk.GestureSingle()
            self.safe_connect(ges, 'begin', lambda *_: window.activate())
            self.add_controller(ges)

        # toggle the 'active' and 'urgent' CSS classes following props
        self.safe_connect(window, 'notify::active', sync_css_class, self)
        self.safe_connect(window, 'notify::urgent', sync_css_class, self)

    def do_unmap(self):
        CleanupHelper.shutdown(self)  # disconnect all safe-connected signal/bindings
        Gtk.Widget.do_unmap(self)


def sync_css_class(obj: GObject.Object, param: GObject.ParamSpec, target: Gtk.Widget):
    """Utility to add/remove a target CSS class based on a bool property of obj."""
    if getattr(obj, param.name):
        target.add_css_class(param.name)
    else:
        target.remove_css_class(param.name)
