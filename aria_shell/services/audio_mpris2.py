"""

MPRIS2 backend for the AriaAudioService

This backend use DBUS to manage mltimedia players exposing the Mpris2 interface

Reference:
https://specifications.freedesktop.org/mpris-spec/latest/index.html

"""
from gi.repository import Gio

from dasbus.connection import SessionMessageBus

from aria_shell.services.audio import AudioService, MediaPlayer
from aria_shell.utils.logger import get_loggers
from aria_shell.utils import Singleton


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class Mpris2Backend(metaclass=Singleton):
    BASE_PATH = 'org.mpris.MediaPlayer2.'

    def __init__(self, aas: AudioService, cancellable: Gio.Cancellable):
        DBG('MPRIS: init...')
        self._cancellable = cancellable  # TODO needed???

        self.aas = aas
        self.bus = SessionMessageBus()

        # build the list of connected players
        for name in self.bus.proxy.ListNames():
            if name.startswith(self.BASE_PATH):
                self.player_add(name)

        # and keep the list updated when new names appear/vanish
        self.bus.proxy.NameOwnerChanged.connect(self.name_owner_changed_cb)

    def name_owner_changed_cb(self, name: str, _old_owner: str, new_owner: str):
        if name.startswith(self.BASE_PATH):
            if new_owner:
                self.player_add(name)
            else:
                self.player_del(name)

    def player_add(self, obj_path: str):
        DBG(f'MPRIS New player at path: {obj_path}')
        player = Mpris2Player(self.bus, obj_path)
        self.aas.player_added(player)

    def player_del(self, obj_path: str):
        DBG(f'MPRIS Player is gone: {obj_path}')
        self.aas.player_removed(obj_path)


class Mpris2Player(MediaPlayer):
    __gtype_name__ = 'Mpris2AudioPlayer'

    OBJ_PATH = '/org/mpris/MediaPlayer2'
    MAIN_IFACE = 'org.mpris.MediaPlayer2'
    PLAYER_IFACE = 'org.mpris.MediaPlayer2.Player'

    _init_props = {
        'Identity', 'PlaybackStatus', 'Volume', 'CanSeek',
        'CanGoNext', 'CanGoPrevious', 'Metadata'
    }

    def __init__(self, bus: SessionMessageBus, service_name):
        super().__init__(pid=service_name)
        # get a proxy for the Player object (on all ifaces)
        self._proxy = bus.get_proxy(service_name, self.OBJ_PATH)

        # read all props (on both ifaces)
        props = self._proxy.GetAll(self.MAIN_IFACE)
        self._on_props_changed(self.MAIN_IFACE, props, [])
        props = self._proxy.GetAll(self.PLAYER_IFACE)
        self._on_props_changed(self.PLAYER_IFACE, props, [])

        # watch properties for changes (on all ifaces!)
        self._proxy.PropertiesChanged.connect(self._on_props_changed)

    def _on_props_changed(self, iface: str, props: dict, invalidated: list):
        # print('Changed', self, iface, props, invalidated)
        try:
            if iface == self.MAIN_IFACE:
                if 'Identity' in props:
                    if (val := props['Identity'].unpack()) != self.name:
                        self.name = val
            elif iface == self.PLAYER_IFACE:
                if 'PlaybackStatus' in props:
                    if (val := props['PlaybackStatus'].unpack()) != self.status:
                        self.status = val
                if 'Volume' in props:
                    if (val := props['Volume'].unpack()) != self.volume:
                        self.volume = val
                    # stupid firefox do not have Volume
                    if self.has_volume is False:
                        self.has_volume = True
                if 'CanSeek' in props:
                    if (val := props['CanSeek'].unpack()) != self.can_seek:
                        self.can_seek = val
                if 'CanGoNext' in props:
                    if (val := props['CanGoNext'].unpack()) != self.can_go_next:
                        self.can_go_next = val
                if 'CanGoPrevious' in props:
                    if (val := props['CanGoPrevious'].unpack()) != self.can_go_prev:
                        self.can_go_prev = val
                if 'Metadata' in props:
                    metadata = props['Metadata'].unpack()
                    # print("META", metadata)
                    if 'xesam:title' in metadata:
                        if (val := metadata['xesam:title']) != self.title:
                            self.title = val
                    if 'xesam:album' in metadata:
                        if (val := metadata['xesam:album']) != self.album:
                            self.album = val
                    if 'xesam:artist' in metadata:
                        val = metadata['xesam:artist']
                        if isinstance(val, list):
                            val = ', '.join(val)
                        if self.artist != val:
                            self.artist = val
                    if 'mpris:artUrl' in metadata:
                        if (val := metadata['mpris:artUrl']) != self.cover:
                            self.cover = val
        except Exception as e:
            ERR(f'Cannot read player properties. Error: {e}. {self}')

    def play(self):
        try:
            self._proxy.PlayPause()
        except Exception as e:
            ERR(f'Cannot execute PlayPause. Error: {e}')

    def next(self):
        try:
            self._proxy.Next()
        except Exception as e:
            ERR(f'Cannot execute Next. Error: {e}')

    def prev(self):
        try:
            self._proxy.Previous()
        except Exception as e:
            ERR(f'Cannot execute Previous. Error: {e}')

    def set_volume(self, volume: float):
        if volume != self.volume:
            try:
                self._proxy.Volume = volume
            except Exception as e:
                ERR(f'Cannot set Volume property. Error: {e}')

    # def rais(self):
    #     self.proxy.Raise(dbus_interface=self.MAIN_IFACE)
