from collections.abc import Callable
from pathlib import Path
from gi.repository import GLib, Gio

from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


SocketSendCallback = Callable[[bool], None]        # send_cb(result: bool)
SocketRecvCallback = Callable[[bytes|None], None]  # recv_cb(data: bytes|None)


def autoconnect(func):
    """Decorator to ensure the connection is available in methods."""
    def wrapper(self: SocketClient, *args, **kwargs):
        self.connect()
        return func(self, *args, **kwargs)
    return wrapper


class SocketClient:
    """
    Useful class to read/write/monitor local unix socket without blocking.

    The socket connection is lazy, it will be established at the first
    read/write operation. You can manually call connect() if needed, and you
    can freely call disconnect(), the socket will be reconnected when needed.

    NOTE:
        The implementation is fully async, except the socket connection that is sync.
        Having the connection async is impracticable with our lazy-connect logic,
        and would make the usage of the class far more complex. As the class is used
        for local socket only this should not be a problem.

    WARNING:
        During an async request no other sync and async calls are allowed,
        result in a CRITICAL error and a None bytes data received in callback.

    """
    PRIORITY = GLib.PRIORITY_DEFAULT
    BUFFER_SIZE = 1024 * 1024

    def __init__(self, socket_path: Path | str, line_buffered = False):
        """
        Args:
            socket_path: the unix path of the socket to connect
            line_buffered: read the socket line by line
        Raise:
            RuntimeError: if the socket is not valid
        """
        if isinstance(socket_path, str):
            socket_path = Path(socket_path)

        if not socket_path or not socket_path.is_socket():
            raise RuntimeError(f'Socket path is not a socket: {socket_path}')

        self._line_buffered = line_buffered
        self._client = Gio.SocketClient()
        self._address = Gio.UnixSocketAddress.new(socket_path.as_posix())
        self._connection: Gio.SocketConnection | None = None
        self._istream: Gio.InputStream | None = None
        self._ostream: Gio.OutputStream | None = None
        self._dstream: Gio.DataInputStream | None = None
        self._cancellable = Gio.Cancellable()

    def __repr__(self):
        return f'<SocketClient fd={self.fd} path={self.path}>'

    def connect(self):
        """Open the socket connection."""
        if not self.connected:
            self._connection = self._client.connect(self._address, self._cancellable)
            self._ostream = self._connection.get_output_stream()
            self._istream = self._connection.get_input_stream()
            if self._line_buffered:
                self._dstream = Gio.DataInputStream.new(self._istream)
            DBG('Socket connected %s', self)

    def disconnect(self):
        """Close the socket connection."""
        if self.connected:
            DBG('Closing socket %s', self)
            if self._ostream:
                self._ostream.close(self._cancellable)
            if self._istream:
                self._istream.close(self._cancellable)
            if self._dstream:
                self._dstream.close(self._cancellable)
            self._connection.close(self._cancellable)

        self._connection = None
        self._ostream = None
        self._istream = None
        self._dstream = None

    @property
    def connected(self) -> bool:
        """Whenever the socket is actually connected."""
        return self._connection and self._connection.is_connected()

    @property
    def busy(self) -> bool:
        """True if an async operation is in progress"""
        return self.connected and (
                self._ostream.has_pending() or self._istream.has_pending())

    @property
    def path(self) -> str:
        """The address path as a string."""
        return self._address.get_path()

    @property
    def fd(self) -> int:
        """The underlying OS socket file descriptor."""
        return self._connection.get_socket().get_fd() if self.connected else -1

    @autoconnect
    def send(self, data: str | bytes, callback: SocketSendCallback = None):
        """Send the given data on the socket."""
        if self._ostream.has_pending():
            CRI(f'Cannot send data, socket is busy! {self}')
            if callable(callback):
                callback(False)
            return

        def _write_done(stream: Gio.OutputStream, result: Gio.AsyncResult):
            try:
                stream.write_all_finish(result)
            except GLib.Error as e:
                ERR('Cannot send data on socket %s. Error: %s', self, e)
                result = False
            else:
                result = True
            if callable(callback):
                callback(result)

        DBG('Sending %d bytes on socket %s', len(data), self)
        self._ostream.write_all_async(
            data.encode() if isinstance(data, str) else data,
            self.PRIORITY, self._cancellable, _write_done
        )

    @autoconnect
    def receive(self, callback: SocketRecvCallback):
        """Receive a single "packet" from the socket."""
        self._read_async(callback)

    @autoconnect
    def send_and_receive(self, data: str | bytes, callback: SocketRecvCallback):
        """Send the data and receive a single "packet"."""
        self.send(data, lambda _: self.receive(callback))

    @autoconnect
    def monitor(self, callback: SocketRecvCallback):
        """Keep an eye on the socket."""
        DBG('Monitoring socket %s', self)
        self._read_async(callback, monitor=True)

    # ----------------------
    # internal async readers
    # ----------------------
    def _read_async(self, callback: SocketRecvCallback, monitor=False):
        if self._istream.has_pending():
            CRI(f'Cannot read data, socket is busy! {self}')
            if callable(callback):
                callback(None)
        else:
            if self._line_buffered:
                self._read_line_async(callback, monitor)
            else:
                self._read_bytes_async(callback, monitor)

    def _read_bytes_async(self, callback: SocketRecvCallback, monitor=False):
        """read bytes from the InputStream self._istream"""
        def _read_next():
            self._istream.read_bytes_async(
                self.BUFFER_SIZE, self.PRIORITY, self._cancellable, _read_done
            )

        def _read_done(stream: Gio.InputStream, result: Gio.AsyncResult):
            try:
                data: bytes = stream.read_bytes_finish(result).get_data()
            except GLib.Error as e:
                ERR('Cannot read bytes from socket %s. Error: %s', self, e)
                callback(None)
            else:
                DBG('Received %d bytes from socket %s', len(data), self)
                callback(data)
                if monitor:
                    _read_next()

        _read_next()

    def _read_line_async(self, callback: SocketRecvCallback, monitor=False):
        """read a line from the DataInputStream self._dstream"""
        def _read_next():
            self._dstream.read_line_async(
                self.PRIORITY, self._cancellable, _read_done
            )

        def _read_done(stream: Gio.DataInputStream, result: Gio.AsyncResult):
            try:
                data, data_len = stream.read_line_finish(result)
            except GLib.Error as e:
                ERR('Cannot read line from socket %s. Error: %s', self, e)
                callback(None)
            else:
                DBG('Received %d bytes from socket %s', data_len, self)
                callback(data)
                if monitor:
                    _read_next()

        _read_next()
