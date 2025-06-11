from __future__ import annotations

import importlib
import traceback

from gi.repository import Gdk

from aria_shell.config import AriaConfig, AriaConfigModel
from aria_shell.gadget import AriaGadget
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


# index of loaded modules: {'clock': ClockModule,...}
_loaded_modules: dict[str, AriaModule] = {}


class AriaModule:
    """
    Base class for all Aria modules.

    Modules can provide gadgets by implementing the gadget_new() method.

    Attributes:
        config_model_class: AriaConfigModel class to use for parsing config section
        initialized: True after module_init() has been called
        gadgets: list of active AriaGadgets, list is aria-managed, do not edit!

    """
    config_model_class = AriaConfigModel

    def __init__(self):
        """
        Create an instance of the module, called on early aria startup.

        Do not make any intensive work here! Just define variables and
        check if the module can run (rise RuntimeError otherwise)

        """
        DBG(f'Module __init__: {self}')
        self.initialized = False
        self.gadgets: list[AriaGadget] = []

    def __repr__(self):
        return f'<AriaModule: {self.__module__}.{self.__class__.__name__}>'

    def module_init(self):
        """
        Initialize the module.

        This is called as later as possible, usually when the first gadget is
        needed on a panel. Here you can initialize all the needed stuff,
        like timers, connections, etc...

        """
        DBG(f'Module init: {self}')
        self.initialized = True

    def module_shutdown(self):
        """
        Shutdown the module.

        All resources must be released at this point.

        """
        DBG(f'Module shutdown: {self}')
        self.initialized = False

    def gadget_new(self,
                   conf: AriaConfigModel,
                   monitor: Gdk.Monitor
                   ) -> AriaGadget | None:
        """
        Create a new AriaGadget instance.

        Args:
            conf: The config section for this instance
            monitor: The monitor where the gadget has been requested

        Returns:
            A newly created AriaGadget or None in case of failure
        """
        # DBG(f'AriaModule gadget_new: {self}')
        return None

#
# def get_module_by_name(name: str) -> AriaModule | None:
#     return _loaded_modules.get(name, None)


def load_modules(names: list[str]):
    for name in names:
        # never import twice
        if name in _loaded_modules:
            continue

        # TODO: if the module is a full path then import directly,
        #       needed to support external modules
        module_name = f'aria_shell.modules.{name.lower()}'
        class_name = f'{name}Module'
        INF(f"Pre-loading module '{name}' from {module_name}.{class_name}")
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
        else:
            # keep track of loaded modules
            _loaded_modules[name] = mod


def unload_all_modules():
    for name, mod in _loaded_modules.items():
        try:
            mod.module_shutdown()
        except Exception as e:
            ERR(f'Cannot shutdown module: {name}. Exception: {e}. Full traceback follow...')
            traceback.print_exc()


def request_module_gadget(name: str, monitor: Gdk.Monitor) -> AriaGadget | None:
    # name of the gadget can contain config instance id, es: "Clock:2"
    if ':' in name:
        mod_name = name.split(':', 1)[0]
    else:
        mod_name = name

    # find the preloaded module
    mod = _loaded_modules.get(mod_name, None)
    if mod is None:
        ERR(f'Cannot find module "{mod_name}" for gadget "{name}"')
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

    # prepare the gadget config, using the model in config_model_class
    if mod.config_model_class:
        conf = AriaConfig().section(name, mod.config_model_class)
    else:
        conf = None

    # request a new gadget instance from the module
    try:
        instance = mod.gadget_new(conf, monitor)
        assert isinstance(instance, AriaGadget)
    except Exception as e:
        ERR(f'Cannot create instance of module: {name}. Exception: {e}. Full traceback follow...')
        traceback.print_exc()
        return None

    # keep track of gadgets inside the module class
    mod.gadgets.append(instance)
    return instance
