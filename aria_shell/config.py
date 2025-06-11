from typing import Mapping, TypeVar

import configparser
from pathlib import Path

from aria_shell.utils import Singleton
from aria_shell.utils.logger import get_loggers
from aria_shell.utils.env import lookup_config_file


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class AriaConfigModel:
    """ Base class for annotated config sections """
    def __init__(self, conf_data: Mapping[str, str]):
        # Config classes must be annotated
        if not hasattr(self, '__annotations__'):
            return

        # fill the class with values in conf_data, validating using annotation
        for key, str_val in conf_data.items():
            if not hasattr(self, key):
                WRN(f"Invalid key '{key}' in config {self.__class__.__name__}")
                continue

            # empty values are ignored
            if not str_val:
                continue

            # get the annotation string for this attribute (mandatory)
            annot: str = self.__annotations__.get(key)
            if annot is None:
                ERR(f'Param {self.__class__.__name__}.{key} miss type annotation!')
                continue

            # DBG(f'§ key: {key}  val: {str_val}  annot: {repr(annot)}')
            if annot in (str, 'str'):
                val = str_val

            elif annot in (int, 'int'):
                try:
                    val = int(str_val)
                except (TypeError, ValueError):
                    ERR(f'Invalid value: {str_val} for key: {key}. Must be integer')
                    continue

            elif annot in (float, 'float'):
                try:
                    val = float(str_val)
                except (TypeError, ValueError):
                    ERR(f'Invalid value: {str_val} for key: {key}. Must be float')
                    continue

            elif annot in (bool, 'bool'):
                val = str_val.lower() in ('true', '1', 'yes')

            elif annot in (list[str], 'list[str]'):
                val = str_val.split()

            else:
                WRN(f'Unknown annotation: {annot} for key: {key}')
                val = str_val

            # run the custom validator if present
            validator = getattr(self, f'validate_{key}', None)
            if validator and callable(validator):
                try:
                    val = validator(val)
                except ValueError as e:
                    ERR(e)
                    continue

            setattr(self, key, val)

    def dump(self):
        """ for debugging purpose only """
        print(self)
        for key in dir(self):
            if key[0] != '_':
                val = getattr(self, key)
                if not callable(val):  # skip methods
                    print(f'  {key} = {repr(val)}')


class AriaConfigGeneralModel(AriaConfigModel):
    """ Model for the [general] main section """
    modules: list[str] = []
    style: str = ''


AriaConfigModelType = TypeVar('AriaConfigModelType', bound=AriaConfigModel)


class AriaConfig(metaclass=Singleton):
    """ Main configuration for AriaShell, from config file """
    def __init__(self):
        self._parser = configparser.ConfigParser(
            strict=False,
            empty_lines_in_values=False,
            interpolation=None,
            comment_prefixes=('#', ';'),
            inline_comment_prefixes=('#', ';'),
        )
        self._general: AriaConfigGeneralModel | None = None

    def load_conf(self, config_file: Path = None):
        """ Load the aria config file, from config_file or searched in system """
        # check file given on command line
        if config_file and not config_file.exists():
            ERR(f'Cannot find the requested config file: {config_file}')

        # search in default system paths
        if not config_file:
            config_file = lookup_config_file('aria.conf')

        # read file
        if config_file and config_file.exists():
            INF(f'Reading config from file: {config_file}')
            self._parser.read(config_file)
        else:
            ERR(f'Cannot find a configuration file')

    @property
    def general(self) -> AriaConfigGeneralModel:
        """ Get the [general] config section """
        if self._general is None:
            self._general = AriaConfigGeneralModel(self.section_dict('general'))
        return self._general

    def section(self, section_name: str,
                model_class: type[AriaConfigModelType]
                ) -> AriaConfigModelType:
        """ Get the given section, wrapped in model_class """
        return model_class(self.section_dict(section_name))

    def section_dict(self, section_name: str) -> Mapping[str, str]:
        """ Return the raw section dict, with keys and values as string """
        try:
            return self._parser[section_name]
        except KeyError:
            return {}

    def sections(self, prefix: str = None) -> list[str]:
        """ List of section names that starts with the given prefix """
        if prefix is None:
            return self._parser.sections()
        else:
            return [s for s in self._parser.sections()
                    if s == prefix or s.startswith(prefix + ':')]
