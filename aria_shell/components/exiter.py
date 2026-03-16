from collections.abc import Callable

from gi.repository import Gtk
from gi.repository import Gtk4LayerShell as GtkLayerShell

from aria_shell.ui import AriaWindow, AriaDialog
from aria_shell.utils import clamp, exec_detached, Timer
from aria_shell.config import AriaConfig, AriaConfigModel
from aria_shell.utils.logger import get_loggers
from aria_shell.i18n import i18n


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


BUTTONS = [
    # name         icon_name          confirm
    ('lock',      'system-lock-screen', False),
    ('suspend',   'system-suspend',     False),
    ('hibernate', 'system-hibernate',   False),
    ('logout',    'system-log-out',     True),
    ('reboot',    'system-reboot',      True),
    ('shutdown',  'system-shutdown',    True),
]


class ExiterConfig(AriaConfigModel):
    lock: str = None
    suspend: str = None
    hibernate: str = None
    logout: str = None
    reboot: str = None
    shutdown: str = None

    columns: int = 3
    ask_confirm: bool = True
    confirm_timeout: int = 30
    grab_display: bool = True
    opacity: int = 100

    @staticmethod
    def validate_columns(val: int):
        return clamp(val, 1, 10)

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


class ExiterButton(Gtk.Button):
    def __init__(self, name: str, label: str, icon_name: str, command: str,
                 want_confirm: bool, callback: Callable[[ExiterButton], None]):
        super().__init__(has_frame=False)
        self.add_css_class('aria-exiter-button')
        self.name = name
        self.label = label
        self.command = command
        self.want_confirm = want_confirm

        icon = Gtk.Image.new_from_icon_name(icon_name)
        icon.add_css_class('aria-exiter-button-icon')

        label = Gtk.Label(label=label)
        label.add_css_class('aria-exiter-button-label')

        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        vbox.append(icon)
        vbox.append(label)
        self.set_child(vbox)
        self.connect('clicked', callback)

    def execute_command(self):
        INF('Running %s command: %s', self.name, self.command)
        exec_detached(self.command)


class AriaExiter(AriaWindow):
    def __init__(self, app: Gtk.Application):
        super().__init__(css_class='aria-exiter', hide_on_escape=True)
        self.set_application(app)
        self.config = AriaConfig().section('exiter', ExiterConfig)

        self.countdown = 0
        self.timer: Timer | None = None
        self.dialog: AriaDialog | None = None

        self.setup_window()
        self.populate_window()

    def setup_window(self):
        self.set_decorated(False)
        self.set_opacity(self.config.opacity / 100.0)

        GtkLayerShell.init_for_window(self)
        GtkLayerShell.set_namespace(self, 'aria-exiter')
        GtkLayerShell.set_layer(self, GtkLayerShell.Layer.OVERLAY)
        GtkLayerShell.set_exclusive_zone(self, -1)
        if self.config.grab_display:
            GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.EXCLUSIVE)
        else:
            GtkLayerShell.set_keyboard_mode(self, GtkLayerShell.KeyboardMode.ON_DEMAND)

    def populate_window(self):
        flow = Gtk.FlowBox(
            orientation=Gtk.Orientation.HORIZONTAL,
            homogeneous=True,
            max_children_per_line=self.config.columns,
            selection_mode=Gtk.SelectionMode.SINGLE,
        )
        flow.add_css_class('aria-exiter-flowbox')
        flow.connect('child_activated', self.child_activated_cb)
        self.set_child(flow)

        for name, icon, want_confirm in BUTTONS:
            if cmd := getattr(self.config, name):
                flow.append(
                    ExiterButton(
                        name=name,
                        label=i18n(name),
                        icon_name=icon,
                        command=cmd,
                        want_confirm=want_confirm,
                        callback=self.button_callback,
                    )
                )

    def child_activated_cb(self, _flow: Gtk.FlowBox, child: Gtk.FlowBoxChild):
        self.button_callback(child.get_child())  # noqa  (pycharm error?)

    def button_callback(self, button: ExiterButton):
        self.hide()
        if button.want_confirm and self.config.ask_confirm:
            self.make_confirm_dialog(button)
        else:
            button.execute_command()

    def make_confirm_dialog(self, button: ExiterButton):
        self.cleanup_and_close_confirm_dialog()
        self.dialog = AriaDialog(
            parent=self,
            title=i18n(f'exiter.confirm_{button.name}1'),
            buttons=[i18n('cancel'), i18n(button.name)],
            callback=self.confirm_dialog_response,
            button=button,
        )
        if self.config.confirm_timeout > 0:
            self.countdown = self.config.confirm_timeout
            self.timer = Timer(1.0, self.timer_cb, immediate=True,
                               dialog=self.dialog, button=button)

    def confirm_dialog_response(self, response: str, button: ExiterButton):
        if response == 'button-2':
            button.execute_command()
        self.cleanup_and_close_confirm_dialog()

    def cleanup_and_close_confirm_dialog(self):
        if self.timer:
            self.timer.stop()
            self.timer = None
        if self.dialog:
            self.dialog.close()
            self.dialog = None

    def timer_cb(self, dialog: AriaDialog, button: ExiterButton) -> bool:
        self.countdown -= 1
        if self.countdown < 1:
            # the countdown has expired
            self.cleanup_and_close_confirm_dialog()
            button.execute_command()
            return False  # stop timer execution
        else:
            # update the countdown in the dialog
            dialog.set_body(
                i18n(f'exiter.confirm_{button.name}2', countdown=self.countdown)
            )
            return True  # continue timer execution
