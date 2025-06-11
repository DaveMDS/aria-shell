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

        # watch properties for changes (and update now)
        self._proxy.PropertiesChanged.connect(self._on_props_changed)
        self._on_props_changed(self.PLAYER_IFACE, self._init_props, [])

    def _on_props_changed(self, iface: str, props: dict|set, invalidated: list):
        print('Changed', self, iface, props, invalidated)
        try:
            if iface == self.PLAYER_IFACE:
                if 'Identity' in props:
                    self.name = self._proxy.Identity
                if 'PlaybackStatus' in props:
                    self.status = self._proxy.PlaybackStatus
                if 'Volume' in props:
                    self.volume = self._proxy.Volume
                if 'CanSeek' in props:
                    self.can_seek = self._proxy.CanSeek
                if 'CanGoNext' in props:
                    self.can_go_next = self._proxy.CanGoNext
                if 'CanGoPrevious' in props:
                    self.can_go_prev = self._proxy.CanGoPrevious
                if 'Metadata' in props:
                    metadata = self._proxy.Metadata
                    if 'xesam:title' in metadata:
                        self.title = metadata['xesam:title'].unpack()
                    if 'xesam:album' in metadata:
                        self.album = metadata['xesam:album'].unpack()
                    if 'xesam:artist' in metadata:
                        artist = metadata['xesam:artist'].unpack()
                        if isinstance(artist, list):
                            artist = ', '.join(artist)
                        self.artist = artist
                    if 'mpris:artUrl' in metadata:
                        self.cover = metadata['mpris:artUrl'].unpack()
        except Exception as e:
            ERR(f'Cannot read player properties. Error: {e}. {self}')

    def play(self):
        self._proxy.PlayPause()

    def next(self):
        self._proxy.Next()

    def prev(self):
        self._proxy.Previous()

    def set_volume(self, volume: float):
        self._proxy.Volume = volume

    # def rais(self):
    #     self.proxy.Raise(dbus_interface=self.MAIN_IFACE)
