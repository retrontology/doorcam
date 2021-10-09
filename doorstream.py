import cv2
import numpy as np
from http.server import BaseHTTPRequestHandler
from doorcam import *

class MJPGStream(BaseHTTPRequestHandler):

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

            while True:
                ret, image = cv2.imencode('.jpg', self.camera.get_current_frame())
                if ret:
                    self.wfile.write(b'--FRAME\r\n')
                    self.send_header('Content-type', 'image/jpeg')
                    self.send_header('Content-length', str(image.size))
                    self.end_headers()
                    self.wfile.write(image.tostring())
                    self.wfile.write(b'\r\n')
                else:
                    continue
        else:
            self.send_error(404)
            self.end_headers()
