import json
import os
from collections.abc import Callable
from pathlib import Path
from typing import NamedTuple

from aria_shell.utils import Singleton
from aria_shell.utils.socket import SocketClient
from aria_shell.utils.logger import get_loggers
from aria_shell.utils.env import XDG_RUNTIME_DIR


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


CommandCallback = Callable[[str|list|dict], None]


class HyprCommand(NamedTuple):
    command: str
    callback: CommandCallback


class HyprlandService(metaclass=Singleton):
    """ Implementation of the hyprland IPC sockets """

    def __init__(self):
        """ raise RuntimeError if hyprland is not available """
        self._commands_queue: list[HyprCommand] = []

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

        # create the command socket
        self._cmd_socket_path = hypr_dir / '.socket.sock'
        if not self._cmd_socket_path.is_socket():
            raise RuntimeError(f'Cannot find hyprland socket {self._cmd_socket_path}')
        self._cmd_socket = SocketClient(self._cmd_socket_path)

        # create the event socket
        self._evt_socket_path = hypr_dir / '.socket2.sock'
        if not self._evt_socket_path.is_socket():
            raise RuntimeError(f'Cannot find hyprland socket {self._evt_socket_path}')
        self._evt_socket = SocketClient(self._evt_socket_path, line_buffered=True)

    def send_command(self, command: str, callback: CommandCallback = None):
        cmd = HyprCommand(command, callback)
        self._commands_queue.append(cmd)
        self._process_queue()

    def _process_queue(self):
        if self._commands_queue and not self._cmd_socket.busy:
            cmd = self._commands_queue.pop(0)
            self._send_command(cmd)

    def _send_command(self, cmd: HyprCommand):
        DBG('Sending hyprland command: %s', cmd.command)

        def _receive_cb(raw_data):
            # NOTE: The hyprland socket MUST be closed after each request,
            # otherwise hypr will block ..."luckily" our socket abstraction
            # auto-reconnect on next operation.
            self._cmd_socket.disconnect()

            # pass decoded data to the user callback
            if callable(cmd.callback) and raw_data:
                try:
                    if cmd.command.startswith('j/'):
                        data = json.loads(raw_data)
                    else:
                        data = raw_data.decode()
                except Exception as e:
                    ERR('Cannot decode response! Error: %s', e)
                else:
                    cmd.callback(data)

            # process next request in queue
            self._process_queue()

        self._cmd_socket.send_and_receive(cmd.command, _receive_cb)

    def watch_events(self, callback: Callable):
        def _monitor_cb(data: bytes|None):
            if data and b'>>' in data:
                event, event_data = data.decode().split('>>', 1)
                DBG('Received hyprland event: %s "%s"', event, event_data)
                callback(event, event_data)
            else:
                WRN('Invalid event from hyprland: "%s"', data)

        self._evt_socket.monitor(_monitor_cb)

    # TODO: unwatch, or class shutdown?