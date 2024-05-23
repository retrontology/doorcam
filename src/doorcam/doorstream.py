import cv2
import numpy as np
from http.server import HTTPServer, BaseHTTPRequestHandler
from socketserver import ThreadingMixIn
from doorcam import Camera
from logging import getLogger
import time
import gi
from gi.repository import Gst, GstRtspServer

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
        self.launch_string = 'appsrc name=source block=true format=GST_FORMAT_TIME ' \
                             'caps=video/x-raw,format=BGR,width=1280,height=720,framerate={}/1 ' \
                             '! videoconvert ! video/x-raw,format=I420 ' \
                             '! x264enc speed-preset=ultrafast tune=zerolatency ! queue ' \
                             '! rtph264pay config-interval=1 name=pay0 pt=96 '.format(self.fps)
        # streams to gst-launch-1.0 rtspsrc location=rtsp://localhost:8554/test latency=50 ! decodebin ! autovideosink

    def on_need_data(self, src, lenght):
        if self.cap.isOpened():
            ret, frame = self.cap.read()
            if ret:
                data = frame.tostring()
                #print(data)
                buf = Gst.Buffer.new_allocate(None, len(data), None)
                buf.fill(0, data)
                buf.duration = self.duration
                timestamp = self.number_frames * self.duration
                buf.pts = buf.dts = int(timestamp)
                buf.offset = timestamp
                self.number_frames += 1
                retval = src.emit('push-buffer', buf)
                #print('pushed buffer, frame {}, duration {} ns, durations {} s'.format(self.number_frames,
                #                                                                       self.duration,
                #                                                                       self.duration / Gst.SECOND))
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
