from typing import NamedTuple
from enum import IntEnum

from dasbus.connection import SessionMessageBus
from dasbus.typing import Str, Int32, UInt32, Variant
from dasbus.server.interface import (
    dbus_interface, dbus_signal,
    returns_multiple_arguments  # noqa   (dasbus issue #139)
)

from gi.repository import GObject, Gio, GdkPixbuf

from aria_shell import __version__ as aria_version
from aria_shell.utils import Singleton, Timer
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class Urgency(IntEnum):
    LOW = 0
    NORMAL = 1
    CRITICAL = 2


class CloseReason(IntEnum):
    EXPIRED = 1    # The notification expired.
    DISMISSED = 2  # The notification was dismissed by the user.
    CLOSED = 3     # The notification was closed by a call to CloseNotification.
    UNDEFINED = 4  # Undefined/reserved reasons.


class Action(NamedTuple):
    id: str
    label: str


class Notification(GObject.Object):
    """This class represent a single notification item in the store."""
    __gtype_name__ = 'Notification'

    # "reactive" properties that can be watched/binded
    summary = GObject.Property(type=str)
    body = GObject.Property(type=str)
    icon = GObject.Property(type=str)
    icon_data = GObject.Property(type=GdkPixbuf.Pixbuf)
    urgency = GObject.Property(type=int)  # How to make an Urgency property?

    _unique_id = 0

    def __init__(self,
                 service: NotificationService,
                 app_name: str,
                 actions: list[Action],
                 expire_in: int,
                 ):
        super().__init__()
        Notification._unique_id += 1
        self.id = Notification._unique_id
        self.app_name = app_name
        self.actions = actions
        self._service = service
        self._timer: Timer | None = None
        if expire_in:
            self._timer = Timer(expire_in, self.close, CloseReason.EXPIRED)

    def __repr__(self):
        return f'<Notification {self.id} app={self.app_name} summary="{self.summary}" icon={self.icon} {self.urgency}>'

    def action(self, action: Action):
        """Emit the give action on the bus."""
        self._service.action_invoked(self, action)

    def close(self, reason: CloseReason):
        """Close the notification."""
        self._service.close_notification(self, reason)

    def shutdown(self):
        """The notification has been closed. (only called from the server!)"""
        if self._timer:
            self._timer.stop()
            self._timer = None


SESSION_BUS = SessionMessageBus()
DBUS_SERVICE = 'org.freedesktop.Notifications'
DBUS_IFACE = 'org.freedesktop.Notifications'
DBUS_PATH = '/org/freedesktop/Notifications'
SPEC_VERSION = '1.3'


