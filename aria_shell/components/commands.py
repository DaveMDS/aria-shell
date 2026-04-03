from __future__ import annotations

from aria_shell.utils import Singleton
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class CommandError(Exception):
    pass


class AriaCommands(metaclass=Singleton):
    def __init__(self, app = None):
        if not app:
            raise RuntimeError('AriaCommands not initialized')

        from aria_shell.ariashell import AriaShell  # avoid recursive imports
        self.app: AriaShell = app

    def run(self, command: str) -> str:
        DBG(f'processing command: "{command}"')

        if ' ' in command:
            command, *params = command.split(' ')
        else:
            params: list[str] = []

        # dispatch to the correct function
        if method := getattr(self, f'_cmd_{command}', None):
            try:
                return method(params)
            except CommandError as e:
                return f'ERROR: {e}'

        return f'Unknown command: {command}'

    @staticmethod
    def _cmd_ping(_params: list[str]) -> str:
        """ ping """
        return 'pong'

    def _cmd_reload(self, _params: list[str]) -> str:
        """ reload """
        self.app.reload()
        return 'OK'

    def _cmd_show(self, params: list[str]) -> str:
        """ show <component> """
        if len(params) != 1:
            raise CommandError('invalid params')

        match params[0]:
            case 'launcher':
                if self.app.launcher:
                    self.app.launcher.toggle()
                    return 'OK'
                raise CommandError('launcher not available')

            case 'terminal':
                if self.app.terminal:
                    self.app.terminal.toggle()
                    return 'OK'
                raise CommandError('terminal not available')

            case 'exiter':
                if self.app.exiter:
                    self.app.exiter.toggle()
                    return 'OK'
                raise CommandError('exiter not available')

            case 'locker':
                if self.app.locker:
                    self.app.locker.lock()
                    return 'OK'
                raise CommandError('exiter not available')

            case _:
                raise CommandError(f'unknown component: {params[0]}')
