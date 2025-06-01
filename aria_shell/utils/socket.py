from collections.abc import Callable
from pathlib import Path
from gi.repository import GLib, Gio


class AriaSocketClient:
    """
    Aria abstraction to read/write a unix socket without blocking

    TODO: I really don't like the implementation! Didn't find a way to keep
          connection client and streams at class level instead of recreating
          everything each time
    """
    PRIORITY = 1
    BUFFER_SIZE = 2 * 1024 * 1024

    def __init__(self, socket_path: str | Path):
        if isinstance(socket_path, str):
            socket_path = Path(socket_path)
        if not socket_path or not socket_path.is_socket():
            raise RuntimeError(f'Socket path is not a socket: {socket_path}')

        self.address = Gio.UnixSocketAddress.new(socket_path.as_posix())
        self.cancellable = Gio.Cancellable()

    def send(self, data: str|bytes):
        """ Send some data on the socket """
        self.send_and_recv(data)

    def send_and_recv(self, data: str|bytes, callback: Callable | None = None, line_buffered = False, **kwargs):
        """ Send some data on the socket and wait for the response (if callback is given) """
        if isinstance(data, str):
            data = data.encode()

        def _read_cb(istream: Gio.DataInputStream, res: Gio.AsyncResult, conn: Gio.SocketConnection):
            if line_buffered:
                received: bytes = istream.read_line_finish(res)[0]
            else:
                raw_data: GLib.Bytes = istream.read_bytes_finish(res)
                received: bytes = raw_data.get_data()
            if callback and callable(callback):
                callback(received, **kwargs)
            if received:
                _read_next(istream, conn)
            # TODO:
            # else:
            #   something to cleanup? the 'conn' reference?

        def _read_next(istream: Gio.DataInputStream, conn: Gio.SocketConnection):
            # we must keep the 'conn' reference around (as user_data)
            if line_buffered:
                istream.read_line_async(
                    self.PRIORITY, self.cancellable, _read_cb, conn
                )
            else:
                istream.read_bytes_async(
                    self.BUFFER_SIZE, self.PRIORITY, self.cancellable, _read_cb, conn
                )

        def _connect_cb(_client: Gio.SocketClient, res: Gio.AsyncResult):
            conn = _client.connect_finish(res)
            ostream = conn.get_output_stream()
            ostream.write_async(data, self.PRIORITY, self.cancellable)
            ostream.flush_async(self.PRIORITY, self.cancellable)
            if callback:
                istream = Gio.DataInputStream.new(conn.get_input_stream())
                _read_next(istream, conn)

        client = Gio.SocketClient()
        client.connect_async(self.address, self.cancellable, _connect_cb)

    def watch(self, callback: Callable, **kwargs):
        """
        Monitor the socket for incoming data, line buffered,
        callback will be called for each received line
        """
        def _read_cb(istream: Gio.DataInputStream, res: Gio.AsyncResult, conn: Gio.SocketConnection):
            received: bytes = istream.read_line_finish(res)[0]
            if callback and callable(callback):
                callback(received, **kwargs)
            if received:
                _read_next(istream, conn)
            # TODO:
            # else:
            #   something to cleanup? the 'conn' reference?

        def _read_next(istream: Gio.DataInputStream, conn: Gio.SocketConnection):
            # we must keep the 'conn' reference around (as user_data)
            istream.read_line_async(self.PRIORITY, self.cancellable, _read_cb, conn)

        def _connect_cb(_client: Gio.SocketClient, res: Gio.AsyncResult):
            conn = _client.connect_finish(res)
            istream = Gio.DataInputStream.new(conn.get_input_stream())
            _read_next(istream, conn)

        client = Gio.SocketClient()
        client.connect_async(self.address, self.cancellable, _connect_cb)

