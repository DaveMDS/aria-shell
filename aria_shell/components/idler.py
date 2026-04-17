"""
This is an experimental idle daemon implementation.

I'm not really sure if this should be an aria responsibility,
seems we are fighting with systemd abilities.

I need users reports to understand how to fully implement (or remove) this.

"""
from typing import TYPE_CHECKING
from dataclasses import dataclass

from aria_shell.components import AriaComponent
from aria_shell.config import AriaConfig, AriaConfigModel
from aria_shell.utils import CleanupHelper, exec_command_or_program
from aria_shell.utils.logger import get_loggers
from aria_shell.services.wayland import (
    WaylandService, ExtIdleNotifierV1, ExtIdleNotificationV1,
)
if TYPE_CHECKING:
    from aria_shell.ariashell import AriaShell

DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class IdlerConfig(AriaConfigModel):
    __section__ = 'idler'
    # the config is fully dynamic


@dataclass
class Timeout:
    seconds: int
    command: str
    resume_command: str = None


class AriaIdler(AriaComponent, CleanupHelper):
    """
    Aria idler manager
    """
    def __init__(self, app: AriaShell):
        super().__init__(app)
        self.config = AriaConfig().section(IdlerConfig)

        # keep track of created Notification objects
        self.notifications: list[ExtIdleNotificationV1] = []

        # get the WaylandService singleton instance
        self.ws = WaylandService()
        if not self.ws or not self.ws.connected:
            WRN('WaylandService is not connected to the compositor.')
            return

        # bind the Notifier manager object
        self.manager: ExtIdleNotifierV1 | None = \
            self.ws.bind_object('ext_idle_notifier_v1', 1, ExtIdleNotifierV1)
        if self.manager is None:
            WRN('The compositor does not support the ext_idle_notifier_v1 protocol.')
            return

        # read all timeouts from config
        for tag, command in self.config.options.items():
            if not tag or not command or '-' in tag:
                continue

            # search an 'XXX-resume' option for the resume command
            resume_command = self.config.options.get(f'{tag}-resume', None)

            # parse seconds from time-tag like: 300s, 5m, 1h
            mult = 60  # parse minutes by default
            if tag[-1] in ('s', 'm', 'h'):
                tag, unit = tag[:-1], tag[-1]
                match unit:
                    case 's':
                        mult = 1
                    case 'm':
                        mult = 60
                    case 'h':
                        mult = 60 * 60

            try:
                timeout = int(tag)
            except ValueError:
                ERR('Invalid time in [idler] config section. Time: %s', tag)
                continue

            self.setup_timeout(
                Timeout(timeout * mult, command, resume_command)
            )

    def setup_timeout(self, timeout: Timeout):
        """Create a Notification object from the given Timeout."""
        DBG('Setup idle notification: %s', timeout)
        notification = self.manager.get_idle_notification(
            timeout.seconds * 1000, self.ws.seat
        )
        notification.user_data = timeout
        notification.dispatcher['idled'] = self.on_idled
        notification.dispatcher['resumed'] = self.on_resumed
        self.notifications.append(notification)
        self.ws.roundtrip()

    @staticmethod
    def on_idled(notification: ExtIdleNotificationV1):
        timeout: Timeout = notification.user_data
        DBG('Idle timeout expired %s', timeout)
        exec_command_or_program(timeout.command)

    @staticmethod
    def on_resumed(notification: ExtIdleNotificationV1):
        timeout: Timeout = notification.user_data
        DBG('Idle timeout resumed %s', timeout)
        if timeout.resume_command:
            exec_command_or_program(timeout.resume_command)

    def shutdown(self):
        while self.notifications:
            notification = self.notifications.pop()
            notification.destroy()
        super().shutdown()
