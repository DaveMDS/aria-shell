from dataclasses import dataclass
from typing import Mapping
import psutil

from gi.repository import GLib, Gdk, Gtk

from aria_shell.ui import AriaGadget, AriaPopup
from aria_shell.utils import safe_format, human_size
from aria_shell.module import AriaModule
from aria_shell.config import AriaConfigModel
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class PerfConfig(AriaConfigModel):
    """ Configuration model """
    format: str = ' {cpu:2.0f}%   {mem:2.0f}%'
    interval: int = 2

    @staticmethod
    def validate_interval(val: int):
        return val if val >= 1 else 1


@dataclass
class SysInfo:
    """ Container updated by the timer, and used to populate each gadget """
    cpu_count: int = 0
    cpu_percent: float = 0
    cpu_freq: float = 0
    cpu_freq_min: float = 0
    cpu_freq_max: float = 0

    load1: float = 0
    load5: float = 0
    load15: float = 0

    load1_percent: float = 0
    load5_percent: float = 0
    load15_percent: float = 0

    mem_total: int = 0
    mem_available: int = 0
    mem_percent: float = 0

    def __repr__(self):
        return (
            f'<SysInfo cores={self.cpu_count}'
            f' cpu={self.cpu_percent:2.0f}%'
            f' freq={self.cpu_freq:4.0f}Mhz'
            f' load1={self.load1:.2f}({self.load1_percent:.0f}%)'
            f' mem={self.mem_percent:2.0f}%'
            f'>'
        )


class PerfModule(AriaModule):
    def __init__(self):
        super().__init__()
        self.timer: int = 0
        self.interval: int = 0
        self.info = SysInfo()

    def module_init(self):
        super().module_init()
        self.info.cpu_count = psutil.cpu_count(logical=True)

    def module_shutdown(self):
        self.stop_timer()
        super().module_shutdown()

    def module_gadget_new(self, user_settings: Mapping[str, str], monitor: Gdk.Monitor):
        super().module_gadget_new(user_settings, monitor)
        conf = PerfConfig(user_settings)

        # recreate the timer if this instance need a shorter interval
        if self.interval and conf.interval < self.interval:
            self.stop_timer()

        # start the global timer, using the interval of this instance
        if not self.timer:
            self.start_timer(conf.interval)

        # create and populate the gadget
        instance = PerfGadget(conf)
        instance.update(self.info)
        return instance

    def start_timer(self, interval: int):
        self.interval = interval
        self.timer = GLib.timeout_add_seconds(
            interval, self.on_timer_tick,
            priority=GLib.PRIORITY_LOW  # noqa
        )
        self.on_timer_tick()

    def stop_timer(self):
        if self.timer:
            GLib.source_remove(self.timer)
            self.timer = 0
            self.interval = 0

    def on_timer_tick(self):
        info = self.info

        # CPU
        freq = psutil.cpu_freq(percpu=False)
        info.cpu_percent = psutil.cpu_percent(interval=0, percpu=False)
        info.cpu_freq = freq.current
        info.cpu_freq_min = freq.min
        info.cpu_freq_max = freq.max

        # load
        load = psutil.getloadavg()
        info.load1, info.load5, info.load15 = load
        info.load1_percent = info.load1 / info.cpu_count * 100
        info.load5_percent = info.load5 / info.cpu_count * 100
        info.load15_percent = info.load15 / info.cpu_count * 100

        # mem
        mem = psutil.virtual_memory()
        info.mem_total = mem.total
        info.mem_available = mem.available
        info.mem_percent = mem.percent

        # redraw all the instances
        for instance in self.instances:
            instance.update(self.info)

        # DBG(self.info)
        return True


class PerfGadget(AriaGadget):
    def __init__(self, conf: PerfConfig):
        super().__init__('cpu', clickable=True)
        self.conf = conf

        self.label = Gtk.Label()
        self.append(self.label)

        self.popup: AriaPopup | None = None
        self.popup_label: Gtk.Label | None = None
        self.last_info: SysInfo | None = None

    def update(self, info: SysInfo):
        self.last_info = info

        # update label
        text = safe_format(
            self.conf.format, PerfConfig.format,
            cpu=info.cpu_percent,
            mem=info.mem_percent,
            load1=info.load1_percent,
            load5=info.load5_percent,
            load15=info.load15_percent,
            info=info,
        )
        self.label.set_text(text)

        # update popup
        if self.popup_label:
            self.update_popup(info)

    def update_popup(self, info: SysInfo):
        if not self.popup_label:
            return
        self.popup_label.set_markup(
            f'Number of CPU cores: {info.cpu_count}\n\n'
            f'Total memory: {human_size(info.mem_total)}\n'
            f'Available memory: {human_size(info.mem_available)}\n\n'
            f'Load average: {info.load1:.2f} {info.load5:.2f} {info.load15:.2f}\n'
            f'Load percent: {info.load1_percent:.0f}% {info.load5_percent:.0f}% {info.load15_percent:.0f}%\n\n'
            f'CPU frequency: {info.cpu_freq:.0f}Mhz  ({info.cpu_freq_min:.0f}-{info.cpu_freq_max:.0f})'
        )

    def on_mouse_down(self, button: int):
        self.toggle_popup()

    def toggle_popup(self):
        if self.popup:
            self.popup.close()
        else:
            lbl = Gtk.Label()
            self.popup = AriaPopup(lbl, self, self.on_popup_destroy)
            self.popup_label = lbl
            self.update_popup(self.last_info)

    def on_popup_destroy(self, _popup):
        self.popup = None

