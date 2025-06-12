from gi.repository import Gtk, Gdk, GObject

from aria_shell.utils import exec_detached
from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule
from aria_shell.config import AriaConfigModel
from aria_shell.services.xdg import XDGDesktopService
from aria_shell.gadget import AriaGadget
from aria_shell.ui import AriaPopup, AriaSlider
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

    def module_init(self):
        super().module_init()

    def module_shutdown(self):
        super().module_shutdown()

    def gadget_new(self, conf: AudioConfigModel, monitor: Gdk.Monitor):
        super().gadget_new(conf, monitor)
        return AudioGadget(conf)


class AudioGadget(AriaGadget):
    def __init__(self, conf: AudioConfigModel):
        super().__init__('audio', clickable=True)
        self.conf = conf
        self.popup: AriaPopup | None = None
        self.aas = AudioService()
        ico = Gtk.Image.new_from_icon_name('audio-volume-medium')
        self.append(ico)

    def on_mouse_down(self, button: int):
        self.toggle_popup()

    def toggle_popup(self):
        if self.popup:
            self.popup.close()
        else:
            # create the popup
            vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)

            # mixer channels
            lbox = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
            lbox.bind_model(self.aas.channels, self.channel_rows_factory)
            vbox.append(lbox)

            # media players
            lbox = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
            lbox.bind_model(self.aas.players, self.player_rows_factory)
            vbox.append(lbox)

            # mixer button
            if self.conf.mixer_command:
                btn = Gtk.Button(label='Mixer')  # TODO i18n
                btn.connect('clicked', self._on_mixer_button_clicked)
                vbox.append(btn)
            # open in and AriaPopup
            self.popup = AriaPopup(vbox, self, self.on_popup_destroy)

    def on_popup_destroy(self, _popup):
        self.popup = None

    def _on_mixer_button_clicked(self, _):
        exec_detached(self.conf.mixer_command)
        if self.popup:
            self.toggle_popup()

    def channel_rows_factory(self, channel: AudioChannel) -> Gtk.ListBoxRow:
        # print('Factory for', channel)
        row = Gtk.ListBoxRow(selectable=False, activatable=False)
        # row.connect('destroy', lambda *_: print('DESTROY....'*6))

        # hide the channel if a matching MediaPlayer exists
        if channel.group == AudioChannelGroup.STREAM:
            for player in self.aas.players:  # type: MediaPlayer
                if player.name.lower() == channel.name.lower():
                    row.set_visible(False)
                    return row

        hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        row.set_child(hbox)

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

        return row

    @staticmethod
    def player_rows_factory(player: MediaPlayer) -> Gtk.ListBoxRow:
        # print('Factory for', player)
        row = Gtk.ListBoxRow(selectable=False, activatable=False)
        # row.connect('destroy', lambda *_: print('DESTROY....'*6))

        # main grid in a frame
        grid = Gtk.Grid()
        frame = Gtk.Frame(label=player.name, label_xalign=0.5)
        frame.set_size_request(200, -1)
        frame.set_child(grid)
        row.set_child(frame)

        # cover image
        def cover_to_file(_, cover: str) -> str | None:
            if cover.startswith('file://'):
                return cover[7:]
            else:  # TODO: write an AriaImage that support urls
                WRN(f'Unsupported cover art url {cover}  TODO!!')
        img = Gtk.Image(pixel_size=80)
        player.bind_property(
            'cover', img, 'file',
            GObject.BindingFlags.SYNC_CREATE,
            transform_to=cover_to_file
        )
        grid.attach(img, 0, 0 , 1, 3)

        # Metadata: title
        lbl_props = {
            'hexpand': True, 'xalign': 0.0, 'wrap': True, 'max_width_chars': 40
        }
        lbl = Gtk.Label(**lbl_props)
        player.bind_property(
            'title', lbl, 'label',
            GObject.BindingFlags.SYNC_CREATE,
        )
        grid.attach(lbl, 1, 0, 1, 1)

        # Metadata: artist
        lbl = Gtk.Label(**lbl_props)
        player.bind_property(
            'artist', lbl, 'label',
            GObject.BindingFlags.SYNC_CREATE,
        )
        grid.attach(lbl, 1, 1, 1, 1)

        # Metadata: album
        lbl = Gtk.Label(**lbl_props)
        player.bind_property(
            'album', lbl, 'label',
            GObject.BindingFlags.SYNC_CREATE,
        )
        grid.attach(lbl, 1, 2, 1, 1)

        # 3 buttons in a row
        hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL,
                       halign=Gtk.Align.CENTER)
        grid.attach(hbox, 0, 3, 2, 1)

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
        # sli.connect('destroy', lambda *_: print('---' * 30))
        sli.set_range(0, 1.0)  # TODO 1.5 (configurabile)
        sli.connect('value-changed', lambda o: player.set_volume(o.props.value))
        player.bind_property(
            # keep slider in sync with player.volume
            'volume', sli, 'value',
            GObject.BindingFlags.SYNC_CREATE,
        )
        grid.attach(sli, 0, 4, 2, 1)

        return row
