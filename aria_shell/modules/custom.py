from gi.repository import Gtk

from aria_shell.components.commands import AriaCommands
from aria_shell.utils import exec_detached
from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule, GadgetRunContext
from aria_shell.config import AriaConfigModel
from aria_shell.gadget import AriaGadget


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class CustomConfigModel(AriaConfigModel):
    label: str = ''
    icon: str = ''
    command: str = ''
    exec: str = ''


class CustomModule(AriaModule):
    config_model_class = CustomConfigModel

    def gadget_new(self, ctx: GadgetRunContext) -> AriaGadget | None:
        super().gadget_new(ctx)
        conf: CustomConfigModel = ctx.config  # noqa
        return CustomGadget(conf)


class CustomGadget(AriaGadget):
    def __init__(self, conf: CustomConfigModel):
        super().__init__('custom', clickable=True)
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
