from pathlib import Path

from gi.repository import Gtk, Gio, GLib

from aria_shell.i18n import i18n
from aria_shell.services.themes import ThemesService
from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule, GadgetRunContext
from aria_shell.config import AriaConfigModel
from aria_shell.gadget import AriaGadget


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class ThemeSelectorConfigModel(AriaConfigModel):
    light_theme: str = ''
    dark_theme: str = ''
    favorites: list[str] = []
    show_user_themes: bool = True
    show_system_themes: bool = True
    show_icon_themes: bool = True
    icon_theme: str = ''  # force icon theme, or 'ignore' to not touch icons
    icon_name: str = 'preferences-color'


class ThemeSelectorModule(AriaModule):
    config_model_class = ThemeSelectorConfigModel

    def gadget_new(self, ctx: GadgetRunContext) -> AriaGadget | None:
        super().gadget_new(ctx)
        return ThemeSelectorGadget(ctx.config)  # noqa - pycharm error?


class ThemeSelectorGadget(AriaGadget):
    ACTION_GROUP = 'menu-items'
    ACTION_NAME = 'action1'

    def __init__(self, config: ThemeSelectorConfigModel):
        super().__init__('themes_selector', clickable=True)
        self.config = config
        self.themes_service = ThemesService()

        # the gadget is just a single icon
        self.icon = Gtk.Image.new_from_icon_name(self.config.icon_name)
        self.append(self.icon)

        # the action called by all menu items
        actions = Gio.SimpleActionGroup()
        action = Gio.SimpleAction.new(self.ACTION_NAME, GLib.VariantType('s'))
        action.connect('activate', self.on_menu_item_activate)
        actions.add_action(action)
        self.insert_action_group(self.ACTION_GROUP, actions)

    def on_menu_item_activate(self, _action: Gio.SimpleAction, theme: GLib.Variant):
        theme: str = theme.get_string()
        if theme.startswith('icon:'):
            # change the icon theme
            self.themes_service.set_icon_theme(Path(theme[5:]))
        else:
            # change the whole desktop theme
            self.themes_service.set_active_theme(
                theme, icon_theme=self.config.icon_theme
            )

    def make_menu_item(self, label: str, name: str, append_to: Gio.Menu):
        item = Gio.MenuItem.new(label, f'{self.ACTION_GROUP}.{self.ACTION_NAME}')
        item.set_attribute_value('target', GLib.Variant('s', name))
        append_to.append_item(item)

    def build_menu_model(self) -> Gio.Menu:
        menu = Gio.Menu()
        make_item = self.make_menu_item

        # light/dark themes - from config file
        if self.config.light_theme:
            make_item(i18n('themes.light'), self.config.light_theme, menu)
        if self.config.dark_theme:
            make_item(i18n('themes.dark'), self.config.dark_theme, menu)

        # favorites - from config file
        if self.config.favorites:
            section = Gio.Menu()
            for theme in self.config.favorites:
                make_item(theme, theme, section)
            menu.append_section(i18n('themes.favorite'), section)

        # user themes - from ~/.themes
        if self.config.show_user_themes:
            if themes := self.themes_service.get_user_themes():
                section = Gio.Menu()
                for theme in themes:
                    make_item(theme.name, theme.folder.name, section)
                menu.append_section(i18n('themes.user'), section)

        # system themes - from /usr/share/themes
        if self.config.show_system_themes:
            if themes := self.themes_service.get_system_themes():
                section = Gio.Menu()
                for theme in themes:
                    make_item(theme.name, theme.folder.name, section)
                menu.append_section(i18n('themes.system'), section)

        # icon themes - all
        if self.config.show_icon_themes:
            if icon_themes := self.themes_service.get_icon_themes():
                section = Gio.Menu()
                for icon_theme in icon_themes:
                    make_item(icon_theme.name, f'icon:{icon_theme}', section)
                menu.append_section(i18n('themes.icon_themes'), section)

        return menu

    def on_mouse_down(self, button: int):
        # create the menu model and the popover menu
        menu_model = self.build_menu_model()
        popover = Gtk.PopoverMenu(menu_model=menu_model)
        # popover.connect('closed', lambda _: print('menu closed'))
        popover.set_parent(self.icon)
        popover.popup()
