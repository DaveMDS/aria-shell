from typing import TYPE_CHECKING, Literal

from gi.repository import Gtk, Gio, Gdk

from aria_shell.components import AriaComponent
from aria_shell.services.notifications import \
    Notification, NotificationService, Urgency, CloseReason, Action
from aria_shell.config import AriaConfig, AriaConfigModel
from aria_shell.ui import AriaWindow, CleanupHelper
from aria_shell.utils import clamp
from aria_shell.utils.logger import get_loggers
if TYPE_CHECKING:
    from aria_shell.ariashell import AriaShell

DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


POSITIONS = {
    'top-left':      [AriaWindow.Edge.TOP, AriaWindow.Edge.LEFT],
    'top-right':     [AriaWindow.Edge.TOP, AriaWindow.Edge.RIGHT],
    'top-center':    [AriaWindow.Edge.TOP],
    'bottom-left':   [AriaWindow.Edge.BOTTOM, AriaWindow.Edge.LEFT],
    'bottom-right':  [AriaWindow.Edge.BOTTOM, AriaWindow.Edge.RIGHT],
    'bottom-center': [AriaWindow.Edge.BOTTOM],
}


class NotificatorConfig(AriaConfigModel):
    enabled: bool = True
    duration: int = 20
    position: Literal['top-left', 'top-right','top-center',
                      'bottom-left','bottom-right','bottom-center'] = 'top-right'
    opacity: int = 100

    @staticmethod
    def validate_duration(val: int):
        return clamp(val, 1, None)

    @staticmethod
    def validate_opacity(val: int):
        return clamp(val, 0, 100)


class AriaNotificator(CleanupHelper, AriaComponent):
    """The notificator window show a ListView of Notification."""
    def __init__(self, app: AriaShell):
        super().__init__(app)

        # load config, nicely wrapped in a NotificatorConfig model
        self.config = AriaConfig().section('notifications', NotificatorConfig)
        if not self.config.enabled:
            raise RuntimeError('Notifications disabled by config')

        # initialize the window
        self.win = AriaWindow(
            app=app,
            namespace='aria-notificator',
            title='Aria notificator',
            layer=AriaWindow.Layer.OVERLAY,
            anchors=POSITIONS[self.config.position],
            keyboard_mode=AriaWindow.KeyboardMode.NONE,
            opacity=self.config.opacity / 100.0,
        )

        # initialize the NotificationService
        self.notification_service = NotificationService()
        self.notification_service.start_server(self.config.duration)
        notifications_model = self.notification_service.get_list_model()
        self.safe_connect(notifications_model, 'items_changed', self._on_items_changed)

        # create the items factory for the ListView
        factory = Gtk.SignalListItemFactory()
        self.safe_connect(factory, 'setup', self._factory_item_setup)
        self.safe_connect(factory, 'bind', self._factory_item_bind)
        self.safe_connect(factory, 'unbind', self._factory_item_unbind)

        # create the ListView
        selection_model = Gtk.NoSelection(model=notifications_model)
        self.list_view = Gtk.ListView(
            model=selection_model,
            factory=factory,
            orientation=Gtk.Orientation.VERTICAL,
        )
        self.list_view.add_css_class('aria-notificator-list')
        self.win.set_child(self.list_view)

    def shutdown(self):
        # stop the notification server
        if self.notification_service:
            self.notification_service.stop_server()
            self.notification_service = None
        # destroy the window
        if self.win:
            self.win.destroy()
            self.win = None
        # disconnect all safe_connected signals
        super().shutdown()

    def _on_items_changed(self, model: Gio.ListStore, _pos, _added, _removed):
        # keep the window at a minimum size (needed when the ListView shrink)
        self.win.set_default_size(-1, -1)
        # only show the window when there are notifications to show
        self.win.show() if model.get_n_items() > 0 else self.win.hide()

    @staticmethod
    def _factory_item_setup(_, list_item: Gtk.ListItem):
        """Create a new empty NotificationView object."""
        list_item.set_child(NotificationView())

    @staticmethod
    def _factory_item_bind(_, list_item: Gtk.ListItem):
        """Bind the previously created NotificationView with the Notification."""
        item: Notification = list_item.get_item()  # noqa
        view: NotificationView = list_item.get_child()  # noqa
        view.bind(item)

    @staticmethod
    def _factory_item_unbind(_, list_item: Gtk.ListItem):
        """Unbind the previously binded Notification."""
        view: NotificationView = list_item.get_child()  # noqa
        view.unbind()


