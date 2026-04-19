from pathlib import  Path

from gi.repository import Gdk, Gtk, GdkPixbuf

from aria_shell.utils import Timer
from aria_shell.utils.logger import get_loggers

# optional GStreamer dependency, for video playback
try:
    import gi
    gi.require_version('Gst', '1.0')
    from gi.repository import Gst
except (ImportError, ValueError):
    Gst = None


DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


def AriaMediaPicture(source: Path, *,
                     content_fit: Gtk.ContentFit = Gtk.ContentFit.FILL,
                     ) -> Gtk.Widget | None:
    """
    A Gtk.Widget that is able to display any supported media files.
    Support static and animated images, movies and shaders.

    This factory function create and return an instance of the specific
    widget based on the file extension of `source`.
    """
    extension = source.suffix.lower().strip('.')

    for picture_class in (StaticPicture, AnimatedPicture, VideoPicture):
        if extension in picture_class.__supported_extensions__:
            return picture_class(source, content_fit=content_fit)

    WRN('Unknow filetype extension "%s" for source: %s', extension, source)
    return None


class StaticPicture(Gtk.Picture):
    """
    A standard Gtk.Picture to show static images
    """
    __supported_extensions__ = {'png', 'jpg', 'jpeg', 'webp'}

    def __init__(self, path: Path, **kwargs):
        super().__init__(**kwargs)
        self.set_filename(path.as_posix())


class AnimatedPicture(Gtk.Picture):
    """
    A Gtk.Picture that is able to show animated images
    """
    __supported_extensions__ = {'gif'}

    def __init__(self, path: Path, **kwargs):
        super().__init__(**kwargs)

        self.animation = GdkPixbuf.PixbufAnimation.new_from_file(path.as_posix())
        self.animation_iter = self.animation.get_iter(None)

        self.frame_timer = Timer(0, self.process_frame, autostart=False)
        self.process_frame(first_frame=True)

    def process_frame(self, first_frame=False):
        # advance the interator to the next frame
        if not first_frame:
            self.animation_iter.advance(None)

        # update the paintable with the current animation frame
        pixbuf = self.animation_iter.get_pixbuf()
        self.set_paintable(Gdk.Texture.new_for_pixbuf(pixbuf))

        # reschedule the timer to "wait" for the frame time
        delay_ms = self.animation_iter.get_delay_time()
        assert delay_ms >= 1
        self.frame_timer.stop()
        self.frame_timer.interval = float(delay_ms) / 1000.0
        self.frame_timer.start()

    def do_unmap(self):
        self.frame_timer.stop()
        self.frame_timer = None
        self.animation_iter = None
        self.animation = None
        Gtk.Picture.do_unmap(self)


class VideoPicture(Gtk.Picture):
    """
    A Gtk.Picture that is able to show a muted video in loop
    """
    __supported_extensions__ = {'mp4', 'webm', 'mkv', 'avi', 'mpg'}

    def __init__(self, path: Path, **kwargs):
        super().__init__(**kwargs)

        if Gst is None:
            WRN('GStreamer not fully installed. Cannot play video in background!')

        self.media = Gtk.MediaFile.new_for_filename(path.as_posix())
        self.media.set_muted(True)
        self.media.set_loop(True)

        self.set_paintable(self.media)
        self.media.play()
