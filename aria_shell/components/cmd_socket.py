from __future__ import annotations

from gi.repository import GLib, Gio, Gtk

from aria_shell.components.commands import AriaCommands
from aria_shell.utils import Singleton
from aria_shell.utils.logger import get_loggers
from aria_shell.utils.env import  ARIA_RUNTIME_DIR


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class AriaCommandSocket(metaclass=Singleton):
    def __init__(self, _app: Gtk.Application):

        self.cancellable = Gio.Cancellable()
        self.cmds = AriaCommands()

        socket_path = ARIA_RUNTIME_DIR / 'cmd.sock'
        socket_path.unlink(missing_ok=True)

        INF(f'Start listening for commands on socket: {socket_path}')
        address = Gio.UnixSocketAddress.new(socket_path.as_posix())
        service = Gio.SocketService()
        service.add_address(address,Gio.SocketType.STREAM, Gio.SocketProtocol.DEFAULT)
        service.connect('incoming', self.on_new_connection)
        service.start()

    def on_new_connection(self, _service: Gio.SocketService, connection: Gio.SocketConnection, *_):
        DBG('Client connected to socket')
        try:
            input_stream = Gio.DataInputStream.new(connection.get_input_stream())
            output_stream = connection.get_output_stream()
            self.read_data_async(input_stream, output_stream, connection)
        except Exception as e:
            ERR(f'Error accepting socket connection: {e}')
            connection.close()

    def read_data_async(self,
                        input_stream: Gio.DataInputStream,
                        output_stream: Gio.OutputStream,
                        connection: Gio.SocketConnection,
                        ):
        """Legge i dati dal client in modo asincrono"""

        def on_write_ready(stream: Gio.OutputStream, task: Gio.Task):
            try:
                stream.write_bytes_finish(task)
            except Exception as e:
                ERR(f'Error writing on socket: {e}')
                connection.close()

        def on_line_ready(stream: Gio.DataInputStream, result: Gio.Task):
            try:
                if received := stream.read_line_finish(result)[0]:
                    # process received data (execute the aria command)
                    response = self.cmds.run(received.decode())
                    # send response
                    output_stream.write_bytes_async(
                        GLib.Bytes.new(response.encode() + b'\n'),
                        GLib.PRIORITY_DEFAULT,
                        None,
                        on_write_ready,
                    )
                    # listen for the next line
                    stream.read_line_async(
                        GLib.PRIORITY_DEFAULT, self.cancellable, on_line_ready
                    )
                else:
                    DBG('Client closed connection')
                    connection.close()
            except Exception as e:
                ERR(f'Error reading from socket: {e}')
                connection.close()

        # start listening for the first line
        input_stream.read_line_async(
            GLib.PRIORITY_DEFAULT, self.cancellable, on_line_ready
        )