@dbus_interface(DBUS_IFACE)
class NotificationService(metaclass=Singleton):
    """ Implementation of the DBUS Notification Server

    The service keep an updated ListStore of Notification objects.

    User can watch/bind the store as a list or the single properties of
    the Notifications items.
    """
    def __init__(self):
        INF('Initializing NotificationService')
        self._store = Gio.ListStore(item_type=Notification)
        self._default_expire = 30
        self._connected = False

    #---------------------------------------------------------------------------
    # Python Api
    #---------------------------------------------------------------------------
    def start_server(self, default_expire: int = None):
        """Publish self on the bus, and register the service name."""
        if default_expire is not None:
            self._default_expire = default_expire
        if not self._connected:
            DBG(f'Publishing {DBUS_SERVICE} on D-Bus')
            try:
                SESSION_BUS.publish_object(DBUS_PATH, self)
                SESSION_BUS.register_service(DBUS_SERVICE)
                self._connected = True
            except Exception as e:
                ERR(f'Cannot register DBUS service: {DBUS_SERVICE}.'
                    f'Error: {e}. Is another Notification daemon running?')

    def stop_server(self):
        """Remove self from the bus."""
        if self._connected:
            DBG(f'Removing {DBUS_SERVICE} from D-Bus')
            SESSION_BUS.unregister_service(DBUS_SERVICE)
            SESSION_BUS.unpublish_object(DBUS_PATH)
            self._store.remove_all()
            self._connected = False

    def get_list_model(self) -> Gio.ListStore:
        """Get the model filled with Notification objects."""
        return self._store

    def action_invoked(self, notification: Notification, action: Action):
        """An action ha been selected by the user. Emit the signal on DBUS."""
        self.ActionInvoked.emit(notification.id, action.id)

    def close_notification(self, notification: Notification, reason: CloseReason):
        """Emit the NotificationClosed DBUS signal and remove Notification from the store."""
        DBG('Close %s %s', notification, reason.name)
        # cleanup the Notification object
        notification.shutdown()
        # emit the signal on DBUS
        self.NotificationClosed.emit(notification.id, reason.value)
        # remove the item from the store
        found, pos = self._store.find(notification)
        if found and pos >= 0:
            self._store.remove(pos)

    def _find_notification_by_id(self, notification_id: int) -> Notification | None:
        notification: Notification
        for notification in self._store:
            if notification.id == notification_id:
                return notification
        return None

    #---------------------------------------------------------------------------
    # Api automatically exposed on DBUS
    #---------------------------------------------------------------------------
    @staticmethod
    def GetCapabilities() -> list[Str]:
        # TODO persistence, sound
        return ['body', 'actions', 'body-markup']

    @returns_multiple_arguments
    def GetServerInformation(self) -> tuple[Str, Str, Str, Str]:
        return 'aria-shell', 'gurumeditation.it', aria_version, SPEC_VERSION

    def Notify(self, app_name: Str, replaces_id: UInt32, app_icon: Str,
               summary: Str, body: Str, actions: list[Str],
               hints: dict[Str, Variant], expire_timeout: Int32,
               ) -> UInt32:
        """A client request to show a notification."""
        INF('Notification from "%s". Summary: "%s"', app_name, summary)

        # decode image-data into a Gdk Pixbuf
        if image_data := hints.get('image-data', hints.get('image_data', None)):
            pixbuf = _decode_image_data(image_data)
        else:
            pixbuf = None

        # image path or icon name
        if image_path := hints.get('image-path', hints.get('image_path', None)):
            image_path = image_path.get_string()

        # urgency
        if 'urgency' in hints:
            urgency = hints['urgency'].get_byte()
        else:
            urgency = Urgency.NORMAL.value

        # actions list
        actions_list = [
            Action(actions[i], actions[i + 1])
            for i in range(0, len(actions), 2)
        ]

        # expire timeout in seconds (-1 means use default)
        if expire_timeout < 0:
            expire_timeout = self._default_expire
        else:
            expire_timeout = int(expire_timeout / 1000)

        # create a new Notification object or reuse and existing one
        notification: Notification | None = None
        if replaces_id > 0:
            notification = self._find_notification_by_id(replaces_id)

        if notification is None:
            notification = Notification(
                service=self,
                app_name=app_name,
                actions=actions_list,
                expire_in=expire_timeout,
            )
            self._store.insert(0, notification)

        # update reactive properties on the Notification object
        notification.summary = summary
        notification.body = body
        notification.icon = image_path or app_icon
        notification.icon_data = pixbuf
        notification.urgency = urgency

        # return the notification ID
        return UInt32(notification.id)

    def CloseNotification(self, notification_id: UInt32):
        """A client request to close an existing notification."""
        DBG('CloseNotification(%s)', notification_id)
        notification = self._find_notification_by_id(notification_id)
        if notification:
            self.close_notification(notification, CloseReason.CLOSED)

    @dbus_signal
    def NotificationClosed(self, notification_id: UInt32, reason: UInt32):
        """Emitted by the server when a notification is closed."""

    @dbus_signal
    def ActionInvoked(self, notification_id: UInt32, action_key: Str):
        """Emitted by the server when user select an action in the notification."""


def _decode_image_data(image_data: Variant) -> GdkPixbuf.Pixbuf:
    """Decode the image as received from DBUS in the form (iiibiiay)

    w, h, row_stride, has_alpha, bps, channels, data = image_data

    NOTE: below is SUPER slow!! 5 seconds with a 1024x1024 image !!!
          the fast trick is to use the get_data_as_bytes function!
    """
    width = image_data.get_child_value(0).get_int32()
    height = image_data.get_child_value(1).get_int32()
    stride = image_data.get_child_value(2).get_int32()
    alpha = image_data.get_child_value(3).get_boolean()
    bps = image_data.get_child_value(4).get_int32()
    # channels = image_data.get_child_value(5).get_int32()
    data = image_data.get_child_value(6).get_data_as_bytes()

    return GdkPixbuf.Pixbuf.new_from_bytes(
        data, GdkPixbuf.Colorspace.RGB, alpha, bps,
        width, height, stride
    )
