import logging
import sys
from collections.abc import Callable
from pathlib import Path


def get_logger(name: str) -> logging.Logger:
    """ To be used in every file """
    return logging.getLogger(name)


def get_loggers(name: str) -> tuple[Callable, ...]:
    """ To be used in every file, much more handy """
    l = get_logger(name)
    return l.debug, l.info, l.warning, l.error, l.critical


class ColorFormatter(logging.Formatter):
    """ Colored log by level, only used when stdout is a tty """
    (BLACK, RED, GREEN, YELLOW, BLUE, MAGENTA, CYAN,
     WHITE, GRAY, DEFAULT) = map(str, range(30, 40))
    STX, ETX = '\x1b[', 'm'
    RESET = '\x1b[0m'
    BOLD = ';1'
    DIM = ';2'
    ITALIC = ';3'
    COLOR_SEQ = {
        logging.DEBUG: STX + DEFAULT + DIM + ITALIC + ETX,
        logging.INFO: STX + BLUE + ETX,
        logging.WARNING: STX + YELLOW + ETX,
        logging.ERROR: STX + RED + ETX,
        logging.CRITICAL: STX + RED + BOLD + ETX,
    }

    def __init__(self, *args, **kargs):
        super().__init__(*args, **kargs)
        if sys.stdout.isatty():
            self.format = self.colored_format

    def colored_format(self, record: logging.LogRecord) -> str:
        color = self.COLOR_SEQ.get(record.levelno)
        formatted = super().format(record)
        return color + formatted + self.RESET if color else formatted


def setup_logger(level: str, fmt: str, file: Path = None) -> logging.Logger:
    # get the root module logger
    logger = logging.getLogger('aria_shell')
    logger.setLevel(level.upper())

    # output to stdout
    stream_handler = logging.StreamHandler(stream=sys.stdout)
    stream_formatter = ColorFormatter(fmt, style='{')
    stream_handler.setFormatter(stream_formatter)
    logger.addHandler(stream_handler)

    # output to file (optional)
    if file:
        file_handler = logging.FileHandler(file)
        file_formatter = logging.Formatter(fmt, style='{')
        file_handler.setFormatter(file_formatter)
        logger.addHandler(file_handler)

    # logger.debug('Logger test - debug')
    # logger.info('Logger test - info')
    # logger.warning('Logger test - warning')
    # logger.error('Logger test - error')
    # logger.critical('Logger test - critical')
    # sys.exit()

    logger.debug('Logger initialized')

    return logger
