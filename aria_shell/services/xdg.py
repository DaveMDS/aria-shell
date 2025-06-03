from operator import attrgetter

from gi.repository import Gtk, Gdk, Gio

from aria_shell.config import AriaConfig
from aria_shell.utils import Singleton, PerfTimer, exec_detached
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class DesktopApp:
    def __init__(self, gapp: Gio.DesktopAppInfo):
        self._gapp = gapp

    def __repr__(self):
        return f"<DesktopApp id='{self.id}' name='{self.name}'>"

    @property
    def id(self) -> str:
        return self._gapp.get_id() or self.name

    @property
    def name(self) -> str:
        return self._gapp.get_name() or self.id

    @property
    def display_name(self) -> str:
        return self._gapp.get_display_name() or self.name or self.id

    @property
    def description(self) -> str | None:
        return self._gapp.get_description()

    @property
    def command_line(self) -> str | None:
        return self._gapp.get_commandline()

    @property
    def executable(self) -> str | None:
        return self._gapp.get_executable()

    @property
    def icon_name(self) -> str | None:
        icon: Gio.ThemedIcon = self._gapp.get_icon()  # noqa
        if icon and hasattr(icon, 'get_names'):
            if names := icon.get_names():
                return names[0]

    def get_icon(self) -> Gtk.Image:
        return Gtk.Image.new_from_icon_name(self.icon_name)

    # def launch(self) -> bool:
    #     """ Run the app using DesktopApp.launch()
    #     This should be the preferred version, also support startup-notify
    #     and other goodies. But the launched process will exit with aria-shel
    #     Must find a way to change that behaviour
    #     """
    #     return self._gapp.launch()

    # def launch_uwsm(self) -> None:
    #     """
    #     Launch the application using UWSM (Universal Wayland Session Manager).
    #     """
    #     self.launch(command_format="uwsm app -- %command%")

    def launch(self) -> bool:
        """ Run the app using Popen and gtk-launch """
        DBG(f'Running app: {self.id} with executable: {self.executable}')

        # use gtk-launch, it handles open in terminal and dbus activatable
        return exec_detached(['gtk-launch', self.id])



class XDGDesktopService(metaclass=Singleton):
    """ Implement XDG DesktopFile and XDGIcons """
    def __init__(self):
        # hack for clients with unusual window class
        self.apps_class_map = AriaConfig().section('apps_class_map')

        # init icon theme
        self.icon_theme = Gtk.IconTheme.get_for_display(Gdk.Display.get_default())
        INF(f'Using XDG icon theme: {self.icon_theme.get_theme_name()}')

        # init .desktop apps "database"
        t = PerfTimer()
        self.apps: dict[str, DesktopApp] = {}
        for gapp in Gio.AppInfo.get_all():
            if not gapp.should_show():
                continue
            aid = gapp.get_id() or gapp.get_name()
            aid = aid.removesuffix('.desktop').lower()
            self.apps[aid] = DesktopApp(gapp=gapp)  # noqa
        INF(f'Loaded {len(self.apps)} .desktop apps (in {t.elapsed})')

        # TODO: Gio.AppInfoMonitor to keep db uptodate

    @staticmethod
    def get_icon(icon_name: str) -> Gtk.Image:
        """ Create an icon by icon name, respect XDG IconTheme """
        return Gtk.Image.new_from_icon_name(icon_name)

    def get_icon_for_window_class(self, class_name: str) -> Gtk.Image:
        """ Search an icon for the given window class """
        # search an app with the given name
        app = self.search_app(class_name)
        if not app and (fixed_name := self.apps_class_map.get(class_name)):
            app = self.search_app(fixed_name)
        if app and app.icon_name and self.icon_theme.has_icon(app.icon_name):
            return Gtk.Image.new_from_icon_name(app.icon_name)
        # as a fallback try in icon_theme directly
        return Gtk.Image.new_from_icon_name(class_name)

    def get_app(self, app_name: str) -> DesktopApp | None:
        """ Get an application by name """
        return self.apps.get(app_name)

    def all_apps(self, sort=True) -> list[DesktopApp]:
        if sort:
            return sorted(self.apps.values(), key=attrgetter('display_name'))
        else:
            return list(self.apps.values())

    def search_app(self, text: str) -> DesktopApp | None:
        """ Search in apps database """
        text = text.lower()

        # a fast perfect id match
        if app := self.apps.get(text):
            return app

        # search text in .id and .name
        for app in self.apps.values():
            if app.id and app.id.find(text) > -1:
                return app
            if app.name and app.name.lower().find(text) > -1:
                return app

        # search 'app_name' in .display_name and .description
        for app in self.apps.values():
            if app.display_name and app.display_name.lower().find(text) > -1:
                return app
            if app.description and app.description.lower().find(text) > -1:
                return app

        return None