class NotificationView(Gtk.Grid):
    """A Gtk.Widget that is able to be binded with a Notification object."""
    __gtype_name__ = 'NotificationView'

    def __init__(self):
        super().__init__()
        self.add_css_class('aria-notification')

        # keep track of connected bindings and handlers
        self.helper = CleanupHelper()

        # icon image
        self.icon = Gtk.Image()
        self.icon.add_css_class('aria-notification-icon')
        self.icon.hide()  # will be shown only if needed

        # summary label
        self.label1 = Gtk.Label(hexpand=True, halign=Gtk.Align.START)
        self.label1.add_css_class('title-4')
        self.label1.add_css_class('aria-notification-summary')

        # body label
        # GOSH, gtk label cannot fit container size by design...
        # the only way I found to make the text wrap without breaking
        # the window size is setting max_width_chars here.
        # This means that the size of the notification is hardcoded here and
        # cannot be changed in CSS...stupid gtk...
        self.label2 = Gtk.Label(use_markup=True, xalign=0,
                                wrap=True, wrap_mode=Gtk.WrapMode.WORD,
                                width_chars=30, max_width_chars=30,
                                natural_wrap_mode=Gtk.NaturalWrapMode.WORD)
        self.label2.add_css_class('aria-notification-body')

        # actions box
        self.actions_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL,
                                   hexpand=True, halign=Gtk.Align.CENTER)
        self.actions_box.add_css_class('aria-notification-actions')

        # layout the children using the grid
        self.attach(self.icon,        column=0, row=0, width=1, height=2)
        self.attach(self.label1,      column=1, row=0, width=1, height=1)
        self.attach(self.label2,      column=1, row=1, width=1, height=1)
        self.attach(self.actions_box, column=0, row=2, width=2, height=1)

        # EventController to receive mouse clicks
        self.event_controller = Gtk.GestureClick(button=0)
        self.add_controller(self.event_controller)

        # currently binded Notification
        self.notification: Notification | None = None

    def bind(self, notification: Notification):
        """Link the widget to the given Notification object."""
        self.notification = notification

        # bind summary and body labels
        self.helper.safe_bind(
            notification, 'summary', self.label1, 'label'
        )
        self.helper.safe_bind(
            notification, 'body', self.label2, 'label'
        )
        # watch properties that cannot be easily binded
        self.helper.safe_connect(
            notification, 'notify::icon', self._icon_changed
        )
        self.helper.safe_connect(
            notification, 'notify::icon-data', self._pixbuf_changed
        )
        self.helper.safe_connect(
            notification, 'notify::urgency', self._urgency_changed
        )
        # create the needed action buttons for this notification
        for action in notification.actions:
            btn = Gtk.Button(label=action.label)
            btn.add_css_class('aria-notification-button')
            self.actions_box.append(btn)
            self.helper.safe_connect(
                btn, 'clicked', self._action_button_clicked, action
            )
        # connect mouse click on the "background" box
        self.helper.safe_connect(
            self.event_controller, 'released', self._notification_clicked
        )

    def unbind(self):
        # cleanup all references to Notification
        self.icon.hide()
        self.helper.shutdown()
        while button := self.actions_box.get_last_child():
            self.actions_box.remove(button)
        self.notification = None

    def _icon_changed(self, notification: Notification, _):
        if notification.icon:
            if notification.icon.startswith('/'):
                self.icon.set_from_file(notification.icon)
            elif notification.icon.startswith('file://'):
                self.icon.set_from_file(notification.icon[7:])
            else:
                self.icon.set_from_icon_name(notification.icon)
            self.icon.show()

    def _pixbuf_changed(self, notification: Notification, _):
        if notification.icon_data:
            texture = Gdk.Texture.new_for_pixbuf(notification.icon_data)
            self.icon.set_from_paintable(texture)
            self.icon.show()

    def _urgency_changed(self, notification: Notification, _):
        self.remove_css_class('urgent')
        self.remove_css_class('non-urgent')
        if notification.urgency == Urgency.CRITICAL:
            self.add_css_class('urgent')
        elif notification.urgency == Urgency.LOW:
            self.add_css_class('non-urgent')

    def _notification_clicked(self, _gesture, _n_press, _x, _y):
        self.notification.close(CloseReason.DISMISSED)

    def _action_button_clicked(self, _button, action: Action):
        self.notification.action(action)
