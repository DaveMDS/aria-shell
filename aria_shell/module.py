from __future__ import annotations
from typing import Mapping, Optional

import importlib
import traceback

from gi.repository import Gtk, Gdk

from aria_shell.config import AriaConfig
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


_loaded_modules: dict[str, AriaModule] = {}  # es: {'clock': ClockModule,...}


class AriaModule:
    """ Base class for all Aria modules """
    def __init__(self):
        self.gadgets: list[Gtk.Widget] = []
        self.initialized = False

    def __repr__(self):
        return f'<AriaModule: {self.__module__}.{self.__class__.__name__}>'

    def module_init(self):
        DBG(f'Module init: {self}')
        self.initialized = True

    def module_shutdown(self):
        DBG(f'Module shutdown: {self}')

    def module_gadget_new(self, user_settings: Mapping,
                          monitor: Gdk.Monitor) -> Gtk.Widget | None:
        # DBG(f'AriaModule module_gadget_new: {self}')
        pass

#
# def get_module_by_name(name: str) -> AriaModule | None:
#     return _loaded_modules.get(name, None)


def load_modules(names: list[str]):
    for name in names:
        # do not import twice
        if name in _loaded_modules:
            continue

        INF(f'Loading module: {name}')
        # TODO: if the module is a full path then import directly,
        #       needed to support external modules
        module_name = 'aria_shell.modules.' + name
        class_name = name.capitalize() + 'Module'
        try:
            # import the python module, es: aria_shell.modules.clock
            pymodule = importlib.import_module(module_name)
            # create an instance of the module main class, es: ClockModule
            cls = getattr(pymodule, class_name)
            mod = cls()
        except ModuleNotFoundError as e:
            ERR(f'Cannot find module: {name}. {e}')
        except Exception as e:
            ERR(f'Cannot load module: {name}. {e}. Full traceback:')
            traceback.print_exc()
            return

        # keep track of loaded modules
        _loaded_modules[name] = mod


def unload_all_modules():
    for name, mod in _loaded_modules.items():
        try:
            mod.module_shutdown()
        except Exception as e:
            ERR(f'Cannot shutdown module: {name}. Exception: {e}. Full traceback follow...')
            traceback.print_exc()


def request_module_gadget(name: str, monitor: Gdk.Monitor) -> Optional[Gtk.Widget]:
    if ':' in name:
        mod_name = name.split(':')[0]
    else:
        mod_name = name
    mod = _loaded_modules.get(mod_name, None)
    if not mod:
        return None

    # lazy initialization of modules: module_init is called just before the
    # first instance is requested
    if not mod.initialized:
        try:
            mod.module_init()
        except Exception as e:
            ERR(f'Cannot init module: {name}. Exception: {e}. Full traceback follow...')
            traceback.print_exc()
            return None

    # request a new instance from the module
    instance_conf = AriaConfig().section(name)
    try:
        instance = mod.module_gadget_new(instance_conf, monitor)
    except Exception as e:
        ERR(f'Cannot create instance of module: {name}. Exception: {e}. Full traceback follow...')
        traceback.print_exc()
        return None

    # keep track of gadgets inside the module class
    mod.gadgets.append(instance)
    return instance
