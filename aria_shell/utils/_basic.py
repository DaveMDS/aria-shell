import os
import time
import subprocess
from collections.abc import Callable

from aria_shell.utils.env import HOME
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class Singleton(type):
    """ usage: MyClass(metaclass=Singleton) """
    _instances = {}

    def __call__(cls, *args, **kwargs):
        instance = cls._instances.get(cls, None)
        if instance is None:
            instance = super().__call__(*args, **kwargs)
            cls._instances[cls] = instance
        return instance


class Signalable:
    """ Make a class able to emit signals to registered clients """
    def __init__(self):
        # 'signal-name' => (callback, *args, **kargs)
        self._handlers: dict[str, list[tuple[Callable, tuple, dict]]] = {}

    def connect(self, signal: str, handler: Callable, *args, **kargs):
        handlers = self._handlers.setdefault(signal, [])
        handlers.append((handler, args, kargs))

    def emit(self, signal: str, *args):
        for h, a, k in self._handlers.get(signal, []):
            h(*args, *a, *k)


def clamp(val, low, high):
    """ Return val clamped between low and high """
    if val < low:
        return low
    if val > high:
        return high
    return val


def safe_format(format1: str, format2: str, **kwargs) -> str:
    """ Safe string format utility, can ingest user malformed formats and
        in case of errors use the default format instead """
    try:
        return format1.format(**kwargs)
    except (KeyError, ValueError):
        ERR(f'Invalid format string: "{format1}". Values: {kwargs}. Using default format: "{format2}"')
        return format2.format(**kwargs)


def exec_detached(cmd: str | list[str]) -> bool:
    """ Run the given command detached from the aria process """
    custom_env = os.environ.copy()
    custom_env.pop('VIRTUAL_ENV', None)
    custom_env.pop('PYTHONHOME', None)
    custom_env.pop('PYTHONPATH', None)
    custom_env['PATH'] = os.defpath
    try:
        subprocess.Popen(
            cmd,
            env=custom_env,
            cwd=HOME,
            start_new_session=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            stdin=subprocess.DEVNULL,
        )
        return True
    except OSError:
        ERR(f'Cannot execute command: "{cmd}"')
        return False


def human_size(bites: int) -> str:
    if bites >= 1099511627776:
        terabytes = bites / 1099511627776.0
        size = f'{terabytes:.1f}TB'
    elif bites >= 1073741824:
        gigabytes = bites / 1073741824.0
        size = f'{gigabytes:.1f}GB'
    elif bites >= 1048576:
        megabytes = bites / 1048576.0
        size = f'{megabytes:.1f}MB'
    elif bites >= 1024:
        kilobytes = bites / 1024.0
        size = f'{kilobytes:.1f}KB'
    else:
        size = '%.0fb' % bites
    return size


class PerfTimer:
    def __init__(self, auto_reset=True):
        self.time = self.now()
        self.auto_reset = auto_reset
        self.init_time = self.time
        self.marks = {}

    @staticmethod
    def now() -> float:
        return time.perf_counter()

    @property
    def seconds(self) -> float:
        """ Elapsed time since last reset, in seconds """
        return self.now() - self.time

    @property
    def elapsed(self) -> str:
        """ Elapsed time since last reset, as a readable string """
        delta = self.now() - self.time
        if self.auto_reset:
            self.reset()
        return self.to_string(delta)

    @property
    def elapsed_total(self) -> str:
        """ Elapsed time since object creation, including all marks recorded """
        delta = self.now() - self.init_time
        total = self.to_string(delta)
        if self.marks:
            marks = ', '.join([f'{k}: {v}' for k, v in self.marks.items()])
            return f'{total} ({marks})'
        else:
            return total

    def reset(self):
        """ Reset the timer to zero """
        self.time = self.now()

    def mark(self, mark_name):
        """ Store the elapsed time with a name """
        self.marks[mark_name] = self.elapsed

    @staticmethod
    def to_string(delta: float):
        """ Return a readable time delta """
        if delta > 60:
            m, s = divmod(delta, 60)
            return '%d min %d sec' % (m, s)
        elif delta < 0.001:
            return '%.5f sec' % delta
        else:
            return '%.3f sec' % delta
