"""

High level audio service to be used by all aria components

The service manage a Gio.ListStore of observable AudioChannels

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

from aria_shell.utils import Singleton, Signalable
from aria_shell.utils.logger import get_loggers


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class AudioChannelGroup(StrEnum):
    INPUT = 'in'
    OUTPUT = 'out'
    STREAM = 'stream'


class AudioChannel(GObject.Object):

    # "reactive" props that can be binded. TRY TO USE Observable instead!!
    volume = GObject.Property(type=float, minimum=0, maximum=1.5)
    muted = GObject.Property(type=bool, default=False)

    def __init__(self,
        *,
        cid: str,  # opaque, backend dependent, must be ashable and printable
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


_sort_weights = {
    AudioChannelGroup.OUTPUT: 1,
    AudioChannelGroup.INPUT: 2,
    AudioChannelGroup.STREAM: 3,
}
def channel_sort(ch1: AudioChannel, ch2: AudioChannel) -> int:
    """ sort channels in the store, return: 0, neg or pos """
    return _sort_weights.get(ch1.group, 0) - _sort_weights.get(ch2.group, 0)


class AudioService(Signalable, metaclass=Singleton):
    def __init__(self):
        super().__init__()
        self._cancellable = Gio.Cancellable()  # TODO use on shutdown !!!

        # AudioChannels list store, with dict index for faster access by id
        self._channels = Gio.ListStore()
        self._ch_index: dict[str, AudioChannel] = {}  # cid => AudioChannel

        # try to load the pipewire backend
        try:
            from .audio_pipewire import PipeWireBackend
            PipeWireBackend(self, self._cancellable)
        except Exception as e:
            ERR(e)

    #
    # public API
    @property
    def channels(self) -> Gio.ListStore:
        return self._channels

    def channel_by_id(self, cid: str) -> AudioChannel | None:
        return self._ch_index.get(cid, None)

    #
    # backends API
    def channel_added(self, cha: AudioChannel):
        DBG(f'AAS: Channel added {cha}')
        self._channels.insert_sorted(cha, channel_sort)
        self._ch_index[cha.cid] = cha

    def channel_removed(self, cha_id: str):
        DBG(f'AAS: Channel removed {cha_id}')

        # pop the Channel from  the index
        cha = self._ch_index.pop(cha_id, None)
        if not cha:
            ERR(f'Removed not existant channel: {repr(cha_id)}')
            return

        # find position in store (needed to remove, hmm...somethig faster?)
        res, pos = self._channels.find(cha)
        if not res or pos < 1:
            ERR(f'Cannot find channel to remove: {cha_id}')
            return

        # remove from the store
        self._channels.remove(pos)
