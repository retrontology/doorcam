import cv2
import numpy as np
from threading import Thread
from doorscreen import *
from doorcam import *
from doorstream import *
from time import sleep
from http.server import HTTPServer
from functools import partial

def main():
    cam = Camera()
    screen = Screen(rotation=cv2.ROTATE_90_CLOCKWISE)
    stream_handler = partial(MJPGStream, cam)
    server = HTTPServer(('0.0.0.0', 8080), stream_handler)
    http_thread = Thread(target=server.serve_forever)
    http_thread.start()
    while True:
        try:
            screen.fb_write_image(cam.get_current_frame())
        except:
            pass
        

if __name__ == '__main__':
    main()