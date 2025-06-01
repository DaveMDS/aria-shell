import json
import os
from collections.abc import Callable
from pathlib import Path

from aria_shell.utils import Singleton
from aria_shell.utils.socket import AriaSocketClient
from aria_shell.utils.logger import get_loggers
from aria_shell.utils.env import XDG_RUNTIME_DIR


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class HyprlandService(metaclass=Singleton):
    """ Implementation of the hyprland IPC sockets """

    def __init__(self):
        """ raise RuntimeError if hyprland is not available """
        # get hyprland current instance signature
        hypr_sig = os.getenv('HYPRLAND_INSTANCE_SIGNATURE')
        if not hypr_sig:
            raise RuntimeError('no hyprland signature found in environment')

        # search hyprland runtime folder
        hypr_dir = XDG_RUNTIME_DIR / 'hypr' / hypr_sig
        if not hypr_dir.is_dir():
            hypr_dir = Path('/tmp/hypr') / hypr_sig  # try pre 0.40.0 path
            if not hypr_dir.is_dir():
                raise RuntimeError('Cannot find hyprland runtime directory')

        # command socket
        self._cmd_socket_path = hypr_dir / '.socket.sock'
        if not self._cmd_socket_path.is_socket():
            raise RuntimeError(f'Cannot find hyprland socket {self._cmd_socket_path}')
        self._cmd_socket = AriaSocketClient(self._cmd_socket_path)

        # event socket
        self._evt_socket_path = hypr_dir / '.socket2.sock'
        if not self._evt_socket_path.is_socket():
            raise RuntimeError(f'Cannot find hyprland socket {self._evt_socket_path}')
        self._evt_socket = AriaSocketClient(self._evt_socket_path)

    def send_command(self, command: str, callback: Callable = None, **kwargs):
        DBG(f'hyprland command: {command}')
        def _cb(raw_data):
            if raw_data and command.startswith('j/'):
                callback(json.loads(raw_data), **kwargs)
            elif raw_data:
                callback(raw_data.decode(), **kwargs)
        if callback:
            self._cmd_socket.send_and_recv(command, _cb)
        else:
            self._cmd_socket.send(command)

    def watch_events(self, callback: Callable, **kwargs):
        def _cb(raw_data):
            data = raw_data.decode()
            if '>>' in data:
                event, event_data = data.split('>>')
                callback(event, event_data, **kwargs)
            else:
                WRN(f'Invalid event from hyprland: "{data}"')
        self._evt_socket.watch(_cb)

    # TODO: unwatch, or class shutdown?