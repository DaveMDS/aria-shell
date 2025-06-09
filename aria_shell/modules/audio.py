from typing import Mapping

from gi.repository import Gtk, Gdk, GObject

from aria_shell.utils import exec_detached
from aria_shell.utils.logger import get_loggers
from aria_shell.module import AriaModule
from aria_shell.config import AriaConfigModel
from aria_shell.services.xdg import XDGDesktopService
from aria_shell.ui import AriaGadget, AriaPopup, AriaSlider
from aria_shell.services.audio import (
    AudioService, AudioChannel, AudioChannelGroup
)


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


class AudioConfig(AriaConfigModel):
    mixer_command: str = ''


class AudioModule(AriaModule):
    def __init__(self):
        super().__init__()

    def module_init(self):
        super().module_init()

    def module_shutdown(self):
        super().module_shutdown()

    def module_instance_new(self, user_settings: Mapping[str, str], monitor: Gdk.Monitor):
        super().module_instance_new(user_settings, monitor)
        conf = AudioConfig(user_settings)
        return AudioGadget(conf)


class AudioGadget(AriaGadget):
    def __init__(self, conf: AudioConfig):
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
            lbox.connect('destroy', lambda *_: print('ListBox::destroy'))
            lbox.bind_model(self.aas.channels, self.channel_rows_factory)
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

    @staticmethod
    def channel_rows_factory(channel: AudioChannel) -> Gtk.Widget:
        # print('Factory for', channel)
        row = Gtk.ListBoxRow(selectable=False, activatable=False)
        # row.connect('destroy', lambda *_: print('DESTROY....'*6))
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
