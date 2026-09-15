from datetime import datetime

from gi.repository import Gtk

from aria_shell.module import AriaModule, GadgetRunContext
from aria_shell.config import AriaConfigModel
from aria_shell.gadget import AriaGadget
from aria_shell.gui import AriaPopover
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
        self.timer = Timer(1, self.timer_cb)

    def module_shutdown(self):
        if self.timer:
            self.timer.stop()
            self.timer = None

    def gadget_factory(self, ctx: GadgetRunContext) -> AriaGadget | None:
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
        self.popover: AriaPopover | None = None

        self.label = Gtk.Label()
        self.append(self.label)

    def update(self, now: datetime):
        text = now.strftime(self.conf.format)
        self.label.set_text(text)

    def mouse_click(self, button: int):
        self.toggle_calendar()

    def toggle_calendar(self):
        if self.popover is None:
            calendar = Gtk.Calendar()
            self.popover = AriaPopover(self, calendar, self.on_popover_closed)
        else:
            self.popover.popdown()

    def on_popover_closed(self, _popover):
        self.popover = None
