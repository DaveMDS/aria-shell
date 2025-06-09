from typing import Mapping

from datetime import datetime

from gi.repository import GLib, Gtk, Gdk

from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule
from aria_shell.config import AriaConfigModel
from aria_shell.ui import AriaPopup, AriaGadget


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class ClockConfig(AriaConfigModel):
    format: str = '%H:%M'
    tooltip_format: str = '%A %d %B %Y'


class ClockModule(AriaModule):
    def __init__(self):
        super().__init__()
        self.timer: int = 0

    def module_init(self):
        super().module_init()
        self.timer = GLib.timeout_add_seconds(
            1, self.timer_cb, priority=GLib.PRIORITY_LOW  # noqa
        )

    def module_shutdown(self):
        if self.timer:
            GLib.source_remove(self.timer)
            self.timer = 0
        super().module_shutdown()

    def module_gadget_new(self, user_settings: Mapping[str, str], monitor: Gdk.Monitor):
        super().module_gadget_new(user_settings, monitor)
        DBG(f'AriaModule module_instance_new {self.__class__.__name__}')
        conf = ClockConfig(user_settings)
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
    def __init__(self, conf: ClockConfig):
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
