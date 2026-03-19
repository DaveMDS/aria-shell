try:
    import gi
    gi.require_version('Vte', '3.91')
    from gi.repository import Vte
    _vte_available = True
except (ImportError, ValueError):
    _vte_available = False

from gi.repository import GLib, Gdk, Gtk, Pango
from gi.repository import Gtk4LayerShell as GtkLayerShell

from aria_shell.ui import AriaWindow
from aria_shell.utils import clamp
from aria_shell.utils.env import HOME, SHELL
from aria_shell.config import AriaConfig, AriaConfigModel
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class TerminalConfig(AriaConfigModel):
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


class AriaTerminal(AriaWindow):
    def __init__(self, app: Gtk.Application):
        INF('Initialize Aria Terminal')
        if not _vte_available:
            raise RuntimeError('Vte not available')

        self.conf = AriaConfig().section('terminal', TerminalConfig)

        super().__init__(
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
        INF('Shutting down Aria Terminal')
        self.terminal = None
        super().destroy()

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
        ec.connect('key-pressed', self._on_key_pressed)
        term.add_controller(ec)

        self.terminal = term
        self.set_child(term)

    def show(self):
        if self.terminal is None:
            self._create_terminal()
        super().show()
        self.terminal.grab_focus()

    def _on_child_exited(self, term, status: int):
        # shell exited, hide the window and destroy the terminal,
        # a new one will be recreated on next show()
        self.hide()
        self.set_child(None)
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
