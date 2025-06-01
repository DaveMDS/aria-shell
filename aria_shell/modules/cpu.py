from __future__ import annotations

from typing import Mapping

import psutil

from gi.repository import GLib, Gdk, Gtk  # noqa

from aria_shell.ui import AriaWidget
from aria_shell.utils import safe_format
# from aria_shell.utils.logger import LOG, ERR, DBG
from aria_shell.module import AriaModule
from aria_shell.config import AriaConfigModel


class CpuConfig(AriaConfigModel):
    format: str = '  {val:.0f}%'
    tooltip_format: str = '{name}: {val:.1f}%'


class CpuModule(AriaModule):
    def __init__(self):
        super().__init__()
        self.timer: int = 0
        self.num_cores: int = 1
        self.values: list[float] = [0.0]

    def module_init(self):
        super().module_init()
        self.num_cores = len(psutil.cpu_percent(interval=0, percpu=True))
        self.timer = GLib.timeout_add_seconds(
            2, self.timer_cb, priority=GLib.PRIORITY_LOW  # noqa
        )
        self.timer_cb()

    def module_shutdown(self):
        if self.timer:
            GLib.source_remove(self.timer)
            self.timer = 0
        super().module_shutdown()

    def module_instance_new(self, user_settings: Mapping[str, str], monitor: Gdk.Monitor):
        super().module_instance_new(user_settings, monitor)
        conf = CpuConfig(user_settings)
        instance = CpuWidget(conf)
        instance.update(self.values)
        return instance

    def timer_cb(self):
        self.values = psutil.cpu_percent(interval=0, percpu=True)
        # DBG("Cpu values:", self.values)
        for instance in self.instances:
            instance.update(self.values)
        return True


class CpuWidget(AriaWidget):
    def __init__(self, conf: CpuConfig):
        super().__init__('cpu')
        self.conf = conf
        self.avg = 0
        self.cnt = 0

        self.label = Gtk.Label()
        self.append(self.label)

    def update(self, vals: list[float]):
        med = sum(vals) / len(vals)

        # update label
        text = safe_format(self.conf.format, CpuConfig.format, val=med)
        self.label.set_text(text)

        # update tooltip
        title = safe_format(self.conf.tooltip_format,
                            CpuConfig.tooltip_format,
                            name=f'Total', val=med)
        lines = [f'<b>{title}</b>']
        for i, val in enumerate(vals, 1):
            text = safe_format(self.conf.tooltip_format,
                               CpuConfig.tooltip_format,
                               name=f'Core{i}', val=val)
            lines.append(text)
            # text.append(self.conf.tooltip_format.format(num=i, val=val))
        text = '\r'.join(lines)
        self.label.set_tooltip_markup(text)
