"""

A service to execute aria commands.

Commands must be registered using register() and must provide a callback
(runner) that will take care of executing the command.

Usage:
AriaCommands().register('pippo', the_pippo_command)

def the_pippo_command(cmd: str, params: list[str]) -> str | None
    # the command stuff ...

    # you can raise CommandFailed with an error message like:
    raise CommandFailed('reason for the fail')

    # or return a message on success, return None is still a success.
    return 'An optional (success) response string'

"""
from collections.abc import Callable

from gi.repository import Gio, GLib

from aria_shell.utils import Singleton
from aria_shell.utils.env import ARIA_RUNTIME_DIR
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)



class CommandFailed(Exception):
    """Exception to raise from command runners to notify a failure."""
    pass


CommandRunner = Callable[[str, list[str]], str|None]
CommandResult = tuple[bool, str]


def the_ping_command(_cmd: str, params: list[str]) -> str:
    """The omni-present ping command, the one and only! :) """
    return f'pong {params}'


class AriaCommands(metaclass=Singleton):
    """
    The main Singleton commands service.
    """
    def __init__(self):
        # index of registered command runners
        self._commands: dict[str, CommandRunner] = {
            'ping': the_ping_command,
        }
        # listen for commands on the socket
        self._socket_listener = SocketListener()

    def register(self, prefix: str, runner: CommandRunner):
        """Register `prefix` as a command. `runner` will be called to execute the command."""
        if prefix in self._commands:
            ERR('Command <%s> already registered', prefix)
        else:
            DBG('Registering command <%s> runner: %s', prefix, runner)
            self._commands[prefix] = runner

    def unregister(self, prefix):
        """Remove a previously registered command."""
        if not prefix in self._commands:
            ERR('Command <%s> is not registered', prefix)
        else:
            DBG('Un-registering command <%s>', prefix)
            del self._commands[prefix]

    def run(self, command: str) -> CommandResult:
        """Execute the given command, dispatching to a registered runner."""
        DBG('Processing command: <%s>', command)
        if not command:
            return False, 'Empty command!'

        # support the config file syntax: 'aria terminal toggle'
        if command.startswith('aria '):
            command = command[5:]

        # split command and params
        params = command.strip().split(' ')
        # TODO handle "params with spaces" ! ma non vedo le virgolette??
        if len(params) > 1:
            command, *params = params
        else:
            params = []

        # find a registered runner
        runner = self._commands.get(command, None)
        if not callable(runner):
            return False, f'Unknown command <{command}>'

        # let the runner execute the command
        try:
            response = runner(command, params)
            return True, response
        except CommandFailed as e:
            return False, str(e)
        except Exception as e:
            ERR('Error running command <%s>. %s: %s', command,
                type(e).__name__, e, exc_info=True)
            return False, str(e)


class SocketListener(metaclass=Singleton):
    def __init__(self):
        self.cancellable = Gio.Cancellable()
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
        """Asynchronous wait-command -> send-response loop."""

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
                    success, response = AriaCommands().run(received.decode())
                    full_reply = f'{'OK' if success else 'ERR'} {response or ''}'
                    # send response
                    output_stream.write_bytes_async(
                        GLib.Bytes.new(full_reply.strip().encode() + b'\n'),
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
                ERR(f'Error reading from socket: {e}', exc_info=True)
                connection.close()

        # start listening for the first line
        input_stream.read_line_async(
            GLib.PRIORITY_DEFAULT, self.cancellable, on_line_ready
        )
