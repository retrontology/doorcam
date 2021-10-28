import cv2
import numpy as np
from http.server import HTTPServer, BaseHTTPRequestHandler
from socketserver import ThreadingMixIn
from doorcam import *

class MJPGServer(ThreadingMixIn, HTTPServer):
    pass

class MJPGHandler(BaseHTTPRequestHandler):

    logger = Logger('doorcam.stream')

    def __init__(self, camera: Camera, *args, **kwargs):
        self.camera = camera
        super().__init__(*args, **kwargs)

    def do_GET(self):
        
        if self.path == '/stream.mjpg':

            self.send_response(200)
            self.send_header('Age', 0)
            self.send_header('Cache-Control', 'no-cache, private')
            self.send_header('Pragma', 'no-cache')
            self.send_header('Content-Type', 'multipart/x-mixed-replace; boundary=FRAME')
            self.end_headers()
            interval = 1.0/self.camera.max_fps
            checkpoint = time.time()
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
                now = time.time()
                while now - checkpoint < interval:
                    time.sleep(0.001)
                    now = time.time()
                checkpoint = now
        else:
            self.send_error(404)
            self.end_headers()
