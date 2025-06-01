from __future__ import annotations

from aria_shell.utils import Singleton
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


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
            except RuntimeError as e:
                return f'ERROR: {e}'

        return f'Unknown command: {command}'
        # match command:
        #
        #     case 'show':
        #         if len(params) != 1:
        #             response = 'invalid params'
        #         elif params[0] == 'launcher':
        #             if self.app.launcher:
        #                 self.app.launcher.show()
        #             else:
        #                 WRN('Launcher not available')
        #         else:
        #             response = f'invalid param: {params[0]}'
        #
        #     case 'ping':
        #         response = command.replace('i', 'o', 1)
        #
        #     case _:
        #         response =
        #
        # return response

    @staticmethod
    def _cmd_ping(_params: list[str]):
        return 'pong'

    def _cmd_show(self, params: list[str]):
        """ show <component> """
        if len(params) != 1:
            raise RuntimeError('invalid params')

        match params[0]:
            case 'launcher':
                if not self.app.launcher:
                    raise RuntimeError('launcher not available')
                self.app.launcher.show()
                return 'OK'

            case _:
                raise RuntimeError(f'unknown component: {params[0]}')