from __future__ import annotations

from traceback import print_tb
from typing import Mapping

from gi.repository import GObject, Gio, Gdk, Gtk, Gtk4LayerShell as GtkLayerShell  # noqa

from aria_shell.services.xdg import XDGDesktopService, DesktopApp
from aria_shell.utils import clamp
from aria_shell.config import AriaConfigModel


class LauncherConfig(AriaConfigModel):
    icon_size: int = 48
    # outputs: list[str] = 'all'
    # position: str = 'top'
    # layer: str = 'bottom'
    # size: str = 'fill'
    # align: str = 'center'
    # margin: int = 0
    # ontheleft: list[str] = []
    #
    # @staticmethod
    # def validate_opacity(val: int):
    #     return clamp(val, 0, 100)
    #
    # @staticmethod
    # def validate_size(val: str):
    #     if val not in SIZES:
    #         raise ValueError(f'Invalid size "{val}" for panel. '
    #                          'Allowed values: ' + ','.join(SIZES))
    #     return val
    #


class ListBoxItem(Gtk.ListBoxRow):
    """ TODO: abstract from DestopApp, to support different search provider """
    def __init__(self, app: DesktopApp, icon_size=32):
        super().__init__()
        self.app = app

        box = Gtk.Box(spacing=6, margin_top=2, margin_bottom=2,
                      margin_start=2, margin_end=2)

        ico = app.get_icon()
        ico.set_pixel_size(icon_size)
        box.append(ico)

        lbl = Gtk.Label(wrap=True, hexpand=True, xalign=0)
        lbl.set_markup(
            f'{app.display_name}\n'
            f'<span font_size="small" alpha="60%">{app.description}</span>'
        )
        box.append(lbl)

        self.set_child(box)

    def match(self, search: str) -> bool:
        """ TODO: some more fuzzy match? """
        if not search:
            return True
        s, a = search.lower(), self.app
        if a.display_name and s in a.display_name.lower():
            return True
        if a.name and s in a.name.lower():
            return True
        if a.description and s in a.description.lower():
            return True
        return False


