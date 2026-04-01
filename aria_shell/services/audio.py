"""

High level audio service to be used by all aria components

The service manage two Gio.ListStore of observable
AudioChannels and MediaPlayers

Usage:
> aas = AriaAudioService()

Show all available mixer channels:
> for channel in aas.channels:
>     print(channel)  # <AudioChannel ...>

Bind a channel property:
> cha = aas.channel_by_id(cid)
> cha.bind_property('volume', slider, 'value',
>                   GObject.BindingFlags.SYNC_CREATE)

Set volume/muted:
> cha.set_volume(vol)
> cha.set_muted()  # no value means toggle

"""
from enum import StrEnum

from gi.repository import Gio, GObject

from aria_shell.utils import Singleton, IndexedListStore
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class AudioChannelGroup(StrEnum):
    INPUT = 'in'
    OUTPUT = 'out'
    STREAM = 'stream'


class AudioChannel(GObject.Object):
    __gtype_name__ = 'AudioChannel'

    # "reactive" props that can be watched/binded
    volume = GObject.Property(type=float, minimum=0, maximum=1.5)
    muted = GObject.Property(type=bool, default=False)

    def __init__(self,
        *,
        cid: str,  # opaque, backend dependent, must be hashable and printable
        group: AudioChannelGroup,
        name: str,
        caption: str,
        icon: str = None, # suggested icon name
        volume: float = 0.0,
        muted: bool = False,
    ):
        super().__init__()
        self.cid = cid
        self.group = group
        self.name = name
        self.caption = caption
        self.icon = icon
        self.muted = muted
        self.volume = volume

    def __repr__(self):
        return (
            f"<AudioChannel {self.cid}:{self.group} name='{self.name}'"
            f" volume={self.volume:.2f} muted={self.muted}>"
        )

    def set_muted(self, muted: bool | None = None):
        raise NotImplemented('Must be implemented in backend')

    def set_volume(self, volume: float):
        raise NotImplemented('Must be implemented in backend')


class MediaPlayer(GObject.Object):
    __gtype_name__ = 'MediaPlayer'

    # "reactive" props that can be watched/binded
    name = GObject.Property(type=str, default='')
    status = GObject.Property(type=str, default='Stopped')
    can_seek = GObject.Property(type=bool, default=False)
    can_go_next = GObject.Property(type=bool, default=True)
    can_go_prev = GObject.Property(type=bool, default=True)
    has_volume = GObject.Property(type=bool, default=False)
    volume = GObject.Property(type=float, minimum=0, maximum=1.5)
    title = GObject.Property(type=str, default='')
    artist = GObject.Property(type=str, default='')
    album = GObject.Property(type=str, default='')
    cover = GObject.Property(type=str, default='')

    def __init__(self, pid: str):
        super().__init__()
        self.pid = pid

    def __repr__(self):
        return (
            f"<MediaPlayer '{self.pid}' name='{self.name}' status={self.status}>"
        )

    def set_volume(self, volume: float):
        raise NotImplemented('Must be implemented in backend')

    def play(self):
        raise NotImplemented('Must be implemented in backend')

    def prev(self):
        raise NotImplemented('Must be implemented in backend')

    def next(self):
        raise NotImplemented('Must be implemented in backend')


_sort_weights = {
    AudioChannelGroup.OUTPUT: 1,
    AudioChannelGroup.INPUT: 2,
    AudioChannelGroup.STREAM: 3,
}
def channel_sort(ch1: AudioChannel, ch2: AudioChannel) -> int:
    """ sort channels in the store, return: 0, neg or pos """
    return _sort_weights.get(ch1.group, 0) - _sort_weights.get(ch2.group, 0)


class AudioService(metaclass=Singleton):
    def __init__(self):
        super().__init__()
        self._cancellable = Gio.Cancellable()  # TODO use on shutdown !!!

        # AudioChannel list store (with index for faster access by cid)
        self._channels = IndexedListStore(item_type=AudioChannel, key_prop='cid')

        # MediaPlayer list store (with index for faster access by pid)
        self._players = IndexedListStore(item_type=MediaPlayer, key_prop='pid')

        # try to load the pipewire backend
        try:
            from .audio_pipewire import PipeWireBackend
            PipeWireBackend(self, self._cancellable)
        except Exception as e:
            ERR(f'Cannot load PipeWire audio backend. Error: {e}')

        # try to load the mpris backend
        try:
            from .audio_mpris2 import Mpris2Backend
            Mpris2Backend(self, self._cancellable)
        except Exception as e:
            ERR(f'Cannot load Mpris2 audio backend. Error: {e}')

    #
    # public API
    @property
    def channels(self) -> IndexedListStore[AudioChannel]:
        return self._channels

    def channel_by_id(self, cid: str) -> AudioChannel | None:
        return self._channels.get(cid)

    @property
    def players(self) -> IndexedListStore[MediaPlayer]:
        return self._players

    def player_by_id(self, pid: str) -> MediaPlayer | None:
        return self._players.get(pid)

    #
    # backends API
    def channel_added(self, cha: AudioChannel):
        DBG(f'AAS: Channel added {cha}')
        self._channels.insert_sorted(cha, channel_sort)

    def channel_removed(self, cha_id: str):
        DBG(f'AAS: Channel removed {cha_id}')
        self._channels.remove_key(cha_id)

    def player_added(self, player: MediaPlayer):
        DBG(f'AAS: Player added {player}')
        self._players.append(player)

    def player_removed(self, pid: str):
        DBG(f'AAS: Player removed id={pid}')
        self._players.remove_key(pid)
