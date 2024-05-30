import cv2
import numpy as np
from http.server import HTTPServer, BaseHTTPRequestHandler
from socketserver import ThreadingMixIn
from .doorcam import Camera
from logging import getLogger
import time
import gi
gi.require_version('GstRtspServer', '1.0')
from gi.repository import Gst, GstRtspServer

GST_DECODE_FLAGS = cv2.IMREAD_COLOR + cv2.IMREAD_LOAD_GDAL

class MJPGServer(ThreadingMixIn, HTTPServer):
    pass

class MJPGHandler(BaseHTTPRequestHandler):

    logger = getLogger('doorcam.stream')

    def __init__(self, camera: Camera, *args, **kwargs):
        self.camera = camera
        self.frame_update = False
        self.camera.add_callback(self.trigger_frame_update)
        super().__init__(*args, **kwargs)

    def trigger_frame_update(self, image):
        self.frame_update = True

    def do_GET(self):
        
        if self.path == '/stream.mjpg':

            self.send_response(200)
            self.send_header('Age', 0)
            self.send_header('Cache-Control', 'no-cache, private')
            self.send_header('Pragma', 'no-cache')
            self.send_header('Content-Type', 'multipart/x-mixed-replace; boundary=FRAME')
            self.end_headers()
            self.logger.info(f'Serving MJPG stream to {self.client_address}')
            while True:
                image = self.camera.current_jpg
                try:
                    self.wfile.write(b'--FRAME\r\n')
                    self.send_header('Content-type', 'image/jpeg')
                    self.send_header('Content-length', str(image.size))
                    self.end_headers()
                    self.wfile.write(image.tostring())
                    self.wfile.write(b'\r\n')
                except Exception as e:
                    self.logger.error(e)
                    self.logger.info(f'Stopping MJPG stream to {self.client_address}')
                    self.camera.remove_callback(self.trigger_frame_update)
                    break
                while not self.frame_update:
                    time.sleep(0.01)
                self.frame_update = False
        else:
            self.send_error(404)
            self.end_headers()

class DoorFactory(GstRtspServer.RTSPMediaFactory):
    def __init__(self, camera:Camera, **properties):
        super(DoorFactory, self).__init__(**properties)
        self.camera = camera
        self.duration = 1 / self.camera.max_fps * Gst.SECOND
        self.launch_string = 'appsrc name=source block=true format=GST_FORMAT_TIME ' \
                             f'caps=video/x-raw,format=BGR,width={self.camera.resolution[0]},height={self.camera.resolution[1]},framerate={self.camera.max_fps}/1 ' \
                             '! videoconvert ! video/x-raw,format=I420 ' \
                             '! v4l2h264enc ! queue ' \
                             '! rtph264pay config-interval=1 name=pay0 pt=96 '

    def on_need_data(self, src, length):
        if self.camera.current_jpg:
            image = cv2.imdecode(self.camera.current_jpg, GST_DECODE_FLAGS)
            data = image.tostring()
            buf = Gst.Buffer.new_allocate(None, len(data), None)
            buf.fill(0, data)
            buf.duration = self.duration
            timestamp = self.number_frames * self.duration
            buf.pts = buf.dts = int(timestamp)
            buf.offset = timestamp
            self.number_frames += 1
            retval = src.emit('push-buffer', buf)
            if retval != Gst.FlowReturn.OK:
                print(retval)

    def do_create_element(self, url):
        return Gst.parse_launch(self.launch_string)

    def do_configure(self, rtsp_media):
        self.number_frames = 0
        appsrc = rtsp_media.get_element().get_child_by_name('source')
        appsrc.connect('need-data', self.on_need_data)


class GstServer(GstRtspServer.RTSPServer):
    def __init__(self, camera:Camera, **properties):
        super(GstServer, self).__init__(**properties)
        self.factory = DoorFactory(camera)
        self.factory.set_shared(True)
        self.get_mount_points().add_factory("/doorcam", self.factory)
        self.attach(None)
