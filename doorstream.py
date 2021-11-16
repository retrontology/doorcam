import cv2
import numpy as np
from http.server import HTTPServer, BaseHTTPRequestHandler
from socketserver import ThreadingMixIn
from doorcam import *
from logging import getLogger

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
