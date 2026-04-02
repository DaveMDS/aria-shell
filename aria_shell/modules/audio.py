from gi.repository import Gtk, GLib, GObject

from aria_shell.utils import exec_detached
from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule, GadgetRunContext
from aria_shell.config import AriaConfigModel
from aria_shell.services.xdg import XDGDesktopService
from aria_shell.gadget import AriaGadget
from aria_shell.ui import AriaSlider, AriaPopover, AriaBox
from aria_shell.services.audio import (
    AudioService, AudioChannel, AudioChannelGroup, MediaPlayer
)

# from edgar:
# __gadget_name__ = 'Audio'
# __gadget_vers__ = '0.2'
# __gadget_auth__ = 'DaveMDS'
# __gadget_mail__ = 'dave@gurumeditation.it'
# __gadget_desc__ = 'The complete audio gadget.'
# __gadget_vapi__ = 2
# __gadget_opts__ = {'popup_on_desktop': True}


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class AudioConfigModel(AriaConfigModel):
    mixer_command: str = ''


class AudioModule(AriaModule):
    config_model_class = AudioConfigModel

    def __init__(self):
        super().__init__()

    def gadget_factory(self, ctx: GadgetRunContext) -> AriaGadget | None:
        conf: AudioConfigModel = ctx.config  # noqa
        return AudioGadget(conf)


