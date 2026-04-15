from typing import TYPE_CHECKING
from operator import attrgetter

from gi.repository import GObject, GLib, Gio, Gdk, Gtk

from aria_shell.components import AriaComponent
from aria_shell.i18n import i18n
from aria_shell.services.commands import AriaCommands, CommandFailed
from aria_shell.services.xdg import XDGDesktopService, DesktopApp
from aria_shell.ui import AriaWindow
from aria_shell.utils import clamp, PerfTimer, CleanupHelper
from aria_shell.config import AriaConfig, AriaConfigModel
from aria_shell.utils.logger import get_loggers
if TYPE_CHECKING:
    from aria_shell.ariashell import AriaShell


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class LauncherConfig(AriaConfigModel):
    icon_size: int = 48
    opacity: int = 100
    width: int = 400
    height: int = 400
    grab_display: bool = True

    @staticmethod
    def validate_icon_size(val: int):
        return clamp(val, 0, 512)

    @staticmethod
    def validate_opacity(val: int):
        return clamp(val, 0, 100)

    @staticmethod
    def validate_width(val: int):
        return clamp(val, 0, 10000)

    @staticmethod
    def validate_height(val: int):
        return clamp(val, 0, 10000)


class AriaLauncher(CleanupHelper, AriaComponent):
    def __init__(self, app: AriaShell):
        super().__init__(app)

        # get launcher config and register the launcher command
        self.conf = AriaConfig().section('launcher', LauncherConfig)
        AriaCommands().register('launcher', self.the_launcher_command)

        # declare internal widgets
        self.list_store = Gio.ListStore()
        self.list_view: Gtk.ListView | None = None
        self.search_entry: Gtk.Entry | None = None

        # crete the window
        self.win = AriaWindow(
            app=app,
            namespace='aria-launcher',
            title='Aria launcher',
            layer=AriaWindow.Layer.OVERLAY,
            grab_display=self.conf.grab_display,
            opacity=self.conf.opacity / 100.0,
            size_request=(self.conf.width, self.conf.height),
        )
        self._populate_window()

        # request keyboard events
        ec = Gtk.EventControllerKey()
        self.safe_connect(ec, 'key-pressed', self._on_win_key_pressed)
        self.win.add_controller(ec)

        # init all providers
        self.providers = [
            ApplicationsProvider(),
        ]

        # perform a first search
        self._on_entry_changed(self.search_entry, '')

    def shutdown(self):
        AriaCommands().unregister('launcher')
        # TODO shutdown properly each provider !!!!!!!!!!!
        self.win.shutdown()
        self.win = None
        self.providers = []
        self.list_store = None
        self.list_view = None
        self.search_entry = None
        super().shutdown()

    def the_launcher_command(self, _, params: list[str]) -> None:
        """Runner for the 'launcher' aria command."""
        if not params or params[0] == 'toggle':
            self.hide() if self.win.is_visible() else self.show()
        elif params and params[0] == 'hide':
            self.hide()
        elif params and params[0] == 'show':
            self.show()
        else:
            raise CommandFailed('Invalid arguments for the <launcher> command')

    def _populate_window(self):
        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        vbox.add_css_class('aria-launcher-box')

        # search Entry
        self.search_entry = entry = Gtk.Entry(
            placeholder_text=i18n('launcher.search'))
        entry.set_icon_from_icon_name(Gtk.EntryIconPosition.PRIMARY, 'search')
        entry.add_css_class('aria-launcher-entry')
        self.safe_connect(entry, 'notify::text', self._on_entry_changed)
        self.safe_connect(entry, 'activate', self.run_selected)
        vbox.append(entry)

        # ListView in a scroller
        self.list_view = self._create_listview()
        scroller = Gtk.ScrolledWindow()
        scroller.set_child(self.list_view)
        vbox.append(scroller)

        self.win.set_child(vbox)

    def _create_listview(self) -> Gtk.ListView:
        # listview
        list_view = Gtk.ListView(vexpand=True, focusable=False,
                                 single_click_activate=True,
                                 tab_behavior=Gtk.ListTabBehavior.ITEM)
        list_view.add_css_class('aria-launcher-list')
        self.safe_connect(list_view, 'activate', self.run_selected)

        # selection model
        ssel = Gtk.SingleSelection(autoselect=True)
        ssel.set_model(self.list_store)
        list_view.set_model(ssel)

        # item factory
        factory = Gtk.SignalListItemFactory()
        self.safe_connect(factory, 'setup', self._factory_item_setup)
        self.safe_connect(factory, 'bind', self._factory_item_bind)
        list_view.set_factory(factory)

        return list_view

    def _factory_item_setup(self, _factory, item: Gtk.ListItem):
        # create a new item object for the list
        hbox = Gtk.Box(spacing=6, hexpand=True,
                       margin_top=2, margin_bottom=2,
                       margin_start=2, margin_end=2)
        # icon
        ico = Gtk.Image(pixel_size=self.conf.icon_size)
        hbox.append(ico)
        # label
        lbl = Gtk.Label(wrap=True, hexpand=True, xalign=0)
        hbox.append(lbl)
        item.set_child(hbox)

    @staticmethod
    def _factory_item_bind(_factory, list_item: Gtk.ListItem):
        # fill the item object with the item data
        item: LauncherItem = list_item.get_item()  # noqa

        hbox: Gtk.Box = list_item.get_child()  # noqa
        ico: Gtk.Image = hbox.get_first_child()  # noqa
        lbl: Gtk.Label = hbox.get_last_child()  # noqa

        title = GLib.markup_escape_text(item.title, -1)
        subtitle = GLib.markup_escape_text(item.subtitle or '', -1)

        ico.set_from_icon_name(item.icon_name)
        lbl.set_markup(
            f'{title}\n'
            f'<span font_size="small" alpha="60%">{subtitle}</span>'
        )

    def _on_entry_changed(self, *_):
        t = PerfTimer()
        # get results from all providers
        items = []
        for provider in self.providers:
            items.extend(provider.search(self.get_search_text()))

        # repopulate the list store
        self.list_store.remove_all()
        for item in sorted(items, key=attrgetter('priority'), reverse=True):
            self.list_store.append(item)

        DBG(f'Found {len(items)} results in {t.elapsed}')


    def _on_win_key_pressed(self, _ec: Gtk.EventControllerKey, keyval: int,
                            _keycode: int, _state: Gdk.ModifierType):
        match keyval:
            case Gdk.KEY_Escape:
                self.hide()
            case Gdk.KEY_Return:
                self.run_selected()
            case Gdk.KEY_Tab:
                return True  # try not to  lose focus
            case Gdk.KEY_Up | Gdk.KEY_Down:
                model: Gtk.SingleSelection = self.list_view.get_model()  # noqa
                nitems = model.get_n_items()
                selected = model.get_selected()
                if nitems and selected != Gtk.INVALID_LIST_POSITION:
                    if keyval == Gdk.KEY_Up and selected > 0:
                        selected -= 1
                    if keyval == Gdk.KEY_Down and selected < nitems - 1:
                        selected += 1
                    self.list_view.scroll_to(
                        selected, Gtk.ListScrollFlags.SELECT
                    )
            case _:
                # print("KEY", keyval, keycode, state, self)
                return False  # not handled, continue propagation

        return True  # handled, stop event propagation

    def show(self):
        self.reset()
        self.win.show()
        self.win.grab_focus()

    def hide(self):
        self.win.hide()

    def reset(self):
        self.search_entry.set_text('')
        self.list_view.scroll_to(0, Gtk.ListScrollFlags.SELECT)

    def get_search_text(self) -> str:
        return self.search_entry.get_text().strip().lower()

    def run_selected(self, *_):
        model: Gtk.SingleSelection = self.list_view.get_model()  # noqa
        item: LauncherItem = model.get_selected_item()  # noqa
        item.selected()
        self.hide()


