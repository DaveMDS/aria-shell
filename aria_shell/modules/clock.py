from datetime import datetime

from gi.repository import Gtk

from aria_shell.module import AriaModule, GadgetRunContext
from aria_shell.config import AriaConfigModel
from aria_shell.gadget import AriaGadget
from aria_shell.ui import AriaPopup
from aria_shell.utils.logger import get_loggers
from aria_shell.utils import Timer


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class ClockConfigModel(AriaConfigModel):
    format: str = '%H:%M'
    tooltip_format: str = '%A %d %B %Y'


class ClockModule(AriaModule):
    config_model_class = ClockConfigModel

    def __init__(self):
        super().__init__()
        self.timer: Timer | None = None

    def module_init(self):
        super().module_init()
        self.timer = Timer(1, self.timer_cb)

    def module_shutdown(self):
        if self.timer:
            self.timer.stop()
            self.timer = None
        super().module_shutdown()

    def gadget_new(self, ctx: GadgetRunContext) -> AriaGadget | None:
        super().gadget_new(ctx)
        conf: ClockConfigModel = ctx.config  # noqa
        instance = ClockGadget(conf)
        self.timer_cb(instance)  # perform a first update
        return instance

    def timer_cb(self, instance=None):
        now = datetime.now()
        if instance:
            instance.update(now)
        else:
            # print("Timer", now)
            for instance in self.gadgets:
                instance.update(now)
        return True


class ClockGadget(AriaGadget):
    def __init__(self, conf: ClockConfigModel):
        super().__init__('clock', clickable=True)
        self.conf = conf

        self.label = Gtk.Label()
        self.popup = None
        self.append(self.label)

    def update(self, now: datetime):
        text = now.strftime(self.conf.format)
        self.label.set_text(text)

    def on_mouse_down(self, button: int):
        self.toggle_calendar()

    def toggle_calendar(self):
        if self.popup is None:
            calendar = Gtk.Calendar()
            self.popup = AriaPopup(calendar, self, self.on_popup_destroy)
        else:
            self.popup.close()

    def on_popup_destroy(self, _popup):
        self.popup = None
