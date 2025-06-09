from typing import Mapping

from gi.repository import Gtk, Gdk

from aria_shell.components.commands import AriaCommands
from aria_shell.utils import exec_detached
from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule
from aria_shell.config import AriaConfigModel
from aria_shell.ui import AriaGadget


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class ButtonConfig(AriaConfigModel):
    label: str = ''
    icon: str = ''
    command: str = ''
    exec: str = ''


class ButtonModule(AriaModule):
    def module_gadget_new(self, user_settings: Mapping[str, str], monitor: Gdk.Monitor):
        super().module_gadget_new(user_settings, monitor)
        conf = ButtonConfig(user_settings)
        return ButtonGadget(conf)


class ButtonGadget(AriaGadget):
    def __init__(self, conf: ButtonConfig):
        super().__init__('button', clickable=True)
        self.conf = conf
        if conf.icon:
            ico = Gtk.Image.new_from_icon_name(conf.icon)
            self.append(ico)
        if conf.label:
            lbl = Gtk.Label(label=conf.label)
            self.append(lbl)

    def on_mouse_down(self, button: int):
        if self.conf.command:
            AriaCommands().run(self.conf.command)

        if self.conf.exec:
            exec_detached(self.conf.exec)
