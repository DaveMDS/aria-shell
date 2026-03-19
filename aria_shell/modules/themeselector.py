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
    light_icon_theme: str = ''  # force icon theme, or 'ignore' to not touch icons
    dark_icon_theme: str = ''   # force icon theme, or 'ignore' to not touch icons
    icon_name: str = 'preferences-color'


class ThemeSelectorModule(AriaModule):
    config_model_class = ThemeSelectorConfigModel

    def gadget_factory(self, ctx: GadgetRunContext) -> AriaGadget | None:
        return ThemeSelectorGadget(ctx.config)  # noqa - pycharm error?


class ThemeSelectorGadget(AriaGadget):
    ACTION_GROUP = 'menu-actions'

    def __init__(self, config: ThemeSelectorConfigModel):
        super().__init__('themes_selector', clickable=True)
        self.config = config
        self.themes_service = ThemesService()

        # the gadget is just a single icon
        self.icon = Gtk.Image.new_from_icon_name(self.config.icon_name)
        self.append(self.icon)

        # create the 4 actions called by the menu items
        actions = Gio.SimpleActionGroup()
        for action_name in ('gtk-theme', 'icon-theme', 'dark', 'light'):
            action = Gio.SimpleAction.new(action_name, GLib.VariantType('s'))
            action.connect('activate', self.on_menu_item_activate, action_name)
            actions.add_action(action)
        self.insert_action_group(self.ACTION_GROUP, actions)

    def on_menu_item_activate(self, _, theme: GLib.Variant, action: str):
        theme: str = theme.get_string()
        match action:
            case 'gtk-theme' | 'light':
                self.themes_service.set_active_theme(
                    theme, icon_theme=self.config.light_icon_theme
                )
            case 'dark':
                self.themes_service.set_active_theme(
                    theme, icon_theme=self.config.dark_icon_theme
                )
            case 'icon-theme':
                self.themes_service.set_icon_theme(Path(theme))

    def make_menu_item(self, menu: Gio.Menu, label: str, value: str, action: str):
        item = Gio.MenuItem.new(label, f'{self.ACTION_GROUP}.{action}')
        item.set_attribute_value('target', GLib.Variant('s', value))
        menu.append_item(item)

    def build_menu_model(self) -> Gio.Menu:
        menu = Gio.Menu()
        make_item = self.make_menu_item

        # light/dark themes - from config file
        if self.config.light_theme:
            make_item(menu, i18n('themes.light'), self.config.light_theme, 'light')
        if self.config.dark_theme:
            make_item(menu, i18n('themes.dark'), self.config.dark_theme, 'dark')

        # favorites - from config file
        if self.config.favorites:
            section = Gio.Menu()
            for theme in self.config.favorites:
                make_item(section, theme, theme, 'gtk-theme')
            menu.append_section(i18n('themes.favorite'), section)

        # user themes - from ~/.themes
        if self.config.show_user_themes:
            if themes := self.themes_service.get_user_themes():
                section = Gio.Menu()
                for theme in themes:
                    make_item(section, theme.name, theme.folder.name, 'gtk-theme')
                menu.append_section(i18n('themes.user'), section)

        # system themes - from /usr/share/themes
        if self.config.show_system_themes:
            if themes := self.themes_service.get_system_themes():
                section = Gio.Menu()
                for theme in themes:
                    make_item(section, theme.name, theme.folder.name, 'gtk-theme')
                menu.append_section(i18n('themes.system'), section)

        # icon themes - all
        if self.config.show_icon_themes:
            if icon_themes := self.themes_service.get_icon_themes():
                section = Gio.Menu()
                for icon_theme in icon_themes:
                    make_item(section, icon_theme.name, str(icon_theme), 'icon-theme')
                menu.append_section(i18n('themes.icon_themes'), section)

        return menu

    def mouse_click(self, button: int):
        # create the menu model and the popover menu
        menu_model = self.build_menu_model()
        popover = Gtk.PopoverMenu(menu_model=menu_model)
        # popover.connect('closed', lambda _: print('menu closed'))
        popover.set_parent(self.icon)
        popover.popup()
