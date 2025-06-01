from pathlib import Path
import os

#
# Environment
UID = os.getuid()
HOME = Path.home()


#
# XDG basedir
XDG_RUNTIME_DIR = Path(
    os.getenv('XDG_RUNTIME_DIR') or f'/run/user/{UID}'
)
XDG_CONFIG_HOME = Path(
    os.getenv('XDG_CONFIG_HOME') or HOME / '.config'
)
XDG_CONFIG_DIRS = os.getenv('XDG_CONFIG_DIRS') or '/etc/xdg'
XDG_CONFIG_DIRS = XDG_CONFIG_DIRS.split(':')
XDG_CONFIG_DIRS = list(map(Path, XDG_CONFIG_DIRS))


#
# Aria directories
ARIA_RUNTIME_DIR = XDG_RUNTIME_DIR / 'aria-shell'
ARIA_RUNTIME_DIR.mkdir(parents=True, exist_ok=True)

ARIA_CONFIG_HOME = XDG_CONFIG_HOME / 'aria-shell'
ARIA_CONFIG_HOME.mkdir(parents=True, exist_ok=True)

ARIA_PACKAGE_DIR = Path(__file__).resolve().parent.parent
ARIA_ASSETS_DIR = ARIA_PACKAGE_DIR / 'assets'


def lookup_config_file(filename: str, prefix='aria-shell') -> Path | None:
    """ Search the first conf file in standard system dirs
    1. ~/.config/aria-shell/filename (XDG_CONFIG_HOME)
    2. /etc/xdg/aria-shell/filename (all XDG_CONFIG_DIRS)
    3. aria python package assets dir
    """
    for path in (XDG_CONFIG_HOME, *XDG_CONFIG_DIRS):
        file = path / prefix / filename
        if file.exists():
            return file
    file = ARIA_ASSETS_DIR / filename
    if file.exists():
        return file