class LauncherItem(GObject.Object):
    """ Abstract base class for launcher items """
    @property
    def priority(self) -> float:
        raise NotImplementedError

    @property
    def title(self) -> str:
        raise NotImplementedError

    @property
    def subtitle(self) -> str:
        raise NotImplementedError

    @property
    def icon_name(self) -> str:
        raise NotImplementedError

    def selected(self):
        raise NotImplementedError

    def __repr__(self):
        return f"<LauncherItem '{self.title}' prio={self.priority}>"


class LauncherProvider:
    """ Base class for all items providers """
    def search(self, text: str) -> list[LauncherItem]:
        raise NotImplementedError


################################################################################
# DGX DestopApp search provider
################################################################################
class ApplicationItem(LauncherItem):
    """ Items returned by the ApplicationProvider """
    def __init__(self, app: DesktopApp, priority: float):
        super().__init__()
        self._app = app
        self._priority = priority

    @property
    def priority(self):
        return self._priority

    @property
    def title(self):
        return self._app.display_name

    @property
    def subtitle(self):
        return self._app.description

    @property
    def icon_name(self):
        return self._app.icon_name

    def selected(self):
        self._app.launch()


class ApplicationsProvider(LauncherProvider):
    """" XDG Desktop App search provider """
    def __init__(self):
        self.xdg_service = XDGDesktopService()
        self.apps = self.xdg_service.all_apps()

    def search(self, search: str) -> list[LauncherItem]:
        results = []
        for app in self.apps:
            prio = 0
            if not search:
                prio = 1
            else:
                for field in app.id, app.name, app.description:
                    if not field:
                        continue
                    field = field.lower()
                    if search == field:
                        prio = 10
                        break
                    elif field.startswith(search):
                        prio = 8
                        break
                    elif search in field:
                        prio = 6
                        break

            if prio > 0:
                results.append(ApplicationItem(app, priority=prio))

        return results
