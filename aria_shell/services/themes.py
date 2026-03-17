from pathlib import Path

from gi.repository import Gio

from aria_shell.utils import Singleton
from aria_shell.utils.env import HOME
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


USER_THEMES_DIR = HOME / '.themes'
SYSTEM_THEMES_DIR = Path('/usr/share/themes')


class ThemesService(metaclass=Singleton):
    """
    Get info about installed FDO system themes, can also set the current theme.

    Support themes with a theme.index spec file, and is able to
    apply icons and cursor themes read from the theme spec file.
    """
    def __init__(self):
        super().__init__()
        self.gsettings = g = Gio.Settings.new('org.gnome.desktop.interface')
        for key in ('gtk-theme', 'icon-theme', 'cursor-theme', 'font-name', 'color-scheme'):
            DBG('%s: %s', key, g.get_string(key))

        # def _gsettings_changed_cb(gsettings: Gio.Settings, field: str):
        #     DBG('GSetting changed: %s value: %s', field,
        #         gsettings.get_string(field))
        #
        # self.gsettings.connect('changed', _gsettings_changed_cb)

    def get_themes(self) -> list[DesktopTheme]:
        """Get all available themes."""
        return self.get_user_themes() + self.get_system_themes()

    def get_user_themes(self) -> list[DesktopTheme]:
        """Get only the themes from ~/.themes."""
        return self._scan_folder(HOME / '.themes')

    def get_system_themes(self) -> list[DesktopTheme]:
        """Get only the themes from /usr/share/themes."""
        return self._scan_folder(Path('/usr/share/themes'))

    @staticmethod
    def _scan_folder(path: Path) -> list[DesktopTheme]:
        DBG('Searching for themes in: %s', path)
        themes = []
        if path.is_dir():
            for p in sorted(path.iterdir()):
                try:
                    theme = DesktopTheme(p)
                except FileNotFoundError:
                    pass
                else:
                    themes.append(theme)
        return themes

    def get_active_theme(self) -> DesktopTheme | None:
        """Get the currently active theme."""
        try:
            return DesktopTheme(self.gsettings.get_string('gtk-theme'))
        except FileNotFoundError:
            return None

    def set_active_theme(self, theme: DesktopTheme | Path | str):
        """Change the current theme."""
        if isinstance(theme, DesktopTheme):
            gtk_theme = theme.folder.name
        elif isinstance(theme, Path):
            gtk_theme = theme.name
        else:
            gtk_theme = theme

        INF('Setting system theme: %s', gtk_theme)
        self.gsettings.set_string('gtk-theme', gtk_theme)

        # TODO parse index.theme and appy icons and cursors!

        # TODO copy the gtk-4.0 folder from theme in ~/.config/gtk-4.0
        # seems totally wrong to me....


class DesktopTheme:
    """
    Class to search, parse and represent a theme from an index.theme file.

    Args:
        theme: can be the theme name or the full path of the theme folder
    Raises:
        FileNotFoundError: index.theme cannot be found and the is invalid
    """
    def __init__(self, theme: Path | str):
        theme = Path(theme) if isinstance(theme, str) else theme

        if theme.is_absolute():
            # full path given, check the index.theme file
            if (theme / 'index.theme').exists():
                self._folder = theme
            else:
                raise FileNotFoundError
        else:
            # theme name given, search in user and system themes folder
            for path in (USER_THEMES_DIR, SYSTEM_THEMES_DIR):
                theme_dir = path / theme
                if (theme_dir / 'index.theme').exists():
                    self._folder =  theme_dir
                    break
            else:
                raise FileNotFoundError

    @property
    def folder(self) -> Path:
        return self._folder

    @property
    def name(self) -> str:
        return self._folder.name
