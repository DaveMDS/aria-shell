"""

Pipewire backend for the AriaAudioService

This backend use the WirePlumber gAPI to manage mixer channels

Reference:
https://pipewire.pages.freedesktop.org/wireplumber/
https://github.com/Alexays/Waybar/blob/master/src/modules/wireplumber.cpp

"""
from __future__ import annotations

try:
    import gi
    gi.require_version('Wp', '0.5')  # WirePlumber API
    from gi.repository import Wp
    _wireplumber_available = True
except (ImportError, ValueError):
    _wireplumber_available = False

from gi.repository import Gio, GLib

from aria_shell.services.audio import AudioChannel, AudioService, AudioChannelGroup
from aria_shell.utils.logger import get_loggers
from aria_shell.utils import Singleton, pack_variant


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class PipeWireAudioChannel(AudioChannel):
    def __init__(self, pwb: PipeWireBackend, **kargs):
        super().__init__(**kargs)
        self._pwb: PipeWireBackend = pwb

    def set_muted(self, muted: bool | None = None):
        self._pwb.set_muted(self, muted)

    def set_volume(self, volume: float | tuple[float, ...]):
        self._pwb.set_volume(self, volume)


class PipeWireBackend(metaclass=Singleton):
    def __init__(self, aas: AudioService, cancellable: Gio.Cancellable):
        if not _wireplumber_available:
            raise RuntimeError('WirePlumber gAPI not available!')

        self._aas = aas
        self._cancellable = cancellable

        # initialize the WirePlumber library
        DBG('WP Initialize the WirePlumber lib...')
        try:
            Wp.init(Wp.InitFlags.PIPEWIRE)
            self._wp_core = Wp.Core()
            # TODO how to set Client name? ourname in wpctl status is python3.13 !
            # TODO connect to Core 'connected' / 'disconnected'
            self._wp_manager = Wp.ObjectManager()
        except Exception as e:
            ERR(f'WP Cannot initialize WirePlumber gAPI. {e}')
            raise RuntimeError

        # init step #1: request the 'default-nodes-api' component
        DBG('WP Loading the default-nodes-api component...')
        self._wp_core.load_component(
            'libwireplumber-module-default-nodes-api',  # component
            'module', None, 'default-nodes-api',  # type, args, provides
            self._cancellable, self._on_nodes_api_ready  # callback
        )
        self._wp_core.connect()

    def _on_nodes_api_ready(self, core: Wp.Core, task):
        if core.load_component_finish(task) is False:
            ERR(f'WP Failed to load the default-nodes-api component')
            return

        # find the nodes API plugin
        self._nodes_api = Wp.Plugin.find(core, 'default-nodes-api')  # noqa
        if not self._nodes_api:
            ERR(f'WP Failed to find the default-nodes-api plugin')
            return

        # init step #2: request the 'mixer-api' component
        DBG('WP Loading the mixer-api component...')
        core.load_component(
            'libwireplumber-module-mixer-api',  # component
            'module',  None, 'mixer-api',  # type, args, provides
            self._cancellable, self._on_mixer_api_ready,  # callback
        )

    def _on_mixer_api_ready(self, core: Wp.Core, task):
        if core.load_component_finish(task) is False:
            ERR(f'WP Failed to load the mixer-api component')
            return

        # find the mixer API plugin
        self._mixer_api = Wp.Plugin.find(core, 'mixer-api')  # noqa
        if not self._mixer_api:
            ERR(f'WP Failed to find the mixer-api plugin')
            return

        # and finally perform the last init steps
        self._finalize_init()

    def _finalize_init(self):
        DBG('WP Init OK')
        # WP object manager signals
        # self._wp_manager.connect('installed', self._on_object_manager_installed)
        self._wp_manager.connect('object-added', self._on_node_added)
        self._wp_manager.connect('object-removed', self._on_node_removed)
        self._wp_manager.connect('objects-changed', self._on_nodes_changed)
        self._wp_manager.add_interest_full(Wp.ObjectInterest.new_type(Wp.Node))
        self._wp_core.install_object_manager(self._wp_manager)

        # setup mixer API
        self._mixer_api.set_property('scale', 1)  # LINEAR(0) CUBIC(1)
        self._mixer_api.connect('changed', self._on_mixer_changed)

        # setup default-nodes API ????
        # asd = self._nodes_api.emit('get-default-node', 'Audio/Sink')
        # print(f'  get-default-node: {asd}')

    # def _on_object_manager_installed(self, *_):
    #     # Here we can notify someone we are fully ready?
    #     print('_on_object_manager_installed', _, self)

    def _on_nodes_changed(self, man: Wp.ObjectManager):
        WRN(f'NODES CHANGED {man} ?? mi serve?? {self}')

    def _on_node_added(self, man: Wp.ObjectManager, node: Wp.Node):
        props = node.get_property('properties')
        DBG(f'WP added node id={node.get_id()} object.id={props.get('object.id')}')

        # Create the aria AudioChannel object from the Wp.Node
        cid = props.get('object.id') or node.get_id()
        cls = props.get('media.class')
        match cls:
            case 'Audio/Sink':
                group = AudioChannelGroup.OUTPUT
                name = props.get('node.description') or props.get('node.name')
                icon_name = props.get('device.icon-name')
                caption = props.get('node.nick')
            case 'Audio/Source':
                group = AudioChannelGroup.INPUT
                name = props.get('node.description') or props.get('node.name')
                icon_name = props.get('device.icon-name')
                caption = props.get('node.nick')
            case 'Stream/Output/Audio':
                group = AudioChannelGroup.STREAM
                name = props.get('application.name') or props.get('node.name')
                icon_name = props.get('application.name').lower()
                caption = props.get('media.name')
                # self._node_dump(node)
            case _:
                DBG(f'WP Skipping class {cls} for node "{props.get('node.name')}"')
                # self._node_dump(node)
                return

        # get the volumes from the mixer-api
        volume, muted = self._read_volume_from_mixer(int(cid))

        # Create the new AudioChannel and send to the manager
        cha = PipeWireAudioChannel(
            self,
            cid=cid,
            group=group,
            name=name,
            caption=caption,
            icon=icon_name,
            volume=volume,
            muted=muted,
        )
        self._aas.channel_added(cha)

    def _on_node_removed(self, man: Wp.ObjectManager, node: Wp.Node):
        # notify the AudioService about the removed id
        cid = node.get_property('properties').get('object.id') or node.get_id()
        self._aas.channel_removed(str(cid))

    def _on_mixer_changed(self, mixer, channel: int):
        # update the channel "reactive" properties with changed values
        if cha := self._aas.channel_by_id(str(channel)):
            volume, muted = self._read_volume_from_mixer(channel)
            if cha.muted != muted:
                cha.muted = muted
            if cha.volume != volume:
                cha.volume = volume

    def _read_volume_from_mixer(self, cid: int) -> (float, bool):
        vol_variant: GLib.Variant = self._mixer_api.emit('get-volume', cid)
        muted, vol = False, 0.0
        if vol_variant and (vol := vol_variant.unpack()):
            # Here we can get all the volumes for a channel, fe: L/R
            # channels = vol.get('channelVolumes', {}).values()
            # vols = tuple([cv.get('volume', 0.0) for cv in channels])
            # names = tuple([cv.get('channel', '') for cv in channels])
            muted = vol.get('mute', False)
            vol = vol.get('volume', 0.0)
        else:
            ERR(f'WP Cannot read volumes for {cid}')
        return vol, muted

    def set_muted(self, cha: PipeWireAudioChannel, muted: bool | None = None):
        if muted is None:
            muted = not cha.muted
        data = pack_variant({'mute': muted})
        self._mixer_api.emit('set-volume', int(cha.cid), data)

    def set_volume(self, cha: PipeWireAudioChannel, volume: float):
        data = pack_variant(volume)
        self._mixer_api.emit('set-volume', int(cha.cid), data)

    @staticmethod
    def _node_dump(node: Wp.Node):
        WRN(f'=========== Wp.Node({node.get_id()}) ===========')
        WRN(f"state: {node.get_state()}")
        WRN(f"n-input-ports: {node.get_n_input_ports()}")
        WRN(f"n-output-ports: {node.get_n_output_ports()}")

        WRN('node properties:')
        for prop in node.list_properties():
            WRN(f'  {prop.get_name()} = {prop}')

        WRN('pipewire properties:')
        node.new_properties_iterator().foreach(
            lambda item: WRN(f'  {item.get_key()} = "{item.get_value()}" {item}')
        )

        WRN('ports:')
        node.new_ports_iterator().foreach(
            lambda port: WRN(f'  {port} {port.get_direction()}')
        )
