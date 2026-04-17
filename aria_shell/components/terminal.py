from typing import TYPE_CHECKING

try:
    import gi
    gi.require_version('Vte', '3.91')
    from gi.repository import Vte
except (ImportError, ValueError):
    Vte = None

from gi.repository import GLib, Gdk, Gtk, Pango
from gi.repository import Gtk4LayerShell as GtkLayerShell

from aria_shell.components import AriaComponent
from aria_shell.ui import AriaWindow
from aria_shell.utils import clamp, CleanupHelper
from aria_shell.utils.env import HOME, SHELL
from aria_shell.config import AriaConfig, AriaConfigModel
from aria_shell.services.commands import AriaCommands, CommandFailed
from aria_shell.utils.logger import get_loggers
if TYPE_CHECKING:
    from aria_shell.ariashell import AriaShell


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class TerminalConfig(AriaConfigModel):
    __section__ = 'terminal'

    opacity: int = 75
    cols: int = 120
    rows: int = 30
    font: str = 'HackNerdFont,SourceCodePro,DroidSansMono,terminus 11'
    shell: str = ''
    hide_on_esc: bool = True
    mouse_autohide: bool = True
    grab_display: bool = True

    @staticmethod
    def validate_opacity(val: int):
        return clamp(val, 0, 100)

    @staticmethod
    def validate_cols(val: int):
        return clamp(val, 10, 10000)

    @staticmethod
    def validate_rows(val: int):
        return clamp(val, 1, 10000)


class AriaTerminal(CleanupHelper, AriaComponent):
    def __init__(self, app: AriaShell):
        super().__init__(app)
        if Vte is None:
            raise RuntimeError('Vte4 not available, embedded terminal is disabled!')

        self.conf = AriaConfig().section(TerminalConfig)
        AriaCommands().register('terminal', self.the_terminal_command)

        self.win = AriaWindow(
            app=app,
            namespace='aria-terminal',
            title='Aria terminal',
            layer = AriaWindow.Layer.OVERLAY,
            grab_display=self.conf.grab_display,
            opacity=self.conf.opacity / 100.0,
            anchors=[AriaWindow.Edge.TOP],
            exclusive_zone=-1,
        )

        self.terminal: Vte.Terminal | None = None
        self._fullscreen: bool = False

    def shutdown(self):
        AriaCommands().unregister('terminal')
        self.terminal = None
        self.win.destroy()
        self.win = None
        super().shutdown()

    def the_terminal_command(self, _, params: list[str]) -> None:
        """Runner for the 'terminal' aria command."""
        if not params or params[0] == 'toggle':
            self.hide() if self.win.is_visible() else self.show()
        elif params and params[0] == 'hide':
            self.hide()
        elif params and params[0] == 'show':
            self.show()
        else:
            raise CommandFailed('Invalid arguments for the <terminal> command')

    def _toggle_fullscreen(self):
        """ Emulate fullscreen using LayerShell """
        self._fullscreen = fs = not self._fullscreen
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.TOP, True)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.BOTTOM, fs)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.LEFT, fs)
        GtkLayerShell.set_anchor(self, GtkLayerShell.Edge.RIGHT, fs)

    def _create_terminal(self):
        term = Vte.Terminal()
        term.set_css_name('Terminal')  # this does not seem to work  :/
        term.add_css_class('aria-terminal')
        term.set_size(self.conf.cols, self.conf.rows)
        # term.set_size_request(400, 400)
        term.set_mouse_autohide(self.conf.mouse_autohide)
        term.set_font(Pango.FontDescription.from_string(self.conf.font))
        term.connect('child-exited', self._on_child_exited)
        term.spawn_async(
            Vte.PtyFlags.DEFAULT,        # pty_flags
            HOME.as_posix(),             # working_directory
            [self.conf.shell or SHELL],  # argv
            ['VIRTUAL_ENV=', 'PYTHONHOME=', 'PYTHONPATH='],  # envv
            GLib.SpawnFlags.DEFAULT,     # spawn_flags
            None,   # child_setup function
            None,   # ??? documentation is wrong??? I cannot find this param! # noqa
            -1,     # timeout (-1 = default) # noqa
            None,   # cancellable  TODO !!
            None,   # callback
        )

        ec = Gtk.EventControllerKey()
        self.safe_connect(ec, 'key-pressed', self._on_key_pressed)
        term.add_controller(ec)

        self.terminal = term
        self.win.set_child(term)

    def show(self):
        if self.terminal is None:
            self._create_terminal()
        self.win.show()
        self.terminal.grab_focus()

    def hide(self):
        self.win.hide()

    def _on_child_exited(self, term, status: int):
        # shell exited, hide the window and destroy the terminal,
        # a new one will be recreated on next show()
        if self.win:
            self.win.hide()
            self.win.set_child(None)
        self.terminal = None

    def _on_key_pressed(self, _ec: Gtk.EventControllerKey,
                        keyval: int, _keycode: int, state: Gdk.ModifierType
                        ) -> bool:
        match keyval:
            case Gdk.KEY_Escape:
                if self.conf.hide_on_esc:
                    self.hide()
                    return True
            case Gdk.KEY_f:
                if state & Gdk.ModifierType.CONTROL_MASK:
                    self._toggle_fullscreen()
                    return True

        return False
