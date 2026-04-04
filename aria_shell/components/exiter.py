from collections.abc import Callable

from gi.repository import Gtk

from aria_shell.ui import AriaWindow, AriaDialog
from aria_shell.utils import clamp, exec_detached, Timer
from aria_shell.config import AriaConfig, AriaConfigModel
from aria_shell.services.commands import AriaCommands, CommandFailed
from aria_shell.utils.logger import get_loggers
from aria_shell.i18n import i18n, MissingTranslation


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


DEFAULT_ICONS = {
    'lock':      'system-lock-screen',
    'suspend':   'system-suspend',
    'hibernate': 'system-hibernate',
    'logout':    'system-log-out',
    'reboot':    'system-reboot',
    'shutdown':  'system-shutdown',
}


class ExiterConfig(AriaConfigModel):
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


class AriaExiter(AriaWindow):
    def __init__(self, app: Gtk.Application):
        INF('Initialize Aria Exiter')
        self.config = AriaConfig().section('exiter', ExiterConfig)
        AriaCommands().register('exiter', self.the_exiter_command)

        super().__init__(
            app=app,
            namespace='aria-exiter',
            title='Aria exiter',
            hide_on_escape=True,
            layer=AriaWindow.Layer.TOP,
            grab_display=self.config.grab_display,
            opacity=self.config.opacity / 100.0,
            # decorated=False,
        )

        self.dialog: AriaDialog | None = None
        self.timer: Timer | None = None
        self.countdown = 0

    # lazily populate the window on 'show'
    def do_show(self):
        if self.get_child() is None:
            self.populate_window()
        AriaWindow.do_show(self)

    def shutdown(self):
        INF('Shutting down Aria Exiter')
        AriaCommands().unregister('exiter')
        if self.dialog:
            self.dialog.destroy()
            self.dialog = None
        if self.timer:
            self.timer.stop()
            self.timer = None
        self.set_child(None)
        super().shutdown()

    def the_exiter_command(self, _, params: list[str]) -> None:
        """Runner for the 'exiter' aria command."""
        if not params or params[0] == 'toggle':
            self.toggle()
        elif params and params[0] == 'hide':
            self.hide()
        elif params and params[0] == 'show':
            self.show()
        else:
            raise CommandFailed('Invalid arguments for the <exiter> command')

    def populate_window(self):
        flow = Gtk.FlowBox(
            orientation=Gtk.Orientation.HORIZONTAL,
            homogeneous=True,
            max_children_per_line=self.config.columns,
            selection_mode=Gtk.SelectionMode.SINGLE,
        )
        flow.add_css_class('aria-exiter-flowbox')
        self.safe_connect(flow, 'child_activated', self.child_activated_cb)
        self.set_child(flow)

        # populate from untyped options in config file
        for name, command in self.config.options.items():
            if name.endswith('-icon'):
                continue
            # commands that start with "!" want a confirm dialog
            if want_confirm := command.startswith('!'):
                command = command[1:].strip()

            # get icon name from the user option 'name-icon' or from the default map
            icon = self.config.options.get(f'{name}-icon', DEFAULT_ICONS.get(name, None))

            # create the button
            flow.append(ExiterButton(
                name=name,
                label=i18n(name),
                icon_name=icon,
                command=command,
                want_confirm=want_confirm,
                callback=self.button_callback,
            ))

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
        try:
            heading = i18n(f'exiter.confirm_{button.name}1', fail=True)
        except MissingTranslation:
            heading = i18n(f'exiter.confirm_generic1')

        self.dialog = AriaDialog(
            parent=self,
            heading=heading,
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
            self.dialog.shutdown()
            self.dialog = None

    def timer_cb(self, dialog: AriaDialog, button: ExiterButton) -> bool:
        self.countdown -= 1
        DBG('Exiter countdown %d', self.countdown)
        if self.countdown < 1:
            # the countdown has expired
            self.cleanup_and_close_confirm_dialog()
            button.execute_command()
            return False  # stop timer execution
        else:
            # update the countdown in the dialog
            try:
                body = i18n(f'exiter.confirm_{button.name}2',
                            fail=True, countdown=self.countdown)
            except MissingTranslation:
                body = i18n(f'exiter.confirm_generic2',
                            countdown=self.countdown)
            dialog.set_body(body)
            return True  # continue timer execution


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
        if self.command.startswith('aria '):
            # run an aria command
            cmd = self.command[5:].strip()
            AriaCommands().run(cmd)
        else:
            # or execute a process
            exec_detached(self.command)
