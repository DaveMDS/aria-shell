from operator import attrgetter

from gi.repository import GObject, GLib, Gio, Gdk, Gtk
from gi.repository import Gtk4LayerShell as GtkLayerShell

from aria_shell.i18n import i18n
from aria_shell.services.xdg import XDGDesktopService, DesktopApp
from aria_shell.ui import AriaWindow, AriaBox
from aria_shell.utils import clamp, PerfTimer
from aria_shell.config import AriaConfigModel
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class LauncherConfig(AriaConfigModel):
    icon_size: int = 48
    opacity: int = 100
    width: int = 400
    height: int = 400

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


class AriaLauncher(AriaWindow):
    def __init__(self, app: Gtk.Application):
        super().__init__(css_class='aria-launcher')
        self.set_application(app)
        self.conf = LauncherConfig(app.conf.section('launcher'))
        self._setup_window()

        # declare internal widgets
        self.list_store = Gio.ListStore()
        self.list_view: Gtk.ListView | None = None
        self.search_entry: Gtk.Entry | None = None
        self._populate_window()

        # init all providers
        self.providers = [
            ApplicationsProvider(),
        ]

        # perform a first search
        self._on_entry_changed(self.search_entry, '')

    def _setup_window(self):
        self.set_decorated(False)
        self.set_opacity(self.conf.opacity / 100.0)
        self.set_size_request(self.conf.width, self.conf.height)

        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_namespace(self, 'aria-launcher')
        GtkLayerShell.set_layer(self, GtkLayerShell.Layer.OVERLAY)
        # GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.TOP, True)
        # GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.BOTTOM, True)
        GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.ON_DEMAND)

        ec = Gtk.EventControllerKey()
        ec.connect('key-pressed', self._on_win_key_pressed)
        self.add_controller(ec)

    def _populate_window(self):
        vbox = AriaBox(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        vbox.add_css_class('aria-launcher-box')

        # search Entry
        self.search_entry = entry = Gtk.Entry(
            placeholder_text=i18n('launcher.search'))
        entry.set_icon_from_icon_name(Gtk.EntryIconPosition.PRIMARY, 'search')
        entry.add_css_class('aria-launcher-entry')
        entry.connect('notify::text', self._on_entry_changed)
        entry.connect('activate', self.run_selected)
        vbox.append(entry)

        # ListView in a scroller
        self.list_view = self._create_listview()
        scroller = Gtk.ScrolledWindow()
        scroller.set_child(self.list_view)
        vbox.append(scroller)

        self.set_child(vbox)

    def _create_listview(self) -> Gtk.ListView:
        # listview
        list_view = Gtk.ListView(vexpand=True, focusable=False,
                                 single_click_activate=True,
                                 tab_behavior=Gtk.ListTabBehavior.ITEM)
        list_view.add_css_class('aria-launcher-list')
        list_view.connect('activate', self.run_selected)

        # selection model
        ssel = Gtk.SingleSelection(autoselect=True)
        ssel.set_model(self.list_store)
        list_view.set_model(ssel)

        # item factory
        factory = Gtk.SignalListItemFactory()
        factory.connect('setup', self._factory_item_setup)
        factory.connect('bind', self._factory_item_bind)
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
        super().show()

    def hide(self):
        super().hide()

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

    def launch(self):
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
