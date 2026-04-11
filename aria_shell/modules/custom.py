from gi.repository import Gtk

from aria_shell.utils import exec_command_or_program
from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule, GadgetRunContext
from aria_shell.config import AriaConfigModel
from aria_shell.gadget import AriaGadget


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)

# NOTE:
# when going further on this, try to keep compatibility with waybar custom
# lots of example in manjaro-sway:
#   /usr/share/sway/templates/waybar/config.jsonc


class CustomConfigModel(AriaConfigModel):
    label: str = ''
    icon: str = ''
    command: str = ''


class CustomModule(AriaModule):
    config_model_class = CustomConfigModel

    def gadget_factory(self, ctx: GadgetRunContext) -> AriaGadget | None:
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

    def mouse_click(self, button: int):
        exec_command_or_program(self.conf.command)