class AudioGadget(AriaGadget):
    def __init__(self, conf: AudioConfigModel):
        super().__init__('audio', clickable=True)
        self.conf = conf
        self.popover: AriaPopover | None = None
        self.aas = AudioService()
        self.icon = Gtk.Image.new_from_icon_name('audio-volume-medium')
        self.append(self.icon)

    def mouse_click(self, button: int):
        self.toggle_popup()

    def toggle_popup(self):
        if self.popover:
            self.popover.popdown()
            return

        # create the popup
        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)

        # mixer channels
        abox = AriaBox(orientation=Gtk.Orientation.VERTICAL)
        abox.bind_model(self.aas.channels, self.channel_rows_factory)
        vbox.append(abox)

        # media players
        abox = AriaBox(orientation=Gtk.Orientation.VERTICAL)
        abox.bind_model(self.aas.players, self.player_rows_factory)
        vbox.append(abox)

        # mixer button
        if self.conf.mixer_command:
            btn = Gtk.Button(label='Mixer')  # TODO i18n
            self.safe_connect(btn, 'clicked', self._on_mixer_button_clicked)
            vbox.append(btn)

        # open in an AriaPopover
        self.popover = AriaPopover(self.icon, vbox, self.on_popover_closed)

    def on_popover_closed(self, _popover):
        self.popover = None

    def _on_mixer_button_clicked(self, _):
        exec_detached(self.conf.mixer_command)
        if self.popover:
            self.popover.popdown()

    def channel_rows_factory(self, channel: AudioChannel) -> Gtk.Widget:
        # print('Factory for', channel)
        hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)

        # hide the channel if a matching MediaPlayer exists
        if channel.group == AudioChannelGroup.STREAM:
            # TODO questo è una vaccata, non funziona!
            #  nascondiamo tutti i volumi nei player invece, sono anche brutti!
            for player in self.aas.players:
                if player.name.lower() == channel.name.lower():
                    hbox.set_visible(False)
                    return hbox

        # icon embedded in a mute toggle button
        if channel.group == AudioChannelGroup.OUTPUT:
            icons = channel.icon, 'audio-volume-medium'
        elif channel.group == AudioChannelGroup.INPUT:
            icons = channel.icon, 'audio-input-microphone'
        else:  # AudioChannelGroup.STREAM
            icons = channel.name.lower(), 'audio-volume-high'

        btn = Gtk.ToggleButton(valign=Gtk.Align.CENTER, halign=Gtk.Align.CENTER)
        ico = XDGDesktopService().get_icon(icons)
        btn.set_child(ico)
        btn.set_tooltip_text('Toggle mute')  # TODO i18n
        # btn.connect('destroy', lambda *_: print('^^^' * 30))
        btn.connect('toggled', lambda o: channel.set_muted(o.get_active()))
        channel.bind_property('muted', btn, 'active', GObject.BindingFlags.SYNC_CREATE)
        hbox.append(btn)

        # Box: name + slider (disabled when muted)
        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, hexpand=True)
        channel.bind_property('muted', vbox, 'sensitive',
                              GObject.BindingFlags.SYNC_CREATE |
                              GObject.BindingFlags.INVERT_BOOLEAN)

        # channel name
        hbox.append(vbox)
        lbl = Gtk.Label(halign=Gtk.Align.START)
        lbl.set_markup(f'{channel.cid}. {channel.name}')
        vbox.append(lbl)

        # volume slider (binded to channel.volume)
        sli = AriaSlider()
        # sli.connect('destroy', lambda *_: print('---' * 30))
        sli.set_range(0, 1.0)  # TODO 1.5 (configurabile)
        sli.connect('value-changed', lambda o: channel.set_volume(o.get_value()))
        channel.bind_property('volume', sli, 'value',
                              GObject.BindingFlags.SYNC_CREATE)
        vbox.append(sli)

        return hbox

    @staticmethod
    def player_rows_factory(player: MediaPlayer) -> Gtk.Widget:
        # print('Factory for', player)

        # main vbox in a frame
        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        frame = Gtk.Frame(label=player.name, label_xalign=0.5)
        frame.set_size_request(200, -1)
        frame.set_child(vbox)

        # cover + metadata in a vbox
        hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        vbox.append(hbox)

        # cover image
        img = Gtk.Image(pixel_size=80)
        def cover_to_file(_, cover: str) -> str | None:
            if not cover:
                return None
            elif cover.startswith('file://'):
                return cover[7:]
            else:  # TODO: write an AriaImage that support urls
                WRN(f'Unsupported cover art url "{cover}"  TODO!!')
                return None
        player.bind_property(
            # set file on cover change
            'cover', img, 'file',
            GObject.BindingFlags.SYNC_CREATE,
            transform_to=cover_to_file
        )
        player.bind_property(
            # hide image if cover not available
            'cover', img, 'visible',
            GObject.BindingFlags.SYNC_CREATE,
            transform_to=lambda _, cover: cover and cover.startswith('file://')
        )
        hbox.append(img)

        # Metadata label
        def metadata_to_markup(_, _title):
            markup: list[str] = []
            if player.title:
                title = GLib.markup_escape_text(player.title)
                markup.append(f'<b>{title}</b>')
            if player.artist:
                artist = GLib.markup_escape_text(player.artist)
                markup.append(f'<span>{artist}</span>')
            if player.album:
                album = GLib.markup_escape_text(player.album)
                markup.append(f'<span>{album}</span>')
            return '\n'.join(markup)
        lbl = Gtk.Label(hexpand=True, xalign=0.0, yalign=0.0,
                        wrap=True, max_width_chars=40, use_markup=True)
        player.bind_property(
            'title', lbl, 'label',
            GObject.BindingFlags.SYNC_CREATE,
            transform_to=metadata_to_markup,
        )
        hbox.append(lbl)

        # 3 buttons in a row
        hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL,
                       halign=Gtk.Align.CENTER)
        vbox.append(hbox)

        # button: previous
        btn = Gtk.Button(icon_name='media-skip-backward')
        btn.connect('clicked', lambda b: player.prev())
        player.bind_property(
            # disable the button when can_go_prev is False
            'can_go_prev', btn, 'sensitive',
            GObject.BindingFlags.SYNC_CREATE
        )
        hbox.append(btn)

        # button: play/pause
        def status_to_icon_name(_, status: str) -> str:
            if status == 'Playing':
                return 'media-playback-pause'
            else:
                return 'media-playback-start'
        play_btn = Gtk.Button(icon_name='media-playback-start')
        play_btn.connect('clicked', lambda b: player.play())
        player.bind_property(
            # adjust icon when status change
            'status', play_btn, 'icon_name',
            GObject.BindingFlags.SYNC_CREATE,
            transform_to=status_to_icon_name,
        )
        hbox.append(play_btn)

        # button: next
        btn = Gtk.Button(icon_name='media-skip-forward')
        btn.connect('clicked', lambda b: player.next())
        player.bind_property(
            # disable the button when can_go_next if False
            'can_go_next', btn, 'sensitive',
             GObject.BindingFlags.SYNC_CREATE,
        )
        hbox.append(btn)

        # volume slider
        sli = AriaSlider(hexpand=True)
        sli.set_range(0, 1.0)  # TODO 1.5 (configurabile)
        sli.connect('value-changed', lambda o: player.set_volume(o.props.value))
        player.bind_property(
            # only show the slider if the player support Volume
            'has_volume', sli, 'visible',
            GObject.BindingFlags.SYNC_CREATE,
        )
        player.bind_property(
            # keep slider in sync with player.volume
            'volume', sli, 'value',
            GObject.BindingFlags.SYNC_CREATE,
        )
        vbox.append(sli)

        return frame