class AriaLauncher(Gtk.Window):
    def __init__(self, app: Gtk.Application):
        super().__init__()
        self.set_application(app)
        self.add_css_class('aria-launcher')

        self.entry: Gtk.Entry | None = None
        # self.list_view: Gtk.ListView | None = None
        self.list_box: Gtk.ListBox | None = None
        self.list_store: Gio.ListStore | None = None

        self.xdg_service = XDGDesktopService()
        self.conf = LauncherConfig(app.conf.section('launcher'))
        self.setup_window()
        self.populate_window()

        ec = Gtk.EventControllerKey()
        ec.connect('key-pressed', self.on_key_pressed)
        self.add_controller(ec)

        # self.connect('destroy', self.on_destroy)
        # self.show()


    def setup_window(self):
        # configure the window
        self.set_decorated(False)
        # self.set_opacity(self.conf.opacity / 100.0)

        # if self.conf.size == 'fill':
        #     geom = self.monitor.get_geometry()
        self.set_size_request(400, 400)  # TODO fixme

        # GtkLayerShell stuff
        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_namespace(self, 'aria-launcher')
        GtkLayerShell.set_layer(self, GtkLayerShell.Layer.OVERLAY)
        # GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.TOP, True)
        # GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.BOTTOM, True)
        # GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.LEFT, True)
        # GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.RIGHT, True)
        GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.ON_DEMAND)

    def populate_window(self):
        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)

        # search search_entry
        self.entry = entry = Gtk.Entry(placeholder_text='Search...')
        entry.set_icon_from_icon_name(Gtk.EntryIconPosition.PRIMARY, 'search')
        entry.add_css_class('aria-launcher-search_entry')
        entry.connect('notify::text', lambda *_: self.list_box.invalidate_filter())
        vbox.append(entry)

        # ListView in a scroller
        self.list_box = lbox = Gtk.ListBox(
            vexpand=True, activate_on_single_click=True
        )
        lbox.add_css_class('aria-launcher-list')
        lbox.set_filter_func(self.list_filter_cb)
        scroller = Gtk.ScrolledWindow()
        scroller.set_child(lbox)
        vbox.append(scroller)

        # populate the list
        for a in self.xdg_service.all_apps():
            lbox.append(ListBoxItem(a, self.conf.icon_size))
        lbox.select_row(lbox.get_row_at_index(0))

        # btn = Gtk.Button(focusable=False, label='Close !')
        # btn.connect('clicked', lambda *_: self.hide())
        # vbox.append(btn)

        self.set_child(vbox)

    def list_filter_cb(self, item: ListBoxItem) -> bool:
        return item.match(self.entry.get_text())

    @staticmethod
    def run_selected():
        # model: Gtk.SingleSelection = self.list_view.get_model()  # noqa
        # item = model.get_selected_item()
        # print('RUN SEL', item.app)

        # TODO !!!!

        print('RUN SEL')

    def show(self):
        super().show()

    def hide(self):
        super().hide()

    def on_key_pressed(self, _ec: Gtk.EventControllerKey,
                       keyval: int, keycode: int, state: Gdk.ModifierType):
        print("KEY", keyval, keycode, state, self)
        # if  state & Gdk.ModifierType.CONTROL_MASK:
        match keyval:
            case Gdk.KEY_Escape:
                self.hide()

            case Gdk.KEY_Up | Gdk.KEY_Down:
                # TODO: find a way to move selection without stealing focus
                self.list_box.emit(
                    'move_cursor',
                    Gtk.MovementStep.DISPLAY_LINES,
                    1 if keyval == Gdk.KEY_Down else -1,
                    False, False
                )
                print('daiii')
                # self.search_entry.grab_focus()

            case _:
                return False  # not, handled, continue propagation

        return True  # handled, stop event propagation




    """
    def setup_listview_(self) -> Gtk.Widget:
        # https://github.com/Taiko2k/GTK4PythonTutorial#using-gridview

        list_view = Gtk.ListView(vexpand=True, focusable=False,
                                 single_click_activate=True,
                                 tab_behavior=Gtk.ListTabBehavior.ITEM)
        list_view.connect('activate', lambda *_: self.run_seleted())


        # store
        self.list_store = Gio.ListStore()
        for a in self.xdg_service.all_apps():
            self.list_store.append(ResultListItem(a))

        # selection model
        ssel = Gtk.SingleSelection()
        ssel.set_model(self.list_store)
        list_view.set_model(ssel)

        # factory
        def f_setup(fact, item):
            print('f_setup', item)
            hbox = Gtk.Box(spacing=6, hexpand=True,
                           margin_top=2, margin_bottom=2,
                           margin_start=2, margin_end=2)
            # icon
            ico = Gtk.Image()
            ico.set_pixel_size(self.conf.icon_size)
            hbox.append(ico)

            # label
            # lbl = Gtk.Label(halign=Gtk.Align.FILL, wrap=True, hexpand=True,
            #                 justify=Gtk.Justification.LEFT)
            lbl = Gtk.Label(wrap=True, hexpand=True, xalign=0)
            hbox.append(lbl)
            item.set_child(hbox)

        def f_bind(fact, item):
            # TODO find a better way to get lbl, ico, etc
            print('f_bind', item)
            app: DesktopApp = item.get_item().app
            hbox: Gtk.Box = item.get_child()

            ico: Gtk.Image = hbox.get_first_child()  # noqa
            ico.set_from_icon_name(app.icon_name)

            lbl: Gtk.Label = hbox.get_last_child()  # noqa
            lbl.set_markup(
                f'{app.display_name}\n'
                f'<span font_size="small" alpha="60%">{app.description}</span>'
            )

        factory = Gtk.SignalListItemFactory()
        factory.connect('setup', f_setup)
        factory.connect('bind', f_bind)
        list_view.set_factory(factory)

        return list_view
    """
