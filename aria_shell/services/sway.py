"""

Implementation of the Sway IPC sockets.

"""

import json
import os
import enum
import sys
from collections.abc import Callable
from typing import Any, NamedTuple

from aria_shell.utils import Singleton
from aria_shell.utils.socket import SocketClient
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


PAYLOAD_MAGIC_STRING = b'i3-ipc'
EVT_OFFSET = 0x80000000


class MessageType(enum.Enum):
    RUN_COMMAND = 0
    GET_WORKSPACES = 1
    SUBSCRIBE = 2
    GET_OUTPUTS = 3
    GET_TREE = 4
    GET_MARKS = 5
    GET_BAR_CONFIG = 6
    GET_VERSION = 7
    GET_BINDING_MODES = 8
    GET_CONFIG = 9
    SEND_TICK = 10
    SYNC = 11
    GET_BINDING_STATE = 12
    GET_INPUTS = 100
    GET_SEATS = 101
    EVT_WORKSPACE = EVT_OFFSET
    EVT_MODE = EVT_OFFSET | 2
    EVT_WINDOW = EVT_OFFSET | 3
    EVT_BARCONFIG = EVT_OFFSET | 4
    EVT_BINDING = EVT_OFFSET | 5
    EVT_SHUTDOWN = EVT_OFFSET | 6
    EVT_TICK = EVT_OFFSET | 7
    EVT_BAR_STATE = EVT_OFFSET | 14
    EVT_INPUT = EVT_OFFSET | 15


class SwayMessage(NamedTuple):
    type: MessageType
    data: str | dict | list | None


class QueueItem(NamedTuple):
    type: MessageType
    data: str | dict | list | None
    callback: MessageCallback


MessageCallback = Callable[[dict|list|None], None]  # msg_cb(response: dict|list|None) -> None
EventCallback   = Callable[[SwayMessage], None]     # event_cb(event: SwayMessage) -> None


class SwayService(metaclass=Singleton):
    """ Implementation of the sway IPC sockets """

    def __init__(self):
        """Raise RuntimeError if sway is not available."""
        swaysock = os.getenv('SWAYSOCK')
        if not swaysock:
            raise RuntimeError('No SWAYSOCK found in environment')
        self._cmd_socket = SocketClient(swaysock)
        self._evt_socket = SocketClient(swaysock)
        self._send_queue: list[QueueItem] = []

    def send_message(self,
                     mtype: MessageType,
                     payload: Any = '',
                     callback: MessageCallback = None):
        """Send a message to Sway, callback will be called with the response."""
        cmd = QueueItem(mtype, payload, callback)
        self._send_queue.append(cmd)
        self._process_queue()

    def _process_queue(self):
        if self._send_queue and not self._cmd_socket.busy:
            cmd = self._send_queue.pop(0)
            self._send_message(cmd)

    def _send_message(self, item: QueueItem):
        DBG('Sway IPC send: %s %s', item.type, item.data)

        def _recv_cb(data: bytes|None):
            if not data:
                if callable(item.callback):
                    item.callback(None)
                return

            for response in self._deserialize(data):
                if response.type != item.type:
                    ERR('IPC response type %s does not match sent type %s',
                        response.type, item.type)
                elif callable(item.callback):
                    item.callback(response.data)

            # process next request in queue
            self._process_queue()

        message = self._serialize(item.type, item.data)
        self._cmd_socket.send_and_receive(message, _recv_cb)

    def subscribe(self, events: list[str], callback: EventCallback):
        """Subscribe to the given events, ex: window,workspace,..."""

        def _process_event(event: SwayMessage):
            if event.type == MessageType.SUBSCRIBE:
                if event.data and event.data.get('success', False):
                    INF('Successfully subscribed Sway IPC events: %s', events)
                else:
                    ERR('Cannot subscribe sway events socket')
            else:
                callback(event)

        def _monitor_cb(b: bytes):
            for event in self._deserialize(b):
                _process_event(event)

        message = self._serialize(MessageType.SUBSCRIBE, json.dumps(events))
        self._evt_socket.monitor(_monitor_cb)
        self._evt_socket.send(message)

    ## ---------------------
    ## Primary IPC messages
    ## ---------------------
    def run_command(self, command: str, callback: MessageCallback = None):
        """Run a Sway command, or series of commands delimited by comma or semicolon."""
        self.send_message(MessageType.RUN_COMMAND, command, callback=callback)

    def get_workspaces(self, callback: MessageCallback):
        """Get a list of all workspaces."""
        self.send_message(MessageType.GET_WORKSPACES, callback=callback)

    def get_outputs(self, callback: MessageCallback):
        """Get a list of all outputs, including the invisible scratchpad output."""
        self.send_message(MessageType.GET_OUTPUTS, callback=callback)

    def get_tree(self, callback: MessageCallback):
        """Get the full node tree."""
        self.send_message(MessageType.GET_TREE, callback=callback)

    @staticmethod
    def _serialize(ptype: MessageType, payload: str | bytes) -> bytes:
        """Serialize the message using the sway IPC format."""
        if isinstance(payload, str):
            payload = payload.encode('utf-8')
        return b''.join([
            PAYLOAD_MAGIC_STRING,
            len(payload).to_bytes(4, byteorder=sys.byteorder),
            ptype.value.to_bytes(4, byteorder=sys.byteorder),
            payload
        ])

    @staticmethod
    def _deserialize(received: bytes) -> list[SwayMessage]:
        """Return the list of SwayMessage decoded from the received bytes."""
        magic_len = len(PAYLOAD_MAGIC_STRING)
        messages = []
        start = 0
        while received and start < len(received):
            try:
                # check and skip the magic string
                assert received[start:start+magic_len] == PAYLOAD_MAGIC_STRING
                start += magic_len

                # extract len, type and raw payload
                length = int.from_bytes(received[start:start+4], byteorder=sys.byteorder)
                type_id = int.from_bytes(received[start+4:start+8], byteorder=sys.byteorder)
                payload = received[start+8:start+8+length]

                # JSON decode the payload and store in list
                data = json.loads(payload)
                msg = SwayMessage(MessageType(type_id), data)
                messages.append(msg)
            except Exception as e:
                ERR('Cannot deserialize sway message. Error %s', e)
                return []

            # jump to next message (if available)
            start = start + 8 + length

        return messages
