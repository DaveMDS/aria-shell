from gi.repository import Gtk

from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule, GadgetRunContext
from aria_shell.config import AriaConfigModel
from aria_shell.gadget import AriaGadget


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class TrayConfigModel(AriaConfigModel):
    pass


class TrayModule(AriaModule):
    config_model_class = TrayConfigModel

    def __init__(self):
        super().__init__()

    def module_init(self):
        super().module_init()

    def module_shutdown(self):
        super().module_shutdown()

    def gadget_new(self, ctx: GadgetRunContext) -> AriaGadget | None:
        super().gadget_new(ctx)
        conf: TrayConfigModel = ctx.config  # noqa
        return TrayGadget(conf)


class TrayGadget(AriaGadget):
    def __init__(self, conf: TrayConfigModel):
        super().__init__('tray', clickable=True)

        lbl = Gtk.Label(label='Tray')
        self.append(lbl)

    def on_mouse_down(self, button: int):
        print('click:', button)
