import os
import time
import subprocess
from collections.abc import Callable

from gi.repository import GLib

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
    # 'signal-name' => [(callback, *args, **kargs), ...]
    _handlers: dict[str, list[tuple[Callable, tuple, dict]]]

    def __new__(cls, *a, **ka):
        instance = super().__new__(cls)
        instance._handlers = {}
        return instance

    def connect(self, signal: str, handler: Callable, *a, **ka):
        self._handlers.setdefault(signal, []).append((handler, a, ka))

    def emit(self, signal: str, *args):
        for h, a, ka in self._handlers.get(signal, []):
            h(*args, *a, *ka)


class Observable:
    """ Make the class properties able to be watched and binded.
    Can be used on: classes, dataclasses, GObjects (probably more)

    Usage:
    > class User(Observable):
    >     name = 'John Doe'
    >     age = 21
    >
    > user = User()
    > user.watch('name', lambda name: print(f'new name: {name}'))
    > user.name = 'Mary'  # this will trigger the watcher and print the new name
    """
    # 'prop_name' => [(callback, **kargs), ...]
    _observers: dict[str, list[tuple[Callable, dict]]]

    def __new__(cls, *a, **ka):
        instance = super().__new__(cls)
        instance._observers = {}
        return instance

    def __setattr__(self, name, value):
        super().__setattr__(name, value)
        for h, ka in self._observers.get(name, []):
            h(value, **ka)

    def watch(self, prop_name: str, handler: Callable,
              immediate: bool = True, **ka):
        self._observers.setdefault(prop_name, []).append((handler, ka))
        if immediate:
            handler(getattr(self, prop_name), **ka)

    # def bind(self):
    #     TODO


class Timer:
    """ Simple Timer class on top of GLib timeout_add

    Params:
        interval (always in seconds) can be int or float:
            float -> create hi resolution timer, for subseconds precision
            int -> create a low resolution, cheaper timer
        callback: Callable to call on each timer tick, must return:
            True: to continue executing the timer
            False: to stop the timer
        autostart: if False the timer will not start on init
        *a *ka: positional and named params to be passed in callback
    """
    def __init__(self,
                 interval: int | float,
                 callback: Callable[[...], [bool]],
                 autostart=True,
                 *a, **ka):
        if not isinstance(interval, int | float):
            raise TypeError('Timer.interval must be int or float')
        if not isinstance(callback, Callable):
            raise TypeError('Timer.callback must be callable')
        if isinstance(interval, float) and interval > 5:
            WRN('for low resolution timer use an int interval !!!')
        self.interval = interval
        self.cb_info = (callback, a, ka)
        self.timeout_id = 0
        if autostart:
            self.start()

    def __repr__(self):
        return f'<Timer #{self.timeout_id} cb={self.cb_info[0]}>'

    def start(self):
        if self.timeout_id == 0:
            if isinstance(self.interval, int):
                self.timeout_id = GLib.timeout_add_seconds(
                    self.interval, self._timeout_cb,
                    priority=GLib.PRIORITY_LOW  # noqa
                )
            elif isinstance(self.interval, float):
                self.timeout_id = GLib.timeout_add(
                    int(self.interval * 1000), self._timeout_cb,
                    priority=GLib.PRIORITY_DEFAULT  # noqa
                )
        else:
            ERR(f'Timer already started {self}')

    def stop(self):
        if self.timeout_id:
            GLib.source_remove(self.timeout_id)
            self.timeout_id = 0
        else:
            ERR(f'Timer not started {self}')

    def _timeout_cb(self):
        callback, a, ka = self.cb_info
        return callback(*a, **ka)


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


def pack_variant(data) -> GLib.Variant:
    """ Pack a python object into a GLib Variant """
    if isinstance(data, str):
        return GLib.Variant.new_string(data)
    elif isinstance(data, bool):
        return GLib.Variant.new_boolean(data)
    elif isinstance(data, int):
        return GLib.Variant.new_int32(data)
    elif isinstance(data, float):
        return GLib.Variant.new_double(data)
    elif isinstance(data, (list, tuple, set)):
        builder = GLib.VariantBuilder(GLib.VariantType('av'))
        for item in data:
            builder.add_value(GLib.Variant.new_variant(pack_variant(item)))
        return builder.end()
    elif isinstance(data, dict):
        builder = GLib.VariantBuilder(GLib.VariantType('a{sv}'))
        for key, val in data.items():
            if key is not None or val is not None:
                builder.add_value(
                    GLib.Variant.new_dict_entry(
                        GLib.Variant.new_string(str(key)),
                        GLib.Variant.new_variant(pack_variant(val))
                    )
                )
        return builder.end()
    else:
        raise TypeError(f'Dunno how to convert {type(data)} to Variant')


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


# class ReactiveValue:
#     """
#     Funziona molto bene, semplice. ma bisogna sempre usare set() e get() !
#     o i suoi surrogati più brevi...quasi hack
#
#     name = ReactiveValue('Mary')
#     name.get()  or  utente.name.value
#     name.set('John')
#     name.watch(lambda name: print(f'new name: {name}'))
#     name.bind(....)
#
#     volendo anche:
#     name()     -> get
#     name(val)  -> set
#
#     class Utente:
#         __init__(name):
#             self.name = ReactiveValue(name)
#     """
#     def __init__(self, default=None):
#         # WRN(f'__init__ {self}')
#         self._listeners = []
#         self.value = default
#
#     def get(self):
#         # WRN(f'__get {self}')
#         return self.value
#
#     def set(self, value):
#         # WRN(f'__set {self} {value}')
#         if value != self.value:
#             self.value = value
#             for handler in self._listeners:
#                 handler(value)
#
#     def watch(self, listener: Callable, immediate: bool = False):
#         self._listeners.append(listener)
#         if immediate:
#             listener(self.value)
#
#     def __call__(self, value = None) -> Any:
#         # WRN(f'__call__ {value}')
#         return self.value