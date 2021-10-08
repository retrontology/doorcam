import cv2
import numpy as np
from http.server import BaseHTTPRequestHandler,HTTPServer



class Stream(BaseHTTPRequestHandler):

    def do_GET(self):
        
        if self.path == '/stream':
            self.send_response(200)
            self.send_header(
                'Content-type',
                'multipart/x-mixed-replace; boundary=--jpgboundary'
            )
            self.end_headers()